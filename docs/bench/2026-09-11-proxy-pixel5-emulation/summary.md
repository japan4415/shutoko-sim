# 計測 集計

- 入力: 2 件（fast4g.json, slow4g.json）
- 合計試行: 360 件 / 10 秒到達: 0 件

## ファイル別 合否

| ファイル | 試行 | 探索 p95 | 初回ロード p95 (cold) | cold 転送量 最大 | メモリ 最大 | 探索 | 初回 | 転送 | メモリ |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| fast4g.json | 180 | 222 ms | 1999 ms | 405.7 KiB | 9.5 MiB | PASS | PASS | PASS | PASS |
| slow4g.json | 180 | 221 ms | 6572 ms | 405.7 KiB | 9.5 MiB | PASS | PASS | PASS | PASS |

## fast4g.json

- 端末: Pixel 5 emulation / desktop macOS / Chromium 153.0.8010.12
- 回線: fast4g (CDP) / CPU スロットル: ×4
- releaseId: c1-real-v1 / 試行: 180 件（cold 90 / warm 90）
- 10 秒到達率: 0.0 % / メモリ出所: performance.memory
- 目標値: 探索 p95 2000 ms / 初回ロード p95 8000 ms / 転送量 10.00 MiB / メモリ 128 MiB

### 条件別

| 条件 | 試行 | 10 秒到達率 | 探索 p50 | 探索 p95 | 初回候補 p50 | 初回候補 p95 | 初回ロード p95 | cold 転送量 最大 | メモリ 最大 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 全体 | 180 | 0.0 % | 167 ms | 222 ms | 437 ms | 1670 ms | 1993 ms | 405.7 KiB | 9.5 MiB |
| cold（キャッシュなし） | 90 | 0.0 % | 167 ms | 222 ms | 1620 ms | 1676 ms | 1999 ms | 405.7 KiB | 9.5 MiB |
| warm（キャッシュあり） | 90 | 0.0 % | 167 ms | 222 ms | 363 ms | 418 ms | 741 ms | - | 9.5 MiB |

### 目標合否

| 目標 | 目標値 | 実測 | 判定 |
| --- | --- | --- | --- |
| 成果物取得済み探索 p95 | 2,000 | 222.0 | PASS |
| 初回ロード〜候補表示 p95（cold） | 8,000 | 1999.1 | PASS |
| cold 転送量 最大（成果物 + pageLoad） | 10,485,760 | 415485.0 | PASS |
| 探索ピークメモリ | 128 | 9.5 | PASS |

### パターン別

