// Exercise the exact web-target glue and binary produced for a browser Worker.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import init, { search } from '../dist/wasm/shutoko_routing.js';

const graph = await readFile(new URL('../fixtures/synthetic-graph.json', import.meta.url), 'utf8');
const request = await readFile(new URL('../fixtures/synthetic-request.json', import.meta.url), 'utf8');
const bytes = await readFile(new URL('../dist/wasm/shutoko_routing_bg.wasm', import.meta.url));
await init({ module_or_path: bytes });
const first = search(graph, request, '{}');
const result = JSON.parse(first);
assert.equal(result.status, 'ok');
assert.ok(result.candidates.length > 0);
const candidate = result.candidates[0];
assert.deepEqual(candidate.edgeIds, ['access', 'entry', 'ab', 'bc', 'ca', 'exit', 'return']);
assert.equal(candidate.duration.baseSeconds, 1980);
assert.equal(candidate.duration.planSeconds, 2376);
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
assert.equal(candidate.snappedOrigin.nodeId, 's');
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
assert.equal(coordCand.snappedOrigin.nodeId, 's');
assert.ok(coordCand.snappedOrigin.distanceMeters < 1.0);
assert.deepEqual(coordCand.origin, { lat: 35.681, lon: 139.7671 });

// NO_CONNECTION case when coordinates are beyond 200m
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
console.log('Web-target WASM loaded; synthetic loop search, new fields, coordinate snap, NO_CONNECTION, determinism and errors passed.');

