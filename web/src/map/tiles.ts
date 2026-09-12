// 地図タイルの提供元定義。Leaflet には依存せず、URL と帰属表記だけをここへ集約する。
// 帰属表示は docs/interfaces.md:167-171（地図・住所検索のデータ利用）に従い常時可視とする。

/** 地図タイルの提供元 1 件分。 */
export interface TileSource {
  /** 表示名（デバッグ・フォールバック判定用）。 */
  name: string;
  /** Leaflet の L.tileLayer に渡す URL テンプレート。 */
  url: string;
  /** このタイル提供元固有の帰属表記（HTML 可）。OSM 帰属は別途併記する。 */
  attribution: string;
  /** タイルが存在する最大ズーム。 */
  maxZoom: number;
  /** {s} の代替サブドメイン（OSM の a/b/c 等）。未指定なら Leaflet 既定。 */
  subdomains?: string;
}

/**
 * 国土地理院（GSI）標準地図。日本国内の道路確認に適し、既定のタイル提供元とする。
 * 国土地理院コンテンツ利用規約に基づき出典を表示する。
 */
export const GSI_TILE_SOURCE: TileSource = {
  name: "GSI 標準地図",
  url: "https://cyberjapandata.gsi.go.jp/xyz/std/{z}/{x}/{y}.png",
  attribution:
    '&copy; <a href="https://maps.gsi.go.jp/development/ichiran.html" target="_blank" rel="noopener">国土地理院</a>',
  maxZoom: 18,
};

/** OSM 標準タイル。tile.openstreetmap.org の a/b/c サブドメインを使う代替提供元。 */
export const OSM_TILE_SOURCE: TileSource = {
  name: "OpenStreetMap",
  url: "https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png",
  attribution:
    '&copy; <a href="https://www.openstreetmap.org/copyright" target="_blank" rel="noopener">OpenStreetMap</a> contributors',
  maxZoom: 19,
  subdomains: "abc",
};

/**
 * OpenStreetMap contributors の帰属表記。地図上に常時表示する義務があるため、
 * どのタイル提供元を使う場合でも地図へ併記する（docs/interfaces.md:169）。
 */
export const OSM_ATTRIBUTION =
  '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors';

/**
 * 利用可能なタイル提供元の一覧。先頭（GSI）を既定として使う。
 * 大量先読み・オフライン取得は行わない（docs/interfaces.md:171）。
 */
export const TILE_SOURCES: readonly TileSource[] = [GSI_TILE_SOURCE, OSM_TILE_SOURCE];

/** 既定のタイル提供元。地図生成時に指定が無ければこれを使う。 */
export const DEFAULT_TILE_SOURCE: TileSource = GSI_TILE_SOURCE;

/** タイル提供元の帰属と OSM 帰属を重複なく連結する。 */
export function tileAttribution(source: TileSource): string {
  return source.attribution.includes("OpenStreetMap")
    ? source.attribution
    : `${source.attribution} | ${OSM_ATTRIBUTION}`;
}
