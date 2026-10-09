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

### 2.1 スライス1 の追加要件（実装時に確定した制約）

legacyRing ペアの sameNode route plan は `membership_id == route:{route_id}:{direction}`
を要求する（`validate.rs` の adjacency 検証）。C2 のペアは公式ランプ ID
（`ramp:c2-inner:*` / `ramp:c2-outer:*`）から `bp:c2-inner:*` / `bp:c2-outer:*` と
なるため、合成 lap は **`route:C2:inner` / `route:C2:outer` に置く必要がある**。
`route:C2:forward` に置いても direction チェックを通らない。

よってスライス1は次の 2 段構成になる:

1. **方向分割**: ロール無し relation の way を carriageway へ割り当てる。
   方向は way の oneway 向き（環状の2つの周回方向）と、verified_bound な C2 ランプ
   （`ramp:c2-inner/outer:*`、25 件）の mainline node がどちらの carriageway に
   接続するかの突き合わせから決定できる。ランプが接続しない成分は未割当のまま
   fail-closed に落とすか、明示的に forward のまま残すかを契約で固定する。
2. **環状合成**: 割当済みの成分鎖を JCT 連絡路（Shutoko エッジのみ・距離予算付き・
   決定論的タイブレーク）で連結し、単一の cyclic relationMainline segment として
   `route:C2:inner` / `route:C2:outer` を再構築する。合成が失敗する成分は
   従来どおり fail-closed（relation の expansion 報告に記録）。

この 2 段とも soundness コア（`route_membership.rs`）の改修であり、C1 の membership
生成を壊さない回帰テスト（C1 はロール付きで現行どおり 1 segment）とセットで
実装・検証する必要がある。

### 2.2 スライス1 前半（方向分割）の実装済み契約

`derive_carriageway_direction_split()` が方向分割を担う。relation に方向ロールが
無く、同一 route の `verified_bound` ランプが 1 件以上あり、member way が全て
oneway のときだけ発動する（それ以外は `None` を返し、従来のロール展開を維持する）。

- **接続点の決定**: ランプ evidence の ground 側端点（entry は `to_node_id`、
  exit は `from_node_id`）から、Shutoko エッジを距離予算
  `CARRIAGEWAY_SEED_REACH_METERS`（3,000 m）以内で Dijkstra 探索し、最初に
  到達した relation mainline node を接続点とする。entry は出辺、exit は入辺を
  たどる（exit は evidence が本線→地上の順に並ぶため逆走になる）。
- **シードの伝播**: entry は接続点の下流、exit は上流の mainline エッジに
  ランプ方向を付与する。続いて「mainline の predecessor（または successor）が
  全て同一方向」の未割当エッジへ固定点まで伝播する。
- **relation 順セグメント単位の確定**: 各セグメントで多数派方向を取り、
  少数派の比率が `C2_DIRECTION_MINORITY_SEGMENT_RATIO`（5%）以上ならその
  セグメント全体を未割当（fail-closed、`ambiguous_segment_ids` に記録）とし、
  未満なら多数派エッジのみを採用して少数派ラベルを捨てる。
- **出力**: `route:C2:inner` / `route:C2:outer` を relation 順の連続 run として
  生成し、`route:C2:forward` は発行しない（all-real-v4 の graph fixture から消える）。
  relation ロール付きの C1 は対象外で、従来どおり inner / outer 各 1 segment。

#### 実データ検証（release all-real-v4 の OSM スナップショット、relation 4256077）

| 項目 | 値 |
|---|---|
| verified_bound シード（接続点解決済み） | 25 / 25（inner 13 / outer 12） |
| シード衝突 | 0 |
| ラベル付与エッジ | 2,310 / 2,479 |
| majority ルール通過 | 1,986（inner 953 / outer 1,033） |
| fail-closed セグメント | 2（`relation:4256077:forward:8, 14`） |
| 未割当エッジ（ラベル無し） | 169 |
| 多数派ルールで棄却した少数派ラベル | 324 |

内訳: ラベル付与 2,310 = 採用 1,986 + 少数派棄却 324。未割当 169 は
ambiguous セグメント（ラベルは付くが方向を確定できない）と、どのシードからも
到達しなかったエッジの合計で、いずれも membership には載せない。

セグメント端の 1〜3 エッジの逆方向ランは JCT 境界の走査ノイズとみなして
切り落とす（`trim_boundary_outliers`）。この処理で `forward:6` と `forward:13`
（各 1 エッジの逸脱）が確定し、`forward:0` も採用に回った。`forward:8` は
52/52 のほぼ半々、`forward:14` は長い内周ランを残すため mixed のまま
fail-closed とする。

