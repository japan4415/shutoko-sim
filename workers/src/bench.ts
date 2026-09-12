import { createErrorResponse } from "./errors";
import { checkRateLimit } from "./ratelimit";
import { Env, HandlerResult } from "./types";

const MAX_BENCH_PAYLOAD_BYTES = 8 * 1024 * 1024;
const MAX_KEY_ATTEMPTS = 5;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** device.deviceName → device.browser の順でスラッグを作る（パストラバーサル防止）。 */
function deviceSlug(device: Record<string, unknown>): string {
  const raw = typeof device.deviceName === "string" ? device.deviceName : device.browser;
  const slug =
    typeof raw === "string" ? raw.replace(/[^a-zA-Z0-9_-]/g, "").slice(0, 32) : "";
  return slug === "" ? "unknown" : slug;
}

function buildBaseKey(device: Record<string, unknown>): string {
  const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
  return `bench/${timestamp}-${deviceSlug(device)}.json`;
}

/** 同秒・同端末の衝突を避けるため、存在すれば -1, -2 … を試す。 */
async function resolveKey(bucket: R2Bucket, baseKey: string): Promise<string> {
  for (let attempt = 0; attempt <= MAX_KEY_ATTEMPTS; attempt++) {
    const key =
      attempt === 0 ? baseKey : baseKey.replace(/\.json$/, `-${String(attempt)}.json`);
    const existing = await bucket.head(key);
    if (existing === null) {
      return key;
    }
  }
  // 全候補が衝突する場合は UUID を添えて一意にする（既存オブジェクトを上書きしない）。
  return baseKey.replace(/\.json$/, `-${crypto.randomUUID()}.json`);
}

export async function handleBenchResult(
  request: Request,
  env: Env
): Promise<HandlerResult> {
  if (request.method !== "POST") {
    return {
      response: createErrorResponse("METHOD_NOT_ALLOWED", 405, false),
      logMeta: { errorCode: "METHOD_NOT_ALLOWED" },
    };
  }

  const contentLength = request.headers.get("content-length");
  const contentLengthValue = contentLength === null ? NaN : Number(contentLength);
  if (Number.isFinite(contentLengthValue) && contentLengthValue > MAX_BENCH_PAYLOAD_BYTES) {
    return {
      response: createErrorResponse("PAYLOAD_TOO_LARGE", 400, false),
      logMeta: { errorCode: "PAYLOAD_TOO_LARGE" },
    };
  }

  // ボディを読む前にレート制限を評価する（不正ペイロードによる無制限な読込を防ぐ）。
  const rateLimitResult = await checkRateLimit(env, request);
  if (rateLimitResult.status === "unavailable") {
    return {
      response: createErrorResponse("RATE_LIMITER_UNAVAILABLE", 503, true),
      logMeta: { errorCode: "RATE_LIMITER_UNAVAILABLE" },
    };
  }
  if (rateLimitResult.status === "limited") {
    return {
      response: createErrorResponse("RATE_LIMITED", 429, true, {
        "Retry-After": String(rateLimitResult.retryAfter),
      }),
      logMeta: { errorCode: "RATE_LIMITED" },
    };
  }

  let arrayBuffer: ArrayBuffer;
  try {
    arrayBuffer = await request.arrayBuffer();
  } catch {
    return {
      response: createErrorResponse("INVALID_BENCH_PAYLOAD", 400, false),
      logMeta: { errorCode: "INVALID_BENCH_PAYLOAD" },
    };
  }

  if (arrayBuffer.byteLength > MAX_BENCH_PAYLOAD_BYTES) {
    return {
      response: createErrorResponse("PAYLOAD_TOO_LARGE", 400, false),
      logMeta: { errorCode: "PAYLOAD_TOO_LARGE" },
    };
  }

  const text = new TextDecoder().decode(arrayBuffer);
  let body: unknown;
  try {
    body = JSON.parse(text);
  } catch {
    return {
      response: createErrorResponse("INVALID_BENCH_PAYLOAD", 400, false),
      logMeta: { errorCode: "INVALID_BENCH_PAYLOAD" },
    };
  }

  // web 側の型定義とは別パッケージのため、最小限の構造検証だけ行う。
  if (
    !isRecord(body) ||
    !Number.isInteger(body.schemaVersion) ||
    body.schemaVersion !== 1 ||
    !isRecord(body.device) ||
    !Array.isArray(body.trials)
  ) {
    return {
      response: createErrorResponse("INVALID_BENCH_PAYLOAD", 400, false),
      logMeta: { errorCode: "INVALID_BENCH_PAYLOAD" },
    };
  }

  if (!env.ARTIFACTS_BUCKET) {
    return {
      response: createErrorResponse("INTERNAL_ERROR", 500, false),
      logMeta: { errorCode: "INTERNAL_ERROR" },
    };
  }

  const baseKey = buildBaseKey(body.device);
  const key = await resolveKey(env.ARTIFACTS_BUCKET, baseKey);
  const byteLength = arrayBuffer.byteLength;

  // 受信した生の arrayBuffer を保存する（改行・フォーマットを保つため再 stringify しない）。
  try {
    await env.ARTIFACTS_BUCKET.put(key, arrayBuffer, {
      httpMetadata: { contentType: "application/json; charset=utf-8" },
    });
  } catch {
    // R2 の一時障害。再試行可能として扱い、構造化ログへ到達させる。
    return {
      response: createErrorResponse("INTERNAL_ERROR", 500, true),
      logMeta: { errorCode: "INTERNAL_ERROR" },
    };
  }

  return {
    response: new Response(JSON.stringify({ key, bytes: byteLength }), {
      status: 200,
      headers: {
        "Content-Type": "application/json; charset=utf-8",
        "Cache-Control": "no-store",
      },
    }),
    logMeta: { benchmarkKey: key, benchmarkBytes: byteLength },
  };
}
