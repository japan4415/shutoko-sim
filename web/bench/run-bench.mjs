#!/usr/bin/env node
// 代理計測ランナー（issue #13 第 2 段）。
//
// Pixel 5 相当のコンテキストで bench.html を自動実行し、CDP で CPU とネットワークを
// スロットルして計測する。cold は試行ごとに `browser.newContext()` を作り直して
// ブラウザキャッシュ・ストレージを完全に分離する（scout-004 F12）。warm は同じ
// コンテキストで cold の直後に 1 回走らせる（bench ページが `?repeats=1` で
// cold 1 回 → warm 1 回を実行するため、1 コンテキストから 2 試行が得られる）。
//
// 使い方:
//   node bench/run-bench.mjs --network fast4g --patterns 0,1 --repeats 1
//   node bench/run-bench.mjs --network slow4g --repeats 3 --out results/slow4g.json
//
// 前提: `workers/` の wrangler dev が `--base-url` で応答し、`vite build` 済みの
// dist（bench.html を含む）が配信されていること。CI には入れない（scout-004 F14）。
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { chromium, devices } from "@playwright/test";
import { importTsModule } from "./ts-import.mjs";

/**
 * Chrome DevTools の回線プリセット実値。単位は **bytes/sec**（bits/sec ではない）。
 * 出典: devtools-frontend NetworkManager.ts（scout-004 F2）。導出は
 * download = 公称 Mbps × 1000^2 / 8 × 0.9、latency = targetLatency × 調整係数。
 * - Fast 4G : 下り 9 Mbps / 上り 1.5 Mbps / targetLatency 60 ms
 * - Slow 4G : 下り 1.6 Mbps / 上り 0.75 Mbps / targetLatency 150 ms
 */
const NETWORK_PRESETS = {
  fast4g: {
    label: "Fast 4G",
    offline: false,
    downloadThroughput: 1_012_500,
    uploadThroughput: 168_750,
    latency: 165,
    connectionType: "cellular4g",
  },
  slow4g: {
    label: "Slow 4G",
    offline: false,
    downloadThroughput: 180_000,
    uploadThroughput: 84_375,
    latency: 562.5,
    connectionType: "cellular4g",
  },
};

/** エミュレートする端末。iOS の engine / JIT / メモリ挙動は再現できない（F1）。 */
const DEVICE_KEY = "Pixel 5";
/** CPU スロットル倍率の既定（中程度スマートフォン相当の暫定値。機種確定後に較正する）。 */
const DEFAULT_CPU_THROTTLE = 4;
/** CDP の JSHeapUsedSize をサンプルする間隔（ms）。 */
const HEAP_SAMPLE_INTERVAL_MS = 250;
/** フル計測の代表パターン数（web/src/bench/patterns.ts の 10 地点 × 3 時間条件）。 */
const PATTERN_COUNT = 30;

function usage() {
  return [
    "usage: node bench/run-bench.mjs [options]",
    "  --network fast4g|slow4g   CDP の回線プリセット（既定 fast4g）",
    `  --cpu <n>                 CPU スロットル倍率（既定 ${String(DEFAULT_CPU_THROTTLE)}）`,
    `  --patterns 0,1,...        実行するパターン index（既定 0..${String(PATTERN_COUNT - 1)}）`,
    "  --repeats <n>             パターンごとのコンテキスト数 = cold 試行数（既定 3）",
    "  --base-url <url>          計測対象のオリジン（既定 http://localhost:8787）",
    "  --out <path>              結果 JSON の出力先（既定 web/bench/results/<network>.json）",
    "  --timeout-ms <n>          1 コンテキストの待ち上限（既定 180000）",
    "  --headed                  ヘッドレスを無効化",
  ].join("\n");
}

