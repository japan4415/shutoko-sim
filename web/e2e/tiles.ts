// E2E の地図タイルは実ネットワーク（GSI）に依存させない。
//
// CI のランナーから GSI へ届かない・絞られることがあり、タイル失敗のフォールバックが
// 出発ボタンを無効化して 30 秒タイムアウトを起こしていた（2026-10-12 の web job）。
// 最小の PNG を返すスタブで、地図の描画・帰属表示・タイル読込イベントだけを決定的に
// 再現する。タイル失敗の経路そのものは (10) が abortTiles で明示的に検証する。
import type { Page } from "@playwright/test";

/** GSI のタイル URL。 */
export const TILE_URL = "**/cyberjapandata.gsi.go.jp/**";

/** 1x1 の PNG。Leaflet が tileload を発火させるのに十分な最小の画像。 */
const TINY_TILE_PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR4nGNgAAIAAAUAAXpeqz8AAAAASUVORK5CYII=",
  "base64",
);

/** タイルを常に成功させる（既定）。実タイルの代わりに最小の PNG を返す。 */
export async function stubTiles(page: Page): Promise<void> {
  await page.route(TILE_URL, (route) =>
    route.fulfill({ status: 200, contentType: "image/png", body: TINY_TILE_PNG }),
  );
}

/** タイルを全て失敗させ、地図のフォールバック状態を作る。 */
export async function abortTiles(page: Page): Promise<void> {
  await page.route(TILE_URL, (route) => route.abort());
}
