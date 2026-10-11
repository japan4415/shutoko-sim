// E2E は `npm run e2e`（vite build → playwright test）で実行する。
// webServer は `workers/` の wrangler dev 1 台（[assets] により静的 + /releases + /api が
// 同一オリジン 8787 で配信される）。事前に `npm --prefix workers run seed:local` 済みとする。
// dev サーバを wrangler に残すのは意図的である（Issue #98）: `cf dev` は新設定系の
// cloudflare.config.ts を要求して workers/wrangler.toml を読めないため、E2E の seed も
// 同じ wrangler のローカル R2 状態を書く。理由の詳細は docs/delivery.md の runbook 2-4。
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  expect: { timeout: 15_000 },
  reporter: [["list"]],
  use: {
    baseURL: "http://localhost:8787",
  },
  webServer: {
    command: "npx wrangler dev --port 8787",
    cwd: "../workers",
    url: "http://localhost:8787/releases/all-real-v5/manifest.json",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
