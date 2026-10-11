// E2E は `npm run e2e`（vite build → playwright test）で実行する。
// webServer は `workers/` の cf dev 1 台（cloudflare.config.ts の [assets] により静的 +
// /releases + /api が同一オリジン 8787 で配信される）。事前に
// `npm --prefix workers run seed:local` 済みとする。
// dev サーバを cf dev にしたのは Issue #98 の E2E 移行。seed（wrangler --local）が書く
// .wrangler/state は cf dev と共通なので、seed は wrangler のままでよい。
// wrangler.toml は Workers Builds の deploy command（旧設定）が読み続けるため残す。
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
    // workers/node_modules/.bin の cf（package.json で固定した 1.0.0-beta.14）を使う。
    command: "npx cf dev --port 8787",
    cwd: "../workers",
    url: "http://localhost:8787/releases/all-real-v5/manifest.json",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
