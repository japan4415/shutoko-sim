// Leaflet を import する唯一のファイル。地図描画の副作用は createMapView 呼び出し時だけに閉じ、
// モジュール import 自体は副作用を持たない（Vitest から安全に読み込めるようにする）。
// 座標は GeoJSON が [lon, lat]、Leaflet が [lat, lon] のため、境界で必ず反転する。
import L from "leaflet";
import type { Candidate } from "../worker/types";
import { deriveSegments } from "./segments";
import type { Coords } from "./segments";
import { DEFAULT_TILE_SOURCE, tileAttribution } from "./tiles";
import type { TileSource } from "./tiles";

export type { Coords } from "./segments";

/** Leaflet 地図の操作インターフェース。DOM 直操作を main.ts から隠蔽する。 */
export interface MapView {
  /** 出発地点マーカーを設置・移動する（重複生成しない）。 */
  setOrigin(p: { lat: number; lon: number }): void;
  /** 候補の経路を描画する。既存の候補レイヤーは破棄する。 */
  renderCandidates(candidates: Candidate[]): void;
  /** 候補を強調表示する。null で全候補を基準スタイルへ戻す。 */
  selectCandidate(id: string | null): void;
  /** 全候補（無ければ出発地点）に地図をフィットさせる。 */
  fitToCandidates(): void;
  /** タイル読み込み失敗のコールバックを登録する（地図表示失敗からの復帰用）。 */
  onTileError(cb: (e: unknown) => void): void;
  /** タイル読み込み成功のコールバックを登録する（失敗状態の解除用）。 */
  onTileLoad(cb: () => void): void;
  /** ビューポートを再評価しタイルを再要求させる（地図復帰用）。 */
  invalidateSize(): void;
  /** イベント購読と地図インスタンスを破棄する。 */
  destroy(): void;
}

/** 線の役割。選択・非選択のスタイル切り替えを役割単位で行う。 */
type SegmentRole = "access" | "loop" | "return";

/** 候補 1 件分の描画レイヤー（基準スタイルを再適用できるよう保持する）。 */
interface CandidateLayer {
  id: string;
  /** 役割ごとの線。 */
  lines: { role: SegmentRole; line: L.Polyline }[];
  /** 課金対象区間。選択状態に依らず常時強調する。特定できなければ null。 */
  charged: L.Polyline | null;
  /** 経路全体の外接矩形算出に使う。 */
  bounds: L.LatLngBounds;
}

// 基準スタイル（非選択）。色に依存せず選択可否が分かるよう、選択時は太さ・不透明度も変える。
const BASE_STYLE: Record<SegmentRole, L.PathOptions> = {
  access: { color: "#5a5f66", weight: 3, opacity: 0.55 },
  loop: { color: "#1c1e21", weight: 5, opacity: 0.7 },
  return: { color: "#5a5f66", weight: 3, opacity: 0.55, dashArray: "6 6" },
};
const SELECTED_STYLE: Record<SegmentRole, L.PathOptions> = {
  access: { color: "#0b5cad", weight: 6, opacity: 1 },
  loop: { color: "#0b5cad", weight: 8, opacity: 1 },
  return: { color: "#0b5cad", weight: 4, opacity: 0.9, dashArray: "6 6" },
};
// 課金対象は色と破線の二重符号化で示す（色覚多様性に配慮）。
const CHARGED_STYLE: L.PathOptions = { color: "#c2410c", weight: 6, opacity: 0.95, dashArray: "2 8" };
const CHARGED_DIM_STYLE: L.PathOptions = { ...CHARGED_STYLE, opacity: 0.45 };
const DIM_STYLE: Record<SegmentRole, L.PathOptions> = {
  access: { ...BASE_STYLE.access, opacity: 0.18 },
  loop: { ...BASE_STYLE.loop, opacity: 0.25 },
  return: { ...BASE_STYLE.return, opacity: 0.18 },
};

/** [lon, lat] を Leaflet の [lat, lon] へ反転する。 */
function toLatLngs(coords: Coords[]): L.LatLngExpression[] {
  return coords.map(([lon, lat]) => [lat, lon]);
}

/** 空の bounds に座標列を加える。 */
function extendBounds(bounds: L.LatLngBounds, latlngs: L.LatLngExpression[]): void {
  for (const latlng of latlngs) {
    bounds.extend(latlng);
  }
}

