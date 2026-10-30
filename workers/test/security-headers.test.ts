import { createExecutionContext, env, waitOnExecutionContext } from "cloudflare:test";
import { beforeAll, describe, expect, it, vi } from "vitest";
import headersFile from "../../web/public/_headers?raw";
import indexHtml from "../../web/index.html?raw";
import worker from "../src/index";
import { SECURITY_HEADERS, STRICT_TRANSPORT_SECURITY_HEADER } from "../src/security";

// Issue #16: 限定公開前のセキュリティヘッダー整備。
// 静的アセットは Worker を経由せず Workers Static Assets が返すため web/public/_headers が、
// Worker が生成する応答（/releases/*, /api/*）は workers/src/security.ts が担当する。
// この suite は両者を突き合わせ、片方だけを直すと落ちるようにする。

const TEST_RELEASE = "all-real-v5";

/** `_headers` の `/*` ブロックのヘッダーを [name, value] に開く。 */
function parseHeadersFile(text: string): Map<string, string> {
  const headers = new Map<string, string>();
  let inRootBlock = false;
  for (const rawLine of text.split("\n")) {
    const line = rawLine.trimEnd();
    if (line.trim().startsWith("#") || line.trim() === "") {
      continue;
    }
    if (!/^\s/.test(line)) {
      // ブロック先頭（URL パターン）。アセット全体に効く `/*` だけを対象にする。
      inRootBlock = line.trim() === "/*";
      continue;
    }
    if (!inRootBlock) {
      continue;
    }
    const separator = line.indexOf(":");
    expect(separator).toBeGreaterThan(0);
    headers.set(line.slice(0, separator).trim().toLowerCase(), line.slice(separator + 1).trim());
  }
  return headers;
}

/** CSP 文字列を「ディレクティブ名 → 値の並び」へ正規化する（順序と空白の差を無視する）。 */
function parseCsp(value: string): Map<string, string[]> {
  const directives = new Map<string, string[]>();
  for (const part of value.split(";")) {
    const tokens = part.trim().split(/\s+/).filter(Boolean);
    if (tokens.length === 0) {
      continue;
    }
    directives.set(tokens[0], tokens.slice(1));
  }
  return directives;
}

/** index.html の meta タグから Content-Security-Policy を取り出す。 */
function metaCsp(html: string): string {
  const match = html.match(
    /<meta\s+http-equiv="Content-Security-Policy"\s+content="([^"]+)"/s
  );
  expect(match, "web/index.html must keep the Content-Security-Policy meta tag").not.toBeNull();
  return match![1].replace(/\s+/g, " ").trim();
}

