// 住所検索クライアント。同一オリジン `POST /api/geocode` を叩き、Worker のエラー封筒を
// GeocodeError へ正規化する。fetch は注入可能にして Vitest（node 環境）で検証する。
// プライバシー: クエリ文字列や座標をログ・例外メッセージへ出さない。

/** ジオコーダが返す 1 件の候補。 */
export interface GeocodeCandidate {
  label: string;
  lat: number;
  lon: number;
}

/** ジオコード失敗。code は Worker の error.code、retryable は再試行可否。 */
export class GeocodeError extends Error {
  readonly code: string;
  readonly retryable: boolean;

  constructor(code: string, retryable: boolean) {
    super(code);
    this.name = "GeocodeError";
    this.code = code;
    this.retryable = retryable;
  }
}

interface ErrorEnvelope {
  error?: { code?: unknown; retryable?: unknown };
}

/** エラー本文から `{ error: { code, retryable } }` を取り出す（不正なら null）。 */
function parseErrorBody(body: unknown): { code: string; retryable: boolean } | null {
  if (!body || typeof body !== "object") {
    return null;
  }
  const envelope = body as ErrorEnvelope;
  const error = envelope.error;
  if (!error || typeof error !== "object" || typeof error.code !== "string") {
    return null;
  }
  return { code: error.code, retryable: error.retryable === true };
}

/** 候補配列の形状を検証する。1 件でも不正なら false（全体を拒否）。 */
function isValidCandidates(value: unknown): value is GeocodeCandidate[] {
  if (!Array.isArray(value)) {
    return false;
  }
  return value.every((item) => {
    if (!item || typeof item !== "object") {
      return false;
    }
    const candidate = item as Partial<GeocodeCandidate>;
    if (typeof candidate.label !== "string") {
      return false;
    }
    const { lat, lon } = candidate;
    if (!Number.isFinite(lat) || !Number.isFinite(lon)) {
      return false;
    }
    return (lat as number) >= -90 && (lat as number) <= 90 && (lon as number) >= -180 && (lon as number) <= 180;
  });
}

/**
 * 住所クエリをジオコードして候補一覧を返す。
 * 非 2xx はエラー封筒を読んで GeocodeError を投げ、通信・中断失敗は FETCH_FAILED にする。
 */
export async function geocode(
  query: string,
  fetchFn: typeof fetch = fetch,
  signal?: AbortSignal,
): Promise<GeocodeCandidate[]> {
  let response: Response;
  try {
    response = await fetchFn("/api/geocode", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ query }),
      signal,
    });
  } catch {
    throw new GeocodeError("FETCH_FAILED", true);
  }

  if (!response.ok) {
    let body: unknown = null;
    try {
      body = await response.json();
    } catch {
      // 本文が JSON でない場合は下の所在不明エラーへフォールバックする。
    }
    const parsed = parseErrorBody(body);
    if (parsed) {
      throw new GeocodeError(parsed.code, parsed.retryable);
    }
    throw new GeocodeError("FETCH_FAILED", true);
  }

  let body: unknown;
  try {
    body = await response.json();
  } catch {
    throw new GeocodeError("INVALID_RESPONSE", false);
  }

  const candidates =
    body && typeof body === "object" ? (body as { candidates?: unknown }).candidates : undefined;
  if (!isValidCandidates(candidates)) {
    throw new GeocodeError("INVALID_RESPONSE", false);
  }
  return candidates;
}
