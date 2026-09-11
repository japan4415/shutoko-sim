// 計測ハーネス（bench.html）のスモーク。
// フル計測（30 パターン × 3 回 × cold/warm）は重くスロットル下で不安定なため CI には
// 入れず、手動実行とする。ここでは「ハーネスが壊れていないこと」だけを最小構成
// （cold 1 回 + warm 1 回、スロットルなし）で検証する。
//
// パターンは 0（神田橋 15〜30）と 1（神田橋 15〜60）の 2 件を回す。パターン 0 は
// scout-003 F4 のとおり TIME_WINDOW で候補 0 件になり status 文言だけを描く分岐、
// パターン 1 は候補カードを描く分岐を通るため、描画経路を両方被覆できる。
import { expect, test } from "@playwright/test";
import { validateEnvelope, type BenchEnvelope, type BenchTrial } from "../src/bench/envelope";

declare global {
  interface Window {
    __benchResult?: BenchEnvelope;
    __benchDone?: boolean;
    __benchError?: string;
  }
}

type TrialWithResources = BenchTrial;

function findTrial(
  envelope: BenchEnvelope,
  patternIndex: number,
  cache: "cold" | "warm",
): TrialWithResources | undefined {
  return envelope.trials.find((trial) => trial.patternIndex === patternIndex && trial.cache === cache);
}

function graphEntry(trial: TrialWithResources | undefined): TrialWithResources["resources"][number] | undefined {
  return trial?.resources.find((entry) => entry.name.includes("graph.json"));
}

test("bench ページが cold/warm の 1 試行ずつを完走し envelope を返す", async ({ page }) => {
  await page.goto("/bench.html?auto=1&patterns=0,1&repeats=1");
  await page.waitForFunction(
    () => window.__benchDone === true || typeof window.__benchError === "string",
    undefined,
    { timeout: 60_000 },
  );
  expect(await page.evaluate(() => window.__benchError ?? null)).toBeNull();

  const raw = await page.evaluate(() => window.__benchResult ?? null);
  const validation = validateEnvelope(raw);
  expect(validation.errors).toEqual([]);
  const envelope = raw as BenchEnvelope;

  // 2 パターン × (cold 1 + warm 1) = 4 試行
  expect(envelope.trials).toHaveLength(4);
  expect(envelope.releaseId).toBe("c1-real-v1");
  expect(envelope.targets.searchP95Ms).toBe(2000);

  const cold0 = findTrial(envelope, 0, "cold");
  const warm0 = findTrial(envelope, 0, "warm");
  const cold1 = findTrial(envelope, 1, "cold");
  const warm1 = findTrial(envelope, 1, "warm");
  for (const [label, trial] of [
    ["pattern0 cold", cold0],
    ["pattern0 warm", warm0],
    ["pattern1 cold", cold1],
    ["pattern1 warm", warm1],
  ] as const) {
    expect(trial, `${label} 試行`).toBeDefined();
    expect(trial?.timeout, `${label} がタイムアウト`).toBe(false);
    expect(trial?.tTransferMs ?? 0, `${label} の tTransfer`).toBeGreaterThan(0);
    expect(trial?.tSearchMs ?? 0, `${label} の tSearch`).toBeGreaterThan(0);
    expect(trial?.tFirstCandidateMs ?? 0, `${label} の tFirstCandidate`).toBeGreaterThan(0);
  }

  // パターン 0（神田橋 15〜30）は TIME_WINDOW で候補 0 件、パターン 1（神田橋 15〜60）は候補 2 件。
  expect(cold0?.resultStatus).toBe("no_candidates");
  expect(cold0?.reason).toBe("TIME_WINDOW");
  expect(cold0?.candidateCount).toBe(0);
  expect(cold1?.resultStatus).toBe("ok");
  expect(cold1?.candidateCount).toBe(2);

  for (const [label, trial] of [
    ["pattern0 cold", cold0],
    ["pattern1 cold", cold1],
  ] as const) {
    // cold は ?bench=<nonce> 付き URL でネットワークから取得している。
    expect(trial?.resources.every((entry) => entry.name.includes("bench=")), label).toBe(true);
    const graph = graphEntry(trial);
    expect(graph, `${label} の graph.json エントリ`).toBeDefined();
    expect(graph?.transferSize ?? 0, `${label} の transferSize`).toBeGreaterThan(0);
    expect(graph?.encodedBodySize ?? 0, `${label} の encodedBodySize`).toBeGreaterThan(0);
    // decodedBodySize は伸長後（転送量ではない）ので encoded より大きい。
    expect(graph?.decodedBodySize ?? 0).toBeGreaterThan(graph?.encodedBodySize ?? 0);
  }

  for (const [label, trial] of [
    ["pattern0 warm", warm0],
    ["pattern1 warm", warm1],
  ] as const) {
    // warm は nonce なし URL でキャッシュから返る。
    expect(trial?.resources.every((entry) => !entry.name.includes("bench=")), label).toBe(true);
    const graph = graphEntry(trial);
    expect(graph, `${label} の graph.json エントリ`).toBeDefined();
    expect(graph?.deliveryType === "cache" || (graph?.transferSize ?? -1) === 0, label).toBe(true);
  }

  // 最後に描画された試行（パターン 1 の warm = 候補 2 件）のカードが残っている。
  await expect(page.locator("#results .card")).toHaveCount(2);
  await expect(page.locator("#status")).toContainText("完了: 4 試行");
  await expect(page.locator("#verdicts li")).toHaveCount(4);
  await expect(page.locator("#validate")).toHaveText("envelope: 検証 OK");
  await expect(page.locator("#summary table tbody tr")).not.toHaveCount(0);
});
