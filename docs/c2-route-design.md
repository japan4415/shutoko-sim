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

**段2（次数平衡、実装済み）の結果**

`balance_carriageway_walk` で、方向別エッジ集合の入出次数差を最小コストの
追加エッジで解消するところまで実装した。次数の釣り合いは達成できる
（内周: run 1,228 + 追加 425 = 1,653、外周: run 1,068 + 追加 1,161 = 2,229）が、
**釣り合った多重グラフは依然として 1 つの弱連結成分にならない**ため、
Hierholzer の閉じたウォークが全エッジを消費できず合成は不成立のまま
（`closed_walk failed`）。残る作業は次数平衡と同時に成分連結も最小コストで
行う結合最適化（平衡のための追加エッジが成分をまたぐように選ぶ）で、
外周は追加エッジが 1,000 本超になることから、run 集合そのものを
1 成分に近づける割当（段1 の延長）を先に詰める方が現実的である。

**段1延長（成分連結）の試作結果: 性能的に不成立**

方向別の弱連結成分を最小コスト経路で連結し、その経路を方向ラベルに取り込む
処理を 2 通り試作した（成分ペア×代表エッジ×Dijkstra / 代表1本＋予算付き
Dijkstra＋最大8回）。いずれも `cargo test` が 400 秒を超えて完走せず、原因は
**連結でエッジを足すほど段2の次数平衡の探索（貪欲ペアリング×Dijkstra）が
組合せ的に重くなる**ことにある。外周は平衡だけで追加 1,161 エッジを要するため、
この漸進的な積み上げ方（割当→連結→平衡）は現実的でないと判断し、試作は破棄した
（リポジトリは段2までの状態を維持）。

### 2.3 lap 契約の決定（案 A: 同一エッジの複数回通過を許す）

C2 の周回は単純閉路にならない（§2.2 で実測）ため、**lap は同一エッジを複数回
通ってよい**ものとする。周回専用の検証契約は次のとおり。

- **lap（周回）**: 連続した有向ウォークであること（`edge[i].to == edge[i+1].from`）、
  非空、上限長以内。**エッジの重複は許す**。`forbiddenTransitions` は lap 全体に
  適用する。開始 `first_edge_id` と終了 `last_edge_id` は従来どおり必須。
- **relation membership segment**: 従来どおり**重複禁止**（単一通過の区間表現）。
  周回の重複は lap 側で表現し、membership には載せない。
- **routing-core の現状確認**: resolved mandatory lap は `ordered_edges`
  （`crates/routing-core/src/graph_v4.rs`）で検証され、これは
  `validate_ordered_graph_edges(..., reject_repeats = false)` を呼ぶ。
  つまり **lap の重複は現行コードでも既に許容されている**。重複を拒否するのは
  relation membership segment（`crates/graph-builder/src/route_membership.rs` の
  `validate_ordered_edges`）と `reject_repeats = true` で呼ばれる端点・短絡区間の
  検証だけであり、契約上の追加改修は不要。
- **意味論**: 同一エッジの再通過は「同じ物理区間を 2 度走る」ことを表す。
  候補生成と時間計算は lap のエッジ列をそのまま距離・所要時間に積算するため、
  追加の仮定は不要。

この契約により、合成器は「run 群＋最小コストの連絡路（重複可）で閉じた
1 本のウォーク」を作ればよくなり、単純閉路を要求しない。

**単一パス貪欲（連結優先＋平衡）の試作結果: これも閉じない**

逐次方式の性能問題を避けるため、連結（成分数の減少）を第一目的・平衡を
第二目的にした単一の貪欲パスを実装し、lap の重複通過も許した。実行時間は
実用範囲（テスト 80〜100 秒）に収まったが、**両方向とも閉じない**
（`uncomposed` のまま）。重複を許しても、連結と平衡を同時に満たす
エッジ追加列を貪欲では見つけられない。

**大域定式化の段1（不整合最小化の局所探索）の結果: 単一エッジ移動では不十分**

各エッジの inner / outer / 未使用ラベルを、両方向の入出次数不整合の総和が
最小になるまで決定的に動かす局所探索（`minimize_carriageway_imbalance`）を
実装した。1 エッジの付け替えでスコアが改善する限り動かす方式だが、
**不整合は 1 も減らず、合成も閉じない**（inner 1,228 / outer 1,068 のまま、
両方向 `uncomposed`）。不整合ノードのバランスを崩さずに改善するには
**複数エッジの同時付け替え**（＝最小費用流の増加路）が必要で、
局所探索の 1 手では到達できない。

**大域定式化の段2（最小費用の増加路による平衡）の結果: 供給/需要の多重度で破綻**

「その時点で最も安い供給→需要の最短経路を追加し、不整合が消えるまで繰り返す」
方式を実装した。各増加は正しい輸送問題の 1 手に見えるが、**収束しない**。

- 内周: 96 回の増加後も sources=6 / sinks=7 が残る（multiset 1,516）
- 外周: 同 sources=6 / sinks=6（multiset 2,412）

原因は 2 点。(a) ノードの次数差は ±1 ではなく **±k の多重度**を持つため、
経路を 1 本足すたびに供給・需要が 1 単位ずつしか減らず、残数が一致しないと
永久に閉じない。(b) 経路の中間ノードの次数は自動的に釣り合うので、
**「供給ノードから需要ノードへの輸送問題」として解く必要がある**（単純な
最短経路の貪欲ではない）。

正しい形は、ノードを頂点・C2 mainline エッジを容量∞・費用=距離のアークとし、
超過ノードを super-source、不足ノードを super-sink に繋いだ
**最小費用流（successive shortest path + ポテンシャル）**である。
実装は 200〜300 行程度で、Cargo 依存を増やさずに収まる見込み。

**大域定式化の段3（最小費用流の実装）の結果: フローは流れるが読み戻しが釣り合わない**

ノードを頂点、C2 mainline エッジを容量 64・費用=距離のアーク、超過ノードを
super-source、不足ノードを super-sink に繋いだ最小費用流を実装した
（successive shortest path + ポテンシャル、`min_cost_carriageway_flow`）。

- フロー自体は完走する（供給 = 需要で max flow が成立）
- しかし流した後の次数不整合ノードが **内周 13 / 外周 12 残る**
  （multiset 1,653 / 2,229）

