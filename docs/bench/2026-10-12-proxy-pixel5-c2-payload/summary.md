# C2 を含む全ネットワークの初回ロード計測（Issue #43）

- 目的: Issue #43 の手順 1〜2（現状の転送経路の把握、路線追加後の増加量の実測）と、実装した対策（成果物 5 件の並列プリフェッチ）の before / after
- 計測日: 2026-10-12
- 端末: Pixel 5 emulation / desktop macOS / Chromium 153.0.8010.12
- 回線: Slow 4G（CDP: 下り 1.6 Mbps / 上り 0.75 Mbps / latency 562.5 ms）、CPU スロットル ×4
- 対象: `all-real-v5`（全 26 route relation / 22,987 エッジ / 22,824 ノード）
- 条件: 10 パターン × 2 反復 = 40 試行（cold 20 / warm 20）
- ハーネス: `npm run bench`（手順は [bench/README.md](../../../web/bench/README.md)、実装は `web/bench/run-bench.mjs`）

## 1. 成果物と転送量（cold 1 試行の実測）

| 成果物 | 生（decodedBodySize） | 転送（transferSize） | 圧縮率 |
| --- | ---: | ---: | ---: |
| graph.json | 7,655,199 (7.3 MiB) | 716,861 | 10.7× |
| shutoko_routing_bg.wasm | 1,641,258 | 351,892 | 4.7× |
| manifest.json | 49,308 | 11,452 | 4.3× |
| shutoko_routing.js | 11,858 | 3,496（import 時は 0） | 3.4× |
| engine.json | 712 | 664 | - |
| **合計（+ pageLoad 分）** | | **1.06 MiB** | |

graph.json が転送量の 66%、wasm が 32% を占める。graph.json の生バイトの内訳は edges 45%、nodes 17%、整形の空白 ≈32%、routeMemberships 3%、billingPairs 1%（`python3` で節ごとに再直列化して計測）。

## 2. 取得経路（Issue #43 の手順 1）

`web/src/worker/pipeline.ts` の `loadRelease` は manifest → engine.json → graph.json → wasm → glue の 5 往復を**直列**に行っていた（glue は `import` でもう 1 往復するが通常はキャッシュヒット）。Slow 4G の RTT 562.5 ms では、この直列化だけで ≈2.8 秒を要する。転送は 1.06 MiB ≈ 5.2 秒、残りは wasm のコンパイル、Rust 側 `prepare` の 7.65 MB JSON パース、sha256 照合などの CPU で、合計が after の ≈8.7 秒になる。

## 3. 実装した対策: 成果物 5 件の並列プリフェッチ

成果物のパスは release ごとに固定であるため、5 件のリクエストを 1 往復で出す。検証は依存順（manifest → engine → graph → wasm → glue）のまま変えず、**hash を照合する前に使う成果物は無い**。変わるのは「前段の失敗時に後段が飛行中になりうる」点だけで、失う帯域は wasm の ≈350 KiB が上限、かつ既に致命的な経路に限る。

## 4. 計測結果（before / after、10 パターン × 2 反復）

| 指標 | before | after | 差 |
| --- | ---: | ---: | ---: |
| 初回ロード p95（cold） | 10,836 ms | **8,691 ms** | **−2,145 ms（−19.8 %）** |
| 初回候補 p95（cold, `tFirstCandidate`） | 10,023 ms | 7,876 ms | −2,147 ms |
| 初回候補 p50（cold） | 9,982 ms | 7,715 ms | −2,267 ms |
| 10 秒到達率（cold） | **15.0 %**（20 試行中 3 件） | **0.0 %** | 消滅 |
| 探索 p95（warm、成果物取得済み） | 59 ms | 67 ms | 誤差 |
| cold 転送量 最大 | 1.06 MiB | 1.06 MiB | ±0 |
| メモリ 最大 | 11.3 MiB | 11.3 MiB | ±0 |

同じ 2 パターン（0,1）× 1 反復の対照計測でも 10,762 ms → 8,401 ms（−2,361 ms）で一致する。

- raw: [slow4g-before.json](slow4g-before.json) / [slow4g-after.json](slow4g-after.json)

## 5. 目標合否（after）

| 目標 | 目標値 | 実測 | 判定 |
| --- | --- | --- | --- |
| 成果物取得済み探索 p95 | 2,000 ms | 67 ms | PASS |
| 初回ロード〜候補表示 p95（cold） | 8,000 ms | 8,691 ms | **FAIL（−691 ms 不足）** |
| cold 転送量 最大 | 10 MiB | 1.06 MiB | PASS |
| 探索ピークメモリ | 128 MiB | 11.3 MiB | PASS |

## 6. 残差と次の対策（Issue #43 の手順 3〜5）

残る 691 ms は**ペイロードと CPU の側**にあり、往復の削減では届かない。選択肢:

1. **整形の除去**（[bench/README.md](../../../web/bench/README.md) 6-2）: graph.json の生 7.65 MB のうち ≈32 % は整形の空白で、これは JS 側と Rust 側の 2 回の JSON パースを重くしている。ただし scout-004 F9 の実測どおり**圧縮後の転送量は 6〜9 % しか減らない**ため、これは CPU 側の対策であって残差 691 ms の主因（転送 5.2 秒）には効かない。成果物のハッシュが変わるため release の再生成が前提
2. **グラフのバイナリ化・ハンドル API**（[bench/README.md](../../../web/bench/README.md) 6-3、根本対策）: ノード ID をインデックス化するなど。生バイトと解析時間の両方に効くが、同じく release 再生成が前提。バイナリは gzip が効きにくくなるため、転送量には中立か悪化しうる点に注意（現行 JSON は 10.7× 圧縮されている）
3. **本番の圧縮方式の確認**: 代理計測は gzip（10.7×）だが、Cloudflare のエッジが brotli を返すなら graph.json はさらに 15〜20 % 小さくなり、残差の大半が消える可能性がある（未検証。ローカルの dev サーバは gzip のみで、本番の圧縮・ヘッダとは挙動が異なる）

### スケール特性（Issue #43 の完了条件 2）

- エッジあたり: 生 333 B/edge、転送 31 B/edge
- 現行フォーマットで 8,000 ms に収まる追加余力: 転送 ≈80 KiB ≈ **2,600 エッジ（現状比 +11 %）**
- 全 26 route relation は既にペイロードに含まれるため、「路線を足す」余地はもう無い。次に増えるのはランプ台帳・バインディング・料金・membership の記述量で、上記の余力を超える前に 1・2 の対策が要る

## 7. 限界

- 代理計測は Chromium + CDP エミュレーションであり、iOS Safari と実機の代理にはならない（実機計測が唯一の正）
- ローカルの dev サーバ（cf dev = workerd）は gzip のみで、本番 Cloudflare の圧縮・ヘッダとは挙動が異なる。転送量は本番値として扱わない
- ノード ID のインデックス化など、成果物のハッシュを変える対策は未実施（release の再生成が必要なため、選定は別途）
