// Exercise the exact web-target glue and binary produced for a browser Worker.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import init, { search, prepare, searchPrepared } from '../dist/wasm/shutoko_routing.js';

const graph = await readFile(new URL('../fixtures/synthetic-graph.json', import.meta.url), 'utf8');
const schema4Graph = await readFile(
  new URL('../fixtures/graph-v4/graph-radial-fixture.json', import.meta.url),
  'utf8',
);
const generatedGraph = await readFile(
  new URL('../fixtures/generated/graph.json', import.meta.url),
  'utf8',
);
const generatedManifest = JSON.parse(
  await readFile(new URL('../fixtures/generated/manifest.json', import.meta.url), 'utf8'),
);
const request = await readFile(new URL('../fixtures/synthetic-request.json', import.meta.url), 'utf8');
const bytes = await readFile(new URL('../dist/wasm/shutoko_routing_bg.wasm', import.meta.url));
await init({ module_or_path: bytes });
const schema4Pg = prepare(schema4Graph, '{}');
const radialRequest = JSON.stringify({
  requestId: 'schema4-radial',
  releaseId: 'graph-v4-fixture-v1',
  originNodeId: 'fixture:node:entry:ground',
  minMinutes: 1,
  maxMinutes: 60,
  vehicleProfile: 'passenger-car-etc',
  pricingAt: '2026-09-16T00:00:00Z',
});
const radialJson = searchPrepared(schema4Pg, radialRequest);
const radialResult = JSON.parse(radialJson);
assert.equal(radialResult.status, 'ok');
const radialCandidate = radialResult.candidates[0];
assert.equal(radialCandidate.pairKind, 'radialReturn');
assert.equal(radialCandidate.duration.shutokoSeconds, 1440);
assert.equal(radialCandidate.shutokoDistanceMeters, 23400);
assert.equal(radialCandidate.edgeRouteLegs.length, 4);
assert.equal(radialCandidate.estimatedLegs.length, 2);
assert.equal(
  radialCandidate.distanceMeters,
  radialCandidate.shutokoDistanceMeters +
    radialCandidate.estimatedLegs.reduce((total, leg) => total + leg.distanceMeters, 0),
);
assert.equal('chargedSectionCount' in radialCandidate.toll, false);
assert.equal(radialCandidate.reasons.includes('ONE_SECTION_TOLL'), false);
assert.equal(searchPrepared(schema4Pg, radialRequest), radialJson);
schema4Pg.free();
const generatedPg = prepare(generatedGraph, '{}');
generatedPg.free();
assert.equal(JSON.parse(generatedGraph).schemaVersion, 4);
assert.equal(generatedManifest.graphSchemaVersion, 4);
assert.equal(generatedManifest.billingPairsVersion, 'v2');
assert.equal(generatedManifest.routePlanVersion, 1);
assert.match(generatedManifest.routeMembershipsSha256, /^[0-9a-f]{64}$/);
assert.throws(
  () => prepare(JSON.stringify({ ...JSON.parse(schema4Graph), schemaVersion: 5 }), '{}'),
  'unknown graph schema version must throw',
);
const first = search(graph, request, '{}');
const result = JSON.parse(first);
assert.equal(result.status, 'ok');
assert.ok(result.candidates.length > 0);
const candidate = result.candidates[0];
// feat/drop-local-roads: 'access' と 'return'（一般道エッジ）はグラフから除去済み。
assert.deepEqual(candidate.edgeIds, ['entry', 'ab', 'bc', 'ca', 'exit']);
assert.equal(candidate.duration.baseSeconds, 1876);
assert.equal(candidate.duration.planSeconds, 2252);
assert.equal(candidate.toll.amountYen, 300);
assert.equal(candidate.toll.chargedSectionCount, 1);

// New candidate fields validation
assert.equal(candidate.geometry.type, 'LineString');
assert.ok(Array.isArray(candidate.geometry.coordinates));
assert.equal(candidate.geometry.coordinates.length, candidate.edgeIds.length + 1);
assert.ok(
  candidate.handoff.mapsUrl.startsWith('https://www.google.com/maps/dir/?api=1&origin='),
  'mapsUrl must start with Google Maps directions API base',
);
assert.ok(candidate.handoff.mapsUrl.length <= 2048, 'mapsUrl length must be <= 2048');
// feat/drop-local-roads: 's' ノード（一般道始点）は除去。originNodeId="i" なので snappedOrigin.nodeId は "i"。
assert.equal(candidate.snappedOrigin.nodeId, 'i');
assert.equal(candidate.snappedOrigin.distanceMeters, 0);
assert.ok(
  candidate.warnings.includes('HANDOFF_WAYPOINTS_UNVERIFIED'),
  'warnings must include HANDOFF_WAYPOINTS_UNVERIFIED',
);
assert.ok(
  !candidate.warnings.includes('EXPERIMENTAL_NO_HANDOFF'),
  'warnings must not include deprecated EXPERIMENTAL_NO_HANDOFF',
);