読み戻し（`shipped`）の集計に欠陥がある可能性が高い。具体的には、
フォワードアークの `CAPACITY - arc.capacity` を「使用量」として数えているが、
増加路が逆向きアークを通る場合（残余グラフのキャンセル）に
フォワードアークの容量が戻るため、**キャンセルを含む正味の流量**を
数えられていない。正しくは各エッジについて
`net = capacity_initial - capacity_now`（フォワード）−（リバース側の増加）を
取るか、フロー反復中に「送った経路」をそのまま `multiset` に積む方式に
切り替える必要がある。

**段3の続き（読み戻し修正後の検証）: フローは正しい、多重集合への適用が誤り**

増加路を流れるたびに正味流量を積む方式へ直し、さらに「送った流量が需要を
満たしているか」を検証する計測を入れて確認した。

- `net_flow` は供給と**完全に一致**する（内周: supply 7・不一致 0 相当、
  外周: supply 6。検証式の符号を誤っていたため当初は不一致に見えていた）
- それでも `multiset` の残差不整合は内周 13 / 外周 12 ノード残る

原因は `net_flow` → `multiset` の適用側にある。

- `shipped` は `units > 0` のエッジしか積まないため、**正味流量が負のエッジ
  （増加路が逆向きに通って既存の流れを打ち消した分）が捨てられる**
- また `multiset` には run エッジが既に 1 回入っているので、補正フローを
  そのまま「追加回数」として足すだけでは、フローの前提
  （run エッジのフロー = 0）と食い違う

正しい適用は、**補正フローのネットワークに run エッジの既存 1 単位を
初期フローとして載せた残余グラフ**で最小費用流を解き、得られた正味流量を
そのまま周回エッジ列（重複込み）として使うこと。現在の実装は
「run エッジのフロー = 0」を前提にフローを解いているため、需要の符号が
run エッジの分だけずれる。

**段4（需要の符号と closed_walk の修正）: フローは実データで実行不能と確定**

残っていた「`min_cost_carriageway_flow` の最終需要チェックをノード単位で
計測する」を実施した。計測は一時的な `eprintln!`（環境変数で gate）で、
内周・外周それぞれについて次を出した。

- `total_supply == total_demand` は常に成立する（各エッジが +1 と -1 を
  1 つずつ寄与するため構造的に自明）
- 増加路の反復は `flow == total_supply` まで到達するか
- 各ノードで `net_flow` の入出次数差が需要と一致するか

その結果、関数内に閉じた欠陥が 2 つ確定した。

1. **需要の符号が逆**。呼び出し側は `demand = -initial`（run 次数差の符号
   反転）を渡していたが、関数は「正の demand = そのノードが受け取るべき
   純流入」として `node → sink` に繋ぐので、返る `net_flow` は
   `-demand = +initial` になる。必要な補正は `-initial` なので 1 段ずれて
   いた（計測例: run `n1→n2→n3` で `demand n1 = -1` に対し `net n1 = +1`、
   不一致 2 ノード）。`demand = initial` に直すと `net_flow` は `-initial`
   を満たし、3 ノード環の周回が閉じる。
2. **`closed_walk` の収集順が誤り**。辺を「辿った時点」（pre-order）で積んで
   最後に `reverse` していたため、分岐のある Euler 多重グラフでは連結で
   ない列が返り、閉路判定も落ちていた。3 ノードの単純閉路でさえ
   `closes = false` になる。Hierholzer どおり「そのノードの出辺を使い切った
   時点」（post-order）で積んでから反転するよう修正した。両方とも
   `route_membership.rs` の単体テストで固定した。

符号を直して実データ（`fixtures/osm/shutoko-all.json`、relation 4256077）を
再計測すると、**フローは完走しない**。

- 内周: `flow=5/7` で増加路が消える（残り供給 2・残り需要 2）
- 外周: `flow=4/6` で同上
- 残り供給から残余グラフを BFS すると到達可能は 15,268 / 22,824 ノードで、
  残り需要 2 ノードはいずれも**非到達**（`reachable=false`）
- 残り供給ノードは全域グラフでも出次数 1（例
  `n:263988138 -> e:w352063139:0:f`、`n:3804571875 -> e:w266086991:31:f`）。
  relation 4256077 のメンバー way 連鎖は 16 箇所で切れており、run はそこで
  止まる。閉路に必要な「run 終端 → run 始端」の有向経路が存在しない。

誤った符号で「フローが完走する」ように見えていたのは、補正が
**run 自身の経路（環状線の進行方向）をなぞる**だけで足りていたためである。
閉路に必要なのは逆に run 終端から始端へ戻る経路で、そこが relation の
欠落区間と一方通行の行き止まりに阻まれる。つまり C2 の閉路合成は
フローの実装ではなく **入力データの有向構造として不可能**であり、
0. の「relation のエッジ集合だけでは閉路にならない」を最小費用流の側から
再確認した形になる。

**段5（大域最適化の事前検証）: 入力グラフに C2 の閉路が存在しない**

大域最適化（各エッジを inner / outer / 未使用に割り当て、次数平衡と単一連結
成分を同時に満たす最小費用循環）を実装する前に、実データで実行可能性を検証
した。結論は「**入力グラフに C2 の閉路が存在しない**」で、定式化を変えても
解けない。以下はすべて Shutoko kind の有向辺・費用=距離での計測である。

- **C2 タグ付き one-way 辺の pool（2,636 辺 / 2,635 頂点）は DAG**。強連結成分
  はすべて大きさ 1 で、有向閉路が 1 本も無い。
- 同じ graph 上の **C1 relation 4256008 は閉じる**（1,185 辺中 1,116 辺が非自明
  SCC 内、最大 SCC 566 頂点）。つまり graph 構築側ではなく relation 4256077
  固有のデータ問題である。
- relation 4256077 のメンバー way 115 本を way の向きのまま有向辺にすると
  2,496 辺で非自明 SCC は 0。**無向**でも 1 連結成分のまま端点次数 1 が 15、
  次数 3 の分岐が 11 あり、単純な環ではない。
