// Issue #98: cf dev / cf deploy が読む新設定系（cloudflare.config.ts と wrangler.config.ts）と、
// Workers Builds の deploy command（`npx wrangler deploy`）が読み続ける wrangler.toml の
// drift を止める。wrangler.toml を削除できるのは、deploy command を `cf deploy` へ切り替える
// Workers Builds のダッシュボード作業（外部作業）の後である。併存期間中はこのテストが
// 両設定が同じ Worker を定義していることを強制する。
import { expect, test } from "vitest";
import cloudflareConfig from "../cloudflare.config";
import wranglerConfig from "../wrangler.config";
import wranglerToml from "../wrangler.toml?raw";

function tomlValue(key: string): string {
  const match = new RegExp(`^${key}\\s*=\\s*"([^"]*)"`, "m").exec(wranglerToml);
  if (match === null) throw new Error(`workers/wrangler.toml has no ${key}`);
  return match[1];
}

/** 新設定の env を検査しやすいように緩く型付けした view。 */
type BindingView = {
  type: string;
  value?: string;
  name?: string;
  namespace?: string;
  simple?: { limit: number; period: number };
};

function cfEnv(): Record<string, BindingView> {
  return cloudflareConfig.worker.env as unknown as Record<string, BindingView>;
}

function tomlRateLimits(): Map<string, { limit: number; period: number }> {
  const map = new Map<string, { limit: number; period: number }>();
  const blockRe =
    /\[\[ratelimits\]\]\s*\nname = "([^"]+)"\s*\nnamespace_id = "([^"]+)"\s*\nsimple = \{ limit = (\d+), period = (\d+) \}/g;
  for (const match of wranglerToml.matchAll(blockRe)) {
    map.set(match[2], { limit: Number(match[3]), period: Number(match[4]) });
  }
  if (map.size === 0) throw new Error("workers/wrangler.toml has no [[ratelimits]]");
  return map;
}

test("cloudflare.config.ts defines the same worker as wrangler.toml", () => {
  const worker = cloudflareConfig.worker;
  expect(worker.name).toBe(tomlValue("name"));
  expect(worker.entrypoint).toBe(tomlValue("main"));
  expect(worker.compatibilityDate).toBe(tomlValue("compatibility_date"));

  const env = cfEnv();
  // env のキーは binding 名 / 変数名（wrangler.toml の binding と vars に対応する）。
  expect(Object.keys(env).sort()).toEqual([
    "ALLOWED_RELEASES",
    "ARTIFACTS_BUCKET",
    "GLOBAL_RATE_LIMITER",
    "IP_RATE_LIMITER",
  ]);

  // allowlist は片側だけ更新しない（rollback 先を狭めない）。
  expect(env.ALLOWED_RELEASES?.type).toBe("text");
  expect(env.ALLOWED_RELEASES?.value).toBe(tomlValue("ALLOWED_RELEASES"));

  // r2 binding は本番バケット（wrangler.toml の bucket_name）と一致する。
  expect(env.ARTIFACTS_BUCKET?.type).toBe("r2");
  expect(env.ARTIFACTS_BUCKET?.name).toBe(tomlValue("bucket_name"));

  // rate limit 2 件（IP 10 req/60s、Global 600 req/60s）を namespace_id で突き合わせる。
  const cfLimits = new Map<string, { limit: number; period: number }>();
  for (const binding of Object.values(env)) {
    if (binding.type === "rate-limit" && binding.namespace !== undefined) {
      cfLimits.set(binding.namespace, {
        limit: binding.simple?.limit ?? Number.NaN,
        period: binding.simple?.period ?? Number.NaN,
      });
    }
  }
  const sorted = (m: Map<string, { limit: number; period: number }>) =>
    [...m.entries()].sort(([a], [b]) => a.localeCompare(b));
  expect(sorted(cfLimits)).toEqual(sorted(tomlRateLimits()));

  // assets の扱い（SPA フォールバック）と配置先、dev ポートも共有する契約。
  expect(worker.assets.notFoundHandling).toBe(tomlValue("not_found_handling"));
  expect(wranglerConfig.assetsDirectory).toBe(tomlValue("directory"));
  expect(wranglerConfig.dev.port).toBe(8787);
});
