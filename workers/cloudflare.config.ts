// Cloudflare の新設定系（--experimental-new-config）で cf dev / cf deploy が読む
// Worker 定義。wrangler.toml（Workers Builds の deploy command が読み続ける旧設定）と
// 併存し、両者の一致は workers/test/cloudflare-config.test.ts が検査する。
// wrangler.toml を削除できるのは、Workers Builds の deploy command を `cf deploy` へ
// 切り替えるダッシュボード作業（外部作業）の後である。
export default {
  worker: {
    name: "shutoko-sim-workers",
    entrypoint: "src/index.ts",
    compatibilityDate: "2026-08-22",
    env: {
      // all-real-v4 以前を allowlist に残すのは rollback 用。Web の既定（web/src/worker/
      // artifact-hashes.ts の DEFAULT_RELEASE_ID）を 1 行戻すだけで配信を戻せるようにする。
      ALLOWED_RELEASES: {
        type: "text",
        value:
          "c1-real-v1,c1-real-v2,all-real-v1,all-real-v2,all-real-v3,all-real-v4,all-real-v5",
      },
      // 新設定系に preview_bucket_name は無い。ローカル（cf dev の R2 シミュレーション）は
      // この bucket 名を Miniflare 状態の名前空間として使い、seed:local（wrangler --local）
      // が同じ名前へ書くので、そのまま読める。本番は R2 の shutoko-artifacts バケット。
      ARTIFACTS_BUCKET: { type: "r2", name: "shutoko-artifacts" },
      IP_RATE_LIMITER: { type: "rate-limit", namespace: "1001", simple: { limit: 10, period: 60 } },
      GLOBAL_RATE_LIMITER: {
        type: "rate-limit",
        namespace: "1002",
        simple: { limit: 600, period: 60 },
      },
    },
    assets: { notFoundHandling: "single-page-application" },
  },
};
