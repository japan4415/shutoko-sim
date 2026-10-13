// 配信レスポンスのセキュリティヘッダー（Issue #16）。
//
// 静的アセット（HTML / JS / CSS / WASM glue）は Cloudflare Workers Static Assets が
// Worker コードを経由せずに返すため、web/public/_headers が担当する。Worker が生成する
// 応答（/releases/*、/api/*）には _headers が適用されないので、同じ方針をここで付与する。
// 文書向けの Content-Security-Policy は _headers と web/index.html の meta タグが持ち、
// 両者の一致は workers/test/security-headers.test.ts が検査する（片方だけ直すと落ちる）。

/**
 * Worker が生成するすべての応答に付与するヘッダー。
 *
 * - `X-Content-Type-Options: nosniff` — R2 から返す成果物の MIME をブラウザに推測させない
 * - `X-Frame-Options: DENY` / CSP の `frame-ancestors 'none'` — クリックジャッキング防止
 *   （meta タグの CSP は `frame-ancestors` を無視するため、ヘッダーでしか効かない）
 * - `Referrer-Policy: no-referrer` — 検索語や座標を含む URL を外部へ漏らさない
 * - `Permissions-Policy` — 位置情報は自オリジンのみ（現在地検索で使う）、他は全て無効
 */
export const SECURITY_HEADERS: ReadonlyArray<readonly [string, string]> = [
  ["X-Content-Type-Options", "nosniff"],
  ["X-Frame-Options", "DENY"],
  ["Referrer-Policy", "no-referrer"],
  [
    "Permissions-Policy",
    "camera=(), microphone=(), payment=(), usb=(), geolocation=(self)",
  ],
];

/**
 * HTTPS の応答にだけ付与するヘッダー。workers.dev は HSTS preload 済みだが、独自ドメインへ
 * 移したときに同じ保証を効かせるため、平文 HTTP（ローカル開発・テスト）では付けない。
 */
export const STRICT_TRANSPORT_SECURITY_HEADER: readonly [string, string] = [
  "Strict-Transport-Security",
  "max-age=31536000; includeSubDomains",
];

/**
 * 応答にセキュリティヘッダーを付与して返す。
 *
 * 既存ヘッダー（成果物の `Cache-Control` / `ETag` / `Content-Type` など）は保持する。
 * ヘッダーを書き換えられない応答（別オリジンからの fetch 結果など）にも耐えるよう、
 * 常にヘッダーを複製した新しい Response を作る。304/204/205 は本文を持てないため、
 * 元の応答と同じく本文を渡さない。
 */
export function withSecurityHeaders(response: Response, request: Request): Response {
  const headers = new Headers(response.headers);
  for (const [name, value] of SECURITY_HEADERS) {
    headers.set(name, value);
  }
  if (isSecureRequest(request)) {
    const [name, value] = STRICT_TRANSPORT_SECURITY_HEADER;
    headers.set(name, value);
  }

  const bodyless =
    response.status === 304 || response.status === 204 || response.status === 205;
  return new Response(bodyless ? null : response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  });
}

/** リクエストが HTTPS 経由かどうか。X-Forwarded-Proto ではなく URL のスキームで判定する。 */
function isSecureRequest(request: Request): boolean {
  try {
    return new URL(request.url).protocol === "https:";
  } catch {
    return false;
  }
}