| # | パターン | 試行 | 10 秒到達率 | 探索 p50 | 探索 p95 | 初回候補 p50 | 初回候補 p95 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | 0: kandabashi-short | 6 | 0.0 % | 198 ms | 203 ms | 392 ms | 1656 ms |
| 1 | 1: kandabashi-mid | 6 | 0.0 % | 203 ms | 205 ms | 404 ms | 1661 ms |
| 2 | 2: kandabashi-long | 6 | 0.0 % | 203 ms | 204 ms | 404 ms | 1660 ms |
| 3 | 3: takaracho-short | 6 | 0.0 % | 161 ms | 163 ms | 368 ms | 1617 ms |
| 4 | 4: takaracho-mid | 6 | 0.0 % | 163 ms | 165 ms | 363 ms | 1614 ms |
| 5 | 5: takaracho-long | 6 | 0.0 % | 163 ms | 164 ms | 363 ms | 1614 ms |
| 6 | 6: kasumigaseki-in-short | 6 | 0.0 % | 216 ms | 218 ms | 408 ms | 1645 ms |
| 7 | 7: kasumigaseki-in-mid | 6 | 0.0 % | 222 ms | 224 ms | 420 ms | 1684 ms |
| 8 | 8: kasumigaseki-in-long | 6 | 0.0 % | 223 ms | 223 ms | 422 ms | 1672 ms |
| 9 | 9: shibakoen-in-short | 6 | 0.0 % | 165 ms | 171 ms | 371 ms | 1621 ms |
| 10 | 10: shibakoen-in-mid | 6 | 0.0 % | 169 ms | 171 ms | 370 ms | 1629 ms |
| 11 | 11: shibakoen-in-long | 6 | 0.0 % | 169 ms | 234 ms | 437 ms | 1702 ms |
| 12 | 12: shibakoen-out-short | 6 | 0.0 % | 127 ms | 128 ms | 328 ms | 1583 ms |
| 13 | 13: shibakoen-out-mid | 6 | 0.0 % | 129 ms | 130 ms | 324 ms | 1587 ms |
| 14 | 14: shibakoen-out-long | 6 | 0.0 % | 128 ms | 129 ms | 327 ms | 1584 ms |
| 15 | 15: kasumigaseki-out-short | 6 | 0.0 % | 211 ms | 213 ms | 409 ms | 1647 ms |
| 16 | 16: kasumigaseki-out-mid | 6 | 0.0 % | 216 ms | 220 ms | 416 ms | 1676 ms |
| 17 | 17: kasumigaseki-out-long | 6 | 0.0 % | 216 ms | 219 ms | 409 ms | 1670 ms |
| 18 | 18: nihonbashi-short | 6 | 0.0 % | 164 ms | 167 ms | 369 ms | 1621 ms |
| 19 | 19: nihonbashi-mid | 6 | 0.0 % | 167 ms | 167 ms | 365 ms | 1618 ms |
| 20 | 20: nihonbashi-long | 6 | 0.0 % | 167 ms | 171 ms | 363 ms | 1626 ms |
| 21 | 21: yurakucho-short | 6 | 0.0 % | 207 ms | 217 ms | 407 ms | 1657 ms |
| 22 | 22: yurakucho-mid | 6 | 0.0 % | 213 ms | 215 ms | 412 ms | 1670 ms |
| 23 | 23: yurakucho-long | 6 | 0.0 % | 214 ms | 216 ms | 411 ms | 1680 ms |
| 24 | 24: ginza-short | 6 | 0.0 % | 31 ms | 32 ms | 225 ms | 1468 ms |
| 25 | 25: ginza-mid | 6 | 0.0 % | 31 ms | 33 ms | 224 ms | 1472 ms |
| 26 | 26: ginza-long | 6 | 0.0 % | 32 ms | 32 ms | 223 ms | 1470 ms |
| 27 | 27: daikancho-short | 6 | 0.0 % | 58 ms | 64 ms | 259 ms | 1499 ms |
| 28 | 28: daikancho-mid | 6 | 0.0 % | 55 ms | 61 ms | 256 ms | 1497 ms |
| 29 | 29: daikancho-long | 6 | 0.0 % | 59 ms | 76 ms | 356 ms | 1496 ms |

## slow4g.json

- 端末: Pixel 5 emulation / desktop macOS / Chromium 153.0.8010.12
- 回線: slow4g (CDP) / CPU スロットル: ×4
- releaseId: c1-real-v1 / 試行: 180 件（cold 90 / warm 90）
- 10 秒到達率: 0.0 % / メモリ出所: performance.memory
- 目標値: 探索 p95 2000 ms / 初回ロード p95 8000 ms / 転送量 10.00 MiB / メモリ 128 MiB

### 条件別

| 条件 | 試行 | 10 秒到達率 | 探索 p50 | 探索 p95 | 初回候補 p50 | 初回候補 p95 | 初回ロード p95 | cold 転送量 最大 | メモリ 最大 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 全体 | 180 | 0.0 % | 167 ms | 221 ms | 847 ms | 5908 ms | 6564 ms | 405.7 KiB | 9.5 MiB |
| cold（キャッシュなし） | 90 | 0.0 % | 168 ms | 222 ms | 5855 ms | 5916 ms | 6572 ms | 405.7 KiB | 9.5 MiB |
| warm（キャッシュあり） | 90 | 0.0 % | 167 ms | 221 ms | 766 ms | 823 ms | 1480 ms | - | 9.5 MiB |

### 目標合否

| 目標 | 目標値 | 実測 | 判定 |
| --- | --- | --- | --- |
| 成果物取得済み探索 p95 | 2,000 | 221.1 | PASS |
| 初回ロード〜候補表示 p95（cold） | 8,000 | 6572.4 | PASS |
| cold 転送量 最大（成果物 + pageLoad） | 10,485,760 | 415485.0 | PASS |
| 探索ピークメモリ | 128 | 9.5 | PASS |

### パターン別

