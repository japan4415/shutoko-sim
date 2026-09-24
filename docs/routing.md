# ルート探索設計

本文では、配信済みの現行挙動と将来の実装方針を区別する。現行の仕様は「現行」、Issue #42 で追加する設計は「設計（未実装）」と明示し、両方を同じ実装済み契約として扱わない。

## 探索問題

[原案](original.md)の「首都高を一周したうえで1区間先で降りると1区間分の料金になる」を前提に、その走り方が時間予算に収まる経路を探す。単なる最短経路や、任意の出口へ抜ける長距離経路は対象ではない。

出発地点 `s`、時間下限 `L`、上限 `U` に対して、座標検索では最近接の構造的に利用可能な入口 tier を優先して探索する（検証済み課金ペアがあれば優先評価し、なければ動的 OD を探索する）。`entryRampId` と `exitRampId` の双方を指定する明示検索でも、課金ペアseedの有無にかかわらず、`routingCapability=routable` の一般入口・一般出口間に「入口→アプローチ→本線閉路→イグレス→出口」を構成する。境界JCT、閉鎖、unsupported、`structural_no_loop`、kind不一致は探索端点にしない。出口から一般道で `s` へ戻ることは今回の企画提案で、首都高上の一周とは区別する。

## 実走行経路と料金の関係

```text
実走行: 出発地点 → 一般道 → 入口 → 基準点 → 一周 → 同じ基準点 → 1区間先の出口 → 一般道 → 出発地点
課金対象: 入口 → 1区間先の出口（実走行の一周分を加算しない）
```

「1区間先」は緯度経度の近さや出口番号の加算では決めない。方向・接続・課金条件を確認した `billingPair` で指定する。現行 schema 2/3 の C1 legacy は、そのペアに定義した本線の基準点へ、同じ進行方向で戻る非空の有向閉路を一周とする。単に道路名が環状線であることや、一般道で出発地へ戻ることを一周とは数えない。放射線を含む一般化は、後述する Directed Route-Plan Lap v1（設計・未実装）で定義する。