- 到達不能の実体: 例えば `n:263988138` から有向に辿れるのは 14 頂点の
  motorway_link ポケットだけで C2 頂点へ戻る道が無い（終端は `primary` 306 号）。
  `n:308925195` も 127 頂点のポケットに落ちる。
- C2 mainline（`highway=motorway` かつ ref/name C2）は 2,002 辺。全ノードで
  出次数 ≤1・入次数 ≤2 で、**5 つの source（入次数 0）と 2 つの sink（出次数 0）**
  を持ち、環ではなく 5 本の chain に分かれた DAG である。
- 方向ごとに独立な最小費用循環（容量 1・費用=距離、方向ラベル維持）を厳密に
  解くと、被覆は内周 1,075 / 1,228・外周 784 / 1,068。平衡は満たすが、非 C2 辺
  が内周 1,519 本（79 km）・外周 1,811 本（73 km）必要で、2 連結成分に割れる。
- 方向ラベルの維持を外すと内周 1,176 / 1,228（うち 421 辺は外周ラベルを流用）
  まで上がるが、これは C2 の片側車線の lap ではなくなる。
- 接続辺を C2 の 50 hop 近傍に制限すると **被覆は 0**（近傍に閉路が無い）。

（注: 当初の試作は方向ごとの平衡ではなく「両方向の合計」の平衡しか課して
おらず、上記はそのバグを修正した再測値である。結論は変わらない。）

「平衡＋単一連結」を満たす最小費用循環自体は存在するが、それは C2 の lap では
なく他路線を 100 km 級にわたって巡る経路になる。C2 の環状片側車線はこの入力
では閉じていない。

**したがって次の一手は、段の積み上げではなく定式化の変更**が妥当である。
ただし段5 のとおり大域最適化の前提（閉じた片側車線の辺集合）が入力に存在
しないため、**C2 の lap を graph から合成すること自体をやめる**のが正しい。

- **案 (c) を実装した（`explicitLapArc`）**: `routePlan.mandatoryLap` に省略可能な
  `explicitArc`（`edgeIds` + `edgeIdsSha256`）を追加した。指定がある場合、
  resolver は membership の relationMainline segment から arc を導出せず、
  宣言された M→B の Shutoko エッジ列をそのまま lap として使う。検証は導出 arc
  と同じ条件（graph に存在・連結・M 始点/B 終点・`firstEdgeId`/`lastEdgeId` と
  一致・sha256 一致・除外ショートコネクタ不使用・コネクタより長い・
  forbidden transition なし）を課す。`billingPairAdjacency` 側の `mandatoryLap`
  も同じフィールドを持つ。
  - relationMainline が断片化していて segment が M→B をまたげない pair でも
    解決できることを単体テストで固定
    （`explicit_lap_arc_composes_a_pair_whose_membership_is_fragmented`）。
  - 改ざん（hash 不一致・非連結・B 終端不一致）は拒否する
    （`explicit_lap_arc_rejects_tampering`）。
- **arc 生成を実装した**: `generate_explicit_lap_arc(graph, memberships,
  membershipId, M, B)` が membership の relationMainline 辺だけを使って M→B の
  arc を切り出し、sha256 を付けて返す。分岐は「B に到達できる辺」で解決するので、
  fork があっても M→B が一意なら成功し、2 本以上の経路があるときだけ fail-closed
  になる。`carriageway_lap_boundaries` は membership ごとの start / end / branch /
  merge ノードを返すので、どの M/B が宣言可能かを先に確認できる。CLI からも
  実行できる（どちらも schema 4 専用、membership id に `:` を含むため arc の
  3 引数はカンマ区切り）。
  ```
  --emit-lap-boundaries route:C2:outer
  --emit-lap-arc route:C2:outer,n:7160486512,n:1686031888
  ```
- **実データでの arc 生成結果**:
  - 外周: 5 starts / 3 ends。一意な chain が 3 本あり、`n:309603951→n:31330124`
    （322 辺）、`n:564683280→n:31330124`（202 辺）、
    `n:7160486512→n:1686031888`（428 辺）はそのまま `explicitArc` にできる。
  - 内周: 5 starts / 3 ends / branch 2 / merge 4。`n:263988137→n:263988138`
    （3 辺）と `n:263988137→n:8283617226`（101 辺）は一意に切り出せる。一方
    `n:31330142→n:3804571875` などは **n:7549622953 で 2 通りの経路が同じ end に
    達する**ため fail-closed。これは labelling が同一ノードから内周辺を 2 本
    出している（どちらかが誤り）ためで、arc 側では解決できない。内周の残りは
    大域割当かラベル修正でこの fork を解消する必要がある。
- **残る作業は pair への記入**: C2 料金ペアはまだ seed に無いので、ペアを追加する
  時点で上の arc を `explicitArc` に記入する。外周の 3 本と内周の 2 本はそのまま
  使える。

**段6（C2 ペア authoring）: 五反田入口 → 初台南出口を実データで投入した**

実在の C2 OD を 1 件選び、公式料金表から根拠を取って pair を組み立て、seed /
adjacency / od-tariffs に入れてフィクスチャまで再生成した。

- 選んだ OD: **五反田入口 → 初台南出口（C2 外周）**。両方とも
  `data/osm-ramp-bindings.json` で `verified_bound`（way 805171060 / 381972983）。
- 料金根拠: 公式「首都高 料金表」2025-04 版 PDF（sha256
  `dc2d80ee…`、リポジトリの `documents[0]` と一致）**page 25（料金・距離表、
  行=五反田・列=初台南）= 350 円 / 5.8 km**（ETC 普通車基本料金）。
  PDF は直接取得が 429 のためテキスト化プロキシ経由で取得し、表のグリッド
  （`extract_tables`）で行・列を確認した。
- 三重の整合確認:
  - 2022-04 ルールの式で 5,800 m を計算すると 350 円（58 unit × 2.952 円
    + 150 円 → ×1.1 → 10 円丸め）。表の 350 円と一致。
  - graph の C2 外周 arc（`n:3387909571 → n:919617354`）は **134 辺 /
    5,749 m** で、billed 5.8 km と整合。
  - 初台南は 五反田 の次の出口（表で 富ヶ谷 は初台南のさらに 0.4 km 先）。
