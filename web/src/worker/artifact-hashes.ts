// Web Worker が扱える releaseId の allowlist。
//
// 以前はここに wasm / glue の固定ハッシュを置いていたが、wasm のビルドは環境をまたいで
// バイト一致しない（ローカル macOS と CI の ubuntu で sha256 が変わる）ため廃止した。
// 改ざん検知に使う期待値は配信側の `releases/<releaseId>/engine.json` から取得する
// （生成は `workers/scripts/seed-local-r2.mjs`、配信は `workers/src/releases.ts`）。

export const KNOWN_RELEASES: readonly string[] = ["c1-real-v1"];
