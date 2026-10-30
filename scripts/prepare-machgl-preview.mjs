#!/usr/bin/env node
// MachGL 先行版プレビューの生成スクリプト。
//
// 正式版の MachGL は 2026-11-09 の提供開始予定で、2026-10-30 時点では CDN も
// npm パッケージも存在しない。先行リリースのため、Geolonia が公開している
// MachGL デモ（https://geolonia.github.io/machgl-demo/）のバンドルとスタイルを
// ビルド時に取得し、次の 2 点だけを加工して web/public/machgl-preview/ へ置く。
//
//   1. 地図インスタンスを window.__machglMap に露出させる（1 箇所の置換）
//   2. style JSON の YOUR-API-KEY を GEOLONIA_API_KEY に差し替える
//
// 取得物はリポジトリへコミットしない（.gitignore 対象）。正式版の CDN が出たら
// このスクリプトを公式 URL の取得へ差し替え、bridge.js はそのまま使う。
//
// GEOLONIA_API_KEY が未設定ならデモキー（YOUR-API-KEY）で生成し、config.json に
// key: "demo" と記録する。本番では Workers Builds の環境変数に実キーを設定する。
// 取得に失敗した場合は config.json を enabled: false にしてビルドを続ける。
// その場合、アプリは「3Dで見る」を出さない。
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OUT = path.join(ROOT, "web/public/machgl-preview");
const BRIDGE = path.join(ROOT, "web/machgl-preview/bridge.js");
const BRIDGE_CSS = path.join(ROOT, "web/machgl-preview/bridge.css");
const DEMO_BASE = "https://geolonia.github.io/machgl-demo/";
const DEMO_KEY = "YOUR-API-KEY";

const apiKey = (process.env.GEOLONIA_API_KEY ?? "").trim();
const usingDemoKey = apiKey.length === 0;
const styleKey = usingDemoKey ? DEMO_KEY : apiKey;

/** 取得に失敗したときに、プレビューを無効化したままビルドを続ける。 */
async function disable(reason) {
  await rm(OUT, { recursive: true, force: true });
  await mkdir(OUT, { recursive: true });
  await writeFile(
    path.join(OUT, "config.json"),
    `${JSON.stringify({ version: 1, enabled: false, reason }, null, 2)}\n`,
  );
  console.warn(`[machgl-preview] disabled: ${reason}`);
}

async function fetchText(url) {
  const response = await fetch(url, {
    headers: { "user-agent": "shutoko-sim-machgl-preview/1 (+https://github.com/japan4415/shutoko-sim)" },
    redirect: "follow",
  });
  if (!response.ok) {
    throw new Error(`${url}: HTTP ${String(response.status)}`);
  }
  return await response.text();
}

async function main() {
  const demoHtml = await fetchText(DEMO_BASE);
  const bundlePath = demoHtml.match(/src="\.\/(assets\/index-[^"]+\.js)"/)?.[1];
  if (bundlePath === undefined) {
    throw new Error("デモ HTML からバンドルのパスを見つけられませんでした");
  }

  const bundle = await fetchText(new URL(bundlePath, DEMO_BASE));
  const constructor = bundle.match(/new ([A-Za-z_$][\w$]*)\(\{container:"map"/);
  if (constructor === null) {
    throw new Error("デモのバンドルから地図コンストラクタを見つけられませんでした");
  }
  let patchedBundle = bundle.replace(
    constructor[0],
    `window.__machglMap=new ${constructor[1]}({container:"map"`,
  );
  // 地図の hash 連携を止める。URL の #<id> をプレビューの受け渡しに使うため、
  // MachGL にズーム・緯度経度へ書き換えられると困る。
  patchedBundle = patchedBundle.replace("hash:!0,maxTileCacheSize", "hash:!1,maxTileCacheSize");

  // デモ HTML のインライン script は CSP で止まる。バンドル側は style-color.json を
  // 直接 fetch するフォールバックを持つので、削除して構わない。
  let page = demoHtml.replace(/<script>try\{var k=localStorage[\s\S]*?<\/script>/, "");
  page = page.replace(/<title>.*?<\/title>/, "<title>3Dルートプレビュー</title>");

  // デモ HTML のインライン <style> は CSP の style-src 'self' で止まるため、
  // 外部 CSS へ出してから読み込む。ブリッジの UI も同じ扱いにする。
  const demoStyle = page.match(/<style[^>]*>([\s\S]*?)<\/style>/)?.[1] ?? "";
  page = page.replace(/<style[^>]*>[\s\S]*?<\/style>/, '<link rel="stylesheet" href="./preview.css">');
  const previewCss = [
    demoStyle.trim(),
    "",
    "/* プレビューではデモ固有の操作パネルを隠し、3Dの地図だけを見せる。 */",
    "#topline, #sun, #keyhint, #card { display: none !important; }",
    'button[aria-label="現在地と向きに合わせる"] { display: none !important; }',
    "",
  ].join("\n");
  page = page.replace(
    "</head>",
    '<link rel="stylesheet" href="./bridge.css">\n</head>',
  );
  page = page.replace("</body>", '<script src="./bridge.js"></script>\n</body>');

  const styles = {};
  for (const name of ["style-color.json", "style-monotone.json"]) {
    const raw = await fetchText(new URL(name, DEMO_BASE));
    const replaced = raw.replaceAll(DEMO_KEY, encodeURIComponent(styleKey));
    // 置換で壊れていないことを JSON として確かめてから書く。
    JSON.parse(replaced);
    styles[name] = replaced;
  }

  const bridge = await readFile(BRIDGE, "utf8");
  const bridgeCss = await readFile(BRIDGE_CSS, "utf8");
  await rm(OUT, { recursive: true, force: true });
  await mkdir(path.join(OUT, "assets"), { recursive: true });
  await writeFile(path.join(OUT, "index.html"), page);
  await writeFile(path.join(OUT, "assets", path.basename(bundlePath)), patchedBundle);
  for (const [name, text] of Object.entries(styles)) {
    await writeFile(path.join(OUT, name), text);
  }
  await writeFile(path.join(OUT, "bridge.js"), bridge);
  await writeFile(path.join(OUT, "bridge.css"), bridgeCss);
  await writeFile(path.join(OUT, "preview.css"), previewCss);
  await writeFile(
    path.join(OUT, "config.json"),
    `${JSON.stringify(
      {
        version: 1,
        enabled: true,
        engine: "geolonia-machgl-demo",
        bundle: bundlePath,
        key: usingDemoKey ? "demo" : "env",
        generatedAt: new Date().toISOString(),
      },
      null,
      2,
    )}\n`,
  );
  console.log(
    `[machgl-preview] prepared ${bundlePath} (${String(patchedBundle.length)} bytes), key=${usingDemoKey ? "demo" : "env"}`,
  );
}

main().catch(async (error) => {
  await disable(error instanceof Error ? error.message : String(error));
});