| # | パターン | 試行 | 10 秒到達率 | 探索 p50 | 探索 p95 | 初回候補 p50 | 初回候補 p95 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | 0: kandabashi-short | 6 | 0.0 % | 198 ms | 201 ms | 792 ms | 5860 ms |
| 1 | 1: kandabashi-mid | 6 | 0.0 % | 203 ms | 204 ms | 799 ms | 5904 ms |
| 2 | 2: kandabashi-long | 6 | 0.0 % | 203 ms | 204 ms | 804 ms | 5884 ms |
| 3 | 3: takaracho-short | 6 | 0.0 % | 161 ms | 163 ms | 771 ms | 5848 ms |
| 4 | 4: takaracho-mid | 6 | 0.0 % | 163 ms | 165 ms | 774 ms | 5858 ms |
| 5 | 5: takaracho-long | 6 | 0.0 % | 162 ms | 164 ms | 772 ms | 5854 ms |
| 6 | 6: kasumigaseki-in-short | 6 | 0.0 % | 215 ms | 217 ms | 824 ms | 5893 ms |
| 7 | 7: kasumigaseki-in-mid | 6 | 0.0 % | 221 ms | 222 ms | 825 ms | 5918 ms |
| 8 | 8: kasumigaseki-in-long | 6 | 0.0 % | 221 ms | 223 ms | 823 ms | 5922 ms |
| 9 | 9: shibakoen-in-short | 6 | 0.0 % | 164 ms | 166 ms | 765 ms | 5851 ms |
| 10 | 10: shibakoen-in-mid | 6 | 0.0 % | 167 ms | 169 ms | 769 ms | 5866 ms |
| 11 | 11: shibakoen-in-long | 6 | 0.0 % | 167 ms | 170 ms | 761 ms | 5860 ms |
| 12 | 12: shibakoen-out-short | 6 | 0.0 % | 125 ms | 127 ms | 719 ms | 5823 ms |
| 13 | 13: shibakoen-out-mid | 6 | 0.0 % | 127 ms | 128 ms | 728 ms | 5814 ms |
| 14 | 14: shibakoen-out-long | 6 | 0.0 % | 129 ms | 134 ms | 736 ms | 5835 ms |
| 15 | 15: kasumigaseki-out-short | 6 | 0.0 % | 212 ms | 230 ms | 824 ms | 5910 ms |
| 16 | 16: kasumigaseki-out-mid | 6 | 0.0 % | 216 ms | 241 ms | 847 ms | 5943 ms |
| 17 | 17: kasumigaseki-out-long | 6 | 0.0 % | 217 ms | 219 ms | 814 ms | 5915 ms |
| 18 | 18: nihonbashi-short | 6 | 0.0 % | 162 ms | 163 ms | 765 ms | 5859 ms |
| 19 | 19: nihonbashi-mid | 6 | 0.0 % | 170 ms | 184 ms | 782 ms | 5881 ms |
| 20 | 20: nihonbashi-long | 6 | 0.0 % | 167 ms | 178 ms | 769 ms | 5885 ms |
| 21 | 21: yurakucho-short | 6 | 0.0 % | 206 ms | 210 ms | 808 ms | 5897 ms |
| 22 | 22: yurakucho-mid | 6 | 0.0 % | 212 ms | 220 ms | 818 ms | 5916 ms |
| 23 | 23: yurakucho-long | 6 | 0.0 % | 212 ms | 212 ms | 812 ms | 5915 ms |
| 24 | 24: ginza-short | 6 | 0.0 % | 31 ms | 33 ms | 625 ms | 5715 ms |
| 25 | 25: ginza-mid | 6 | 0.0 % | 31 ms | 32 ms | 643 ms | 5711 ms |
| 26 | 26: ginza-long | 6 | 0.0 % | 31 ms | 33 ms | 626 ms | 5712 ms |
| 27 | 27: daikancho-short | 6 | 0.0 % | 53 ms | 53 ms | 660 ms | 5732 ms |
| 28 | 28: daikancho-mid | 6 | 0.0 % | 53 ms | 55 ms | 660 ms | 5737 ms |
| 29 | 29: daikancho-long | 6 | 0.0 % | 53 ms | 58 ms | 657 ms | 5742 ms |

## 注記

- 初回ロード p95 は cold 試行の `firstLoadMs`（pageLoad の responseEnd〜loadEventEnd + 試行の tFirstCandidate）で見る。
- 転送量は cold 試行の `transferSize` 合計の最大 + pageLoad 分。`decodedBodySize` は伸長後サイズなので使わない。
- 代理計測の値は Chromium + CPU スロットル + CDP 回線エミュレーションであり、実機（特に iOS Safari）の代理にはならない。
