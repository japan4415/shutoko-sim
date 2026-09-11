# 性能計測（bench）の手順とレポート雛形

issue #13 の性能目標（[delivery.md](../delivery.md) の「性能目標と測定」）を測るための
計測ハーネスとレポート置き場。計測は 2 経路ある。

1. **実機計測**（唯一の正）: スマートフォンのブラウザで `/bench.html` を開いて計測する。
   結果 JSON を `docs/bench/<date>-<device>/` に置く。
2. **代理計測**（下限見積もり）: Playwright で Pixel 5 相当のコンテキストを作り、
   CDP で回線と CPU をエミュレートして自動実行する。`npm run bench`。
   Chromium 限定であり、iOS Safari の代理にはならない。

## 1. 目標値と計測値の定義

### 目標値（docs/delivery.md より）

| 目標 | 値 | 判定に使う計測値 |
| --- | --- | --- |
| 成果物取得済みの探索 p95 | 2,000 ms 以内 | 全試行の `tSearchMs` の p95 |
| 初回ロード〜候補表示 p95 | 8,000 ms 以内 | cold 試行の `firstLoadMs` の p95 |
| 圧縮転送量 | 10 MiB 以内 | cold 試行の転送量合計の最大（+ ページロード分） |
| 探索ピークメモリ | 128 MiB 以内 | `memoryPeakMiB` の最大（`memorySource` を必ず併記） |
| 10 秒上限到達率 | 低いほどよい（目標値は未設定） | 10 秒で `terminate` した試行数 / 試行数 |

p95 は **nearest-rank 法**（昇順ソートの `ceil(p × n)` 番目）で求める。線形補間しないので、
返る値は必ず実測値のどれかになる（`web/src/bench/summarize.ts` の `percentile`）。

### 各計測値の定義

| 計測値 | 定義 | 実装 |
| --- | --- | --- |
| `tTransferMs` | Worker 生成〜成果物 5 件の取得・照合・WASM init 完了 | `web/src/bench/main.ts` |
| `tSearchMs` | `search` 呼び出し直前〜直後（**Worker 内で計測**）。成果物取得は含まない | `web/src/worker/search-worker.ts` |
| `tFirstCandidateMs` | Worker 生成〜候補カード（または status 文言）の描画確定（二重 `requestAnimationFrame` 後） | `web/src/bench/main.ts` |
| `firstLoadMs` | `tFirstCandidateMs` + `pageLoad.navigation` の `responseEnd`〜`loadEventEnd` | `web/src/bench/summarize.ts` |
| cold 転送量 | cold 試行の Resource Timing `transferSize` の合計 + ページロード分の最大 | `web/src/bench/summarize.ts` |
| `memoryPeakMiB` | メインスレッドと Worker のメモリサンプルの最大 | `web/src/bench/main.ts` |
| `memorySource` | そのメモリ値の出所（下記） | `web/src/bench/envelope.ts` |

**転送量は `transferSize` と `encodedBodySize` だけを見る。`decodedBodySize` は伸長後サイズで
転送量ではない**（graph.json では約 7.6〜10.2 倍あり、使うと 1 桁過大評価になる）。
`transferSize` が全エントリで 0（キャッシュヒット・cross-origin マスク）のときだけ
`encodedBodySize` の合計を下限として使う。

cold の転送量合計には **glue（`shutoko_routing.js`）の 2 エントリ**が含まれる
（text としての取得とモジュール import。2 回目は通常キャッシュヒットで数バイト）。
また `pageLoad.resources` は bench ページ自身の html / JS / CSS で、`/releases/` 配下は
Worker 側で収集するため二重計上しない。

### `memorySource` の値

| 値 | 意味 |
| --- | --- |
| `"performance.memory"` | ページ内 JS の `performance.memory`。**Chromium は 10 MB 単位に量子化**される |
| `"cdp-performance-metrics"` | 代理計測ランナーの CDP `Performance.getMetrics` の `JSHeapUsedSize`（250 ms 間隔の最大）。量子化は無いが**メインアイソレートのみ**で Worker のヒープを含まない |
| `"manual"` | 実機の Safari Web Inspector / Xcode Instruments で読んだ値を手入力したもの |
| `null` | どの手段でも取れなかった（判定は `UNKNOWN`） |

いずれも **WASM のリニアメモリを含まない**。128 MiB 判定はこの近似で行う。

## 2. 実機での計測手順