describe("Delivery security headers (Issue #16)", () => {
  const assetHeaders = parseHeadersFile(headersFile);

  beforeAll(async () => {
    await env.ARTIFACTS_BUCKET.put(
      `releases/${TEST_RELEASE}/manifest.json`,
      JSON.stringify({ schemaVersion: 1, releaseId: TEST_RELEASE })
    );
    await env.ARTIFACTS_BUCKET.put(
      `releases/${TEST_RELEASE}/snap-index.json`,
      JSON.stringify({ schemaVersion: 1, releaseId: TEST_RELEASE, items: [] })
    );
  });

  async function fetchFrom(
    url: string,
    init?: RequestInit
  ): Promise<Response> {
    const ctx = createExecutionContext();
    const res = await worker.fetch(new Request(url, init), env, ctx);
    await waitOnExecutionContext(ctx);
    return res;
  }

  it("_headers と index.html の CSP は同じディレクティブを持ち、frame-ancestors はヘッダーだけが持つ", () => {
    const fromFile = assetHeaders.get("content-security-policy");
    expect(fromFile, "web/public/_headers must define Content-Security-Policy for /*").toBeDefined();
    const headerDirectives = parseCsp(fromFile!);
    const metaDirectives = parseCsp(metaCsp(indexHtml));

    // frame-ancestors は meta では無視される（Chromium が警告を出す）ため、ヘッダーだけが持つ。
    expect(headerDirectives.has("frame-ancestors")).toBe(true);
    expect(metaDirectives.has("frame-ancestors")).toBe(false);

    headerDirectives.delete("frame-ancestors");
    expect(headerDirectives).toEqual(metaDirectives);
  });

  it("_headers は Worker と同じ非 CSP ヘッダーを持つ", () => {
    for (const [name, value] of SECURITY_HEADERS) {
      expect(
        assetHeaders.get(name.toLowerCase()),
        `web/public/_headers must send ${name} like the Worker does`
      ).toBe(value);
    }
    const [hstsName, hstsValue] = STRICT_TRANSPORT_SECURITY_HEADER;
    expect(assetHeaders.get(hstsName.toLowerCase())).toBe(hstsValue);
  });

  it("アセット向け CSP は WASM・ワーカー・地図タイル・同一オリジン API を許可する", () => {
    const directives = parseCsp(assetHeaders.get("content-security-policy")!);
    expect(directives.get("default-src")).toEqual(["'self'"]);
    expect(directives.get("script-src")).toEqual(["'self'", "'wasm-unsafe-eval'"]);
    // 3Dプレビューの CSS は外部ファイルなので、通常 UI と同じくインラインスタイルは許さない。
    expect(directives.get("style-src")).toEqual(["'self'"]);
    expect(directives.get("worker-src")).toEqual(["'self'", "blob:"]);
    expect(directives.get("connect-src")).toEqual(
      expect.arrayContaining([
        "'self'",
        "https://tileserver.geolonia.com",
        "https://api.geolonia.com",
        "https://cdn.geolonia.com",
        "https://glyphs.geolonia.com",
        "https://d23l04mwnd60ho.cloudfront.net",
        "https://stars.optgeo.org",
      ]),
    );
    expect(directives.get("img-src")).toEqual(
      expect.arrayContaining([
        "'self'",
        "data:",
        "blob:",
        "https://cyberjapandata.gsi.go.jp",
        "https://*.tile.openstreetmap.org",
        "https://cdn.geolonia.com",
        "https://api.geolonia.com",
      ]),
    );
    expect(directives.get("frame-ancestors")).toEqual(["'none'"]);
    expect(directives.get("object-src")).toEqual(["'none'"]);
    expect(directives.get("base-uri")).toEqual(["'self'"]);
  });

  it("Worker が生成する応答にも同じヘッダーが付く", async () => {
    const responses = [
      await fetchFrom(`http://localhost/releases/${TEST_RELEASE}/snap-index.json`),
      await fetchFrom("http://localhost/unknown/route"),
      await fetchFrom("http://localhost/api/geocode", { method: "GET" }),
      await fetchFrom("http://localhost/api/bench-result", { method: "GET" }),
    ];
    for (const res of responses) {
      for (const [name, value] of SECURITY_HEADERS) {
        expect(res.headers.get(name), `${res.url} must send ${name}`).toBe(value);
      }
    }
  });

  it("成果物のキャッシュヘッダーを壊さない", async () => {
    const res = await fetchFrom(`http://localhost/releases/${TEST_RELEASE}/snap-index.json`);
    expect(res.status).toBe(200);
    expect(res.headers.get("Cache-Control")).toBe("public, max-age=31536000, immutable");
    expect(res.headers.get("ETag")).not.toBeNull();
    expect(res.headers.get("Content-Type")).toBe("application/json; charset=utf-8");
    expect(res.headers.get("X-Content-Type-Options")).toBe("nosniff");
  });

  it("304 Not Modified でも本文なしでヘッダーだけが付く", async () => {
    const first = await fetchFrom(`http://localhost/releases/${TEST_RELEASE}/snap-index.json`);
    const etag = first.headers.get("ETag");
    expect(etag).not.toBeNull();
    const res = await fetchFrom(`http://localhost/releases/${TEST_RELEASE}/snap-index.json`, {
      headers: { "If-None-Match": etag! },
    });
    expect(res.status).toBe(304);
    expect(await res.text()).toBe("");
    expect(res.headers.get("X-Content-Type-Options")).toBe("nosniff");
  });

  it("HSTS は HTTPS の応答にだけ付く", async () => {
    const [hstsName, hstsValue] = STRICT_TRANSPORT_SECURITY_HEADER;
    const plain = await fetchFrom(`http://localhost/releases/${TEST_RELEASE}/snap-index.json`);
    expect(plain.headers.get(hstsName)).toBeNull();

    const secure = await fetchFrom(
      `https://example.com/releases/${TEST_RELEASE}/snap-index.json`
    );
    expect(secure.headers.get(hstsName)).toBe(hstsValue);
  });

  it("アクセスログにクエリ・座標・検索語を残さない", async () => {
    const lines: string[] = [];
    const spy = vi.spyOn(console, "log").mockImplementation((...args: unknown[]) => {
      lines.push(args.map(String).join(" "));
    });
    try {
      const res = await fetchFrom(
        "http://localhost/api/geocode?q=%E6%9D%B1%E4%BA%AC%E9%A7%85&lat=35.6812&lon=139.7671",
        { method: "POST", body: JSON.stringify({ q: "東京駅" }) }
      );
      expect(res.status).not.toBe(404);
    } finally {
      spy.mockRestore();
    }

    expect(lines.length, "Worker must log exactly one structured line per request").toBeGreaterThan(0);
    for (const line of lines) {
      expect(line).not.toContain("東京駅");
      expect(line).not.toContain("35.6812");
      expect(line).not.toContain("139.7671");
      expect(line).not.toContain("q=");
      expect(line).toContain('"event"');
    }
  });
});