// Coordinate-based search case
const coordReq = JSON.parse(request);
delete coordReq.originNodeId;
coordReq.origin = { lat: 35.681, lon: 139.7671 };
const coordRes = JSON.parse(search(graph, JSON.stringify(coordReq), '{}'));
assert.equal(coordRes.status, 'ok');
assert.ok(coordRes.candidates.length > 0);
const coordCand = coordRes.candidates[0];
// feat/drop-local-roads: 座標 (35.681, 139.7671) から最近傍 Entry from-node は "i"（約 66m）。
assert.equal(coordCand.snappedOrigin.nodeId, 'i');
assert.ok(coordCand.snappedOrigin.distanceMeters < 100.0);
assert.deepEqual(coordCand.origin, { lat: 35.681, lon: 139.7671 });

const topologyGraph = JSON.parse(schema4Graph);
topologyGraph.billingPairs = [];
topologyGraph.ramps[0].facilityId = topologyGraph.ramps[1].facilityId;
topologyGraph.edges.push({
  id: 'fixture:edge:topology:cycle',
  from: 'fixture:node:exit:connector',
  to: 'fixture:node:entry:ramp-end',
  kind: 'shutoko',
  durationSeconds: 600,
  distanceMeters: 10000,
});
const topologyResult = JSON.parse(
  search(
    JSON.stringify(topologyGraph),
    JSON.stringify({
      requestId: 'topology-only',
      releaseId: 'graph-v4-fixture-v1',
      origin: { lat: 35.1, lon: 139.1 },
      minMinutes: 1,
      maxMinutes: 120,
      vehicleProfile: 'passenger-car-etc',
      pricingAt: '2026-09-16T00:00:00Z',
    }),
    '{}',
  ),
);
assert.equal(topologyResult.status, 'ok');
const topologyCandidate = topologyResult.candidates[0];
assert.equal(topologyCandidate.pairKind, 'topologyOnly');
assert.equal(topologyCandidate.eligibilityStatus, 'topology_only');
assert.equal(topologyCandidate.loopValidationStatus, 'topology_only');
assert.equal(topologyCandidate.tariffStatus, 'unpriced');
assert.equal('chargedSectionCount' in topologyCandidate.toll, false);
assert.equal(topologyCandidate.reasons.includes('ONE_SECTION_TOLL'), false);
assert.equal('edgeRouteLegs' in topologyCandidate, false);

// NO_CONNECTION case when coordinates are beyond 30 km from the nearest Entry access point
const noConnReq = JSON.parse(request);
delete noConnReq.originNodeId;
noConnReq.origin = { lat: 35.0, lon: 139.0 };
const noConnRes = JSON.parse(search(graph, JSON.stringify(noConnReq), '{}'));
assert.equal(noConnRes.status, 'no_candidates');
assert.equal(noConnRes.reason, 'NO_CONNECTION');
assert.deepEqual(noConnRes.candidates, []);

assert.equal(search(graph, request, '{}'), first, 'WASM search must be deterministic');
assert.throws(() => search('{', request, '{}'), 'invalid graph JSON must throw');
const changed = JSON.parse(request);
changed.maxMinutes = 0;
assert.throws(
  () => search(graph, JSON.stringify(changed), '{}'),
  (e) => e instanceof Error && JSON.parse(e.message).code === 'INVALID_INPUT',
);
const oversizedTimestamp = JSON.parse(graph);
oversizedTimestamp.billingPairs[0].prices[0].effectiveFrom =
  `2026-01-01T00:00:00.${'0'.repeat(100_000)}Z`;
assert.throws(
  () => search(JSON.stringify(oversizedTimestamp), request, '{}'),
  'oversized timestamps must be rejected before candidate cloning',
);

// PreparedGraph validation
const pg = prepare(graph, '{}');
const preparedRes = searchPrepared(pg, request);
assert.equal(preparedRes, first, 'searchPrepared must match one-shot search result');
const coordPreparedRes = searchPrepared(pg, JSON.stringify(coordReq));
assert.equal(coordPreparedRes, JSON.stringify(coordRes), 'searchPrepared coordinate result must match one-shot');

// Performance benchmark (measure prepare, search, searchPrepared)
const WARMUP_ITERS = 5;
const BENCH_ITERS = 20;

for (let i = 0; i < WARMUP_ITERS; i++) {
  const p = prepare(graph, '{}');
  searchPrepared(p, request);
  p.free();
  search(graph, request, '{}');
}

const tPrepareStart = performance.now();
for (let i = 0; i < BENCH_ITERS; i++) {
  const p = prepare(graph, '{}');
  p.free();
}
const avgPrepareMs = (performance.now() - tPrepareStart) / BENCH_ITERS;

const benchPg = prepare(graph, '{}');
const tSearchPreparedStart = performance.now();
for (let i = 0; i < BENCH_ITERS; i++) {
  searchPrepared(benchPg, request);
}
const avgSearchPreparedMs = (performance.now() - tSearchPreparedStart) / BENCH_ITERS;
benchPg.free();

const tSearchStart = performance.now();
for (let i = 0; i < BENCH_ITERS; i++) {
  search(graph, request, '{}');
}
const avgSearchMs = (performance.now() - tSearchStart) / BENCH_ITERS;

// Free validation
pg.free();
assert.throws(() => searchPrepared(pg, request), 'use-after-free on WasmPreparedGraph must throw');

console.log(`[bench] prepare=${avgPrepareMs.toFixed(2)}ms / search=${avgSearchMs.toFixed(2)}ms / searchPrepared=${avgSearchPreparedMs.toFixed(2)}ms`);
console.log('Web-target WASM loaded; schema 2/4 readers, synthetic search, coordinate snap, determinism, PreparedGraph and errors passed.');

