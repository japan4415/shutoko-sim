// Leaflet を import する唯一のファイル。地図描画の副作用は createMapView 呼び出し時だけに閉じ、
// モジュール import 自体は副作用を持たない（Vitest から安全に読み込めるようにする）。
// 座標は GeoJSON が [lon, lat]、Leaflet が [lat, lon] のため、境界で必ず反転する。
import L from "leaflet";
import type { Candidate } from "../worker/types";
import { fitBoundsAnimation, planOriginFocus } from "./focus";
import { deriveSegments } from "./segments";
import type { Coords } from "./segments";
import { DEFAULT_TILE_SOURCE, tileAttribution } from "./tiles";
import type { TileSource } from "./tiles";

export type { Coords } from "./segments";
export { FOCUS_ORIGIN_MIN_ZOOM, planOriginFocus, fitBoundsAnimation } from "./focus";
export type { OriginFocusPlan } from "./focus";

/** Leaflet 地図の操作インターフェース。DOM 直操作を main.ts から隠蔽する。 */
export interface MapView {
  /** 出発地点マーカーを設置・移動する（重複生成しない）。 */
  setOrigin(p: { lat: number; lon: number }): void;
  /**
   * 出発地点が現在の表示範囲外なら pan/zoom して位置を見せる。
   * 範囲内なら動かさない。prefers-reduced-motion ではアニメーションしない。
   */
  focusOrigin(p: { lat: number; lon: number }): void;
  /** 候補の経路を描画する。既存の候補レイヤーは破棄する。 */
  renderCandidates(candidates: Candidate[]): void;
  /** 候補を強調表示する。null で全候補を基準スタイルへ戻す。 */
  selectCandidate(id: string | null): void;
  /** 全候補（無ければ出発地点）に地図をフィットさせる。 */
  fitToCandidates(): void;
  /**
   * 地図タップで座標を拾うモードを切り替える。
   * Leaflet はドラッグ後に click を発火しないため、パン・ズームと競合しない。
   */
  setPickMode(enabled: boolean): void;
  /** タップで選んだ確定前の地点を表示する。null で消す。 */
  setPendingOrigin(p: { lat: number; lon: number } | null): void;
  /** タップ座標の通知先を登録する（複数登録可）。 */
  onPickOrigin(cb: (p: { lat: number; lon: number }) => void): void;
  /** タイル読み込み失敗のコールバックを登録する（地図表示失敗からの復帰用）。 */
  onTileError(cb: (e: unknown) => void): void;
  /** タイル読み込み成功のコールバックを登録する（失敗状態の解除用）。 */
  onTileLoad(cb: () => void): void;
  /** タイルレイヤーを作り直して再取得させる（地図復帰用）。 */
  reloadTiles(): void;
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
  options?: {
    tileSource?: TileSource;
    /** モーション低減設定の読み取り。省略時は matchMedia を参照する。 */
    prefersReducedMotion?: () => boolean;
  },
): MapView {
  const source = options?.tileSource ?? DEFAULT_TILE_SOURCE;
  const reducedMotion =
    options?.prefersReducedMotion ??
    (() =>
      typeof window !== "undefined" &&
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  const map = L.map(container, { attributionControl: false, zoomControl: true });
  // 探索前から地図を見せ、タイル取得を開始するための初期表示（東京中心）。
  map.setView([35.6812, 139.7671], 12);

  // 帰属表示は常時可視。タイル提供元の帰属と OSM 帰属を併記する（docs/interfaces.md:169）。
  // レイヤー依存で消えないよう、固定文言の帰属コントロールを明示的に設置する。
  L.control
    .attribution({ position: "bottomright", prefix: false })
    .addAttribution(tileAttribution(source))
    .addTo(map);

  const tileErrorCallbacks: ((e: unknown) => void)[] = [];
  const tileLoadCallbacks: (() => void)[] = [];

  let tileLayer: L.TileLayer | null = null;

  function attachTiles(): L.TileLayer {
    const layer = L.tileLayer(source.url, {
      maxZoom: source.maxZoom,
      attribution: tileAttribution(source),
      ...(source.subdomains !== undefined ? { subdomains: source.subdomains } : {}),
    });
    layer.on("tileerror", (event: L.TileErrorEvent) => {
      for (const cb of tileErrorCallbacks) {
        cb(event);
      }
    });
    layer.on("tileload", () => {
      for (const cb of tileLoadCallbacks) {
        cb();
      }
    });
    layer.addTo(map);
    tileLayer = layer;
    return layer;
  }
  attachTiles();

  const originMarkerKey = "origin";
  const markers = new Map<string, L.Marker>();
  let candidateLayers: CandidateLayer[] = [];
  let selectedId: string | null = null;
  // 地図タップで座標を拾うモード。有効なときだけ click を pending として通知する。
  let pickModeEnabled = false;
  const pickCallbacks: ((p: { lat: number; lon: number }) => void)[] = [];
  let pendingMarker: L.Marker | null = null;

  // Leaflet はパン/ズーム操作の後続 click を抑制するため、ドラッグと競合しない。
  map.on("click", (event: L.LeafletMouseEvent) => {
    if (!pickModeEnabled) {
      return;
    }
    const point = { lat: event.latlng.lat, lon: event.latlng.lng };
    for (const cb of pickCallbacks) {
      cb(point);
    }
  });

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

  /**
   * 確定した出発地点が表示範囲外のときだけ pan/zoom する。
   * 既に見えている場合は動かさず、reduced-motion ではアニメーションしない。
   */
  function focusOrigin(p: { lat: number; lon: number }): void {
    const latlng = L.latLng(p.lat, p.lon);
    const plan = planOriginFocus(map.getBounds().contains(latlng), map.getZoom(), reducedMotion());
    if (plan === null) {
      return;
    }
    if (plan.action === "setView") {
      map.setView(latlng, plan.zoom, { animate: plan.animate });
    } else {
      map.panTo(latlng, { animate: plan.animate });
    }
  }

  /** タップで選んだ確定前の地点を表示する（出発地マーカーとは別の見た目にする）。 */
  function setPendingOrigin(p: { lat: number; lon: number } | null): void {
    if (p === null) {
      if (pendingMarker !== null) {
        pendingMarker.remove();
        pendingMarker = null;
      }
      return;
    }
    if (pendingMarker !== null) {
      pendingMarker.setLatLng([p.lat, p.lon]);
      return;
    }
    pendingMarker = L.marker([p.lat, p.lon], {
      title: "候補地点",
      icon: L.divIcon({
        className: "pending-marker",
        iconSize: [22, 22],
        iconAnchor: [11, 11],
        html: '<span class="pending-marker__ring" aria-hidden="true"></span>',
      }),
    });
    pendingMarker.bindTooltip("候補地点（未確定）", { direction: "top" });
    pendingMarker.addTo(map);
  }

  function setPickMode(enabled: boolean): void {
    pickModeEnabled = enabled;
    container.classList.toggle("map--pick", enabled);
    if (enabled) {
      // タッチ端末にはカーソルが無いため、モード中は枠でも状態を示す（design F6）。
      // ダブルタップズームは 1 回目のタップで pending を置いた直後にズームし
      // 意図しない地点が残るため、モード中は無効化する（design F11）。
      map.doubleClickZoom.disable();
    } else {
      map.doubleClickZoom.enable();
    }
  }

  function onPickOrigin(cb: (p: { lat: number; lon: number }) => void): void {
    pickCallbacks.push(cb);
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
      // prefers-reduced-motion では fitBounds のアニメーションも止める（design F4）。
      map.fitBounds(bounds, { padding: [24, 24], ...fitBoundsAnimation(reducedMotion()) });
    }
  }

  function onTileError(cb: (e: unknown) => void): void {
    tileErrorCallbacks.push(cb);
  }

  function onTileLoad(cb: () => void): void {
    tileLoadCallbacks.push(cb);
  }

  function reloadTiles(): void {
    // 既存レイヤーを破棄して作り直すことで、キャッシュ済みの失敗タイルに依らず再取得させる。
    if (tileLayer !== null) {
      tileLayer.remove();
    }
    attachTiles();
  }

  function destroy(): void {
    tileErrorCallbacks.length = 0;
    tileLoadCallbacks.length = 0;
    pickCallbacks.length = 0;
    pickModeEnabled = false;
    candidateLayers = [];
    markers.clear();
    pendingMarker = null;
    map.off();
    map.remove();
  }

  return {
    setOrigin,
    focusOrigin,
    renderCandidates,
    selectCandidate,
    fitToCandidates,
    setPickMode,
    setPendingOrigin,
    onPickOrigin,
    onTileError,
    onTileLoad,
    reloadTiles,
    destroy,
  };
}
