// MachGL 先行版の3Dプレビューを開くためのクライアント側ロジック。
//
// MachGL の地図そのものは /machgl-preview/ の別タブで動く。このモジュールは
// 候補からプレビュー用のペイロードを作り、localStorage 経由で渡してタブを開く。
// 地図ライブラリをアプリのバンドルへ取り込まないので、初回ロードの予算は変わらない。
import type { CardModel } from "../ui/model";
import type { Candidate } from "../worker/types";
import { deriveSegments } from "./segments";
import type { Coords } from "./segments";

/** プレビューのタブへ渡す点。GeoJSON と同じ [lon, lat] ではなく緯度経度を明示する。 */
export interface PreviewPoint {
  lon: number;
  lat: number;
  label: string;
}

/** 役割ごとの線。role は legacy の access/loop/return か radial の route leg role。 */
export interface PreviewSegment {
  role: string;
  coords: Coords[];
}

export interface PreviewPayload {
  version: 1;
  /** 1 始まりの候補番号。 */
  candidateIndex: number;
  summary: {
    planMinutes: number;
    toll: string;
    chargedSection: string;
    entryLabel: string;
    exitLabel: string;
  };
  segments: PreviewSegment[];
  charged: Coords[];
  origin: PreviewPoint | null;
  entry: PreviewPoint | null;
  exit: PreviewPoint | null;
  reducedMotion: boolean;
  autoplay: boolean;
  /** 古い localStorage 項目の掃除に使う。ブリッジは無視する。 */
  savedAt: number;
}

export interface PreviewOptions {
  reducedMotion: boolean;
}

export interface PreviewWindow {
  /** 2D 地図の代わりに 3D を出すかどうかの判定。 */
  available: boolean;
  /** 3D プレビューのタブを開く。ポップアップがブロックされたら false。 */
  launch(candidate: Candidate, model: CardModel): boolean;
}

const STORAGE_PREFIX = "machgl-preview:";
const PREVIEW_PATH = "/machgl-preview/";

/**
 * WebGL2 が使えるかを一度だけ判定する。MachGL は WebGL2 前提で、2D フォールバックを
 * 持たない。使えない端末では「3Dで見る」を出さず、従来の Leaflet 地図だけを見せる。
 */
export function hasWebgl2(): boolean {
  try {
    const canvas = document.createElement("canvas");
    return canvas.getContext("webgl2") !== null;
  } catch {
    return false;
  }
}

/**
 * ビルド時に生成した /machgl-preview/config.json を読む。
 * 正式版前のデモ取得に失敗したビルドでは enabled: false になり、ボタンを出さない。
 */
export async function loadPreviewConfig(): Promise<boolean> {
  try {
    const response = await fetch("/machgl-preview/config.json", {
      cache: "no-store",
      credentials: "same-origin",
    });
    if (!response.ok) {
      return false;
    }
    const value: unknown = await response.json();
    return (
      value !== null &&
      typeof value === "object" &&
      (value as { enabled?: unknown }).enabled === true
    );
  } catch {
    return false;
  }
}

/** 1 始まりの候補番号を含む、地図に渡す線分を作る。 */
function buildPreviewSegments(candidate: Candidate): {
  segments: PreviewSegment[];
  charged: Coords[];
} {
  const derived = deriveSegments(candidate);
  const segments: PreviewSegment[] = [];
  if (derived.routeLegs.length > 0) {
    for (const leg of derived.routeLegs) {
      if (leg.coords.length >= 2) {
        segments.push({ role: leg.role, coords: leg.coords });
      }
    }
  } else {
    for (const [role, coords] of [
      ["access", derived.access],
      ["loop", derived.loop],
      ["return", derived.return],
    ] as const) {
      if (coords.length >= 2) {
        segments.push({ role, coords });
      }
    }
  }
  return { segments, charged: derived.charged };
}

function firstCoord(segments: readonly PreviewSegment[]): Coords | null {
  for (const segment of segments) {
    if (segment.coords.length > 0) {
      return segment.coords[0];
    }
  }
  return null;
}

function lastCoord(segments: readonly PreviewSegment[]): Coords | null {
  for (let index = segments.length - 1; index >= 0; index -= 1) {
    const coords = segments[index].coords;
    if (coords.length > 0) {
      return coords[coords.length - 1];
    }
  }
  return null;
}

function pointOf(coords: Coords | null, label: string): PreviewPoint | null {
  if (coords === null) {
    return null;
  }
  const [lon, lat] = coords;
  if (!Number.isFinite(lon) || !Number.isFinite(lat)) {
    return null;
  }
  return { lon, lat, label };
}

