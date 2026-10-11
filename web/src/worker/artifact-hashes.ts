// Web Worker が扱える releaseId の allowlist。
//
// 以前はここに wasm / glue の固定ハッシュを置いていたが、wasm のビルドは環境をまたいで
// バイト一致しない（ローカル macOS と CI の ubuntu で sha256 が変わる）ため廃止した。
// 改ざん検知に使う期待値は配信側の `releases/<releaseId>/engine.json` から取得する
// （生成は `workers/scripts/seed-local-r2.mjs`、配信は `workers/src/releases.ts`）。

export const KNOWN_RELEASES: readonly string[] = [
  "c1-real-v1",
  "c1-real-v2",
  "all-real-v1",
  "all-real-v2",
  "all-real-v3",
  "all-real-v4",
  "all-real-v5",
];
// 既定は最新 release。異常時は 1 行戻して all-real-v4（C2 ペア投入前の版）へ切り戻す。
// workers/wrangler.toml の ALLOWED_RELEASES に all-real-v4 を残したまま運用する。
// web/test/release-rollback.test.ts が Web の allowlist と Worker の allowlist の同期、
// および「既定の直前の版が rollback 先になる」関係を検査する。
export const DEFAULT_RELEASE_ID = "all-real-v5";
