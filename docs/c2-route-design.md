# Issue #44 設計ノート: 中央環状線 C2 を対象路線に追加する

status: draft（実装前の設計判断材料。Issue #44 の完了条件に対する現状ギャップと、
推奨実装スライスを記録する）

## 1. 現状（2026-10 時点、release all-real-v4）

| 項目 | 状態 |
|---|---|
| OSM 取得 | 済。`scripts/fetch-osm.sh` は全 24 路線を取得し、C2 relation 4256077 を含む |
| graph への収録 | 済。`routeMemberships` に `route:C2:forward`（relationMainline 16 segment、非閉路）と `route:C2:inner` / `route:C2:outer`（boundRamp のみ）が存在 |
| C2 ランプ台帳 | 41 件（inner 21 / outer 20）。`verified_bound` 25 件（routable 24 + structural_no_loop 1）/ `unsupported` 16 件 |
| C2 課金ペア | **0 件**。`data/billing-pairs-seed.json`（C1 9 + radial 2）と `data/od-tariffs.json` assignments（10 件、すべて C1 + 2号）に C2 は無い |
| UI | `SUPPORTED_AREA_TEXT` は「対応範囲は首都高速 都心環状線（C1）とその接続ランプの検証済み入出口です。…」で C1 専用 |
| KNOWN_RELEASES | `c1-real-v1`〜`all-real-v4`（C2 ペア投入時は新 releaseId が必要） |

## 2. 本質的ブロッカー: C2 の環状 membership が構築できない

C1 は relation 4256008 が方向ロール（`Inner directions` / `Outer directions`）付きで
JCT 連絡路を含むため、`route:C1:inner` / `route:C1:outer` が「cyclic relationMainline
segment ちょうど 1 本」の閉路を形成する。legacyRing ペアの mandatory lap
（`relation_mainline_lap`）はこの閉路を要求する。

C2 relation 4256077 は:

- member 115 本すべて role 空欄（方向ロール無し）
- JCT 連絡路（江戸橋・箱崎・大井・大橋・小菅等）が relation 非加入

結果、`ref=C2` の way 群を有向グラフで見ると **35 の到達成分に分断**され、
segment 境界間は JCT 連絡路経由でも relation membership 内では到達不能。
ゆえに:

- `route:C2:inner` / `route:C2:outer` の cyclic relationMainline が作れない
- legacyRing sameNode プランの mandatory lap が解決できず、ペア導出・
  schema 4 serialization の両方が fail-closed になる
- 注意: ペアの**区間**（入口→次の出口）は 1 成分内に収まるため
  `relation_segments_match_required` の単一 segment 照合は成立しうるが、
  **lap（全周）**が 16 segment + JCT 連絡路の合成を要求する点がブロッカー

なお OSM 上流の role 付与だけでは不十分（JCT 連絡路の member 追加も必要）で、
上流修正は大規模編集かつ再取得とデータ鮮度の問題を伴う。

## 3. 対処案の比較

| 案 | 内容 | 評価 |
|---|---|---|
| (a) OSM 上流修正 | relation 4256077 への方向ロール付与 + JCT 連絡路 member 追加 | 綺麗だが OSM コミュニティ編集が必要で、再取得・データ鮮度・再現性の問題。短期の解にならない |
| (b) builder 拡張: multi-segment lap 合成 | `route_membership.rs` / `billing.rs` で JCT 連絡路経由の複数 segment lap 合成を gate 付きで許可 | C2 を機械検証に乗せられる正攻法。ただし fail-closed 原則に触る改修で、lap 合成の妥当性 gate（連絡路の公式性）設計が必要 |
| (c) ペアを別 plan 形態で登録 | radialReturn 系の明示 routePlan（directedSegments + entry/exitEndpoint）の仕組みを C2 に流用し、lap を明示エッジ列で宣言 | 既存の seed schema v2 機械構造を再利用できる。ただし lap エッジ列の手動宣言が 1 ペアごとに必要で、radial 機構が「lap を別路線に取る」前提でないかの確認が必要 |

## 4. 推奨スライス（案 (b) を段階的に入れる）

1. **Lap 合成の導入（最小）**: `route:C2:forward` の 16 segment を JCT 連絡路で
   連結した合成 lap を、`route_membership.rs` で「multi-segment cyclic lap」として
   構築する。合成には隣接 segment 間の連絡路が graph 上で Shutoko エッジとして
   存在し、かつ relation 4256077 の way 群と接続することを gate とする。
   - `relation_mainline_lap` は「cyclic 1 本」に加えて「合成 lap」を受け入れる
   - schema 4 の `routeMemberships` には合成 lap を単一 segment としてシリアライズ
     するか、複数 segment + lap 証跡として持たせるかを契約テストで固定する
2. **ペア 1 本の縦通し**: C2 の隣接 1 区間（例: 板橋本町入口 → 次の出口）を
   公式料金表の C2 行の距離セル審査（PDF セル単位の `distanceEvidence`。OSM 距離
   フォールバックは禁止）とともに seed 登録し、導出 gate 8/8 を通す
3. **UI・リリース**: `SUPPORTED_AREA_TEXT` を C1+C2 表記へ、`KNOWN_RELEASES` に
   新 releaseId（`all-real-v5`）を追加し、`all-real-v4` を rollback 先として残す
4. **docs**: product.md / delivery.md / routing.md の対応範囲記述を更新

## 5. 料金データの要件

- tariff v3 は `distanceEvidence`（公式 PDF のページ / 行 / ラベル / セル）を必須とし、
  OSM 距離からのフォールバックを禁止している（deprecatedAssignments の reason に明記）
- `tariffRules` に 2026-10 改定の距離式（32.472 円/km・0.1km 量子・10 円切上・
  最低 300 円・最高 2,130 円・端末 150 円）があり、C2 行の距離セル審査が確定すれば
  金額は式導出可能
- `.cache/official-fare/` の公式 PDF から C2 区間の距離セルを抽出する作業が
  ペア登録の単位コスト（C1 8 ペアに対し C2 は入口/出口 25 verified_bound から
  隣接ペア数十件が想定される）

## 6. 転送量（#43 への影響）

クライアントは graph.json（約 7.6MB）を取得する設計。billingPairs 10 件で約 69KB
なので、C2 ペア数十件の追加による増分は数十 KB 〜 100KB 程度で、Slow 4G 初回
ロード予算への追加影響は小さい。#43（転送量予算の構造化）は別途進めるべきだが、
C2 ペア追加の block にはならない。

## 7. 完了条件との対応

- [x] C2 の入口から出発地点を指定して候補が返る → **未達（lap 合成 + ペア登録が必要）**
- [x] 既存の C1 の 8 ペアと挙動が壊れていない → 現状維持（全テスト緑）
- [x] Slow 4G 初回ロードが 8,000 ms 以内 → ペア追加の増分は小さく、#43 と併せて計測