function parseCli(argv) {
  const { values } = parseArgs({
    args: argv,
    options: {
      network: { type: "string", default: "fast4g" },
      cpu: { type: "string", default: String(DEFAULT_CPU_THROTTLE) },
      patterns: { type: "string" },
      repeats: { type: "string", default: "3" },
      "base-url": { type: "string", default: "http://localhost:8787" },
      out: { type: "string" },
      "timeout-ms": { type: "string", default: "180000" },
      headed: { type: "boolean", default: false },
      help: { type: "boolean", default: false },
    },
    allowPositionals: false,
  });
  if (values.help) {
    console.log(usage());
    process.exit(0);
  }
  const network = values.network;
  if (!(network in NETWORK_PRESETS)) {
    throw new Error(`--network は ${Object.keys(NETWORK_PRESETS).join(" / ")} のいずれか: ${network}`);
  }
  const cpu = Number(values.cpu);
  if (!Number.isInteger(cpu) || cpu < 0) {
    throw new Error(`--cpu は 0 以上の整数: ${values.cpu}`);
  }
  const repeats = Number(values.repeats);
  if (!Number.isInteger(repeats) || repeats < 1) {
    throw new Error(`--repeats は 1 以上の整数: ${values.repeats}`);
  }
  const patterns =
    values.patterns === undefined
      ? Array.from({ length: PATTERN_COUNT }, (_, index) => index)
      : [...new Set(values.patterns.split(",").map((part) => Number(part.trim())))];
  if (patterns.length === 0 || patterns.some((index) => !Number.isInteger(index) || index < 0)) {
    throw new Error(`--patterns は 0 以上の整数のカンマ区切り: ${values.patterns ?? ""}`);
  }
  const timeoutMs = Number(values["timeout-ms"]);
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    throw new Error(`--timeout-ms は正の数: ${values["timeout-ms"]}`);
  }
  const defaultOut = fileURLToPath(new URL(`./results/${network}.json`, import.meta.url));
  return {
    network,
    preset: NETWORK_PRESETS[network],
    cpu,
    repeats,
    patterns: patterns.sort((a, b) => a - b),
    baseUrl: values["base-url"].replace(/\/+$/, ""),
    out: values.out === undefined ? defaultOut : resolve(process.cwd(), values.out),
    timeoutMs,
    headless: !values.headed,
  };
}

/** macOS / Linux / Windows の表示名。 */
function osLabel() {
  if (process.platform === "darwin") return "macOS";
  if (process.platform === "win32") return "Windows";
  if (process.platform === "linux") return "Linux";
  return process.platform;
}

/**
 * CDP `Performance.getMetrics` の `JSHeapUsedSize` を一定間隔でサンプルし、最大値を返す。
 * `performance.memory` と違い 10 MB 量子化が無いが、レンダラプロセス単位の値であり
 * WASM リニアメモリは含まない（docs/bench/README.md の限界を参照）。
 */
function startHeapSampler(cdp) {
  let peakBytes = null;
  const sample = async () => {
    try {
      const { metrics } = await cdp.send("Performance.getMetrics");
      const entry = metrics.find((metric) => metric.name === "JSHeapUsedSize");
      if (entry !== undefined && Number.isFinite(entry.value)) {
        peakBytes = peakBytes === null ? entry.value : Math.max(peakBytes, entry.value);
      }
    } catch {
      // コンテキスト破棄と競合したときの失敗は無視する（補助記録のため）。
    }
  };
  const timer = setInterval(() => void sample(), HEAP_SAMPLE_INTERVAL_MS);
  return {
    async stop() {
      clearInterval(timer);
      await sample();
      return peakBytes === null ? null : peakBytes / (1024 * 1024);
    },
  };
}

/** 数値を桁区切りで表示する（ログ用）。 */
function fmt(value, digits = 0) {
  return value === null || !Number.isFinite(value) ? "-" : value.toFixed(digits);
}

/**
 * 1 コンテキスト分の計測を実行する。
 * @returns {Promise<{envelope: object, cdpPeakMiB: number|null, consoleErrors: string[], elapsedMs: number}>}
 */
async function measureContext(browser, contextOptions, cli, patternIndex, repeat) {
  const startedAt = Date.now();
  const context = await browser.newContext(contextOptions);
  const consoleErrors = [];
  try {
    const page = await context.newPage();
    page.on("console", (message) => {
      if (message.type() === "error") {
        consoleErrors.push(message.text());
      }
    });
    const cdp = await context.newCDPSession(page);
    await cdp.send("Emulation.setCPUThrottlingRate", { rate: cli.cpu });
    await cdp.send("Network.enable");
    // スロットルは CDP の回線プリセット値そのもの（bytes/sec）。
    await cdp.send("Network.emulateNetworkConditions", cli.preset);
    await cdp.send("Performance.enable");
    const sampler = startHeapSampler(cdp);

    const url = `${cli.baseUrl}/bench.html?auto=1&patterns=${String(patternIndex)}&repeats=1`;
    let envelope;
    let cdpPeakMiB;
    try {
      await page.goto(url, { waitUntil: "load", timeout: cli.timeoutMs });
      await page.waitForFunction(
        () => window.__benchDone === true || typeof window.__benchError === "string",
        undefined,
        { timeout: cli.timeoutMs, polling: 500 },
      );
      const error = await page.evaluate(() => window.__benchError ?? null);
      if (error !== null) {
        throw new Error(`bench ページが失敗しました: ${error}`);
      }
      envelope = await page.evaluate(() => window.__benchResult ?? null);
      if (envelope === null || typeof envelope !== "object") {
        throw new Error("window.__benchResult が取得できませんでした");
      }
    } finally {
      cdpPeakMiB = await sampler.stop();
    }
    return { envelope, cdpPeakMiB, consoleErrors, elapsedMs: Date.now() - startedAt };
  } finally {
    await context.close();
  }
}

