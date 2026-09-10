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
assert.equal(search(graph, request, '{}'), first, 'WASM search must be deterministic');
assert.throws(() => search('{', request, '{}'), 'invalid graph JSON must throw');
const changed = JSON.parse(request);
changed.maxMinutes = 0;
assert.throws(() => search(graph, JSON.stringify(changed), '{}'), 'invalid bounds must throw');
const oversizedTimestamp = JSON.parse(graph);
oversizedTimestamp.billingPairs[0].prices[0].effectiveFrom =
  `2026-01-01T00:00:00.${'0'.repeat(100_000)}Z`;
assert.throws(
  () => search(JSON.stringify(oversizedTimestamp), request, '{}'),
  'oversized timestamps must be rejected before candidate cloning',
);
console.log('Web-target WASM loaded; synthetic loop search, determinism and errors passed.');