1. 端末のブラウザで計測ページを開く。
   - 本番: `https://<本番ドメイン>/bench.html`
   - ローカル: `workers/` で `npx wrangler dev --port 8787` を起動し、
     `http://<PCのIP>:8787/bench.html`（同一 LAN の端末から）
2. 端末情報（端末名 / OS / ブラウザ / 回線）を入力する。**iOS はここに Instruments で読んだ
   ピークメモリを入力する**（ページ内 JS からメモリを取る手段が無いため。`memorySource` は
   `"manual"` になる）。
3. 「計測開始」を押す。既定で 30 パターン × cold 3 回 / warm 3 回 = 180 試行を回す。
   端末では長時間かかるため、時間条件の違う代表数パターンに絞る場合は `?patterns=0,7,9` を使う。
4. 完了後に表示される JSON をダウンロードし、`docs/bench/<date>-<device>/` に置く
   （例: `docs/bench/2026-09-20-pixel8a/`）。ファイル名は `device.json` でよい。
5. 集計する。

   ```bash
   cd web
   npm run bench:summarize -- ../docs/bench/2026-09-20-pixel8a/device.json
   # 複数の端末・回線をまとめて 1 つの Markdown にする場合
   npm run bench:summarize -- ../docs/bench/2026-09-20-pixel8a/device.json \
     ../docs/bench/2026-09-20-iphone15/device.json --out ../docs/bench/2026-09-20-summary.md
   ```

6. 下の「記入欄」を埋めて、そのディレクトリの `report.md` に残す。

**注意**: 成果物は `Cache-Control: immutable` で 1 年キャッシュされる。cold を取り直すには
計測ページの nonce 付き URL（計測開始ボタンの経路）を使うか、ブラウザのサイトデータを消す。
warm だけを測りたいときは、計測ページを一度再読み込みしてから開始する。

## 3. 代理計測（Playwright）

```bash
cd web
npm ci
npm run build                              # dist に bench.html を出す
bash ../scripts/build-wasm.sh              # dist/wasm（seed:local の入力）
npm --prefix ../workers ci
npm --prefix ../workers run seed:local     # ローカル R2 へ成果物を投入
(cd ../workers && npx wrangler dev --port 8787 &)   # 計測中は立てたままにする

npm run bench -- --network fast4g --repeats 3 --out ../docs/bench/<date>-proxy/fast4g.json
npm run bench -- --network slow4g --repeats 3 --out ../docs/bench/<date>-proxy/slow4g.json
npm run bench:summarize -- ../docs/bench/<date>-proxy/fast4g.json \
  ../docs/bench/<date>-proxy/slow4g.json --out ../docs/bench/<date>-proxy/summary.md
```

`npm run bench` のオプション（`node bench/run-bench.mjs --help` でも出る）:

| オプション | 既定 | 内容 |
| --- | --- | --- |
| `--network` | `fast4g` | `fast4g` / `slow4g`（CDP の回線プリセット。単位 bytes/sec） |
| `--cpu` | `4` | CDP の CPU スロットル倍率 |
| `--patterns` | 全 30 | 実行するパターン index のカンマ区切り |
| `--repeats` | `3` | パターンごとの**コンテキスト数 = cold 試行数**（warm も同数得られる） |
| `--base-url` | `http://localhost:8787` | 計測対象のオリジン |
| `--out` | `web/bench/results/<network>.json` | 結果 JSON の出力先 |
| `--timeout-ms` | `180000` | 1 コンテキストの待ち上限 |
| `--headed` | ヘッドレス | 画面を出して実行する |

回線プリセットの実値は Chromium の devtools-frontend `NetworkManager.ts` 由来
（scout-004 F2）。**bytes/sec であって bits/sec ではない**。

| プリセット | download | upload | latency |
| --- | --- | --- | --- |
| Fast 4G | 1,012,500 B/s | 168,750 B/s | 165 ms |
| Slow 4G | 180,000 B/s | 84,375 B/s | 562.5 ms |

cold は試行ごとに `browser.newContext()` を作り直してブラウザキャッシュとストレージを
分離する（scout-004 F12）。warm は同じコンテキストで cold の直後に 1 回走らせる
（計測ページが `?repeats=1` で cold 1 回 → warm 1 回を実行するため、1 コンテキストから
2 試行が回収できる）。したがって `--repeats 3` は cold 3 回 + warm 3 回になる。

生の出力は `web/bench/results/`（`.gitignore` 済み）。**レポートする成果物は
`docs/bench/<date>-proxy/` に置いてコミットする。**

### 2026-09-11 の代理計測結果