function originPoint(candidate: Candidate): PreviewPoint | null {
  const snapped = candidate.snappedOrigin;
  if (Number.isFinite(snapped.lat) && Number.isFinite(snapped.lon)) {
    return { lon: snapped.lon, lat: snapped.lat, label: "出発地" };
  }
  const origin = candidate.origin;
  if (origin !== null && Number.isFinite(origin.lat) && Number.isFinite(origin.lon)) {
    return { lon: origin.lon, lat: origin.lat, label: "出発地" };
  }
  return null;
}

/**
 * 候補から、プレビューのタブが読む JSON を作る。
 * 点の位置は正確な入口・出口の座標ではなく、ルートの区間端を使う近似である。
 */
export function buildPreviewPayload(
  candidate: Candidate,
  model: CardModel,
  options: PreviewOptions,
): PreviewPayload {
  const { segments, charged } = buildPreviewSegments(candidate);
  const entryLabel = candidate.entry.name ?? candidate.entry.rampId ?? candidate.entryId;
  const exitLabel = candidate.exit.name ?? candidate.exit.rampId ?? candidate.exitId;

  let entryCoords: Coords | null = null;
  let exitCoords: Coords | null = null;
  if (candidate.pairKind === "radialReturn") {
    const entryLeg = segments.find((segment) => segment.role === "entry_approach");
    const exitLeg = segments.find((segment) => segment.role === "exit_approach");
    entryCoords = entryLeg?.coords.at(-1) ?? firstCoord(segments);
    exitCoords = exitLeg?.coords[0] ?? lastCoord(segments);
  } else {
    const loop = segments.find((segment) => segment.role === "loop");
    entryCoords = loop?.coords[0] ?? firstCoord(segments);
    exitCoords = loop?.coords.at(-1) ?? lastCoord(segments);
  }

  return {
    version: 1,
    candidateIndex: model.index,
    summary: {
      planMinutes: model.planMinutes,
      toll: model.toll,
      chargedSection: model.chargedSection,
      entryLabel,
      exitLabel,
    },
    segments,
    charged,
    origin: originPoint(candidate),
    entry: pointOf(entryCoords, entryLabel),
    exit: pointOf(exitCoords, exitLabel),
    reducedMotion: options.reducedMotion,
    autoplay: !options.reducedMotion,
    savedAt: Date.now(),
  };
}

function createId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `${String(Date.now())}-${Math.random().toString(36).slice(2)}`;
}

/** プレビュー用の URL と保存キー。保存に失敗したときは null。 */
export function preparePreviewLaunch(
  candidate: Candidate,
  model: CardModel,
  options: PreviewOptions,
): string | null {
  const payload = buildPreviewPayload(candidate, model, options);
  const id = createId();
  const key = `${STORAGE_PREFIX}${id}`;
  try {
    // 前回の失敗などで取り残された項目を掃除する（1時間より古いもの）。
    for (let index = 0; index < window.localStorage.length; index += 1) {
      const existingKey = window.localStorage.key(index);
      if (existingKey === null || !existingKey.startsWith(STORAGE_PREFIX)) {
        continue;
      }
      try {
        const raw = window.localStorage.getItem(existingKey);
        const savedAt = raw === null ? null : (JSON.parse(raw) as { savedAt?: unknown }).savedAt;
        if (typeof savedAt !== "number" || Date.now() - savedAt > 60 * 60 * 1000) {
          window.localStorage.removeItem(existingKey);
        }
      } catch {
        window.localStorage.removeItem(existingKey);
      }
    }
    window.localStorage.setItem(key, JSON.stringify(payload));
  } catch {
    return null;
  }
  const url = new URL(PREVIEW_PATH, window.location.origin);
  url.hash = id;
  return url.toString();
}

/** 候補から3Dプレビューのタブを開く。ポップアップがブロックされたら false。 */
export function openPreview(
  candidate: Candidate,
  model: CardModel,
  options: PreviewOptions,
): boolean {
  const href = preparePreviewLaunch(candidate, model, options);
  if (href === null) {
    return false;
  }
  const opened = window.open(href, "_blank");
  if (opened === null) {
    // 保存した項目は残さない（開けなかったタブの分を再利用しない）。
    try {
      const id = new URL(href).hash.replace(/^#/, "");
      window.localStorage.removeItem(`${STORAGE_PREFIX}${id}`);
    } catch {
      // 掃除に失敗しても、次回の起動時に古い項目として消える。
    }
    return false;
  }
  try {
    opened.opener = null;
  } catch {
    // 同一オリジンのプレビューなので、opener を切れなくても致命的ではない。
  }
  return true;
}