C1 回帰は `test_real_c2_carriageway_direction_split_from_verified_bound_ramps`
が同一テスト内で確認する（C1 inner / outer は relationMainline 各 1 segment のまま）。

#### 残るギャップ（スライス1 後半＝環状合成、未実装）

`relation_segments_match_required`（`crates/routing-core/src/graph_v4.rs`）は
解決対象の membership に **`relationMainline` segment をちょうど 1 本**しか
許さないため、複数 run のままでは legacyRing ペアが解決できない。方向ごとに
単一の cyclic segment へ畳む必要がある。

**割当の現状（2026-10、再伝播まで実装済み）**

ほぼ半々に割れていたセグメントは「そのセグメントのラベルを外して隣接
セグメントから再伝播させる」ことで確定し、fail-closed は 0 件になった。

| 項目 | 値 |
|---|---|
| ラベル付与 | 2,310 / 2,479 |
| 採用（inner / outer） | 1,228 / 1,068 = 2,296 |
| 出口到達フィルタ＋境界トリムで棄却 | 14 |
| 未割当（ラベル無し） | 169 |
| fail-closed セグメント | 0 |
| 方向別の成分 | 内周 1,124 + 34、外周 単一 1,151 |

**合成が閉じない理由（本質）**

連絡路の探索を「run 外エッジのみ」から「run エッジも可」に緩めると、
始点→終点の最短経路は**ほぼ必ず run エッジそのもの**になる
（内周: 採用 1,158 エッジに対し連絡路の重複エッジ 1,127、外周: 1,151 に対し 343）。
つまり採用エッジ集合は「単純閉路の部分集合」ではなく、**環状線の同じ区間を
複数回走る形でしか繋がらない**。Hierholzer で閉じた多重グラフを作っても
circuit 長が多重グラフ長に届かない（内周 1,725 / 2,307、外周 245 / 1,917）ことから、
次数の釣り合いを満たす単純閉路はこの集合には存在しない。

**したがって次の一手は、合成器の調整ではなく割当の再設計**になる。

0-a. **出口到達フィルタ（段1、実装済み）**: 各方向について、verified_bound な
   出口ランプの接続点から one-way を逆向きにたどり、その方向にラベルされた
   エッジだけを残す（`apply_exit_reachability_filter`）。JCT 分岐で誤って
   ラベルされた枝を落とし、方向別エッジ集合を「同一方向の出口へ到達できる」
   集合に揃える。効果は inner 1,209→1,228 / outer 1,088→1,068。
0. **relation のエッジ集合だけでは閉路にならない（実測で確定）**: 採用エッジを
   入口ランプから one-way のまま辿ると、必ず dead-end に当たる（relation 2,479
   エッジでは内周 5 始点すべて、外周 6 始点すべて）。relation 外の `ref=C2` /
   中央環状線 way を 164 エッジ足した 2,636 エッジの pool でも閉じない。
   つまり環状線の一部の区間は relation に含まれておらず、また pool 内には
   分岐（同一ノードから複数の C2 エッジ）が存在するため、局所的な
   「未使用の後続が 1 本なら進む」規則では carriageway を決められない。
1. **relation 順に依存しない割当**: 現在は relation メンバー順のセグメント単位で
   多数派を取るため、環状線の同じ物理区間を走る別チェーンが混ざる。JCT ごとに
   carriageway を決めるには、relation 順ではなく**有向グラフの次数**（oneway の
   連続性と入出次数の釣り合い）でラベルを解く必要がある。具体的には、
   ・各方向について「下流にその方向の出口ランプがある」エッジの集合を
     ラベル伝播の整合条件として使い、分岐では整合しない枝を落とす
   ・そのうえで入出次数が釣り合うまで、未割当エッジを最小コストで取り込む
   という 2 段の解き方にする（現行の relation セグメント多数派は廃止）。
2. **周回の定義を 1 本の単純閉路に限らない**: 合成 lap が同じエッジを 2 回以上
   通ることを許すなら、`relation_segments_match_required` の位置照合と
   `validate_ordered_edges` の重複禁止を、**周回（lap）専用の検証**として
   緩める設計が要る（現行の単純パス前提とは別契約）。
3. **lap の代替**: C2 ペアの lap を「relation mainline の単純閉路」ではなく
   「往復を含む明示エッジ列」として seed 側で宣言する（radialReturn 系の
   明示 routePlan を流用）。§3 の案 (c) に相当し、合成を諦めてペア単位で
   経路を宣言する方向。

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
