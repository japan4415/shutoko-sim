// MachGL 先行版プレビューのブリッジ。
//
// このファイルは scripts/prepare-machgl-preview.mjs が web/public/machgl-preview/ へ
// コピーする。デモのバンドルが window.__machglMap を作るのを待ち、アプリから
// localStorage 経由で渡されたルートを GeoJSON として重ね、走行プレビューを動かす。
//
// 正式版の MachGL が出たら、バンドルの取得元を公式 CDN に差し替えればこのファイルは
// そのまま使える（window.__machglMap の露出だけは不要になる）。
(() => {
  "use strict";

  const STORAGE_PREFIX = "machgl-preview:";
  const CHANNEL = "machgl-preview";
  const MAP_TIMEOUT_MS = 20000;
  const FLY_STEP_MS = 850;
  const FLY_STEP_FALLBACK_MS = 1000;

  /** @typedef {{ lon: number, lat: number, label?: string }} PreviewPoint */
  /** @typedef {{ role: string, coords: [number, number][] }} PreviewSegment */
  /**
   * @typedef {{
   *   version: 1,
   *   candidateIndex: number,
   *   summary: { planMinutes: number, toll: string, chargedSection: string, entryLabel: string, exitLabel: string },
   *   segments: PreviewSegment[],
   *   charged: [number, number][],
   *   origin: PreviewPoint | null,
   *   entry: PreviewPoint | null,
   *   exit: PreviewPoint | null,
   *   reducedMotion: boolean,
   *   autoplay: boolean,
   * }} PreviewPayload
   */

  const state = {
    /** @type {any} */ map: null,
    /** @type {PreviewPayload | null} */ payload: null,
    /** @type {string[]} */ layers: [],
    /** @type {string[]} */ sources: [],
    /** @type {[number, number][]} */ loopCoords: [],
    /** @type {{c: [number, number], b: number}[]} */ waypoints: [],
    /** @type {number} */ flyToken: 0,
    /** @type {boolean} */ flying: false,
    /** @type {number | null} */ autoplayTimer: null,
    /** @type {number | null} */ stepTimer: null,
    /** @type {HTMLElement | null} */ playButton: null,
    /** @type {HTMLElement | null} */ status: null,
    /** @type {HTMLElement | null} */ loading: null,
    /** @type {HTMLElement | null} */ error: null,
  };

  const STYLES = {
    access: { color: "#94a3b8", width: 4, opacity: 0.8, dash: [4, 4] },
    loop: { color: "#0b5cad", width: 7, opacity: 0.98, glowColor: "#38bdf8", core: true },
    return: { color: "#94a3b8", width: 4, opacity: 0.8, dash: [4, 4] },
    entry_approach: { color: "#0b5cad", width: 6, opacity: 0.98, core: true },
    mandatory_lap: { color: "#0b5cad", width: 7, opacity: 0.98, glowColor: "#38bdf8", core: true },
    return_corridor: { color: "#0b5cad", width: 6, opacity: 0.95, dash: [10, 6] },
    exit_approach: { color: "#0b5cad", width: 6, opacity: 0.95, dash: [8, 4, 2, 4] },
  };

  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

  /** @param {number} lon @param {number} lat */
  const lineFeature = (coords) => ({
    type: "Feature",
    properties: {},
    geometry: { type: "LineString", coordinates: coords },
  });

  /** @param {number} lon @param {number} lat */
  const pointFeature = (lon, lat) => ({
    type: "Feature",
    properties: {},
    geometry: { type: "Point", coordinates: [lon, lat] },
  });

  /** URL の #<id> からアプリが置いたルートを取り出す。読んだら消す。 */
  function readPayload() {
    const id = window.location.hash.replace(/^#/, "");
    window.history.replaceState(null, "", window.location.pathname + window.location.search);
    if (id === "") {
      return null;
    }
    const key = `${STORAGE_PREFIX}${id}`;
    let raw = null;
    try {
      raw = window.localStorage.getItem(key);
      window.localStorage.removeItem(key);
    } catch {
      return null;
    }
    if (raw === null) {
      return null;
    }
    try {
      const value = JSON.parse(raw);
      return value && typeof value === "object" ? value : null;
    } catch {
      return null;
    }
  }

    function injectUi(payload) {
    const header = document.createElement("header");
    header.className = "mp-header";
    const summary = payload?.summary;
    const title = summary ? `候補${String(payload.candidateIndex)} / 3Dプレビュー` : "3Dルートプレビュー";
    const meta = summary
      ? `計画 約${String(summary.planMinutes)}分 ・ ${summary.toll} ・ ${summary.entryLabel} → ${summary.exitLabel}`
      : "候補カードの「3Dで見る」から開いてください。";
    header.innerHTML = `
      <div class="mp-header-copy">
        <p class="mp-kicker">首都高 3Dプレビュー</p>
        <h1 class="mp-title"></h1>
        <p class="mp-meta"></p>
        <p class="mp-credit">3D都市モデル: 国土交通省 Project PLATEAU（CC BY 4.0）を加工 ・ 地図: Geolonia / 国土地理院 / © OpenStreetMap contributors</p>
      </div>
      <a class="mp-back" href="../">アプリに戻る</a>
    `;
    header.querySelector(".mp-title").textContent = title;
    header.querySelector(".mp-meta").textContent = meta;
    document.body.appendChild(header);

    const controls = document.createElement("div");
    controls.className = "mp-controls";
    controls.setAttribute("role", "group");
    controls.setAttribute("aria-label", "3Dプレビューの操作");
    controls.innerHTML = `
      <button type="button" data-action="fly" disabled>▶ 走行プレビュー</button>
      <button type="button" data-action="overview" disabled>全体を見る</button>
      <span class="mp-status" role="status" aria-live="polite"></span>
    `;
    document.body.appendChild(controls);
    state.playButton = controls.querySelector('button[data-action="fly"]');
    state.status = controls.querySelector(".mp-status");
    controls.addEventListener("click", (event) => {
      const button = event.target instanceof Element ? event.target.closest("button") : null;
      const action = button?.getAttribute("data-action");
      if (action === "fly") {
        if (state.flying) {
          stopFly();
        } else {
          startFly();
        }
      } else if (action === "overview") {
        stopFly();
        fitOverview();
      }
    });

    const loading = document.createElement("div");
    loading.className = "mp-loading";
    loading.setAttribute("role", "status");
    loading.innerHTML =
      "<div><p>3D地図を読み込み中…</p><p class=\"mp-hint\">初回は地図タイルの取得に少し時間がかかります。</p></div>";
    document.body.appendChild(loading);
    state.loading = loading;

    const error = document.createElement("div");
    error.className = "mp-error";
    error.innerHTML = `<div><p class="mp-error-message"></p><p><a href="../">アプリに戻る</a></p></div>`;
    document.body.appendChild(error);
    state.error = error;
  }

  function setStatus(text) {
    if (state.status) {
      state.status.textContent = text;
    }
  }

  function showError(message) {
    if (state.loading) {
      state.loading.remove();
      state.loading = null;
    }
    if (state.error) {
      const messageEl = state.error.querySelector(".mp-error-message");
      if (messageEl) {
        messageEl.textContent = message;
      }
      state.error.setAttribute("data-open", "true");
    }
    setStatus("");
  }

  function hideLoading() {
    if (state.loading) {
      state.loading.remove();
      state.loading = null;
    }
  }

  function setControlsEnabled(enabled) {
    for (const button of document.querySelectorAll(".mp-controls button")) {
      button.disabled = !enabled;
    }
  }

  function updatePlayButton() {
    if (!state.playButton) {
      return;
    }
    state.playButton.textContent = state.flying ? "■ 停止" : "▶ 走行プレビュー";
    state.playButton.setAttribute("aria-pressed", state.flying ? "true" : "false");
  }

  function clearRoute() {
    const map = state.map;
    if (!map) {
      return;
    }
    for (const id of [...state.layers].reverse()) {
      try {
        map.removeLayer(id);
      } catch {
        // 追加に失敗したレイヤーは消えていてもよい。
      }
    }
    for (const id of [...state.sources].reverse()) {
      try {
        map.removeSource(id);
      } catch {
        // 同上。
      }
    }
    state.layers = [];
    state.sources = [];
  }

  /**
   * @param {string} name
   * @param {[number, number][]} coords
   * @param {{color: string, width: number, opacity: number, dash?: number[]}} style
   * @param {boolean} glow
   */
  function addLine(name, coords, style, glow) {
    const map = state.map;
    if (!map || !Array.isArray(coords) || coords.length < 2) {
      return;
    }
    const sourceId = `mp-${name}`;
    map.addSource(sourceId, { type: "geojson", data: lineFeature(coords) });
    state.sources.push(sourceId);
    if (glow) {
      const glowId = `${sourceId}-glow`;
      map.addLayer({
        id: glowId,
        type: "line",
        source: sourceId,
        paint: {
          "line-color": style.glowColor ?? style.color,
          "line-width": style.width + 10,
          "line-opacity": 0.22,
          "line-blur": 10,
        },
      });
      state.layers.push(glowId);
    }
    const layer = {
      id: `${sourceId}-line`,
      type: "line",
      source: sourceId,
      paint: {
        "line-color": style.color,
        "line-width": style.width,
        "line-opacity": style.opacity,
      },
    };
    if (style.dash) {
      layer.paint["line-dasharray"] = style.dash;
    }
    map.addLayer(layer);
    state.layers.push(layer.id);
    if (style.core) {
      const coreId = `${sourceId}-core`;
      map.addLayer({
        id: coreId,
        type: "line",
        source: sourceId,
        paint: { "line-color": "#f8fafc", "line-width": 2, "line-opacity": 0.9 },
      });
      state.layers.push(coreId);
    }
  }

  /**
   * @param {string} name
   * @param {number} lon
   * @param {number} lat
   * @param {string} color
   * @param {number} radius
   */
  function addPoint(name, lon, lat, color, radius) {
    const map = state.map;
    if (!map || !Number.isFinite(lon) || !Number.isFinite(lat)) {
      return;
    }
    const sourceId = `mp-${name}`;
    map.addSource(sourceId, { type: "geojson", data: pointFeature(lon, lat) });
    state.sources.push(sourceId);
    map.addLayer({
      id: `${sourceId}-halo`,
      type: "circle",
      source: sourceId,
      paint: { "circle-radius": radius + 8, "circle-color": color, "circle-opacity": 0.22 },
    });
    map.addLayer({
      id: `${sourceId}-dot`,
      type: "circle",
      source: sourceId,
      paint: {
        "circle-radius": radius,
        "circle-color": color,
        "circle-stroke-color": "#ffffff",
        "circle-stroke-width": 2,
      },
    });
    state.layers.push(`${sourceId}-halo`, `${sourceId}-dot`);
  }

  /**
   * ルートを立体のリボンにするための、区間ごとの四角形を作る。
   * 長いポリゴンを1枚にまとめるとカーブで自己交差するため、必ず区間ごとに分ける。
   * @param {[number, number][]} coords
   * @param {string} kind
   * @param {number} halfWidthMeters
   */
  function ribbonFeatures(coords, kind, halfWidthMeters) {
    const features = [];
    const rad = Math.PI / 180;
    for (let index = 1; index < coords.length; index += 1) {
      const a = coords[index - 1];
      const b = coords[index];
      const midLat = ((a[1] + b[1]) / 2) * rad;
      const mx = 111320 * Math.cos(midLat);
      const my = 110540;
      const ax = a[0] * mx;
      const ay = a[1] * my;
      const bx = b[0] * mx;
      const by = b[1] * my;
      const dx = bx - ax;
      const dy = by - ay;
      const length = Math.hypot(dx, dy);
      if (!(length > 1)) {
        continue;
      }
      const nx = (-dy / length) * halfWidthMeters;
      const ny = (dx / length) * halfWidthMeters;
      const leftA = [(ax + nx) / mx, (ay + ny) / my];
      const leftB = [(bx + nx) / mx, (by + ny) / my];
      const rightB = [(bx - nx) / mx, (by - ny) / my];
      const rightA = [(ax - nx) / mx, (ay - ny) / my];
      features.push({
        type: "Feature",
        properties: { kind },
        geometry: {
          type: "Polygon",
          coordinates: [[leftA, leftB, rightB, rightA, leftA]],
        },
      });
    }
    return features;
  }

  /** @param {PreviewPayload} payload */
  function addRibbon(payload) {
    const map = state.map;
    if (!map) {
      return;
    }
    const features = [];
    for (const part of payload.segments) {
      features.push(...ribbonFeatures(part.coords, "route", 6));
    }
    if (Array.isArray(payload.charged)) {
      features.push(...ribbonFeatures(payload.charged, "charged", 7));
    }
    if (features.length === 0) {
      return;
    }
    map.addSource("mp-ribbon", {
      type: "geojson",
      data: { type: "FeatureCollection", features },
    });
    state.sources.push("mp-ribbon");
    map.addLayer({
      id: "mp-ribbon-3d",
      type: "fill-extrusion",
      source: "mp-ribbon",
      paint: {
        "fill-extrusion-color": ["match", ["get", "kind"], "charged", "#f97316", "#0b5cad"],
        "fill-extrusion-height": 8,
        "fill-extrusion-base": 0,
        "fill-extrusion-opacity": 0.82,
      },
    });
    state.layers.push("mp-ribbon-3d");
  }

  /** @param {[number, number][]} coords */
  function boundsOf(coords) {
    let minLon = Infinity;
    let minLat = Infinity;
    let maxLon = -Infinity;
    let maxLat = -Infinity;
    for (const [lon, lat] of coords) {
      if (!Number.isFinite(lon) || !Number.isFinite(lat)) {
        continue;
      }
      if (lon < minLon) minLon = lon;
      if (lat < minLat) minLat = lat;
      if (lon > maxLon) maxLon = lon;
      if (lat > maxLat) maxLat = lat;
    }
    if (!Number.isFinite(minLon) || !Number.isFinite(minLat)) {
      return null;
    }
    return [
      [minLon, minLat],
      [maxLon, maxLat],
    ];
  }

  /** @param {[number, number][]} coords */
  function fitOverview(coords = state.payload?.allCoords ?? []) {
    const map = state.map;
    const bounds = boundsOf(coords);
    if (!map || bounds === null) {
      return;
    }
    try {
      map.fitBounds(bounds, { padding: 56, maxZoom: 15.2, duration: 0 });
      const center = map.getCenter();
      map.jumpTo({
        center: [center.lng, center.lat],
        zoom: map.getZoom(),
        pitch: 48,
        bearing: 0,
      });
    } catch {
      // 範囲計算に失敗しても、現在のカメラのまま操作は続けられる。
    }
  }

  /** @param {PreviewPayload} payload */
  function applyRoute(payload) {
    const map = state.map;
    if (!map) {
      return;
    }
    stopFly();
    clearRoute();
    const allCoords = [];
    for (const [index, part] of payload.segments.entries()) {
      const style = STYLES[part.role] ?? STYLES.loop;
      addLine(`seg-${String(index)}`, part.coords, style, true);
      allCoords.push(...part.coords);
    }
    if (Array.isArray(payload.charged) && payload.charged.length >= 2) {
      addLine("charged-glow", payload.charged, { color: "#f97316", width: 18, opacity: 0.3 }, false);
      addLine(
        "charged",
        payload.charged,
        { color: "#f97316", width: 8, opacity: 1, dash: [2, 8] },
        false,
      );
      allCoords.push(...payload.charged);
    }
    if (payload.origin) {
      addPoint("origin", payload.origin.lon, payload.origin.lat, "#f97316", 7);
    }
    if (payload.entry) {
      addPoint("entry", payload.entry.lon, payload.entry.lat, "#22d3ee", 5);
    }
    if (payload.exit) {
      addPoint("exit", payload.exit.lon, payload.exit.lat, "#a3e635", 5);
    }
    addRibbon(payload);
    state.payload = { ...payload, allCoords };
    state.loopCoords =
      payload.segments.find((part) => part.role === "mandatory_lap")?.coords ??
      payload.segments.find((part) => part.role === "loop")?.coords ??
      allCoords;
    state.waypoints = buildWaypoints(state.loopCoords);
    fitOverview(allCoords);
    setControlsEnabled(true);
    hideLoading();
    // 小さい画面では自動再生しない。30 秒近いカメラ移動は、開いた直後に
    // 意図せず始まると見失いやすい。ボタンから明示的に始められる。
    if (payload.autoplay && !payload.reducedMotion && window.innerWidth >= 560) {
      clearTimeout(state.autoplayTimer);
      state.autoplayTimer = window.setTimeout(() => {
        if (!state.flying && state.waypoints.length > 1) {
          startFly();
        }
      }, 1400);
    }
  }

  /** @param {[number, number][]} coords */
  function buildWaypoints(coords) {
    if (coords.length < 2) {
      return [];
    }
    const R = 6371008.8;
    const rad = Math.PI / 180;
    const distance = (a, b) => {
      const dLat = (b[1] - a[1]) * rad;
      const dLon = (b[0] - a[0]) * rad;
      const x =
        Math.sin(dLat / 2) ** 2 +
        Math.cos(a[1] * rad) * Math.cos(b[1] * rad) * Math.sin(dLon / 2) ** 2;
      return 2 * R * Math.asin(Math.sqrt(x));
    };
    const cumulative = [0];
    for (let i = 1; i < coords.length; i += 1) {
      cumulative.push(cumulative[i - 1] + distance(coords[i - 1], coords[i]));
    }
    const total = cumulative[cumulative.length - 1];
    if (!(total > 0)) {
      return [];
    }
    const at = (target) => {
      let low = 0;
      let high = cumulative.length - 1;
      while (low < high) {
        const mid = (low + high) >> 1;
        if (cumulative[mid] < target) low = mid + 1;
        else high = mid;
      }
      const index = Math.max(1, low);
      const span = cumulative[index] - cumulative[index - 1] || 1;
      const t = (target - cumulative[index - 1]) / span;
      const a = coords[index - 1];
      const b = coords[index];
      return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    };
    const waypoints = [];
    const stepMeters = 450;
    for (let target = 0; target <= total; target += stepMeters) {
      const here = at(target);
      const ahead = at(Math.min(total, target + 90));
      const bearing =
        Math.atan2((ahead[0] - here[0]) * Math.cos(here[1] * rad), ahead[1] - here[1]) / rad;
      waypoints.push({ c: here, b: (bearing + 360) % 360 });
    }
    return waypoints;
  }

  function startFly() {
    const map = state.map;
    if (!map || state.waypoints.length < 2) {
      return;
    }
    clearTimeout(state.autoplayTimer);
    state.flyToken += 1;
    const token = state.flyToken;
    state.flying = true;
    updatePlayButton();
    setStatus("走行プレビュー中");
    const first = state.waypoints[0];
    map.jumpTo({ center: first.c, zoom: 15.8, pitch: 56, bearing: first.b });
    step(token, 1);
  }

  /**
   * @param {number} token
   * @param {number} index
   */
  function step(token, index) {
    const map = state.map;
    if (!map || token !== state.flyToken || !state.flying) {
      return;
    }
    if (index >= state.waypoints.length) {
      state.flying = false;
      updatePlayButton();
      setStatus("プレビュー終了");
      return;
    }
    const waypoint = state.waypoints[index];
    let advanced = false;
    const advance = () => {
      if (advanced || token !== state.flyToken || !state.flying) {
        return;
      }
      advanced = true;
      clearTimeout(state.stepTimer);
      step(token, index + 1);
    };
    map.once("moveend", advance);
    state.stepTimer = window.setTimeout(advance, FLY_STEP_FALLBACK_MS);
    map.easeTo({
      center: waypoint.c,
      zoom: 15.8,
      pitch: 56,
      bearing: waypoint.b,
      duration: FLY_STEP_MS,
      easing: (t) => t,
    });
  }

  function stopFly() {
    state.flyToken += 1;
    state.flying = false;
    clearTimeout(state.autoplayTimer);
    clearTimeout(state.stepTimer);
    state.autoplayTimer = null;
    state.stepTimer = null;
    updatePlayButton();
    setStatus("");
  }

  async function waitForMap() {
    const deadline = performance.now() + MAP_TIMEOUT_MS;
    while (typeof window.__machglMap === "undefined") {
      if (performance.now() > deadline) {
        throw new Error("3D地図の初期化がタイムアウトしました。");
      }
      await sleep(50);
    }
    const map = window.__machglMap;
    while (typeof map.isStyleLoaded === "function" && !map.isStyleLoaded()) {
      if (performance.now() > deadline) {
        break;
      }
      await sleep(50);
    }
    return map;
  }

  async function main() {
    const payload = readPayload();
    injectUi(payload);
    if (payload === null) {
      showError(
        "プレビュー情報が見つかりません。アプリの候補カードから「3Dで見る」を押して開き直してください。",
      );
      return;
    }
    // 形が壊れている JSON をそのまま描画しない。
    if (
      !Array.isArray(payload.segments) ||
      typeof payload.summary !== "object" ||
      payload.summary === null
    ) {
      showError("プレビュー情報を読み取れませんでした。アプリから開き直してください。");
      return;
    }
    try {
      const map = await waitForMap();
      state.map = map;
      const canvas = map.getCanvas();
      for (const eventName of ["mousedown", "wheel", "touchstart"]) {
        canvas.addEventListener(eventName, () => stopFly(), { passive: true });
      }
      applyRoute(payload);
      window.addEventListener("pagehide", () => {
        try {
          map.remove();
        } catch {
          // すでに破棄されていれば何もしない。
        }
      });
    } catch (error) {
      showError(error instanceof Error ? error.message : String(error));
    }
  }

  window.addEventListener("error", (event) => {
    if (state.map === null) {
      showError(event.message || "3D地図の読み込み中にエラーが発生しました。");
    }
  });

  void main();
})();
