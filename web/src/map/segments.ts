// 候補の走行エッジ列から地図描画用のセグメント（座標列）を導出する純粋関数。
// Leaflet に依存しないため Vitest（node env）で検証できる。
import type { Candidate, RoutePlanSegmentRole } from "../worker/types";

/** GeoJSON と同じ [経度, 緯度] の順。 */
export type Coords = [number, number];

export interface RouteSegment {
  role: RoutePlanSegmentRole;
  coords: Coords[];
}

/** セグメント種別ごとの座標列。各区間は連続した座標ラン。 */
export interface DerivedSegments {
  /** 周回に入るまでの走行区間。 */
  access: Coords[];
  /** 首都高を周回する区間。 */
  loop: Coords[];
  /** 周回を終えてからの走行区間。 */
  return: Coords[];
  routeLegs: RouteSegment[];
  /** セグメント分解できなかった全経路（フォールバック描画用）。 */
  main: Coords[];
}

/**
 * geometry.coordinates は走行した各エッジの始終点を順に並べるため、
 * `coordinates.length === edgeIds.length + 1` が成立する。エッジ i の座標は
 * `coordinates[i]`（始点）と `coordinates[i + 1]`（終点）で、隣接エッジ間の
 * 重複端点は持たない。セグメント境界をエッジ index の range [from, toExclusive)
 * で表し、その座標範囲は `coordinates[from]` 〜 `coordinates[toExclusive]` になる。
 *
 * loop.edgeIds は edgeIds の連続部分列として現れる（access → loop → return の順）。
 * その開始 index と長さから access / loop / return の 3 区間に分ける。
 * legacyRing の入口から出口までの走行経路には周回が含まれる。
 * 料金には別の根拠があり、課金区間の座標列は候補に含まれないため、走行経路からは推定しない。
 *
 * edgeIds と座標数が整合しない場合や loop の境界が見つからない場合も、
 * main に全座標を入れることで経路全体を表示できる。
 */
export function deriveSegments(candidate: Candidate): DerivedSegments {
  const all = candidate.geometry.coordinates as Coords[];
  const empty: DerivedSegments = {
    access: [],
    loop: [],
    return: [],
    routeLegs: [],
    main: [],
  };
  const edgeIds = candidate.edgeIds;

  // 座標数とエッジ数の整合（coordinates.length === edgeIds.length + 1）を検証する。
  if (edgeIds.length + 1 !== all.length || all.length < 2) {
    return { ...empty, main: all.slice() };
  }

  const routeLegs =
    candidate.pairKind === "radialReturn"
      ? candidate.edgeRouteLegs.map((leg) => ({
          role: leg.role,
          coords: sliceEdges(all, leg.startEdgeIndex, leg.endEdgeIndexExclusive),
        }))
      : [];

  let loopStart: number | null;
  let loopEndExclusive: number;
  if (candidate.pairKind === "radialReturn") {
    const mandatoryLap = candidate.edgeRouteLegs.find((leg) => leg.role === "mandatory_lap");
    loopStart = mandatoryLap?.startEdgeIndex ?? null;
    loopEndExclusive = mandatoryLap?.endEdgeIndexExclusive ?? -1;
  } else {
    loopStart = contiguousIndex(edgeIds, candidate.loop.edgeIds);
    loopEndExclusive = loopStart === null ? -1 : loopStart + candidate.loop.edgeIds.length;
  }
  if (loopStart === null || loopEndExclusive <= loopStart) {
    return { ...empty, routeLegs, main: all.slice() };
  }

  return {
    access: sliceEdges(all, 0, loopStart),
    loop: sliceEdges(all, loopStart, loopEndExclusive),
    return: sliceEdges(all, loopEndExclusive, edgeIds.length),
    routeLegs,
    main: all.slice(),
  };
}

/** edgeIds 内で needle が連続部分列として現れる開始 index。無ければ null。 */
function contiguousIndex(edgeIds: readonly string[], needle: readonly string[]): number | null {
  if (needle.length === 0 || needle.length > edgeIds.length) {
    return null;
  }
  for (let start = 0; start + needle.length <= edgeIds.length; start += 1) {
    let matched = true;
    for (let offset = 0; offset < needle.length; offset += 1) {
      if (edgeIds[start + offset] !== needle[offset]) {
        matched = false;
        break;
      }
    }
    if (matched) {
      return start;
    }
  }
  return null;
}

/** エッジ index の range [from, toExclusive) を座標ランへ変換する（端点を 1 つ含む）。 */
function sliceEdges(coords: readonly Coords[], from: number, toExclusive: number): Coords[] {
  if (from < 0 || toExclusive > coords.length - 1 || toExclusive <= from) {
    return [];
  }
  return coords.slice(from, toExclusive + 1).map((point) => [point[0], point[1]]);
}

/**
 * 座標列の外接矩形。[[minLon, minLat], [maxLon, maxLat]] を返す。空なら null。
 * Leaflet の fitBounds にそのまま渡せる形式。
 */
export function boundsOf(coords: readonly Coords[]): [[number, number], [number, number]] | null {
  if (coords.length === 0) {
    return null;
  }
  let minLon = Infinity;
  let minLat = Infinity;
  let maxLon = -Infinity;
  let maxLat = -Infinity;
  for (const [lon, lat] of coords) {
    if (!Number.isFinite(lon) || !Number.isFinite(lat)) {
      continue;
    }
    if (lon < minLon) {
      minLon = lon;
    }
    if (lat < minLat) {
      minLat = lat;
    }
    if (lon > maxLon) {
      maxLon = lon;
    }
    if (lat > maxLat) {
      maxLat = lat;
    }
  }
  if (!Number.isFinite(minLon) || !Number.isFinite(minLat)) {
    return null;
  }
  return [
    [minLon, minLat],
    [maxLon, maxLat],
  ];
}
