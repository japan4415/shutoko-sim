// cf dev / cf deploy の tooling 設定（新設定系の wrangler.config.ts）。
// Worker 定義（bindings・assets の扱い）は cloudflare.config.ts に置き、このファイルは
// ビルドと開発サーバの道具だけを持つ。assets の置き場所、型生成、dev ポートはここに置く
// （新設定系では Worker 定義側に assets の directory / dev の port を置かない）。
// 新設定系は実験的でスキーマが版ごとに動く（wrangler 4.136 では dev.types だったものが
// 4.149 では top-level types になった）。cf と wrangler は package.json で固定する。
export default {
  assetsDirectory: "../web/dist",
  types: { generate: false },
  dev: { port: 8787 },
};
