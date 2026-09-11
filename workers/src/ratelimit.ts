import { Env } from "./types";

export type RateLimitResult =
  | { status: "allowed" }
  | { status: "limited"; retryAfter: number }
  | { status: "unavailable" };

export async function checkRateLimit(
  env: Env,
  request: Request
): Promise<RateLimitResult> {
  if (!env.IP_RATE_LIMITER || typeof env.IP_RATE_LIMITER.limit !== "function") {
    return { status: "unavailable" };
  }

  if (!env.GLOBAL_RATE_LIMITER || typeof env.GLOBAL_RATE_LIMITER.limit !== "function") {
    return { status: "unavailable" };
  }

  const ip = request.headers.get("CF-Connecting-IP") || "127.0.0.1";

  try {
    const ipRes = await env.IP_RATE_LIMITER.limit({ key: ip });
    if (!ipRes.success) {
      return { status: "limited", retryAfter: 60 };
    }
  } catch {
    return { status: "unavailable" };
  }

  try {
    const globalRes = await env.GLOBAL_RATE_LIMITER.limit({ key: "global" });
    if (!globalRes.success) {
      return { status: "limited", retryAfter: 60 };
    }
  } catch {
    return { status: "unavailable" };
  }

  return { status: "allowed" };
}
