# 計測 集計

- 入力: 1 件（device.json）
- 合計試行: 180 件 / 10 秒到達: 0 件

## device.json

- 端末: Galaxy Fold8 Ultra / Android 17 / Chrome
- 回線: Wi-Fi / CPU スロットル: なし
- releaseId: c1-real-v1 / 試行: 180 件（cold 90 / warm 90）
- 10 秒到達率: 0.0 % / メモリ出所: performance.memory
- 目標値: 探索 p95 2000 ms / 初回ロード p95 8000 ms / 転送量 10.00 MiB / メモリ 128 MiB

### 条件別

| 条件 | 試行 | 10 秒到達率 | 探索 p50 | 探索 p95 | 初回候補 p50 | 初回候補 p95 | 初回ロード p95 | cold 転送量 最大 | メモリ 最大 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 全体 | 180 | 0.0 % | 218 ms | 289 ms | 732 ms | 1772 ms | 2155 ms | 461.7 KiB | 9.5 MiB |
| cold（キャッシュなし） | 90 | 0.0 % | 227 ms | 300 ms | 1672 ms | 1860 ms | 2243 ms | 461.7 KiB | 9.5 MiB |
| warm（キャッシュあり） | 90 | 0.0 % | 201 ms | 264 ms | 303 ms | 376 ms | 759 ms | - | 9.5 MiB |

### 目標合否

| 目標 | 目標値 | 実測 | 判定 |
| --- | --- | --- | --- |
| 成果物取得済み探索 p95 | 2,000 | 288.7 | PASS |
| 初回ロード〜候補表示 p95（cold） | 8,000 | 2242.6 | PASS |
| cold 転送量 最大（成果物 + pageLoad） | 10,485,760 | 472812.0 | PASS |
| 探索ピークメモリ | 128 | 9.5 | PASS |

### パターン別

| # | パターン | 試行 | 10 秒到達率 | 探索 p50 | 探索 p95 | 初回候補 p50 | 初回候補 p95 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | 0: kandabashi-short | 6 | 0.0 % | 235 ms | 259 ms | 327 ms | 2126 ms |
| 1 | 1: kandabashi-mid | 6 | 0.0 % | 245 ms | 269 ms | 347 ms | 1734 ms |
| 2 | 2: kandabashi-long | 6 | 0.0 % | 248 ms | 267 ms | 318 ms | 1694 ms |
| 3 | 3: takaracho-short | 6 | 0.0 % | 193 ms | 226 ms | 287 ms | 1645 ms |
| 4 | 4: takaracho-mid | 6 | 0.0 % | 199 ms | 224 ms | 300 ms | 1748 ms |
| 5 | 5: takaracho-long | 6 | 0.0 % | 206 ms | 229 ms | 298 ms | 1846 ms |
| 6 | 6: kasumigaseki-in-short | 6 | 0.0 % | 263 ms | 282 ms | 363 ms | 1728 ms |
| 7 | 7: kasumigaseki-in-mid | 6 | 0.0 % | 266 ms | 313 ms | 361 ms | 1763 ms |
| 8 | 8: kasumigaseki-in-long | 6 | 0.0 % | 275 ms | 302 ms | 384 ms | 1792 ms |
| 9 | 9: shibakoen-in-short | 6 | 0.0 % | 210 ms | 243 ms | 311 ms | 1743 ms |
| 10 | 10: shibakoen-in-mid | 6 | 0.0 % | 209 ms | 221 ms | 304 ms | 1687 ms |
| 11 | 11: shibakoen-in-long | 6 | 0.0 % | 208 ms | 237 ms | 313 ms | 1735 ms |
| 12 | 12: shibakoen-out-short | 6 | 0.0 % | 154 ms | 182 ms | 256 ms | 1756 ms |
| 13 | 13: shibakoen-out-mid | 6 | 0.0 % | 159 ms | 191 ms | 257 ms | 1691 ms |
| 14 | 14: shibakoen-out-long | 6 | 0.0 % | 156 ms | 192 ms | 272 ms | 1739 ms |
| 15 | 15: kasumigaseki-out-short | 6 | 0.0 % | 262 ms | 294 ms | 371 ms | 1819 ms |
| 16 | 16: kasumigaseki-out-mid | 6 | 0.0 % | 267 ms | 291 ms | 376 ms | 1754 ms |
| 17 | 17: kasumigaseki-out-long | 6 | 0.0 % | 258 ms | 288 ms | 372 ms | 1741 ms |
| 18 | 18: nihonbashi-short | 6 | 0.0 % | 201 ms | 228 ms | 305 ms | 1759 ms |
| 19 | 19: nihonbashi-mid | 6 | 0.0 % | 198 ms | 227 ms | 307 ms | 1719 ms |
| 20 | 20: nihonbashi-long | 6 | 0.0 % | 202 ms | 240 ms | 319 ms | 1772 ms |
| 21 | 21: yurakucho-short | 6 | 0.0 % | 247 ms | 285 ms | 353 ms | 1860 ms |
| 22 | 22: yurakucho-mid | 6 | 0.0 % | 255 ms | 640 ms | 732 ms | 30194 ms |
| 23 | 23: yurakucho-long | 6 | 0.0 % | 257 ms | 289 ms | 379 ms | 1986 ms |
| 24 | 24: ginza-short | 6 | 0.0 % | 57 ms | 78 ms | 185 ms | 1622 ms |
| 25 | 25: ginza-mid | 6 | 0.0 % | 45 ms | 79 ms | 168 ms | 1607 ms |
| 26 | 26: ginza-long | 6 | 0.0 % | 39 ms | 81 ms | 140 ms | 1648 ms |
| 27 | 27: daikancho-short | 6 | 0.0 % | 73 ms | 105 ms | 198 ms | 1621 ms |
| 28 | 28: daikancho-mid | 6 | 0.0 % | 64 ms | 100 ms | 202 ms | 1928 ms |
| 29 | 29: daikancho-long | 6 | 0.0 % | 69 ms | 97 ms | 199 ms | 1533 ms |

## 注記

- 初回ロード p95 は cold 試行の `firstLoadMs`（pageLoad の responseEnd〜loadEventEnd + 試行の tFirstCandidate）で見る。
- 転送量は cold 試行の `transferSize` 合計の最大 + pageLoad 分。`decodedBodySize` は伸長後サイズなので使わない。
- 代理計測の値は Chromium + CPU スロットル + CDP 回線エミュレーションであり、実機（特に iOS Safari）の代理にはならない。
