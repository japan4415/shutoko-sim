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

/** [`gateTiles`] のゲート。`allow` を true にすると以降のタイル要求が成功する。 */
export interface TileGate {
  allow: boolean;
}

/**
 * 「ゲートが開くまで失敗、開いたらスタブ PNG を返す」ルートを 1 本だけ登録する。
 *
 * abort → stub の切り替えを `unrouteAll` で行うと、解除した瞬間に実タイル（GSI）が読めて
 * アプリが自然回復し、再読み込みボタンが消えた状態で `click` が待ち続けてテスト全体が
 * タイムアウトする（(10) が 2026-10-13 に CI とローカルで再現）。1 本のルート内で
 * 切り替えれば、その隙間が存在しない。
 */
export async function gateTiles(page: Page, gate: TileGate): Promise<void> {
  await page.route(TILE_URL, (route) => {
    if (!gate.allow) {
      return route.abort();
    }
    return route.fulfill({ status: 200, contentType: "image/png", body: TINY_TILE_PNG });
  });
}
