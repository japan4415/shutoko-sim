// bench/ 配下の Node スクリプトから src/bench/*.ts を読み込むための薄いローダー。
//
// 集計（summarize.ts）と検証（envelope.ts）は計測ページと Node の双方から使う
// 単一のソースであり、二重実装すると定義がずれる。そこで Vite の TS 変換 API
// （Vite 8 は rolldown ベースなので `transformWithOxc`）でその場で ESM へ変換し、
// data: URL として動的 import する。
//
// - 追加依存を増やさない（esbuild / tsx は devDependencies に入れない）
// - 一時ファイルを作らない（worktree の外に書き込まない）
// - 変換対象は型 import だけの依存しか持たない純粋モジュールに限る
//   （data: URL には基底 URL が無いため、実行時の相対 import は解決できない）
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

/**
 * `src/bench/<name>.ts` を読み込み、そのモジュール名前空間を返す。
 * @param {string} relativePath bench/ からの相対パス（例: "../src/bench/summarize.ts"）
 */
export async function importTsModule(relativePath) {
  const fileUrl = new URL(relativePath, import.meta.url);
  const filename = fileURLToPath(fileUrl);
  const source = await readFile(filename, "utf8");
  const vite = await import("vite");
  const transform = vite.transformWithOxc ?? vite.transformWithEsbuild;
  if (typeof transform !== "function") {
    throw new Error(
      "vite の TS 変換 API（transformWithOxc / transformWithEsbuild）が見つかりません",
    );
  }
  const { code } = await transform(source, filename, { lang: "ts", loader: "ts", format: "esm" });
  const dataUrl = `data:text/javascript;base64,${Buffer.from(code).toString("base64")}`;
  return import(dataUrl);
}