- 2026-10 改定版（現行期間）の同じセルは **370 円 / 5.8 km**（改定版料金表
  page 25、sha256 `1dd86cf7…`）。`www.shutoko.jp` は今も HTTP 429
  （Vercel checkpoint）を返すため、以前のレビューで取得済みだった
  `.cache/official-fare/ryoukin-kaitei_toll_rates.pdf`（sha256 が記録と一致）を
  使ってテキスト層から読み直した。2026-10 ルールの式は 五反田 行の 7 セル
  （820/1,270/1,920/370/1,800/610/750 円）をすべて再現し、距離列も 2025-04 版と
  一致する。
- 生成物: `all-real-v4` フィクスチャで **billing pair 12 件（legacy 10 +
  radial 2）**。C2 ペアは legacyRing、anchor `n:3387909571`、
  `anchorToExitEdgeIds` 141 辺、tariff 350 円 / 5,800 m、eligibility
  `verified_one_section_ahead`。`fixtures/generated/*` を再生成し、
  `docs/data-pipeline.md` のサイズ表も実測に更新した。

**途中で止まっていた本当の原因**（最初のコミットで「membership id 衝突」と
書いたが誤りだったので訂正する）:

1. **legacy 検証は単一 run 前提だった**。`validate_relation_constrained_legacy_first_exit`
   は「初期辺を含む relationMainline セグメント」1 本の巡回順で relation order を
   作り、exit の分岐ノードがその中に現れることを要求する。C2 は membership が
   7 run に分かれ、ペアの走行は run 3 → 0 → 5 とまたがるため、単一 run では
   分岐ノードに到達できない。→ 到達可能性でガイドする前進ウォークを追加し、
   その順序が分岐ノードに実際に到達したときだけ採用する（C1 は従来どおり
   1 本の run で完結するので挙動不変）。
2. `relation_segments_match_required`（routing-core）も「relationMainline
   セグメントがちょうど 1 本」を要求していた。→ いずれか 1 本が要求辺列を
   含めばよい、に緩和。
3. **adjacency の `exitApproachEdgeIds` の起点を間違えていた**。exit の
   approach は「出口ランプ way の辺」ではなく「分岐ノードから出口の直前までの
   Shutoko 辺」で、起点は分岐ノードでなければならない。初台南では
   `e:w381972981:0..2` + `e:w381972983:0..2` の 6 辺（最後の Exit 辺は pair 側が
   付ける）。ここを直した時点で legacy 経路も通った。
4. 副次的に、`--release-id all-real-test` のような **C2 を展開しない構成**では
   route:C2:outer が ramp membership だけになる。C2 ペアを含む seed を回す
   リリースは C2 を展開する必要がある（フィクスチャ生成は `all-real-v4` なので
   影響なし。テスト側の legacy CLI 実行に `--all-route-relations` を足した）。

**段7（導出レポートの mandatory lap を分割 membership でも解決する）**

導出レポート（`pair-candidates.json`）のゲート 5 は
`docs/data-pipeline.md` で「M→B の通常長弧が `lapCount=1` で構成される」と
定義されているが、実装 `relation_mainline_lap` は「relationMainline セグメント
がちょうど 1 本の巡回列」を要求していたため、run に分割された C2 では
`PAIR_DERIVATION_RELATION_MAINLINE_AMBIGUOUS` で hold になっていた。

ゲート定義どおりに直した。環状合成できた membership（C1）は従来どおり
initial Edge から巡回列を回して境界（first の from / last の to が anchor）を
検証する。分割された membership（C2）は、initial Edge から exit の分流点へ
向かって「そのノードから分流点へ到達できる後続」が各ノードで一意な場合だけ
その弧を mandatory lap として採用する（`fragmented_relation_mainline_lap`）。
一意でなければ従来どおり fail-closed で hold に落ちる。

結果:

- C2 ペアの mandatory lap は **134 辺 / sha256
  `a5dcd6ef90ee479b6e3a39def414551e34ca624b9a3d0d5a12ce22a71c512fdc`**
  （`--emit-lap-arc` が出す arc と一致）。`sourceSegmentIds` は通過した 3 本の
  run（`relation:4256077:outer:split:0|3|5`）で、ゲートは `passed`。
- 候補 12 件 = **eligible 11 / hold 1**（残る hold は芝公園入口外回りの
  `access:conditional` のみ）。
- C1 と radial の 11 候補は lap の status・hash・source segment が変更前と
  **完全に一致**（この緩和は分割 membership のときだけ効く）。

**段8（2 件目: 内周の C2 ペアと、OD 全探索による arc 可用性）**

大橋JCT の fork をコード側で解こうとすると、既存のラベリング前提（run が fork で
切れていること）を壊し、C2 の割当が 2296 辺 → 1442 辺まで落ちた（未割当は
169 → 1035）。チェーンを結合する案は撤回し、原因を記録する:

- `n:7549622953`（内周）と `n:3387909708`（外周）は **大橋JCT**。relation は
  本線（`highway=motorway`、way 331692348 / 331922711）と JCT リンク
  （`highway=motorway_link`、way 331922702 / 331922695）を同じノードから
  分岐させており、`build_relation_member_paths` はリンクが混ざる fork で
  経路を切る。
- `n:8283617223` では member way 44211094 の終点の先を member way 891237687
  が**途中から**引き継ぐ（同 way は 309603951 から始まり 309603970 へ抜ける）。
  経路継続は member の内部ノードからの再開が必要になる。
- これらは relation 側（way の分割・役割）で直すべき問題で、コード側で
  併せて対応するとラベリングが壊れる。エラーは発生箇所（ノード・辺・member
  way）を報告するようにしたので、上流修正の検証に使える。

**代わりに、既存の arc で 2 件目を追加した**。全 OD 組み合わせ（内周 5 入口 ×
8 出口、外周 6 入口 × 6 出口）について、接続点 M→B の弧が一意に決まるかを
総当たりで調べ、公式料金表の距離と突き合わせた。採用したのは:

- **滝野川入口 → 中野長者橋出口（C2 内周）**: 142 辺 / 7,230 m、公式 7.6 km。
  2025-04 が 410 円、2026-10 改定が 440 円（いずれも rule 式が 7,600 m で
  再現）。**C2 内周では最初の商品ペア**で、弧の上に他の一般出口がない
  （= first exit 条件を満たす）。