[首都高の料金距離説明](https://www.shutoko.jp/fee/fee-info/pay_etc/distance/)は、複数経路がある場合に入口出口間の首都高最短経路を料金距離とする原則を示している。具体的なペアの料金と利用条件は別途確認し、実走行距離へ単価を掛けて料金を計算しない。

## Issue #42: Directed Route-Plan Lap v1（route membership実装済み・残りは設計・未実装）

本節は、環状線だけを扱う現行モデルと、放射線から環状線を通って元の路線へ戻る経路を設計したもの。Issue #62 で seed schema v2 の parser と diagnostic radial pair 型を実装し、Issue #63 で graph-builder 内限定の `RouteMembershipIndex`、relation mainline / bound ramp の生成・検証、`find_first_exit_on_corridor` の基本処理を実装した。`--graph-schema 4` を明示した場合だけ `graph.json` の最上位に `routeMemberships[]` を含める。route plan の分解、schema 4 reader、公開 release への反映は #64〜#66 の範囲であり、現行 C1 8 ペアの挙動は変えない。

### 採用案は「指定 route の長弧を1周する」

対象は、一般入口から放射線へ入り、1本の環状線を指定方向で長弧として1通り、反対方向の放射線へ戻って一般出口で降りる経路に限定する。2本以上の環状線をまたぐ `multipleLoopPlans` は今回対象外である。

```text
entry approach
  → merge node M
  → 指定 route・direction の通常長弧（mandatory lap、lapCount = 1）
  → branch node B
  → return corridor
  → 最初の一般 Exit
  → exit approach
```

「1周」は、次の条件を満たす `routePlanLapV1` とする。

- 開始境界 M と終了境界 B を明示し、間に `routeId`、`direction`、最初・最後のエッジを宣言する。
- 環状線上の通常長弧を1通りだけ使う。JCT の短い連結路を周回として数えない。
- 放射線では M と B を別のノードとして保持し、どちらか一方へ寄せない。
- 全体が単純閉路である必要はない。entry と return corridor が同じノードを再訪することは許す。
- 同じエッジを2回通った場合も、(edge ID, 出現順) として保持する。`edgeIds` を集合にまとめない。
- 周回数を増やす代わりに時間を埋めない。`lapCount` は1に固定する。

現行C1の`anchorOsmNodeId`は、`anchorKind=sameNode`として扱う。既存のanchor、First Exit、料金レコード、検証状態は変えない。

### 「1区間先」は商品上の入口・出口関係であり、料金区間数ではない

「1区間先」は、mandatory lap を終えて return corridor へ入った後、その路線と方向を保った最初に到達する一般 Exit と定義する。次のいずれも採用理由にしない。

- 全グラフで、出口までの距離が最短の Exit
- entry approach 中に通り過ぎた C1 の Exit
- entry と同じ施設名の Exit
- exact binding が未解決だから、その次の supported Exit
- 別の JCT arm へ短く切り替える近道

2号目黒線では、公式施設順と現行スナップから天現寺が次の出口候補となる。天現寺出口の exact directed OSM binding が解決するまでは、`verified_one_section_ahead` に昇格させない。目黒出口や他の supported 施設へ飛ばして「1区間先」を作らない。

「1区間」は料金制度に存在する計算項目ではない。商品上で表示する入口 → 出口の関係であり、金額は常に公式の入口・出口間 `billingDistanceMeters` と料金規則から別に求める。金額、料金距離、端点 support、商品適格性を一つの `status` に混在させない。

### 比較した案と不採用理由

| 案 | 周回と出口 | 採否 | 判断理由 |
| --- | --- | --- | --- |
| A. M→B の通常長弧を1周とする | B へ入った return corridor の最初の一般 Exit | 採用 | Issue #42 の「放射線→環状線→同じ放射線」に最も直接対応し、短い connector を明確に除外できる。C1 も同じ型へ正規化できる。 |
| B. M→B→M を閉路として1周する | M へ戻ってから M→B へ進み、その先の Exit | 不採用 | 幾何的には真の閉路だが、もう1周分の C1 長弧が必要で、往復の説明が難しくなる。短い connector を周回として数える危険もある。 |
| C. C1 周回後に C1 最初の Exit で降りる | C1 の最初の Exit | 不採用 | 説明は簡単だが、entry と同じ放射線へ戻る要件を外し、inner C1 の既知 First Exit 不整合も引き継ぐ。 |
| D. 同じ施設名の Exit まで戻る | 同名施設 Exit | 不採用 | 「目黒→目黒」は自然でも first exit の意味を定義できず、binding 欠損時の skip 理由にもならない。 |
| E. 任意の 5km 以上 SCC 閉路 | Exit は別途探索 | 不採用 | 路線・方向・JCT の役割を保証せず、短い接続路を誤って選ぶ。現行の動的 OD 診断に限って保持する。 |

### anchor は2種類の判別値で表す

`anchorKind` の値は `sameNode` と `directedJunction` の2つだけとし、seed、generated graph、Candidateで共通使う。旧名の `sameNodeLoopAnchor` / `directedJunctionLoopAnchor` は別typeとして作らない。

| `anchorKind` | 必須field | 用途 |
| --- | --- | --- |
| `sameNode` | `nodeId`, `routeId`, `direction`, `arcPolicy` | 現行C1。開始と終了を同じノードとして表す。 |
| `directedJunction` | `mergeNodeId`, `branchNodeId`, `mergeTerminalEdgeId`, `branchInitialEdgeId`, `routeId`, `direction`, `arcPolicy`, `excludedShortConnector` | 放射線 → 環状線 → 放射線。MとBをdirected JCT pairとして表す。`mergeTerminalEdgeId`はM直前、`branchInitialEdgeId`はB直後のEdge。 |

mandatory lap自身のfirst / last Edgeは`routePlan.mandatoryLap.firstEdgeId` / `lastEdgeId`に置く。`anchorNodeId`、`entryToAnchorEdgeIds`、`anchorToExitEdgeIds`は`legacyRing` variant専用である。radial variantにMを`anchorNodeId`として詰め込まず、M/B pairと`resolvedRouteSegments`を正本にする。schema 1からgraphへ正規化する際、routeとdirectionは一意に解けたmembershipから導出し、候補が0件または複数ならgraph 4への昇格を拒否する。

### route と direction は本線relationとramp bindingを別々に証明する

現行 Edge には route membership と走行方向がない。edge kind だけで First Exit を求めると、B 付近の C1 出口や別 arm への近道を先に拾う。Issue #63 で、graph-builder の明示的な `--graph-schema 4` 出力に top-level `routeMemberships[]` を追加した。`RouteMembershipIndex` は OSM route relation の ordered member と way の node 順を `relationMainline` として写像し、正規ランプ台帳と exact directed binding を `boundRamp` として別々に保持する。

1つの`RouteMembershipIndex`は`membershipId`、`routeId`、`direction`、`segments[]`を持つ。各`RouteMembershipSegment`は`sourceKind`で由来を分ける。

- `sourceKind=relationMainline`: OSM route relationのordered memberとwayのnode順をgraph Edgeへ写像する。`sourceRelationId`とsnapshot hashを必須にする。
- `sourceKind=boundRamp`: 正規ランプ台帳とexact directed bindingから、wayをまたぐ順序付きEdge列を作る。`bindingEvidenceId`を必須にし、relation memberであることを求めない。

OSM route relationはmainlineを列挙し、目黒entry way `207535708`や天現寺exit候補way `172358461` / `422023171`を含まない。rampをrelationの連続Edge列へ強制すると、正しいbindingを誤って無検証にする。mainlineとrampを同じ`sourceKind`へ混ぜない。

route plan の leg は `sourceSegmentIds[]` で mainline と ramp の由来を明示する。`mandatory_lap` は 1 つの `relationMainline` の連続部分列でなければならない。entry、return、exit は `relationMainline` と `boundRamp` を順番に連結できるが、各 segment 内部の Edge 順、node 接続、hash、binding 証拠を個別に検証する。Edge ごとに route metadata を複製せず、index から検索・検証する。名前や最接近 node だけで所属を補わない。

次の異常系は graph-builder の synthetic fixture と Issue #63 実装で検証する。

- C1内回りを要求しているのにouterのmember Edgeを使う。
- 一ノ橋JCTで別armへ切り替える近道を使う。
- relationに含まれないmainline wayを使う。
- relationに含まれないrampをbinding証拠なしで使う。
- multi-way rampのway順、node接続、Edge順、hashを検証する。
- 同名または近接した別JCTのEdgeを使う。
- excluded short connectorをmandatory lapとして選ぶ。
- relationの順序と逆順にたどる。

### 2号目黒線の課金ペア形状（設計・未実装）

目黒入口から2号上り、一ノ橋 JCT で C1 に入る。entry Edge は `e:w207535708:0:f`、ramp ID は `ramp:2-inbound:meguro-entry` であり、現行 binding は `verified_bound` である。C1 を長弧で1通りした後、2号下りへ戻り、次の一般 Exit 候補を天現寺とする。

| 項目 | C1 inner | C1 outer |
| --- | --- | --- |
| 定義 ID | `bp:2-inbound:meguro:c1-inner:tengenji` | `bp:2-inbound:meguro:c1-outer:tengenji` |
| 目黒入口から M まで | 3,808m、Edge 終端 `e:w4853804:16:f` | 3,887m、Edge 終端 `e:w45248631:3:f` |
| M → B の長弧 | 13,304m、527 edges | 13,352m、546 edges |
| M / B | `n:574460576` / `n:574460605` | `n:31297008` / `n:31297000` |
| 長弧の first / last Edge | `e:w23297444:43:f` / `e:w23297444:19:f` | `e:w24039737:24:f` / `e:w24039737:3:f` |
| 除外する B → M 短 connector | way `23297444`、23 edges、493m | way `24039737`、20 edges、461m |
| B 後の2号下り initial Edge | `e:w45248411:0:f` | `e:w4853805:0:f` |
| 天現寺候補までの距離 | 1,972m | 1,846m |
| 天現寺 Exit の exact binding | 未解決 | 未解決 |
| 公開可否 | `unverified`、公開 blocked | `unverified`、公開 blocked |
| 料金 | `unpriced`、`amountYen=null`、`billingDistanceMeters=null` | 同左 |

この2件は、wire-level schemaとroute shapeを確定した課金ペア設計である。Issue #62でschema適合のinner / outer diagnostic fixture、parser test、snapshot、C1非回帰テストを追加した。天現寺exact directed bindingが解決し、route/direction、First Exit、全端点がすべて通ったときだけ`Graph.billingPairs`の`radialReturn`として昇格する。解決前のplanをpublic candidateとして出さない。

目黒入口 → 目黒出口の現行dynamic ODは別分類にする。entry Edgeは`e:w207535708:0:f`、exit Edgeは`e:w207535709:0:f`で、routing topology上は到達可能である。しかし物理的には天現寺Exitが先であり、exact bindingがなければ目黒を「1区間先」にできない。routing v2では`topology_only`とし、「1区間先」「最低料金」、`time_per_yen`の対象から外す。`routingCapability=routable`は道路を追跡できることを示すが、商品eligibilityの証拠ではない。現行pre-v2 outputは後述の互換fieldをdynamic ODにも残しているため、公開契約への移行完了まではこの節の`topology_only`を実装済みと読まない。

天現寺・荏原・戸越入口については、現行support dataでentry / exitのexact directed bindingが未解決である。候補wayや施設名をnearest nodeへ割り当てて補わない。天現寺binding issueは、multi-way ramp候補を順序付き`osmWayIds`と`edgeIds`で表し、ground ↔ mainline接続、ramp IDの逆引き、公式施設順、node接続とhashを同じevidence modelで扱う。`supportState=verified_bound`では解決した`directedSegments[]`を必須とし、`unresolved` / `unsupported`では空配列と`bindingCandidates[]`だけを許す。

### C1 8ペアは legacy adapter で変更しない

8件はいずれも `pairKind` を持たない現行 v1 形式として読む。schema 2 の seed でも配列要素の意味と値は変えず、生成 graph の schema 4 で `pairKind=legacyRing` として明示する。`sameNode` anchor、entry / exit、First Exitの判定、300円→300円のlegacy price records、verified 2件 / unverified 6件を保つ。

| ID | 意味 | 基準点 | 状態と First Exit 結果 |
| --- | --- | ---: | --- |
| `bp:c1-outer:kandabashi-takaracho` | 神田橋入口 → C1 outer → 宝町出口 | `499831338` | `verified`。宝町 Exit `e:w297864314:11:f` まで2,027mで唯一の First Exit。 |
| `bp:c1-outer:kasumigaseki-daikancho` | 霞が関入口 → C1 outer → 代官町出口 | `577255571` | `verified`。endpoint exact audit と outer First Exit contract を維持。 |
| `bp:c1-outer:ginza-shibakoen` | 銀座入口 → C1 outer → 芝公園出口 | `31254160` | `unverified`。同距離の2 Exit と endpoint exact reverse-map が未確定。 |
| `bp:c1-outer:shibakoen-iikura` | 芝公園入口 → C1 outer → 飯倉出口 | `31296971` | `unverified`。endpoint exact reverse-map が未確定。 |
| `bp:c1-inner:kasumigaseki-shibakoen` | 霞が関入口 → C1 inner → 芝公園出口 | `264877748` | `unverified`。way `203832854`（高樹町 Exit）が seed の芝公園 Exit より先。 |
| `bp:c1-inner:daikancho-kasumigaseki` | 代官町入口 → C1 inner → 霞が関出口 | `297945194` | `unverified`。endpoint exact reverse-map が未確定。 |
| `bp:c1-inner:shibakoen-shiodome` | 芝公園入口 → C1 inner → 汐留出口 | `31295430` | `unverified`。endpoint exact reverse-map が未確定。 |
| `bp:c1-inner:takaracho-kandabashi` | 宝町入口 → C1 inner → 神田橋出口 | `1891818143` | `unverified`。way `199311811` が seed の神田橋 Exit より先。 |

既知の First Exit mismatch はこの設計で解消しない。legacy adapter の contract test として、2件の verified 結果と6件の unverified 結果を固定する。2号を同じ generated graph へ追加する際も、この表を変更にしない。

### 所要時間・経路表示は正規の route leg で表す

routing v2の首都高時間は、完成した順序付きEdge列の各出現について`durationSeconds`を合計した値にする。2号往路、C1 mandatory lap、2号復路をそれぞれ1回ずつ数え、dedupしたEdge集合から時間を組み立てない。

Candidate v2は高速道路の区間と一般道の概算区間を別配列にする。

- `edgeRouteLegs`: `edgeIds`のindexを持ち、`entry_approach`、`mandatory_lap`、`return_corridor`、`exit_approach`の4 roleだけを使う。
- `estimatedLegs`: `surface_access`と`surface_return`を保持する。各legは`estimated=true`、`distanceMeters`、`durationSeconds`を持ち、Edge indexとEdge geometryは持たない。

`startEdgeIndex`は包含、`endEdgeIndexExclusive`は除外とする。`edgeRouteLegs`をCandidate順に並べると、`[0, edgeIds.length)`を重複も欠落もなく覆うことを契約テストにする。同じEdge IDが複数indexに現れるのは許すが、index範囲が重なるのは許さない。`post_lap_transfer`という別roleは採用せず、seed、generated graph、Candidate、UIで`return_corridor`だけを使う。

距離と時間の定義も同時に固定する。

- `shutokoDistanceMeters`は`edgeIds`に対応する首都高Edge距離の合計とする。
- `distanceMeters`は`shutokoDistanceMeters`に`surface_access`と`surface_return`の`distanceMeters`を加えた利用者側の総距離とする。
- `duration.shutokoSeconds`は4つの`edgeRouteLegs`に対応するEdge時間の合計、`accessSeconds`と`returnSeconds`は各`estimatedLegs.durationSeconds`と一致させる。
- `duration.baseSeconds=accessSeconds+shutokoSeconds+returnSeconds`とし、bufferとplan timeは現行式を保つ。

現行pre-v2 Candidateは`distanceMeters`と`shutokoDistanceMeters`をどちらも首都高Edge距離へ設定し、surface距離と時間をDurationだけで表現する。routing v2への移行では、上の定義へ揃えたcore回帰testとUI表示を同時に更新する。

`geometry`は首都高Edgeに対応する線分を連結したもので、Webが推定する一般道区間は含めない。地図と詳細画面には`entry_approach → mandatory_lap → return_corridor → exit_approach`を番号とテキストで示す。色だけで順序を示さず、surface access / returnはEdge付き経路に含めず、推定距離と時間として別に表示する。

### Google Maps URL は道路・方向を保証しない

Google Maps の URL は origin、destination、waypoint を渡せるが、近接 JCT の arm や C1 の道路・向きを強制できない。Waypoint の順序だけを示しても、Google が M/B へ正しく snap し、長弧を維持することは保証されない。この制約を正式に採用し、放射線候補の公開 handoff は既定で無効とする。

将来の実装候補は、leg 単位の split handoff である。

1. surface access: origin → 目黒入口
2. loop transfer: 目黒入口 → M → C1 長弧の距離中点 → B
3. surface return: B → 天現寺 Exit → origin

各 URL は waypoint 3点以下、完成長2,048文字以下とし、Google の自動 nav は開始しない。leg ごとの確認と手動継続は利用者に委ねる。ただし、この分割化だけでは道路・向きを保証できない。実装 issue 10 で URL 生成、unit test、E2E を実装し、issue 11 で Android / iOS、Web / app の実機 matrix と release gate を別に作る。

device verification manifest には `routePlanId`、`releaseId`、URL builder version、leg URL hash、期待する道路・向き、OS / browser / app version、検証日時、結果、期限を記録する。必要条件の1件でも missing、failed、expired の場合、放射線候補の public departure を必ず無効にする。C1 legacy の単一 URL と warning は現行互換として残せるが、同じ保証を radial へ転用しない。

### Issue #41 までは1区間の商品状態と金額を分離する

新routing contractでは`eligibilityStatus`、`amountYen`、`billingDistanceMeters`、`tariffStatus`を正本にする。`chargedSectionCount=1`と`ONE_SECTION_TOLL`は、`legacyRing` adapterだけで維持し、radial outputには出さない。「1区間先関係」と「料金制度上の1区間」を同じ表示にしない。

現行pre-v2 dynamic ODは、seed由来かどうかに関係なく`chargedSectionCount=1`と`ONE_SECTION_TOLL`を生成し、`status=ok`、`rankingMode=shutoko_time`で返す。UIも全候補を「1区間料金」と表示する。これは商品eligibilityの証拠ではなく、移行前の互換fieldである。routing v2ではdynamic ODを`eligibilityStatus=topology_only`へ移し、reason、charged section、UI表示を撤去する。移行を完了するまで現行挙動を設計済みと読まない。

Issue #41で公式billing distanceと版管理済み料金規則を確定するまでは、次の規律を適用する。

- 2号 radial の `amountYen=null`、`billingDistanceMeters=null`、`tariffStatus=unpriced`。
- 約21km の OSM driven distance を billing distance へ代入しない。
- 「1区間の最低料金」「1区間料金」と表示しない。
- `time_per_yen` を計算しない。unpriced 候補を含む cohort は `shutoko_time` で比較する。
- routing v2の動的ODは`topology_only`とし、商品ranking cohortに入れない。

#41 後も金額は entry → exit の公式 billing distance で決める。C1 8件は既存の300円→300円を当面保持しつつ、#41 で全件を再計算し、金額 source と tariff rule version を明示する。2号が下限料金より高くなる場合も、OSM 経路長に代えて公式距離で計算する。

### 実装 issue は単独で検証できる順に分ける

graph schema 4 を builder だけが先に出力する段階は、Issue #63 の opt-in として維持する。`--graph-schema 4` を明示した場合だけ top-level `routeMemberships[]` を生成し、既定の schema 2 出力・generated fixtures・manifest は変更しない。graph reader と release wiring は #65/#66 で整える。

| Issue | 実装範囲 | 主な受け入れ条件 | 依存 |
| ---: | --- | --- | --- |
| 1 | seed schema v2 と diagnostic pair 型 | `schemaVersion`を明示的に1 / 2へdispatchし、全nested structでunknown fieldを拒否する。未知version / kind / field fixtureを通し、既存C1 8要素のID・意味・価格・状態・anchorを保つ。radial endpointは`directedSegments[]`と未解決`bindingCandidates[]`を区別する。 | なし |
| 2 | `RouteMembershipIndex` とOSM relation / ramp binding provenance（#63実装済み） | `--graph-schema 4` の明示時だけ `relationMainline` と `boundRamp` を別 segment として生成し、way順、node接続、Edge順、hash、binding証拠を個別に検証する。逆方向、非所属mainline way、ramp証拠なし、short connector、relationの逆順を拒否する。#64のradial route planへの統合は未実装。 | 1 |
| 3 | directed mandatory lap と return-corridor First Exit | synthetic radial fixtureでM→B長弧、return corridor、first general Exitを分解する。C1 legacyを完全維持し、segment内反復を拒否しつつ、route planが宣言したsegment間反復を許可する。 | 1, 2 |
| 4 | graph schema 4 reader と consumer 契約 | core、WASM型、Web Workerがschema 2 / 3 / 4を読む。`legacyRing` / `radialReturn`、`sameNode` / `directedJunction`を判別し、wire fragmentとfield failure fixtureを追加する。未知kind / version、部分data、route legの重複・欠落を拒否する。 | 1, 3 |
| 5 | graph schema 4 の atomic release activation | builderの既定output、core reader、WASM contract、Web pipeline、Workers artifact allowlist、新しいversioned release ID、manifest hashを同時に整合させる。旧releaseはrollback用に残す。 | 2, 3, 4 |
| 6 | 天現寺 exact directed binding | multi-way ramp corpus、ground ↔ mainline topology、ramp ID inverse-map、公式施設順を同じsupport evidenceとして扱う。候補から一意な`directedSegments[]`だけ昇格し、way順・node接続・Edge順・hashを固定する。解決できなければ根拠付きunresolved / unsupportedのままにする。 | なし |
| 7 | 2号 inner / outer radial pair 統合 | schema適合fixtureとC1 non-regressionが通る。exact binding未完ならdiagnostic planのみとする。完了時だけGraph radial pairとpublic eligibilityへ昇格し、#41までtariffは未算出とする。 | 3, 5, 6 |
| 8 | Candidate route legs と product / tariff 状態 | synthetic Candidate fixtureで4 highway legsがEdge列を重複なく被覆し、surface legsが距離・時間を明示する。`distanceMeters`を総距離、`shutokoDistanceMeters`をEdge距離の合計にする。pre-v2 dynamic ODのcharged section / reasonを撤去し、radialに`chargedSectionCount`と`ONE_SECTION_TOLL`を出さない。 | 4 |
| 9 | Web の順序表示 | entry / lap / return / exitを番号・線種・テキストで提示し、surface概算とhighway経路を混同しない。総距離とhighway距離を同じ定義で表示し、unpriced / topology_onlyへ「1区間料金」を出さない。C1 UI regressionを維持する。 | 5, 8 |
| 10 | split Maps URL 生成 | 3 waypoint / 2,048文字制限、legごとの手動継続、URL builder unit / E2Eを実装する。道路・向きを強制できないため、実機gate通過までpublic handoffを無効にする。 | 8 |
| 11 | Maps 実機検証と release gate | Android / iOS × Web / appの必要matrixをmanifestへ記録する。失敗・期限切れでradial public departureを無効にし、C1への副作用がないことを確認する。device未接続でもcode issue 10は完了可能とする。 | 10 |
| 12 | Issue #41 後の tariff 統合 | 公式billing distance、車種、税率、単価、最低・上限、丸め、effective intervalを版管理し、C1 8件と2号代表pairを再検証する。OSM distance fallbackとradialの`time_per_yen`無効状態を維持しない。 | #41, 7, 8 |

Issue #42 の設計完了は、この節と seed / graph の wire-level schema により2号課金ペア形状を確定することとする。実装済み verified public pair を Issue #42 の design 完了条件には含めない。天現寺 binding、schema 実装、device gate はそれぞれ実装上の公開を止める条件として残す。

## 時間条件（現行 C1 legacy と routing v2 の共通式）

この節以降の探索と検証の中心は、現行pre-v2 C1 / dynamic OD契約である。routing v2が引き継ぐのは、時間予算、buffer、一般道access / returnの概算方法だけである。現行の`anchor`、一周、`T_entry_to_anchor`という語をradial designへそのまま適用しない。

内部では秒を使う。

```text
現行 C1:
T_shutoko_legacy = T_entry_to_anchor + T_loop + T_anchor_to_exit

routing v2:
T_shutoko_v2 = sum(entry_approach, mandatory_lap, return_corridor, exit_approach のEdge時間)

T_base = T_access + T_shutoko + T_return
buffer = max(300, ceil(T_base * 0.20))
T_plan = T_base + buffer
採用条件: L * 60 <= T_base かつ T_plan <= U * 60
```

`T_shutoko`は、現行C1ではanchor前後の3項、routing v2では4つのhighway legのEdge時間を合計した値である。`T_access`（現在地→入口）と`T_return`（出口→現在地）は一般道経路探索ではなく「直線距離 × 迂回係数1.3 ÷ 30 km/h」の概算値で、v2では各`estimatedLegs.durationSeconds`へ対応する。一方通行・河川・鉄道などで実際の所要時間と大きくずれうる（「精度の制約」節参照）。5分または20%の余裕は仮値で、交通予測の信頼区間ではない。「推定時間」は`T_base`、「余裕込みの計画時間」は`T_plan`と呼ぶ。表示は切り上げた分、採否は丸め前の秒を用いる。休憩は含めない。`L = U`など余裕を確保できない入力は有効だが候補なしになり得る。

### 東京駅・目黒等の既知の時間境界と最近接入口契約

座標検索では最近接の構造的に利用可能な入口 tier を優先する。東京駅（35.6812, 139.7671）の現行全線グラフでは、最近接の宝町入口 tier（`ramp:c1-inner:takaracho-entry` → `ramp:c1-outer:takaracho-exit`）が選択される。実測の `minPlanSeconds` は1,610秒（約26.83分）であり、15〜26分窓では最近接 tier の周回が上限を超えるため `no_candidates/TIME_WINDOW`（`minPlanSeconds=1610`）を返し、15〜27分窓で `ok` となる。また、30〜60分窓でも同じ宝町 tier から計画時間2,795秒・周回長20,205mの候補が1件成立する（料金は未算出、`shutoko_time`）。完全評価された最近接 tier に合法周回が存在する場合、その tier が `TIME_WINDOW` や `NO_HANDOFF` の診断を確定し、遠方の入口へフォールスルーしない（時間枠拡大による復帰導線を保証する）。構造的に周回が存在しない（structurally dead）最近接 tier のみ後続 tier へフォールスルーする。

また、目黒受入契約として、目黒の代表2座標（`35.635681, 139.718489` および `35.63239, 139.71524`）では、15〜60分・30〜120分・52〜120分の各窓において、最近接の `ramp:2-inbound:meguro-entry`（目黒入口）および `ramp:2-outbound:meguro-exit`（目黒出口）が選択される。料金根拠のない動的 OD のため料金は未算出（`null`）となり、順位付けは `shutoko_time` が適用される。

## 道路グラフ（現行 graph schema 2）

ノードは交差点・ランプの接続点、エッジは方向付き走行区間。首都高本線（Shutoko）・入口ランプ（Entry）・出口ランプ（Exit）・JCT の分岐、一方通行、禁止遷移を保持する。**一般道エッジはグラフに含めない**。地理的な交差だけで接続しない。探索状態は直前エッジと制限の判定に必要な履歴も持ち、基準点への帰還時にも遷移を検証する。

出発地点から入口へのアクセスおよび出口から出発地点への帰路は、一般道経路探索では求めず「直線距離 × 迂回係数 1.3 ÷ 30 km/h」で概算し、時間予算に含める。入口アクセス地点（Entry エッジの from ノード）が1件も得られない場合は対応範囲外とする。投影前後の位置を表示し、建物内や私有地から道路までの移動が未モデル化であることを確認できるようにする。

正しく解釈できない時間依存・複雑な通行制限を持つ区間は初期データから除外する。現行Edgeには時間依存モデルを持たず、Edge kindごとの固定速度で静的時間を決める。Shutokoは60 km/h、Entry / Exitランプは40 km/hで、manifestの`timeModelVersion=v1-static-speeds`が成果物全体を表す。現行Graph / Edgeに道路種別別の欠損fallback値、欠損flag、Edgeごとのmodel versionは存在しない。

将来、時間帯・車種・道路種別ごとの欠損fallbackを追加する場合は、manifestのglobal versionだけで差分を証明せず、Edgeごとの`timeModelVersion`、`estimated` / `missing` provenance、fallback根拠を必須にする。制限速度だけを実所要時間と断定しない。

### 精度の制約

一般道グラフを排除したことで以下の精度が失われる。

1. **アクセス時間の精度**: 直線距離 × 1.3 の概算のため、一方通行・河川・鉄道をまたぐ地域では実際と大きくずれうる。
2. **一般道の禁止遷移遵守**: 一般道の交差点禁止転回は考慮しない。Google マップ側のルーティングに委ねられる。
3. **帰路の経路特定**: 出口から出発地点への経路は1本に確定せず、時間も概算値になる。
4. **`snappedOrigin` の一意性**: 入口アクセス地点は最大 `SearchLimits.max_access_entries`（デフォルト 0 = 無制限、全 Entry アクセス地点）件あり、候補ごとに異なる入口アクセス地点を持ちうる。

## 現行 pre-v2 の探索手順

この手順は現行graph schema 2、C1 legacy pair、最近接入口tier、dynamic ODを前提とする。routing v2では、手順3のanchor / SCC閉路カタログを`routePlan`、`resolvedRouteSegments`、route / direction proofへ置き換え、手順4の分解を4つの正規legで行う。時間予算、access / returnの概算、最近接入口の診断は共通する。

1. **入口アクセス地点の選定**: リクエストが座標（`origin: { lat, lon }`）の場合、WASM 内で Entry エッジの from ノード（入口アクセス地点）を対象に等距円筒近似（Equirectangular approximation、東京付近 `cos(lat)` 補正）で距離を計算し、近い順に最大 `SearchLimits.max_access_entries`（デフォルト 0 = 無制限、全 Entry アクセス地点）件を選ぶ。最寄りの入口アクセス地点が `SearchLimits.max_access_distance_meters`（デフォルト 30,000 m、0 は無制限）を超える場合も探索を行わず `status: "no_candidates"`, `reason: "NO_CONNECTION"` を返す。入口アクセス地点が1件も得られない場合、および従来の検証済み課金ペア探索において `max_access_entries` の制限で課金ペアの入口がいずれも選ばれない場合も同じ `NO_CONNECTION` を返す。`originNodeId` が直接指定された場合はその Entry エッジの from ノードを単一の入口アクセス地点として採用する。
   - 候補の有無にかかわらず、座標入力では最近接の入口アクセス地点を `nearestAccess`（`{ nodeId, lat, lon, distanceMeters }`）として返す。
   - 合法（禁止遷移を満たす）な周回が1件でも見つかった場合、時間枠で棄却したものを含む `planSeconds` の最小値を `minPlanSeconds` として返す。
   - 各入口アクセス地点 `i` から出発座標 `s` へのアクセス時間（`T_access`）は `haversine(s, i) × 1.3 ÷ 30 km/h` で概算する。出口 `o` から `s` への帰路時間（`T_return`）も `haversine(o_coord, s) × 1.3 ÷ 30 km/h` で概算する。
2. **探索モードの選択**:
   - `entryRampId` と `exitRampId` の双方がある場合、ID、`RampKind`、Entry/Exitエッジ、一般道側ノード、本線側ノードのexact directed bindingとrouting capabilityを検証し、課金ペアseedから独立した明示OD探索を行う。料金根拠がないODも経路は返せるが、`amountYen` / `billingDistanceMeters` / `tollSource` は `null` とし、OSM実走距離から料金を作らない。
   - 座標検索（`origin` 座標が指定されている場合）は、近傍の入口アクセス地点を出発地からの距離順にソートした入口 tier を評価する最近接入口優先探索を行う。各 tier 内で検証済み課金ペアがあれば優先評価し、なければ構造的に利用可能な一般出口への動的 OD を探索する。
   - 座標検索において、完全評価された最近接 tier に合法周回が存在する場合は、その tier が `TIME_WINDOW` / `NO_HANDOFF` 診断を担い、遠方の入口へフォールスルーせず確定する。構造的に周回が存在しない（structurally dead）最近接 tier のみ後続 tier へフォールスルーする。
   - 座標検索で料金根拠のない動的 OD 候補を含む場合、候補集合全体の順位付けは `shutoko_time`（首都高走行時間順）に切り替える。
   - `originNodeId` 指定（`origin` なし）や、`origin` 座標を伴わない片側ランプ指定（`entryRampId` または `exitRampId` のいずれか片方のみ指定）の検索は、入口 tier による動的 OD 探索を行わず、従来の検証済み課金ペア探索経路（legacy verified-pair path）を維持する。`origin` 座標と片側ランプ指定を併用した場合は座標検索（最近接入口優先探索）が優先され、指定ランプはその tier 探索内の制約として扱われる。
3. **SCC・逆到達性索引と本線閉路カタログ**:
   - `prepare` 時に Shutoko エッジだけの forward/reverse adjacency と強連結成分（SCC）を構築する。各 cyclic SCC からノードID順に最大32アンカーを均等抽出し、アンカーの各先頭エッジについて逆Dijkstraで決定論的な最短帰還路を作る。順序は `(アプローチ+イグレスの時間下界, anchorNodeId)`、同距離経路はedge ID辞書順で固定する。
   - 全線グラフでは単純路の全列挙を行わない。明示ODは入口からのforward Dijkstraと出口へのreverse Dijkstraにより、両方から到達可能なcyclic SCCカタログだけを評価する。座標・課金ペア検索も大規模グラフでは同じ逆最短閉路を用いる。小規模契約fixtureだけは全単純閉路列挙を維持する。
   - **マイクロループ排除（Micro-loop Filtering）**: JCT の渡り線やジャンクション内の極小周回（Uターンランプ等）による実質的に周回ドライブの体をなさない短絡ループを排除するため、ループ長が `min_loop_meters`（デフォルト 5,000m = 5km）未満の閉路を自動的に除外する。
   - verified-bound 232端点は同じ静的到達性契約で全件分類する。197件は `routable`、35件（入口13・出口22）は循環SCCへ接続できない `structural_no_loop` であり、後者は探索前に `NO_LOOP` 診断へ利用できる。単にboundであることを機能保証とはしない。
   - **最大エッジ数の拡大**: C1 単独にとどまらず、C2（中央環状線、約47km）や湾岸線・放射線を跨ぐ広域周回ルートを探索できるよう、`max_loop_edges` の上限シーリングを 2,000 から 5,000 へ引き上げた。
4. Entryエッジ、アプローチ、本線閉路、イグレス、Exitエッジを別フェーズで結合する。`min_loop_meters` は本線閉路だけに適用し、入口・出口への接続距離で5,000m条件を満たしたことにはしない。
5. 全体の方向・禁止遷移、一度の入場と退出、1区間先の出口、一周の成立、時間条件を再検証する。
6. **料金算出とスキーマ分離**:
   - **実走行距離（`shutoko_distance_meters`）**: 首都高本線およびランプの実走エッジ長の積算値。周回を含む実走距離を正確に記録。
   - **料金距離（`toll.billing_distance_meters`）**: 入口〜出口間の法定・最短料金距離（OD テーブルまたは最短経路）。
   - **公式普通車 ETC 料金（`amount_yen`）**:
     - 料金距離 ≤ 4.3km: 下限料金 300 円
     - 料金距離 > 4.3km: 検証済み料金距離がある場合だけ、版管理された規則または料金表を利用する。料金根拠のない明示ODへOSM距離を代入しない。
     - 料金テーブル（`prices` / `od_tariffs`）の有効期間（`effective_from` / `effective_to`）とリクエストの `pricing_at` を照合。
7. **幾何合成と Google マップ引き継ぎ**:
   - **GeoJSON LineString 合成**: 全エッジ列（entry → loop → exit）から単一の LineString 座標列を生成。
   - **通過路線名の抽出**: 通過したエッジの `name` を重複除去した順序付きリスト `roadNames` として生成。
   - **Google Maps URLs 生成**: `select_waypoints` で最大3点の経由地を選定して URL を構成。URL 長が 2,048 文字を超える場合は `NO_HANDOFF` として候補を除外。

### Google マップ引き継ぎの暫定経由地選定ルール（#8 依存）

Google マップ上でのナビゲーションにおいて、一周を短絡（ショートカット）せず周回を維持するため、`crates/routing-core/src/handoff.rs` の `select_waypoints` に暫定ルールを分離・カプセル化している:
1. **入口アクセス地点**（Entry エッジの from ノード、街路側の入口起点）
2. **周回の距離中点ノード**（loop 内の累積距離が 50% に最も近いエッジの `to` ノード）
3. **出口ノード**（Exit エッジの to ノード、街路側の出口終点）

これにより Google マップは「現在地 → 入口アクセス地点（一般道案内）→ 周回中点 → 出口ノード → 現在地」の経路を案内する。現在地から入口までの一般道案内は Google マップ側に委ねる。

> [!IMPORTANT]
> このルールは issue #8（実機検証 spike）の物理端末検証結果に基づいて差し替える前提の暫定実装である。候補の `warnings` には必ず `HANDOFF_WAYPOINTS_UNVERIFIED` が含まれる。2026-09 に Android Chrome + Google マップアプリ「あり」の代表1系列（神田橋入口 → C1 外回り → 宝町出口）で周回維持を確認したが、残条件（アプリ「なし」・iOS Safari・経由地点の系列網羅）は未検証であり、暫定実装を維持する（[検証記録](delivery.md) 参照）。

本線閉路はSCC、逆到達性、正の所要時間を使うDijkstra下界で枝刈りする。既定値は全探索の展開状態100,000、小規模fixture列挙のbeam幅200、閉路の最大エッジ数2,000（受理上限5,000）、最小ループ長5,000m、従来課金ペア上限10とする。上限到達は `truncated` とし、経路不存在に読み替えない。UI は探索開始10秒で Web Worker を終了でき、その場合は候補を返さず再試行を案内する。同じ入力・成果物版・探索上限では同じ展開順と結果にする。

列挙が資源上限で打ち切られた場合、`minPlanSeconds` は真の最小を証明しないため `null` になり得る。UIは到達不能や「最短でもN分」を断定しない。また `time_per_yen` 順位と距離加重Jaccard除外により、自ペア候補が別ペア候補に置換され得るため、課金ペアの存在だけで特定候補の表示を保証しない。

## コスパの評価（現行 ranking と v2 の分離）

この節では、現行C1 pairとdynamic ODのrankingを説明する。routing v2のradial候補は`eligibilityStatus=verified_one_section_ahead`だけを商品cohortへ入れ、`unverified`と`topology_only`を除外する。金額比較の規則は同じだが、legacy / radial adapterの混在を許さない。

最初に「一周後に1区間先で退出」という必須条件で絞る。1区間だからすべての入出口ペアが同額とは仮定しない。

金額が確認済みの候補同士は `首都高走行秒数 / 料金円` の大きい順、同値なら首都高時間が長い順、一般道時間が短い順、安定 ID 順にする。無料等で料金0円のデータは初期対象に含めず別途扱いを設計する。

料金額が未確認なら比率は計算しない。同じ課金ペア内では首都高時間の長い順で比較できる。異なるペアをまたぐ場合は、金額比較ができないことを明示して時間基準の暫定推薦とする。検索候補集合に未確認額がある場合は集合全体をこの時間基準に切り替え、確認済みだけを不当に優先しない。全候補で金額が確認できた場合だけ金額あたりの比較を使う。「一番」は探索した候補集合内の順位であって、全経路の最適保証ではない。

多様性は首都高有向エッジの距離加重 Jaccard 類似度（共通エッジ距離 / 和集合エッジ距離）で確認する。選出済み候補との類似度0.8以上を除き、少なければ水増ししない。重複エッジはこの類似度では1回、実走行時間・距離では通過回数分を数える。

## 検証（現行 C1 legacy）

現行C1 / dynamic ODは、人工グラフで空閉路、anchorへの帰還、帰還時の禁止遷移、入口から出口への短絡、間違った1区間先、二周、途中退出・再入場、マイクロループ排除を検出する。接続区間と一周部分の重複を誤って落とさないことも確認する。小規模では全列挙した閉路と比較し、探索打ち切りによる候補欠落と不正経路を区別する。

routing v2は別に、4 legのindex完全被覆、segment内反復、relation / ramp由来、multi-way binding、First Exit exact binding、`distanceMeters`の距離式、unpriced / topology_onlyの非表示、pre-v2 dynamic compatibility fieldの撤去を検証する。

実データでは方向別入出口ペア、一周の道路列、JCT、高架、データ境界を人手でも検証する。通行可能性と課金ペアの確認は別項目とし、どちらかが未確認なら公開候補に使わない。