`docs/bench/2026-09-11-proxy-pixel5-emulation/`（`fast4g.json` / `slow4g.json` / `summary.md`）。
Pixel 5 相当 / Chromium 153.0.8010.12 / CPU スロットル ×4 / 30 パターン × cold 3 回・warm 3 回
（各条件 180 試行、失敗 0 コンテキスト、タイムアウト 0）。所要は Fast 4G 5.3 分 / Slow 4G 17.3 分。

| 条件 | 探索 p95 | 初回ロード p95（cold） | cold 転送量 最大 | ピークメモリ | 10 秒到達率 | 合否 |
| --- | --- | --- | --- | --- | --- | --- |
| Fast 4G | 222 ms | 1,999 ms | 405.7 KiB | 9.5 MiB | 0.0 % | 4 目標すべて PASS |
| Slow 4G | 221 ms | 6,572 ms | 405.7 KiB | 9.5 MiB | 0.0 % | 4 目標すべて PASS |

- cold の `tFirstCandidate` p95 は Fast 4G 1,676 ms / Slow 4G 5,916 ms、warm は 418 ms / 823 ms。
  回線の差はほぼ逐次取得の RTT（Fast 4G 8 × 165 ms、Slow 4G 8 × 562.5 ms）で説明できる。
- 探索 p95 が両条件でほぼ同じ（222 / 221 ms）なのは、**CPU スロットルが Worker に効かず、
  探索が回線の影響も受けない**ためである（上の限界 2）。この値は実機の探索時間の代理にならない。
- Slow 4G の初回ロードは 8 秒予算に対して**余裕が約 1.4 秒**しかない。目標を割る前に
  成果物取得の並列化（6-1）を検討する。
- 転送量が両条件で同一（405.7 KiB）なのは内容が決定的なためで、回線条件に依存しない。

## 4. 代理計測の限界（レポートに必ず併記する）

1. **Chromium 限定**。CDP は Chromium 以外で例外になる（scout-004 F1）。Playwright の
   `devices['Pixel 5']` は UA / viewport / touch の模擬にすぎず、iOS のエンジン・JIT・
   メモリ挙動は再現しない。**iOS 側は実機計測だけが担う。** 代理計測は Android Chrome 系の
   下限見積もりとして扱う。
2. **CPU スロットルは探索に効かない**。CDP `Emulation.setCPUThrottlingRate` はレンダラの
   メインスレッドにのみ作用し、探索（WASM）が走る **Web Worker には適用されない**。
   実測でも `--cpu 1` と `--cpu 20` で `tSearchMs` が変わらない（249 ms / 245 ms。
   同じ条件でメインスレッド側の `tFirstCandidateMs` は 1,683 ms → 1,880 ms と 12 % 悪化する）。
   したがって**代理計測の探索時間は「スロットルなしのデスクトップ CPU」の値**であり、
   実機の探索時間の下限にもなっていない。`--cpu` は実質メインスレッド（ページ JS と描画）
   にしか効かない。Playwright は dedicated worker のターゲットに CDP セッションを張る API を
   公開していないため、この Run のスコープでは解消できない。
3. **CPU 4 倍の妥当性は未較正**。中程度のスマートフォン相当の暫定値として置いている。
   対象機種が確定したら、実機の `tSearchMs` と代理計測の `tSearchMs` の比で較正する。
4. **ローカルの圧縮は本番エッジと一致しない**。同一の graph.json（2,907,908 B）に対して、
   workerd（ローカル `wrangler dev`）の br は 247,561 B、本番エッジの br は 275,572 B で、
   **ローカルのほうが 11.3 % 小さい**（scout-004 F6）。転送量の絶対値は ±10 % 程度の誤差を
   見込む。ローカルの値は「圧縮が効いていること」「10 MiB 予算に収まること」の判定には
   使える。なお今回の `wrangler dev` は graph.json を **gzip** で返した
   （encodedBodySize 283,717 B。scout-004 F6 の workerd gzip 実測値と一致）。br ではない。
5. **本番は `engine.json` が未投入**。本番の `/releases/c1-real-v1/` に `engine.json` が
   無いため、本番エッジに対するベンチは cold が完走しない（`loadRelease` が 404 で落ちる）。
   代理計測は `seed:local` で `engine.json` を入れたローカル R2 に対して行う。
6. **メモリの分解能**。`performance.memory` は 10 MB 量子化、CDP の `JSHeapUsedSize` は
   メインアイソレートのみ。どちらも WASM リニアメモリを含まない。128 MiB 判定は粗い近似で
   あることを明記する。