- 導出レポートも 8 ゲートすべて `passed`（候補 13 件 = eligible 12 / hold 1）。

他の OD（滝野川→富ヶ谷 +89 m、西池袋→富ヶ谷 +272 m、小菅→平井大橋 +356 m など）
も arc は一意に取れる。ただし弧の上に別の出口がある OD は first exit 条件で
落ちるため、追加するなら「弧の上に他の出口がない」ことを先に確認する。

**段9（3 件目: 小菅入口 → 平井大橋出口と、却下した西池袋→富ヶ谷）**

段8 の候補のうち、弧の上に他の一般出口がない OD を実データで確定して 1 件追加
した。

- 採用: **小菅入口 → 平井大橋出口（C2 外周）**。弧は `n:370270147 → n:698279318`
  の 67 辺 / 4,444 m で、`firstExitInitialEdgeId = e:w32898851:1:f`、exit approach は
  `e:w383231007:0..2` + `e:w55555913:0..3` の 7 辺。公式表 page 25 の
  小菅入口 × 平井大橋出口セル（PDF の grid では行 平井大橋・列 小菅）は
  2025-04 が 320 円 / 4.8 km、2026-10 改定が 340 円 / 4.8 km で、いずれも
  4,800 m に対する規則式（弧との差 +356 m）と一致する。導出レポートは 8 ゲート
  すべて `passed`（候補 14 件 = eligible 13 / hold 1）。
- **却下: 西池袋入口 → 富ヶ谷出口（C2 内周）**。弧は一意（`n:1075239275 →
  n:3851577677`、117 辺 / 6,828 m、公式 7.1 km = +272 m）だが、弧の途中
  `n:3851530202` に **中野長者橋出口**の分流点があり、first exit ゲートの
  `RELATION_FIRST_EXIT_MISMATCH` で落ちる。滝野川→富ヶ谷（+89 m）も同じ理由で
  落ちる。全 OD 組み合わせの監査結果は「弧が一意 = 27 件、うち first exit を
  満たす = 11 件」で、商品化できるのはその 11 件のうち公式表にセルがあるものだけ
  である。監査は `.cache/match_c2_pairs.py` と `.cache/c2-arc-audit.py`
  （どちらも git 管理外の作業用スクリプト）で再現できる。

- **対応済み（段10 で実装）**: `explicitArc` の同一エッジ再訪。lap は 1 本の arc
  内で同じエッジを 2 回以上通ってよい。
- **authoring 用ツール（実装済み）**: seed に貼る値は CLI で生成できる。
  `--emit-lap-boundaries <membershipId>`、`--emit-lap-arc
  <membershipId>,<M>,<B>`、`--emit-ramp-endpoint <rampId>`（いずれも
  `--graph-schema 4 --inventory … --bindings …` と併用、出力は JSON 単体）。
- 併せて **案 (a)（OSM 上流修正）** を段11 で実測した。結論は「member の削除・
  分割だけでは run はつながらず、方向ロール付与と分岐ノードの解消が必要」である。
- 大域最適化（min-cost flow / 整数計画）は、上記いずれかで閉路が用意できた後の
  「割当の最適化」としてのみ意味を持つ。閉路そのものの生成には使えない。

**段10（`explicitArc` の同一エッジ再訪を実装した）**

段5 で残していた「lap が同じエッジを 2 回通る明示エッジ列」を解禁した。周回の
契約は段2.3 のとおりで、変更点は次の 3 か所である。

1. `resolve_explicit_lap_arc`（graph-builder）が `validate_ordered_edges` では
   なく `validate_ordered_edge_sequence_allowing_repeats` で arc を検証する。
   連続性・Shutoko 限定・M 始点/B 終点・`firstEdgeId`/`lastEdgeId` 一致・
   sha256 一致・除外ショートコネクタ不使用・コネクタより長い・forbidden
   transition なし、の各条件は変わらない。
2. `validate_resolved_route_plan(_segments)` は mandatory lap の leg だけ repeat を
   許し、他の leg（entry approach / return corridor / exit approach）は従来どおり
   単純列を要求する。
3. **routing-core 側の wire 検証**も同じ契約へ揃えた。`validate_radial_return_pair`
   は lap の leg だけ `reject_repeats = false` とし、lap の source binding に明示
   マーカー `explicit:<membershipId>` を許可する。明示 arc は 1 本の membership
   segment の部分列ではないため、従来の source segment 照合
   （`validate_resolved_segment_sources`）は適用できない。マーカーは mandatory lap
   にのみ許され、M/B 境界・除外ショートコネクタ・連結性の検証は引き続き効く。

回帰テストは builder 側の `explicit_lap_arc_allows_a_repeated_edge`
（`n:2 → n:3 → n:2 → n:3 → n:4` の往復を受理し、非連結な列は拒否）と
`promotes_an_explicit_lap_that_revisits_an_edge`（schema 4 へ昇格し、routing-core の
`prepare_json` が同じ lap を受理する）。

**段11（案 (a) の実測: 上流 relation 修正は何を直す必要があるか）**

上流 OSM を編集できないため、提案した relation 修正を**スナップショットのコピーに
適用して検証する道具**を追加した。

```
python3 scripts/apply-osm-relation-repairs.py --osm fixtures/osm/shutoko-all.json \
    --repairs data/osm-relation-repairs.json --out /tmp/shutoko-repaired.json
```

`data/osm-relation-repairs.json` は 3 種類の編集（`removeMemberWay` /
`splitMemberWayAtNode` / `setMemberRole`）を表現でき、適用後は通常どおり
graph-builder に読ませる。検証は builder 自身の診断メッセージで行う。

実測（すべて `all-real-v4` の入力で、relation 4256077 だけを差し替えた build）:

| 修正 | 結果 |
| --- | --- |
| なし（現行） | inner 9 run / 1,228 辺、outer 7 run / 1,068 辺。分割は 2,296 辺採用 / 169 辺未割当。一意に取れる弧は 27 件（first exit を満たすのは 11 件） |
| JCT リンク member の削除（28127318 / 541983769 / 331922702 / 331922695） | member walk が `n:8283617223` で止まり、build が `relation 4256077 direction forward would traverse a way outside the relation after member 44211094 at node n:8283617223 via edge e:w891237687:2:f to member way 44180278` で fail-closed。**診断が次の修正対象（node・edge・member way）をそのまま指す** |
| 内部継続の分割（891237687@n:8283617223、44130240@n:370270234） | build は成功するが membership は同一（inner 9 run / 1,228 辺、outer 7 run / 1,068 辺、距離も同じ）。C2 の run は**辺ラベルで切られており**、member 鎖の接続では変わらない |
| 削除 + 分割 | 派生ラベリングが悪化（2,296 → 1,424 辺採用 / 947 辺未割当）し、既存ペアも `legacy billing pair bp:c2-outer:kosuge-hirai-ohashi must resolve through an ordered mainline segment sequence (matches=0)` で解決不能になる |
| 方向ロール付与（現行ラベルから導出した 103 member、証拠の無い 12 member は削除） | build は成功し inner 1,237 辺 / outer 1,089 辺（+30 辺）になるが、run 数は 9 / 7 のまま、一意な弧も 27 件のまま |
| **`data/osm-relation-repairs.json`（roles 100 + 削除 15 + 分割 2）** | build は成功し **inner 7 run / 1,223 辺、outer 6 run / 1,087 辺**。run 端と run 始点の個数が inner 7 / 7、outer 6 / 6 で揃い（現行は 7 / 9、6 / 7）、**一意に取れる弧は 27 → 32 件に増える**（first exit を満たす 11 件は変わらない） |

`data/osm-relation-repairs.json` は上記 3 種の編集を 1 つの候補 changeset として
まとめたもので、`status: proposed_upstream_not_applied` と
`humanReviewRequired`（ロールは現行ラベル由来なので測量の裏付けが要る、証拠の
無い member は削除より方向付与を優先する）を明記している。release fixture は
このファイルを読まない。

**結論**: 案 (a) は段8 が挙げた 2 点（大橋JCT の fork と way 891237687 の内部継続）
だけでは足りない。run がつながらない本当の理由は、**member 鎖が JCT の fork /
merge と「続きの member が無いノード」で切れていること**で、上流には次の 3 つが
必要になる。

1. **方向ロール**: C1 relation 4256008 と同じ `Inner directions` / `Outer directions`
   を mainline member に付与する（role 無しのままだと builder は派生分割に頼り、
   2,296 辺しか採用できない）。方向の証拠は verified_bound ランプの接続点だが、
   現行ラベルでも 103 member は一意に決まり、12 member（大半が JCT リンク・
   ランプ・短い本線片）は人手判断が要る。
2. **fork / merge の解消**: relation 4256077 には 17 ノードで入出次数が 2 以上ある
   （例: `n:7549622953` 大橋JCT 内周 = 本線 331692348 とリンク 331922702、
   `n:263988137` = 本線 44209387 とリンク 28127318、`n:3387909578` = 本線
   331922708 と五反田出口ランプ 541983769）。どちらが同じ carriageway の続きかを
   relation 側で確定する必要がある。
3. **欠落 member の追加**: 方向ロール付与後の run 端（start に一致しないもの）へ
   有向に到達できる relation 外の道は、外周の `n:31330124 → n:6534476215`
   （22 way / 88 辺 / 10,376 m）を除いて見つからない。残りの区間は member 順・
   向きの問題か、片方向の行き止まりであり、単純な member 追加では閉じない。

**段12（幾何＋公開情報で carriageway ラベルを監査した）**

段11 では「ロール 100 件は現行ラベル由来なので測量の裏付けが要る、証拠の無い
15 member は方向付与か削除の判断が要る」と書いた。公開情報と OSM のタグでどこまで
機械的に決まるかを実測した結果、**大半は自動で確認でき、残る対象は 5 way
（約 100 辺）まで絞れた**。

道具: `scripts/audit-c2-carriageway-labels.py`

1. **anchor**: verified_bound な C2 ランプ 25 件について、ランプ自身の OSM way の
   末端（entry は合流点、exit は分流点）に最も近く、進行方向が一致する membership
   辺をそのランプの carriageway とみなす。ランプの方向は公式路線図に由来する
   `data/ramp-inventory.json` の値で、派生分割のラベルは一切使わない。
2. **伝播**: 共線継続（300 m 以内・cos ≥ 0.99）は同じ carriageway、逆平行ペア
   （60 m 以内・cos ≤ −0.9）は反対側。ramp 到達可能性フィルタは使わない。

結果（`all-real-v4`、relation 4256077）:

| 指標 | 値 |
| --- | ---: |
| anchor を置けたランプ | 20 / 25（高松・小菅 outer・中環小松川・清新町・王子北は合流/分流点の 150 m 以内に同方向の membership 辺が無い） |
| anchor と派生ラベルの一致 | 19 / 20（不一致は中環大井南 inner exit が way 331692346 上に 0 m で乗る件） |
| 分類できた membership 辺 | 1,796 / 2,296（78 %） |
| member way 単位 | 完全一致 59 / 部分一致 12 / 不一致 5 / 未分類 38 / ロール対象外 1 |

不一致 5 件（いずれも幾何は全票一致で、派生ラベルと逆）:

- **王子〜江北（飛鳥山トンネル周辺、lon 139.732–139.745）**: `44209387` /
  `780870438` / `44211094`（計 101 辺）は file=inner、幾何は outer。ただし
  Wikipedia の「中央環状王子線は飛鳥山トンネルを除くほぼ全区間が内回りを上、
  外回りを下にした上下 2 段」の**例外区間**にあたるため、`layer` による裏付けが
  使えない（実際この 3 way は layer=2/未設定で、周辺の上下 2 段区間の
  layer=3=inner / layer=2=outer パターンから外れる）。
- **大井JCT（lat 35.61, lon 139.756）**: `331692346` / `4854098` は file=outer、
  幾何は inner。中環大井南 inner exit の分流点が `331692346` の直上（距離 0 m、
  cos 1.00）に乗っており、2 次元幾何では上下（層）の別を判定できない。ここは
  品川線のトンネル区間で、両 carriageway が同じ緯度経度に重なる。