async function main() {
  const cli = parseCli(process.argv.slice(2));
  const { validateEnvelope } = await importTsModule("../src/bench/envelope.ts");

  const deviceSettings = devices[DEVICE_KEY];
  if (deviceSettings === undefined) {
    throw new Error(`Playwright の devices に ${DEVICE_KEY} がありません`);
  }

  const totalContexts = cli.patterns.length * cli.repeats;
  console.error(
    `[bench] ${cli.preset.label} (CDP) / CPU ×${String(cli.cpu)} / ${DEVICE_KEY} emulation / ` +
      `${String(cli.patterns.length)} patterns × ${String(cli.repeats)} repeats ` +
      `= ${String(totalContexts)} contexts (cold+warm 各 ${String(totalContexts)} 試行)`,
  );
  console.error(`[bench] base=${cli.baseUrl} out=${cli.out}`);

  const browser = await chromium.launch({ headless: cli.headless });
  const browserVersion = browser.version();
  const trials = [];
  const failures = [];
  const contextTimings = [];
  let pageLoad = null;
  let releaseId = null;
  let targets = null;
  let deviceUa = null;
  let devicePlatform = null;
  let cdpSampledContexts = 0;
  let done = 0;

  try {
    for (const patternIndex of cli.patterns) {
      for (let repeat = 1; repeat <= cli.repeats; repeat += 1) {
        done += 1;
        const label = `pattern ${String(patternIndex)} repeat ${String(repeat)}/${String(cli.repeats)}`;
        try {
          const result = await measureContext(
            browser,
            deviceSettings,
            cli,
            patternIndex,
            repeat,
          );
          const envelope = result.envelope;
          if (countTrials(envelope) === 0) {
            throw new Error("envelope に試行がありません");
          }
          if (pageLoad === null) {
            // 初回ロードの定義（summarize.ts の firstLoadMs）は pageLoad の
            // responseEnd〜loadEventEnd を使うため、代表として最初の cold コンテキストの
            // ページロードを採用する（各コンテキストは空キャッシュから読み込むので同等）。
            pageLoad = envelope.pageLoad;
            releaseId = envelope.releaseId;
            targets = envelope.targets;
            deviceUa = envelope.device.ua;
            devicePlatform = envelope.device.platform;
          }
          const contextsCdpPeakMiB = result.cdpPeakMiB;
          if (contextsCdpPeakMiB !== null) {
            cdpSampledContexts += 1;
          }
          for (const trial of envelope.trials) {
            // 各コンテキストは repeat=1 で 1 回だけ実行するため、通しの試行番号に振り直す。
            trial.repeat = repeat;
            if (contextsCdpPeakMiB !== null) {
              trial.memoryPeakMiB = contextsCdpPeakMiB;
              trial.memorySource = "cdp-performance-metrics";
            }
            trials.push(trial);
          }
          contextTimings.push({
            patternIndex,
            repeat,
            elapsedMs: result.elapsedMs,
            trials: envelope.trials.length,
            cdpPeakMiB: contextsCdpPeakMiB,
            consoleErrors: result.consoleErrors,
          });
          const cold = envelope.trials.find((trial) => trial.cache === "cold");
          console.error(
            `[bench] ${String(done)}/${String(totalContexts)} ${label}: ` +
              `${String(result.elapsedMs)} ms / cold tSearch=${fmt(cold?.tSearchMs ?? null)} ms ` +
              `tFirstCandidate=${fmt(cold?.tFirstCandidateMs ?? null)} ms ` +
              `timeout=${String(hasTimeout(envelope))} ` +
              `cdpHeap=${fmt(contextsCdpPeakMiB, 1)} MiB`,
          );
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          failures.push({ patternIndex, repeat, message });
          contextTimings.push({ patternIndex, repeat, elapsedMs: null, error: message });
          console.error(
            `[bench] ${String(done)}/${String(totalContexts)} ${label}: 失敗 ${message}`,
          );
        }
      }
    }
  } finally {
    await browser.close();
  }

  if (trials.length === 0) {
    throw new Error("1 試行も取得できませんでした（wrangler dev と vite build を確認してください）");
  }

  /** メモリの出所を試行から決める（summarize.ts の derivedMemorySource と同じ規則）。 */
  let memorySource = null;
  let bestPeak = null;
  for (const trial of trials) {
    if (trial.memoryPeakMiB === null || trial.memorySource === null) continue;
    if (bestPeak === null || trial.memoryPeakMiB > bestPeak) {
      bestPeak = trial.memoryPeakMiB;
      memorySource = trial.memorySource;
    }
  }

  const envelope = {
    schemaVersion: 1,
    createdAt: new Date().toISOString(),
    releaseId,
    device: {
      ua: deviceUa,
      platform: devicePlatform,
      deviceName: `${DEVICE_KEY} emulation`,
      os: `desktop ${osLabel()}`,
      browser: `Chromium ${browserVersion}`,
      network: `${cli.network} (CDP)`,
      cpuThrottle: cli.cpu,
      note:
        `CDP Network.emulateNetworkConditions ${cli.preset.label}: ` +
        `download ${cli.preset.downloadThroughput.toLocaleString("en-US")} B/s / ` +
        `upload ${cli.preset.uploadThroughput.toLocaleString("en-US")} B/s / ` +
        `latency ${String(cli.preset.latency)} ms. ` +
        `CPU ×${String(cli.cpu)} は暫定値で、実機機種の確定後に較正する。` +
        `cold はコンテキストごとに newContext で分離し、warm は同一コンテキストで cold の直後に 1 回。`,
    },
    memorySource,
    memoryManualMiB: null,
    pageLoad,
    targets,
    trials,
  };

  const validation = validateEnvelope(envelope);
  if (!validation.valid) {
    console.error(`[bench] envelope 検証 NG: ${validation.errors.join(" / ")}`);
  }

  await mkdir(dirname(cli.out), { recursive: true });
  await writeFile(cli.out, `${JSON.stringify(envelope, null, 2)}\n`, "utf8");

  const elapsed = contextTimings.filter((timing) => timing.elapsedMs !== null);
  const summary = {
    network: cli.network,
    cpu: cli.cpu,
    repeats: cli.repeats,
    patterns: cli.patterns.length,
    contexts: totalContexts,
    contextsCompleted: elapsed.length,
    failures: failures.length,
    trials: trials.length,
    coldTrials: trials.filter((trial) => trial.cache === "cold").length,
    warmTrials: trials.filter((trial) => trial.cache === "warm").length,
    timeouts: trials.filter((trial) => trial.timeout).length,
    cdpSampledContexts,
    memorySource,
    memoryPeakMiB: bestPeak,
    validateOk: validation.valid,
    contextElapsedMs: {
      min: elapsed.length === 0 ? null : Math.min(...elapsed.map((timing) => timing.elapsedMs)),
      max: elapsed.length === 0 ? null : Math.max(...elapsed.map((timing) => timing.elapsedMs)),
      total: elapsed.reduce((sum, timing) => sum + timing.elapsedMs, 0),
    },
    consoleErrors: contextTimings.flatMap((timing) => timing.consoleErrors ?? []).slice(0, 20),
    out: cli.out,
  };
  console.log(`BENCH_RUN_SUMMARY ${JSON.stringify(summary)}`);
  console.error(
    `[bench] 完了: ${String(trials.length)} 試行 / 失敗 ${String(failures.length)} コンテキスト / ` +
      `検証 ${validation.valid ? "OK" : "NG"} / 出力 ${cli.out}`,
  );
  if (failures.length > 0) {
    console.error(`[bench] 失敗の内訳: ${JSON.stringify(failures)}`);
  }
}

/** envelope の試行数（壊れた値でも落ちないようにする）。 */
function countTrials(envelope) {
  return Array.isArray(envelope.trials) ? envelope.trials.length : 0;
}

/** タイムアウトした試行があるか。 */
function hasTimeout(envelope) {
  return Array.isArray(envelope.trials) ? envelope.trials.some((trial) => trial.timeout === true) : false;
}

await main();
