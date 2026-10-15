# 代理計測 2026-10-15: graph schema 5 導入後（Pixel 5 代理・Slow 4G）

Issue #43 の手順 5（再計測）。#103（成果物 5 件の並列取得）と #114（配布 wasm の `name`
セクション除去）、#115（graph schema 5: ノード ID の差分エンコーディング + compact 出力）
をすべて取り込んだ `all-real-v5` を、2026-10-12 の計測（[同条件](../2026-10-12-proxy-pixel5-c2-payload/summary.md):
Pixel 5 代理 / Chromium 153 / CPU スロットル ×4 / Slow 4G / パターン 0〜9 × cold 2 回・warm 2 回）
と同じ方法で測り直したもの。

## 結論: 初回ロード p95（cold）が予算内に収まった

| 指標 | 2026-10-12（並列化のみ） | **2026-10-15（schema 5 + wasm strip）** | 差 |
| --- | ---: | ---: | ---: |
| 初回ロード p95（cold） | 8,691 ms | **7,495 ms** | **−1,196 ms（−13.8 %）** |
| 初回候補 p95（cold） | 7,876 ms | 6,732 ms | −1,144 ms |
| cold 転送量 最大 | 1.06 MiB | **882.2 KiB** | **−199 KiB（−18.3 %）** |
| 探索 p95（warm） | 67 ms | 49 ms | −18 ms |
| 10 秒到達率（cold） | 0.0 % | 0.0 % | 変化なし |
| メモリ 最大 | 11.3 MiB | 9.5 MiB | −1.8 MiB |

転送量の削減（−199 KiB）は、schema 5 による graph.json の圧縮後サイズ削減
（gzip -9 比較で −123,939 B、`cf dev` の低圧縮 gzip でも −139 KiB）と、wasm の
`name` セクション除去（−48,884 B）で説明がつく。残差 691 ms に対して転送削減だけでも
約 −1.0 秒の効果で、予測（−0.86 秒）を上回った。

## 目標合否


- 入力: 1 件（slow4g.json）
- 合計試行: 40 件 / 10 秒到達: 0 件

## slow4g.json

- 端末: Pixel 5 emulation / desktop macOS / Chromium 153.0.8010.12
- 回線: slow4g (CDP) / CPU スロットル: ×4
- releaseId: all-real-v5 / 試行: 40 件（cold 20 / warm 20）
- 10 秒到達率: 0.0 % / メモリ出所: performance.memory
- 目標値: 探索 p95 2000 ms / 初回ロード p95 8000 ms / 転送量 10.00 MiB / メモリ 128 MiB

### 条件別

| 条件 | 試行 | 10 秒到達率 | 探索 p50 | 探索 p95 | 初回候補 p50 | 初回候補 p95 | 初回ロード p95 | cold 転送量 最大 | メモリ 最大 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 全体 | 40 | 0.0 % | 39 ms | 49 ms | 1150 ms | 6598 ms | 7361 ms | 882.2 KiB | 9.5 MiB |
| cold（キャッシュなし） | 20 | 0.0 % | 39 ms | 48 ms | 6437 ms | 6732 ms | 7495 ms | 882.2 KiB | 9.5 MiB |
| warm（キャッシュあり） | 20 | 0.0 % | 37 ms | 49 ms | 941 ms | 1129 ms | 1892 ms | - | 9.5 MiB |

### 目標合否

| 目標 | 目標値 | 実測 | 判定 |
| --- | --- | --- | --- |
| 成果物取得済み探索 p95 | 2,000 | 49.0 | PASS |
| 初回ロード〜候補表示 p95（cold） | 8,000 | 7495.4 | PASS |
| cold 転送量 最大（成果物 + pageLoad） | 10,485,760 | 903327.0 | PASS |
| 探索ピークメモリ | 128 | 9.5 | PASS |

### パターン別

| # | パターン | 試行 | 10 秒到達率 | 探索 p50 | 探索 p95 | 初回候補 p50 | 初回候補 p95 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | 0: kandabashi-short | 4 | 0.0 % | 37 ms | 45 ms | 1129 ms | 6534 ms |
| 1 | 1: kandabashi-mid | 4 | 0.0 % | 42 ms | 49 ms | 1124 ms | 6732 ms |
| 2 | 2: kandabashi-long | 4 | 0.0 % | 29 ms | 42 ms | 880 ms | 6370 ms |
| 3 | 3: takaracho-short | 4 | 0.0 % | 30 ms | 34 ms | 876 ms | 6379 ms |
| 4 | 4: takaracho-mid | 4 | 0.0 % | 37 ms | 46 ms | 941 ms | 6482 ms |
| 5 | 5: takaracho-long | 4 | 0.0 % | 39 ms | 69 ms | 1109 ms | 6568 ms |
| 6 | 6: kasumigaseki-in-short | 4 | 0.0 % | 39 ms | 48 ms | 1150 ms | 6781 ms |
| 7 | 7: kasumigaseki-in-mid | 4 | 0.0 % | 33 ms | 51 ms | 1014 ms | 6516 ms |
| 8 | 8: kasumigaseki-in-long | 4 | 0.0 % | 41 ms | 48 ms | 966 ms | 6430 ms |
| 9 | 9: shibakoen-in-short | 4 | 0.0 % | 30 ms | 36 ms | 934 ms | 6431 ms |

## 注記

- 初回ロード p95 は cold 試行の `firstLoadMs`（pageLoad の responseEnd〜loadEventEnd + 試行の tFirstCandidate）で見る。
- 転送量は cold 試行の `transferSize` 合計の最大 + pageLoad 分。`decodedBodySize` は伸長後サイズなので使わない。
- 代理計測の値は Chromium + CPU スロットル + CDP 回線エミュレーションであり、実機（特に iOS Safari）の代理にはならない。
- 配信はローカルの `cf dev`（workerd の低圧縮 gzip）。本番 Cloudflare は brotli を選ぶため転送量は本番値として扱わない（graph.json は brotli が gzip より大きい実測がある一方、wasm は brotli 優位。#43 のコメント参照）。
- 計測中のコンソールに `The Content Security Policy directive 'frame-ancestors' is ignored when delivered via a <meta> element` が記録された。これは bench.html の meta CSP に frame-ancestors が残っているため（製品ページ `index.html` では #110 で除去済み）。別途外す。


## スケール特性（Issue #43 の完了条件 2）

graph.json の gzip 転送量は edge 数にほぼ線形（現行: 22,987 edge で 525,118 B → **1 edge あたり ≈22.8 B**）。
初回ロード予算の残余（8,000 − 7,495 = **505 ms**）は Slow 4G で ≈99 KiB ≈ **4,400 edge**。
C1 単体の初期実装が 1,719 edge（2026-09-12 計測）だったので、**残余予算は C1 約 2.6 本分**。

したがって現状の構造（全路線を 1 つの graph.json に載せる）のまま次の路線を追加すると、
予算は 1〜2 路線で使い切る。路線追加を続けるには bench/README.md §6 の先の対策
（路線単位の分割配信と遅延ロード、または R2 ハンドル経由の部分読み）が次の一手になる。