**商品への影響は無い**: 3 ペア（五反田→初台南、滝野川→中野長者橋、
小菅→平井大橋）の経路はどれもこの 5 way を通らない（`entryToAnchorEdgeIds` +
`anchorToExitEdgeIds` の way 集合で実測）。リスクは王子・板橋・大井方面の OD を
追加するときに限られる。

**段12a（監査の精度を上げた: 合流/分流点の取り方を修正）**

段12 の anchor は「binding の `motorwayNodeId` に最も近い membership 辺」で
代用していたが、これは誤りだった。exit の `motorwayNodeId` は Exit 辺の from
（ランプを降りた先）であり、本線との分流点ではない。正しくは

- entry: 合流点から **非 membership の Shutoko 辺を前向き**にたどり、最初に
  membership のノードへ着いた所が合流点
- exit: 分流点から **逆向き**にたどり、同じく最初の membership ノードが分流点

とし、そのノードでランプと同じ向きに走る membership 辺を anchor にする
（`scripts/audit-c2-carriageway-labels.py` の `junction()`）。この手順は 3 ペアが
`billing-pair-adjacency.json` に記録している合流/分流点 **6 件すべてを再現**する
（五反田 `n:3387909571`、初台南 `n:919617354`、滝野川 `n:4597822558`、
中野長者橋 `n:3851530202`、小菅 `n:370270147`、平井大橋 `n:698279318`）。

修正後の結果:

| 指標 | 段12（旧） | 修正後 |
| --- | ---: | ---: |
| anchor 設置 | 20 / 25 | 20 / 25 |
| anchor と派生ラベルの不一致 | **1** | **0** |
| 分類できた membership 辺 | 1,796 / 2,296 | 1,796 / 2,296 |
| way 単位（一致 / 部分 / 不一致 / 未分類） | 59 / 12 / 5 / 38 | 59 / 12 / 5 / 26 |

**大井JCT の疑いは解消した**。旧監査の「中環大井南 inner exit が outer ラベル
の way 331692346 に 0 m で乗る」は、binding の node を分流点と取り違えた
アーティファクトだった。正しい分流点 `n:6534476215` では同方向の membership 辺が
無く（＝この区間の内周本線が membership に無い）、`331692346`/`4854098` の outer
ラベルと公式ランプ方向は矛盾しない。

残る不一致は王子〜江北の 1 本の鎖（`44209387` → `44211322` → `28127449` →
`780870438` → `44211094`、計 166 辺）で、幾何は outer 全票、file は inner。

**段12b（membership に片側の carriageway が欠けている区間）**

監査に「その way の最寄りの *反対側* carriageway の辺は何 m 先か」を追加した。
環状線では上下 2 段または並行で必ず隣にあるため、これが数百 m 離れる区間は
**membership に片側しか入っていない**ことを意味する。

| way | file ラベル | 最寄りの反対側 carriageway の辺 | 距離 |
| --- | --- | --- | ---: |
| `44211322` | inner | `28127320` (outer) | 745 m |
| `709826346` | inner | `28127320` (outer) | 700 m |
| `44209384` | inner | `28127320` (outer) | 702 m |
| `28127449` | inner | `891237687` (outer) | 916 m |
| `28127324` | inner | `28127320` (outer) | 435 m |
| `44289390` | inner | `28127320` (outer) | 156 m |
| `1335055570` | inner | `28127113` (outer) | 170 m |

王子〜飛鳥山〜江北（lat 35.749–35.765、lon 139.71–139.76）は、**片側の
carriageway の辺が membership に存在しない**（169 辺の未割当のうちこの区間に
あるのは `28127324`(7)・`44289388`(7)・`867182415`(2)・`156344608`(1) の 17 辺
だけで、丸ごと 1 本分の欠落を埋められない）。飛鳥山トンネル内の内周
`28127324`(layer −1) と、ラベルが付かなかった `44289388`(layer −2) が対になる
ことから、欠けているのは外周側の可能性が高いが、王子北（内周）出口ランプ
`28127318` が鎖の南端 2 m にあること・上下 2 段の内外則が東側（lon 139.82）では
3 か所で逆転していることから、`layer` だけでは決められない。

**結論（段11・段12 の更新）**: 5 way のうち**大井JCT の 2 way は疑い解消**
（公式ランプ方向と一致）。残りは王子〜江北の鎖 166 辺で、問題は「ラベルが
逆」なのか「relation に片側の member が無い」のかの 2 択まで絞られた。どちらでも
**relation 4256077 はこの区間に穴がある**という点は同じで、n:8283617223 で
member walk が切れる場所と一致する。決めるには公式路線図の内外回り矢印か
航空写真で、王子北（内周出口）と王子南（外周出口）の接続先を確認する必要がある
（両ランプは 35.749–35.7525、lon 139.720–139.740 に 1.8 km 離れて位置する）。

**段12c（王子〜江北は確定: 内周ラベル 101 辺の誤り）**

外部情報を使わず、**member 鎖の向きとノード共有**で決着した。

1. anchor 済みの内周鎖は**西行き**である。`28127324`(layer −1) → `44289390`(layer 2)
   → `156344609`(layer 3、滝野川 inner entry の anchor) と、3 way すべて西向き
   （lon 139.7403 → 139.7142）。
2. 西向き鎖 `44180277` → `28127449` → `44211322` → `44209384` / `709826346` は
   **同じノード `n:308931779` で 1 の鎖に接続する**（`44209384` と `709826346` の
   終端 = `28127324` の始端 = `n:308931779`）。西行きのまま内周鎖へ入るので、
   この 5 way は内周で正しい。
3. 東向き鎖 `44209387` → `780870438` → `44211094`（101 辺）は、同じ station を
   **逆向き**に走り、始端 `n:263988137` は 1 の鎖の始端から 10 m しか離れていない。
   環状線で同じ station を逆向きに走るのは反対側の carriageway だから、
   **この 3 way は外周**であり、file の inner は誤り。

独立な裏付け 3 系統:

| 系統 | 内容 |
| --- | --- |
| 幾何監査（段12a） | まさにこの 3 way を outer と判定（全票） |
| layer と公表構造 | 江北の station は `44180277`(layer 3 = 上 = 内回り) と `44211094`(layer 2 = 下 = 外回り) の対で、Wikipedia の「王子線は内回りを上、外回りを下」と一致 |
| OSM の案内表示 | `44211094` の `destination=常磐道;湾岸線;京王道路;東関東道`（北東方面）は、江北から北東へ向かう外周の向きと一致 |

