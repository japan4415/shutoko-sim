import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

// 開発時は Vite(5173)、成果物と API は Cloudflare Worker(8787) へ server.proxy で中継する。
export default defineConfig({
  server: {
    proxy: {
      "/releases": "http://localhost:8787",
      "/api": "http://localhost:8787",
    },
  },
  build: {
    rollupOptions: {
      // 通常 UI（index.html）と計測ページ（bench.html）の 2 エントリを dist に出す。
      // 計測ページは #13 の性能計測専用で、通常 UI からは参照しない。
      input: {
        index: fileURLToPath(new URL("./index.html", import.meta.url)),
        bench: fileURLToPath(new URL("./bench.html", import.meta.url)),
      },
    },
  },
  test: {
    environment: "node",
    include: ["test/**/*.test.ts"],
  },
});
