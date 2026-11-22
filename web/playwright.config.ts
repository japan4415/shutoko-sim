// E2E は `npm run e2e`（vite build → playwright test）で実行する。
// webServer は `workers/` の cf dev 1 台（cloudflare.config.ts の [assets] により静的 +
// /releases + /api が同一オリジン 8787 で配信される）。事前に
// `npm --prefix workers run seed:local` 済みとする。
// dev サーバを cf dev にしたのは Issue #98 の E2E 移行。seed（wrangler --local）が書く
// .wrangler/state は cf dev と共通なので、seed は wrangler のままでよい。
// workers/vitest-pool-workers 用の wrangler.toml は残り、本番 deploy は cf deploy（新設定系）が読む。
import { defineConfig } from "@playwright/test";

// 別の作業ツリーやアプリが既定ポートを使っている場合も、ローカル R2 で検証できるようにする。
const port = Number(process.env.SHUTOKO_E2E_PORT ?? 8787);
if (!Number.isInteger(port) || port < 1024 || port > 65535) {
  throw new Error("SHUTOKO_E2E_PORT must be an integer between 1024 and 65535");
}
const baseURL = `http://localhost:${String(port)}`;

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  expect: { timeout: 15_000 },
  reporter: [["list"]],
  use: {
    baseURL,
  },
  webServer: {
    // workers/node_modules/.bin の cf（package.json で固定）を使う。
    command: `npx cf dev --port ${String(port)}`,
    cwd: "../workers",
    url: `${baseURL}/releases/all-real-v5/manifest.json`,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
