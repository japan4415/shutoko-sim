import { defineConfig } from "vitest/config";

// 開発時は Vite(5173)、成果物と API は Cloudflare Worker(8787) へ server.proxy で中継する。
export default defineConfig({
  server: {
    proxy: {
      "/releases": "http://localhost:8787",
      "/api": "http://localhost:8787",
    },
  },
  test: {
    environment: "node",
    include: ["test/**/*.test.ts"],
  },
});
