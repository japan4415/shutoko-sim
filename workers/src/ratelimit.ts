import { Env } from "./types";

export interface RateLimitResult {
  allowed: boolean;
  retryAfter?: number;
}

export async function checkRateLimit(
  env: Env,
  request: Request
): Promise<RateLimitResult> {
  const ip = request.headers.get("CF-Connecting-IP") || "127.0.0.1";

  if (env.IP_RATE_LIMITER && typeof env.IP_RATE_LIMITER.limit === "function") {
    const ipRes = await env.IP_RATE_LIMITER.limit({ key: ip });
    if (!ipRes.success) {
      return { allowed: false, retryAfter: 60 };
    }
  }

  if (env.GLOBAL_RATE_LIMITER && typeof env.GLOBAL_RATE_LIMITER.limit === "function") {
    const globalRes = await env.GLOBAL_RATE_LIMITER.limit({ key: "global" });
    if (!globalRes.success) {
      return { allowed: false, retryAfter: 60 };
    }
  }

  return { allowed: true };
}
