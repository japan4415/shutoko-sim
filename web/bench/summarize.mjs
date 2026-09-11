#!/usr/bin/env node
// 計測結果 JSON（bench envelope。代理計測ランナーの出力でも、実機の bench ページが
// ダウンロードした JSON でもよい）を読み、Markdown の集計表を stdout に出す CLI。
//
// 集計と合否判定は計測ページと同じ `src/bench/summarize.ts` の aggregate / judge を使う
// （二重実装による定義ずれを避けるため。読み込みは bench/ts-import.mjs が Vite の
// TS 変換 API で行う）。
//
// 使い方:
//   node bench/summarize.mjs docs/bench/2026-09-11-proxy-pixel5-emulation/fast4g.json
//   node bench/summarize.mjs fast4g.json slow4g.json --out docs/bench/<date>-<device>/summary.md
//
// 複数ファイルを渡しても条件を混ぜて合算はしない（回線条件が違うと p95 の意味が
// 変わるため）。ファイルごとに節を作り、冒頭にファイル別の合否サマリを出す。
import { readFile, writeFile } from "node:fs/promises";
import { basename, resolve } from "node:path";
import { parseArgs } from "node:util";
import { importTsModule } from "./ts-import.mjs";

function usage() {
  return [
    "usage: node bench/summarize.mjs <result.json> [<result.json> ...] [--out <path>]",
    "",
    "  <result.json>  bench ページ / 代理計測ランナーが出力した envelope の JSON",
    "  --out <path>   Markdown を標準出力ではなくファイルにも書き出す",
  ].join("\n");
}

const MARK = { pass: "PASS", fail: "FAIL", unknown: "UNKNOWN" };

function fmtMs(value) {
  return value === null || value === undefined ? "-" : `${value.toFixed(0)} ms`;
}

function fmtBytes(value) {
  if (value === null || value === undefined) return "-";
  if (value >= 1024 * 1024) return `${(value / (1024 * 1024)).toFixed(2)} MiB`;
  return `${(value / 1024).toFixed(1)} KiB`;
}

function fmtMiB(value) {
  return value === null || value === undefined ? "-" : `${value.toFixed(1)} MiB`;
}

function fmtRate(value) {
  return `${(value * 100).toFixed(1)} %`;
}

/** 表の行を組み立てる（セル内の `|` はエスケープする）。 */
function row(cells) {
  return `| ${cells.map((cell) => String(cell).replace(/\|/g, "\\|")).join(" | ")} |`;
}

function groupRows(agg) {
  const groups = [agg.overall, agg.cold, agg.warm];
  return groups.map((group) =>
    row([
      group.label,
      group.trialCount,
      fmtRate(group.timeoutRate),
      fmtMs(group.tSearch.p50),
      fmtMs(group.tSearch.p95),
      fmtMs(group.tFirstCandidate.p50),
      fmtMs(group.tFirstCandidate.p95),
      fmtMs(group.firstLoad.p95),
      fmtBytes(group.coldTransferBytes),
      fmtMiB(group.memoryPeakMiB),
    ]),
  );
}

function verdictRows(verdicts) {
  return verdicts.map((verdict) =>
    row([
      verdict.label,
      verdict.target.toLocaleString("ja-JP"),
      verdict.actual === null ? "計測不能" : verdict.actual.toFixed(1),
      MARK[verdict.verdict],
    ]),
  );
}

function patternRows(agg) {
  return agg.byPattern.map((group) =>
    row([
      group.key.replace("pattern-", ""),
      group.label,
      group.trialCount,
      fmtRate(group.timeoutRate),
      fmtMs(group.tSearch.p50),
      fmtMs(group.tSearch.p95),
      fmtMs(group.tFirstCandidate.p50),
      fmtMs(group.tFirstCandidate.p95),
    ]),
  );
}