**内部矛盾テストを監査に追加した**（`scripts/audit-c2-carriageway-labels.py`）。
同一 station の 2 本は必ず逆走行なので、同じラベル同士が逆走行で並ぶのは矛盾で
あり、外部情報なしで検出できる。実測は 21 way 対 / 792 辺対で、上位は王子〜江北
（`44180277`×`44211094` 224、`44211322`×`780870438` 185、`28127449`×`780870438`
169）と大橋JCT（`766679040`×`768600220` 82、`78334074`×`766679040` 58）に集中する。

**結論（段11・段12 の更新）**: 5 way の判定は次のとおり。

- **大井JCT の 2 way**（`331692346` / `4854098`）: 疑い解消。公式ランプ方向と矛盾しない
- **王子〜江北の 3 way**（`44209387` / `780870438` / `44211094`、計 101 辺）:
  **外周であるのに内周とラベルされている**。これが段12b の「反対側 carriageway が
  700〜916 m 離れている」原因でもある（内周側に誤って入っているため、外周が
  membership から消えている）

つまり relation 4256077 に member の欠落は無く、**方向分割のラベル付けがこの
101 辺で誤っている**。修正は code 側（方向分割の anchor 伝播）で行うべきで、
上流 OSM の changeset はこの修正後のラベルを role として書き出す形になる。
3 ペアの経路はこの区間を通らないため、商品データへの影響は無い。

**段13（修正: 王子北 binding の誤りと、幾何ペアリングによる欠落補完）**

段12c の結論どおり code 側を直した。原因は 2 つで、両方を直した。

1. **データ（`data/osm-ramp-bindings.json`）**: `ramp:c2-inner:oji-kita-exit` の
   binding が **川口線（S1）のランプ way 592279108**（C2 本線から 1.6 km）を指して
   いた。合流/分流点の探索が 1,966 m さまよって**外周の東行き鎖に着地**し、そこを
   内周として seed していた。seeds は ramp id 順に適用されるため、正しい
   王子南（外周）の seed より先に当たり、誤りが確定していた。
   - 正しい binding: 王子北の出口ランプは way 53541951（標識 `赤羽;十条駅;王子駅`、
     Wikipedia の王子北出入口の本線標識と一致）が member way 28127449 と
     **ノード 676408802 を共有**して分流し、way 780869433 を経て way 210795860 の
     Exit 辺（n:1908031288 → n:2208424576）で北本通りに降りる。
   - binding は Exit 辺の way と両端ノードを記録する契約なので、way 210795860 と
     その両端を登録した。

2. **コード（`fill_carriageway_labels_by_pairing`）**: seed と relation 順の伝播は
   JCT で切れた run に届かず、`apply_exit_reachability_filter` も membership を
   たどるため同じ穴で止まる。そこで**環状線の幾何**で埋める: 1 つの station で 2 本の
   carriageway は逆向きに並ぶので、60 m 以内・cos ≤ −0.95 の辺は反対側、
   300 m 以内・cos ≥ 0.99 の辺は同じ carriageway。seed から外へ固定点まで広げる。
   既存ラベルは書き換えず、辺 ID 順・近い証拠優先で決定的に適用する。

効果（`all-real-v4` 再生成）:

| 指標 | 修正前 | 修正後 |
| --- | ---: | ---: |
| 採用エッジ（内周 / 外周） | 1,228 / 1,068 | **1,197 / 1,252** |
| 未割当（mainline 2,479 中） | 169 | **0** |
| 同一ラベル逆平行ペア（矛盾検出） | 792 辺対 / 21 way 対 | **291 / 12**（すべて大橋JCT のリンク way で、lane の折り返し） |
| 王子〜江北の鎖 `44209387` / `780870438` / `44211094` | inner（誤り） | **outer** |
| 大井・五反田の内周本線 `331922708`（146 辺） | inner | inner（維持） |

3 ペアの mandatory lap は **ハッシュまで不変**（`39ae3b0d…` / `a5dcd6ef…` /
`cf8552bf…`）。C1 の role 展開も不変。`fixtures/generated/*` と
`docs/data-pipeline.md` のサイズ表・`routeMembershipsSha256` を更新した。

**changeset 案の更新（段11 の再生成）**: 修正後のラベルから role を導出すると
**115 member のうち 111 件が一意に決まり**（mixed 0 件）、残り 4 件
（`541983769` / `805171062` / `78334075` / `766677827`）は member walk が届かない
ランプ・JCT リンクなので削除候補になる。`data/osm-relation-repairs.json` は
roles 111 + 削除 4 + 分割 2 に更新した。この changeset を
スナップショットのコピーに適用して build すると、relation が role を持つため
builder は派生分割ではなく **role 展開**（C1 と同じ経路）を使い、
`route:C2:inner` 7 run / 1,223 辺、`route:C2:outer` 6 run / 1,087 辺が得られ、
3 ペアもそのまま解決する（= 上流が role を持てば graph 側の推定は不要になる）。

**公開情報で決まる範囲**（当初「人手の測量が要る」と書いた部分の訂正）:

| 情報源 | 決まること |
| --- | --- |
| 公式路線図（`data/ramp-inventory.json` のランプ方向） | 25 ランプがどちらの carriageway に属するか。これが anchor になる |
| Wikipedia の路線構造（上下 2 段・層） | 上下 2 段区間では内回りが上・外回りが下。`layer` タグと突き合わせて内外が決まる |
| OSM のジオメトリ | 並行/逆平行の関係から same / opposite carriageway が決まる（78 % の辺） |
| **残る不明** | 飛鳥山トンネル区間（上下 2 段でない）と JCT 内部の立体交差。ここは路線図の矢印か航空写真での確認が要る（対象 5 way ≒ 100 辺） |

したがって run を 1 本の環状列に畳む作業は引き続き上流の編集待ちであり、
graph 側で合成する案 (b) は段2.2〜段5 のとおり実測で否定されている。今回の
道具は、その編集が filed されたときに「どのノードがまだ切れているか」を同じ
診断で確認するために使う。

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