7. **共有マシンの負荷**。代理計測は CPU とネットワークを占有する。他の重い処理（CI・
   ビルド・E2E）と同時に走らせると値が悪化する。計測中は並行実行しない。

## 5. 時間条件 3 件は探索コストを変えない

代表出発地点 10 件 × 時間条件 3 件 = 30 パターンは、**探索の計算量としては約 10 パターンに
縮退する**（scout-003 F4）。実 WASM で計測すると、神田橋入口では `max_minutes` を
15〜240 のどこに置いても `expandedStates` は 57,215 で完全に一定だった。時間窓は
探索後の候補フィルタ（`TIME_WINDOW`）にのみ作用する。

3 条件を残している理由は、結果カード件数と `TIME_WINDOW` 分岐の被覆である
（例: 神田橋 15〜30 は候補 0 件、15〜60 は候補 2 件）。**探索 p95 を 30 パターン分
測っても、独立な情報は 10 パターン分しか無い**ことをレポートで断る。

## 6. 目標を超過したときの改善候補

1. **成果物取得の並列化（RTT 8 → 4）**。`loadRelease` は manifest → engine → graph →
   wasm → glue を `await` で逐次取得するため最低 5 RTT、加えてページ本体で 3 RTT = 8 RTT。
   Fast 4G 165 ms × 8 = 1.32 s、Slow 4G 562.5 ms × 8 = 4.50 s が RTT だけで消費される
   （scout-004 F10）。manifest が全ファイルの URL を持つなら、engine / graph / wasm / glue の
   4 件は並列取得できる。**初回ロード 8 秒に対して最も効く候補。**
2. **graph.json の整形除去は効かない**。pretty-print を外しても圧縮後で 6〜9 % しか
   減らない（scout-004 F9）。先に 1 を試す。
3. **グラフのバイナリ化・ハンドル API**。graph.json は伸長後 2.9 MB あり、JSON parse と
   `SHA-256` 照合が cold の CPU を食う。バイナリ化（または R2 のハンドル経由で必要な
   部分だけ読む API）は根本対策だが、成果物の版を上げる必要がある。
4. **WASM の探索コスト自体**。`expandedStates` は起点ごとに 232〜57,480 と約 250 倍の開きが
   ある（scout-003 F5）。最悪ケース（exp ≈ 57k）で p95 2 秒を判定する。探索アルゴリズムの
   改善は別 issue。

## 7. レポート記入欄（雛形）

`docs/bench/<date>-<device>/report.md` に以下をコピーして埋める。

```markdown
# 性能計測レポート <date> <device>

## 計測条件

| 項目 | 値 |
| --- | --- |
| 端末 | |
| OS / バージョン | |
| ブラウザ / バージョン | |
| 回線 | （例: Wi-Fi / 4G / 5G。実測が望ましい） |
| 計測日 | |
| 成果物版（releaseId） | |
| 計測方法 | 実機 / 代理計測（Chromium + CDP） |
| 試行数 | パターン数 × cold/warm 回数 |

## 結果

（`npm run bench:summarize -- <file>` の出力を貼る）

| 目標 | 目標値 | 実測 | 合否 |
| --- | --- | --- | --- |
| 探索 p95 | 2,000 ms | | |
| 初回ロード p95（cold） | 8,000 ms | | |
| cold 転送量 最大 | 10 MiB | | |
| 探索ピークメモリ | 128 MiB | | |
| 10 秒上限到達率 | 低いほどよい | | |

## 判断

- 目標に対する合否と、その根拠:
- 代理計測の値をどう扱ったか（実機との比、較正の有無）:
- 次のアクション（改善候補 / 再計測 / 範囲縮小 / サーバー実行案の検討）:
```

## 8. 計測の検証（ハーネスが壊れていないこと）

フル代理計測は実行時間が長く、CPU / ネットワークスロットルは共有 CI ランナーで不安定に
なるため **CI には入れない**（scout-004 F14）。代わりに二段で腐敗を検知する。

- **Vitest**: 集計・p95・合否判定・envelope の検証を純粋関数として検証する
  （`web/test/bench-summarize.test.ts` / `web/test/bench-envelope.test.ts`。ブラウザ不要で決定論的）。
- **Playwright smoke**: 1 パターン × cold/warm 1 回をスロットルなしで回し、envelope が揃うこと
  と通常 UI が壊れていないことを見る（`web/e2e/bench-smoke.spec.ts`。既存の `npm run e2e` に含まれる）。

この 2 つは `npm run e2e` と `npm test` に入っているため、CI への追加配線は不要。