/** 1 ファイル分の節。 */
function renderSection(name, envelope, agg, verdicts, validation, targets) {
  const lines = [];
  const device = envelope.device ?? {};
  const throttle = device.cpuThrottle === undefined || device.cpuThrottle === null
    ? "なし"
    : `×${String(device.cpuThrottle)}`;
  lines.push(`## ${name}`, "");
  if (!validation.valid) {
    lines.push(`> ⚠ envelope の検証に失敗しました: ${validation.errors.join(" / ")}`, "");
  }
  lines.push(
    `- 端末: ${device.deviceName ?? "-"} / ${device.os ?? "-"} / ${device.browser ?? "-"}`,
    `- 回線: ${device.network ?? "-"} / CPU スロットル: ${throttle}`,
    `- releaseId: ${envelope.releaseId ?? "-"} / 試行: ${String(agg.trialCount)} 件` +
      `（cold ${String(agg.cold.trialCount)} / warm ${String(agg.warm.trialCount)}）`,
    `- 10 秒到達率: ${fmtRate(agg.timeoutRate)} / メモリ出所: ${agg.memorySource ?? "取得できず"}`,
    `- 目標値: 探索 p95 ${String(targets.searchP95Ms)} ms / 初回ロード p95 ${String(targets.firstLoadP95Ms)} ms /` +
      ` 転送量 ${fmtBytes(targets.transferBytesMax)} / メモリ ${String(targets.memoryMiBMax)} MiB`,
    "",
    "### 条件別",
    "",
    row([
      "条件",
      "試行",
      "10 秒到達率",
      "探索 p50",
      "探索 p95",
      "初回候補 p50",
      "初回候補 p95",
      "初回ロード p95",
      "cold 転送量 最大",
      "メモリ 最大",
    ]),
    row(Array.from({ length: 10 }, () => "---")),
    ...groupRows(agg),
    "",
    "### 目標合否",
    "",
    row(["目標", "目標値", "実測", "判定"]),
    row(["---", "---", "---", "---"]),
    ...verdictRows(verdicts),
    "",
    "### パターン別",
    "",
    row(["#", "パターン", "試行", "10 秒到達率", "探索 p50", "探索 p95", "初回候補 p50", "初回候補 p95"]),
    row(Array.from({ length: 8 }, () => "---")),
    ...patternRows(agg),
    "",
  );
  return lines;
}

async function main() {
  const { values, positionals } = parseArgs({
    args: process.argv.slice(2),
    options: { out: { type: "string" }, help: { type: "boolean", default: false } },
    allowPositionals: true,
  });
  if (values.help || positionals.length === 0) {
    console.log(usage());
    process.exit(values.help ? 0 : 1);
  }

  const { aggregate, judge } = await importTsModule("../src/bench/summarize.ts");
  const { BENCH_TARGETS, validateEnvelope } = await importTsModule("../src/bench/envelope.ts");

  const inputs = [];
  for (const path of positionals) {
    const raw = await readFile(resolve(process.cwd(), path), "utf8");
    const envelope = JSON.parse(raw);
    const validation = validateEnvelope(envelope);
    const agg = aggregate(envelope);
    // 目標値は envelope に埋まっているが、無い（古い）ファイルは既定値で判定する。
    const targets = envelope.targets ?? BENCH_TARGETS;
    inputs.push({
      name: basename(path),
      envelope,
      agg,
      verdicts: judge(agg, targets),
      validation,
      targets,
    });
  }

  const lines = ["# 計測 集計", ""];
  const totalTrials = inputs.reduce((sum, input) => sum + input.agg.trialCount, 0);
  const timeouts = inputs.reduce((sum, input) => sum + input.agg.timeoutCount, 0);
  lines.push(
    `- 入力: ${String(inputs.length)} 件（${inputs.map((input) => input.name).join(", ")}）`,
    `- 合計試行: ${String(totalTrials)} 件 / 10 秒到達: ${String(timeouts)} 件`,
    "",
  );

  if (inputs.length > 1) {
    lines.push(
      "## ファイル別 合否",
      "",
      row(["ファイル", "試行", "探索 p95", "初回ロード p95 (cold)", "cold 転送量 最大", "メモリ 最大", "探索", "初回", "転送", "メモリ"]),
      row(Array.from({ length: 10 }, () => "---")),
    );
    for (const input of inputs) {
      const byKey = new Map(input.verdicts.map((verdict) => [verdict.key, verdict]));
      lines.push(
        row([
          input.name,
          input.agg.trialCount,
          fmtMs(input.agg.overall.tSearch.p95),
          fmtMs(input.agg.cold.firstLoad.p95),
          fmtBytes(input.agg.cold.coldTransferBytes),
          fmtMiB(input.agg.overall.memoryPeakMiB),
          MARK[byKey.get("searchP95").verdict],
          MARK[byKey.get("firstLoadP95").verdict],
          MARK[byKey.get("transfer").verdict],
          MARK[byKey.get("memory").verdict],
        ]),
      );
    }
    lines.push("");
  }

  for (const input of inputs) {
    lines.push(
      ...renderSection(input.name, input.envelope, input.agg, input.verdicts, input.validation, input.targets),
    );
  }

  lines.push(
    "## 注記",
    "",
    "- 初回ロード p95 は cold 試行の `firstLoadMs`（pageLoad の responseEnd〜loadEventEnd + 試行の tFirstCandidate）で見る。",
    "- 転送量は cold 試行の `transferSize` 合計の最大 + pageLoad 分。`decodedBodySize` は伸長後サイズなので使わない。",
    "- 代理計測の値は Chromium + CPU スロットル + CDP 回線エミュレーションであり、実機（特に iOS Safari）の代理にはならない。",
    "",
  );

  const markdown = lines.join("\n");
  process.stdout.write(markdown);
  if (values.out !== undefined) {
    const out = resolve(process.cwd(), values.out);
    await writeFile(out, markdown, "utf8");
    process.stderr.write(`[bench:summarize] ${out} へ書き出しました\n`);
  }
}

await main();