/** 地図を生成する。コンテナ要素以外の DOM グローバルには触れない。 */
export function createMapView(
  container: HTMLElement,
  options?: { tileSource?: TileSource },
): MapView {
  const source = options?.tileSource ?? DEFAULT_TILE_SOURCE;
  const map = L.map(container, { attributionControl: false, zoomControl: true });
  // 探索前から地図を見せ、タイル取得を開始するための初期表示（東京中心）。
  map.setView([35.6812, 139.7671], 12);

  // 帰属表示は常時可視。タイル提供元の帰属と OSM 帰属を併記する（docs/interfaces.md:169）。
  // レイヤー依存で消えないよう、固定文言の帰属コントロールを明示的に設置する。
  L.control
    .attribution({ position: "bottomright", prefix: false })
    .addAttribution(tileAttribution(source))
    .addTo(map);

  const tileLayer = L.tileLayer(source.url, {
    maxZoom: source.maxZoom,
    attribution: tileAttribution(source),
    ...(source.subdomains !== undefined ? { subdomains: source.subdomains } : {}),
  });
  tileLayer.addTo(map);

  const tileErrorCallbacks: ((e: unknown) => void)[] = [];
  const tileLoadCallbacks: (() => void)[] = [];
  tileLayer.on("tileerror", (event: L.TileErrorEvent) => {
    for (const cb of tileErrorCallbacks) {
      cb(event);
    }
  });
  tileLayer.on("tileload", () => {
    for (const cb of tileLoadCallbacks) {
      cb();
    }
  });

  const originMarkerKey = "origin";
  const markers = new Map<string, L.Marker>();
  let candidateLayers: CandidateLayer[] = [];
  let selectedId: string | null = null;

  function applyStyles(): void {
    const hasSelection = selectedId !== null;
    for (const layer of candidateLayers) {
      const selected = layer.id === selectedId;
      for (const { role, line } of layer.lines) {
        line.setStyle(
          selected ? SELECTED_STYLE[role] : hasSelection ? DIM_STYLE[role] : BASE_STYLE[role],
        );
      }
      // 課金区間は選択に関わらず常時強調する。非選択の候補ではやや抑える。
      if (layer.charged !== null) {
        layer.charged.setStyle(selected || !hasSelection ? CHARGED_STYLE : CHARGED_DIM_STYLE);
      }
    }
  }

  function setOrigin(p: { lat: number; lon: number }): void {
    const existing = markers.get(originMarkerKey);
    if (existing !== undefined) {
      existing.setLatLng([p.lat, p.lon]);
      return;
    }
    // Leaflet 既定のマーカー画像は Vite のバンドルで URL が壊れるため、CSS の divIcon を使う。
    const marker = L.marker([p.lat, p.lon], {
      title: "出発地",
      icon: L.divIcon({
        className: "origin-marker",
        iconSize: [18, 18],
        iconAnchor: [9, 9],
        html: '<span class="origin-marker__dot" aria-hidden="true"></span>',
      }),
    });
    marker.bindTooltip("出発地", { direction: "top" });
    marker.addTo(map);
    markers.set(originMarkerKey, marker);
  }

  function renderCandidates(candidates: Candidate[]): void {
    for (const layer of candidateLayers) {
      if (layer.charged !== null) {
        map.removeLayer(layer.charged);
      }
      for (const { line } of layer.lines) {
        map.removeLayer(line);
      }
    }
    candidateLayers = [];

    for (const candidate of candidates) {
      const segments = deriveSegments(candidate);
      const bounds = L.latLngBounds([]);
      const lines: { role: SegmentRole; line: L.Polyline }[] = [];

      // access・loop・return を役割付きで描く。スタイルは applyStyles が一括で決める。
      const parts: { role: SegmentRole; coords: Coords[] }[] = [
        { role: "access", coords: segments.access },
        { role: "loop", coords: segments.loop },
        { role: "return", coords: segments.return },
      ];
      for (const { role, coords } of parts) {
        if (coords.length < 2) {
          continue;
        }
        const latlngs = toLatLngs(coords);
        const line = L.polyline(latlngs, BASE_STYLE[role]);
        line.addTo(map);
        lines.push({ role, line });
        extendBounds(bounds, latlngs);
      }
      // 区間分解に失敗した場合は main（全経路）を 1 本の基準線として描く。
      if (lines.length === 0 && segments.main.length >= 2) {
        const latlngs = toLatLngs(segments.main);
        const line = L.polyline(latlngs, BASE_STYLE.loop);
        line.addTo(map);
        lines.push({ role: "loop", line });
        extendBounds(bounds, latlngs);
      }

      let charged: L.Polyline | null = null;
      if (segments.charged.length >= 2) {
        const chargedLatLngs = toLatLngs(segments.charged);
        charged = L.polyline(chargedLatLngs, CHARGED_STYLE);
        charged.addTo(map);
        charged.bindTooltip("課金対象 1区間", { sticky: true });
        extendBounds(bounds, chargedLatLngs);
      }

      candidateLayers.push({ id: candidate.id, lines, charged, bounds });
    }

    applyStyles();
  }

  function selectCandidate(id: string | null): void {
    selectedId = id;
    applyStyles();
    if (id !== null) {
      const layer = candidateLayers.find((l) => l.id === id);
      layer?.charged?.openTooltip();
    }
  }

  function fitToCandidates(): void {
    const bounds = L.latLngBounds([]);
    for (const layer of candidateLayers) {
      if (layer.bounds.isValid()) {
        bounds.extend(layer.bounds);
      }
    }
    const origin = markers.get(originMarkerKey);
    if (origin !== undefined) {
      bounds.extend(origin.getLatLng());
    }
    if (bounds.isValid()) {
      map.fitBounds(bounds, { padding: [24, 24] });
    }
  }

  function onTileError(cb: (e: unknown) => void): void {
    tileErrorCallbacks.push(cb);
  }

  function onTileLoad(cb: () => void): void {
    tileLoadCallbacks.push(cb);
  }

  function invalidateSize(): void {
    map.invalidateSize();
  }

  function destroy(): void {
    tileErrorCallbacks.length = 0;
    tileLoadCallbacks.length = 0;
    candidateLayers = [];
    markers.clear();
    map.off();
    map.remove();
  }

  return {
    setOrigin,
    renderCandidates,
    selectCandidate,
    fitToCandidates,
    onTileError,
    onTileLoad,
    invalidateSize,
    destroy,
  };
}
