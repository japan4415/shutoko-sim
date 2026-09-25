# 実データ道路グラフ・課金ペア生成パイプライン

本ドキュメントでは、OpenStreetMap（OSM）実データから首都高速道路の全24路線と接続ランプを抽出し、決定論的な成果物（`graph.json`、`od-tariffs.json`、`pair-candidates.json`、`ramps.json`、`snap-index.json`、`manifest.json`）を生成するオフラインパイプラインの仕様と手順を記録する。一般道はルーティンググラフのエッジには含めないが、入口・出口ランプの分類コンテキストとして取得・参照し、ランプ端点の確定（`firstPublicRoadConnection/v1`）と 1 区間先出口の判定（relation 制約つきの First Exit）に使う。現行公開 release は `all-real-v4`（graph schema 4、`billingPairsVersion=v3`、`tariffModelVersion=1`）で、rollback 先は `all-real-v3`。

## 1. ライセンスと帰属表示

- **データ提供元**: [OpenStreetMap](https://www.openstreetmap.org/)
- **著作権・帰属表示**: `© OpenStreetMap contributors`
- **ライセンス**: [Open Database License (ODbL) 1.0](https://opendatacommons.org/licenses/odbl/1-0/)
- **権利表記 URL**: [https://www.openstreetmap.org/copyright](https://www.openstreetmap.org/copyright)

本プロジェクトが生成する派生成果物は ODbL に準拠して配布・利用される。

## 2. OSM 実データ取得手順

### 現行 `all-real-v4` の取得仕様

- **Overpass API エンドポイント**:
  - 主系: `https://overpass-api.de/api/interpreter`
  - 副系: `https://overpass.kumi.systems/api/interpreter`
- **クエリの正本**: `scripts/fetch-osm.sh`。既定は全24路線の relation を取得し、ランプを4 hop、context way を1 hop展開する
- **出力先**: `fixtures/osm/shutoko-all.json`
- **source date**: `2026-09-16`
- **ファイル SHA-256**: `566f3d7910c3962600e05d0e9d442b0621ae2bcac817fd375b60267f8a22a4c9`
- **ファイルサイズ**: 4,203,540 bytes
- **要素数**: 合計 26,847 要素（ノード 23,661 / way 3,125 / リレーション 61）
- **生成 release**: `all-real-v4`（graph schema 4）

### C1限定スナップショット（歴史・回帰用）

旧 C1 限定手順は診断と C1 回帰にだけ使う。現行 release の既定入力、CI 再生成、公開統計ではない。

- **実行条件**: `ROUTES_FILTER=c1 ./scripts/fetch-osm.sh`
- **出力先**: `fixtures/osm/shutoko-c1.json`
- **取得日時（UTC）**: `2026-09-15T04:38:52Z`（副系エンドポイント使用）
- **クエリ SHA-256**: `ae8754b341bce5bd0acb79f70fb8c71175b332679b44d6f3425b6e1a00b1510e`
- **ファイル SHA-256**: `c39bf4051e4cd6478377e2cc67e9710d885390593e5a5afb18c3b175a464ca52`
- **ファイルサイズ**: 369,367 bytes
- **要素数**: 合計 2,051 要素（ノード 1,745 / way 299 / リレーション 7）
- **旧 release 名**: `c1-real-v2`（graph schema 2）

### 対象範囲

一般道（`trunk`、`primary`、`secondary`、`residential`、`service` 等）は**ルーティンググラフには含めない**。座標検索は入口までの一般道を解かず、首都高上の探索だけを行う。入口・出口ランプの**分類コンテキスト**として一般道ウェイは取得する。

- **全24路線**: `network=首都高速道路` のrelationを正本とし、tagの欠落routeを既知relation IDで補完する。C1はrelation `4256008`、C1/JCTを含む2号目黒線はrelation `4256339`を使う。
- **接続ランプ（`motorway_link`）**: 全路線の本線wayから最大4 hopで到達するwayを展開する。
- **context way**: 展開済みの`motorway_link`とnodeを共有し、`motorway` / `motorway_link`以外の`highway`を持つway。ルーティンググラフには入れず、ランプ終端が車両通行可能な一般道へ接続するかの証明に使う。way要素の`nodes`と`tags`だけを出力し、context nodeの座標は含めない。
- **現行スナップショットの実測**: `motorway_link` way 1,619件、context way 712件、両者が共有するnode 557件。context wayの内訳は`secondary` 114、`trunk` 111、`tertiary` 110、`footway` 72、`service` 56、`unclassified` 33、`primary` 107、`primary_link` 14、`tertiary_link` 11、`trunk_link` 38、`secondary_link` 9、`residential` 19、`pedestrian` 3、`proposed` 11、`construction` 2、`rest_area` 1、`steps` 1。
- **右左折・Uターン禁止制限**: 高速道路本線およびランプ（`ew_all`・`links`）に関連する `type=restriction` リレーション
  - `no_*`（via=node）: from エッジから to エッジへの禁止遷移ペア（長さ 2）を生成。
  - `only_*`（via=node）: via ノードにおける to 以外の代替流出エッジを自動特定し、禁止遷移ペアとして生成。
  - `via=way`（Uターン制限等）: from エッジ、via エッジ列、to エッジを連結する長さ 3 以上の禁止エッジ列を生成。
  - `restriction:conditional`（時間帯・車種条件付き制限）: 静的道路グラフでは一意に評価できないためスキップし、標準エラー出力およびマニフェストへ記録。端点 resolver では `hgv:conditional` のみ passenger-car 製品に無関係な例外として無視し、他の `*:conditional`、`oneway:conditional`、`reversible`、`alternating` は fail-closed とする。
  - `only_*`（via=way）: 静的道路グラフ生成では現時点で未サポートとし、該当関係が存在する場合はスキップしてマニフェストへ記録。

### 入口/出口ランプの分類方式

ランプエッジ（`motorway_link`）を入口（Entry）・出口（Exit）・首都高本線（Shutoko）に分類する際、以下の優先順位付き規則を適用する（issue #32 対応）。

**廃止した旧ヒューリスティック**:
- **路線名パターンマッチ**（`is_shutoko_numbered_route_link()`）: 「首都高 + ＜数字＞号」の路線名から号数を抽出して判定。湾岸線・中央環状線 C2・八重洲線等、号数を持たない路線を正しく扱えなかった。路線拡充のたびに路線名リストのメンテナンスが必要だった
- **距離しきい値**（`JCT_DETECTION_MAX_ENTRY_DIST_METERS = 550`）: 実出口の最遠 466m と JCT 連絡路の最小 665m の間のマージンが薄く、路線拡充で破綻するリスクがあった

**現行の優先順位付き規則**:
1. **地表接続シグナル（一次）**: ランプ端点ノードが、車両通行可能な一般道 way のノード集合に含まれるか。`is_vehicle_highway(highway) -> Option<bool>` が道路種別の構造的区分を担い、歩道・歩行者専用路（footway / pedestrian / cycleway / steps等）は共有nodeがあっても地表接続の証拠に数えない
2. **OSM ノードタグシグナル（二次）**: `highway=traffic_signals` → 入口/出口の証拠、`highway=motorway_junction` → 本線側の証拠
3. 一次シグナルが決定的ならそれを採用。二次と矛盾する場合は警告を記録
4. どちらでも決まらない場合は `undecidable_ramp_edges` を加算し警告を出したうえで保守的に Shutoko に分類

**現行 `all-real-v4` の結果**:

| 種別 | エッジ数 |
|------|---------:|
| Entry | 168 |
| Exit | 198 |
| Shutoko | 22,621 |
| Local | 0 |
| undecidable_ramp_edges | 0 |

合計 22,987 エッジのうち、ランプ端点として一般道へ接続しない内部 `motorway_link` 601 エッジは dropped としてグラフ不入である。グラフの端点分類は `firstPublicRoadConnection/v1` の判定結果に従うため、`all-real-v3` と比べて Shutoko エッジが減り Exit エッジが増えている。

**C1限定 `c1-real-v1` の結果（歴史）**:

| 種別 | エッジ数 |
|------|---------:|
| Entry | 15 |
| Exit | 17 |
| Shutoko | 1,687 |
| Local | 0 |
| undecidable_ramp_edges | 0 |

旧ロジックとの差異: `e:w4848898:0:f`（way 4848898「首都高速都心環状線」`ref=C1` の先頭セグメント）が Entry の誤分類から正しく Shutoko に修正された（始端ノードが車両通行可能な一般道と接続していないため）。

### 再現実行手順
```bash
./scripts/fetch-osm.sh [OUTPUT_PATH] [ENDPOINT]
```
引数を省略した場合は全24路線を取得し、`fixtures/osm/shutoko-all.json` に出力する。C1限定診断には `ROUTES_FILTER=c1` を明示し、既定入力と混同しない。

## 3. 宣言的課金シード（`data/billing-pairs-seed.json`）

料金適格性および「1区間先」の出口関係は OSM 幾何のみから自動判別できないため、人手検証と公式資料に基づく宣言的シード定義を行う。
ビルダーはビルド時に以下の厳格な自動検証を実施する:
- **最初の出口（First Exit）検証**: 本線基準点（`anchorOsmNodeId`）から首都高速本線エッジを経由して最初に到達可能な出口エッジのみが検証済みペアとして採用可能。後続の出口（例: 神田橋入口・宝町出口に対してさらに下流の新富町出口）は `FIRST_EXIT_MISMATCH` として拒否される。探索は単純路（ノード再訪なし）を列挙する Dijkstra で、状態キーは `(node, suffix, 訪問ノード集合)`（`suffix` は直近 `L-1` 本のエッジ ID、`L` は禁止遷移の最大長）。同一キーへ到達した経路は将来の可否が完全に同一なので枝刈りは健全であり、PR #5 の反例 A/B/C のような訪問集合違いの誤った枝刈りは発生しない。単純路列挙は最悪指数的であるため展開状態数に明示的な上限（200,000）を設け、超過時は探索予算超過として `FIRST_EXIT_SEARCH_FAILED` を返す（無報告の打ち切りはしない）。非空周回の検出（`has_non_empty_shutoko_loop`）は walk 意味論（閉じた歩道の存在、ノード再訪許容）で `(node, suffix)` の有限状態空間を BFS し、旧 `path.len() > 2000` の無報告打ち切りは撤廃済み。
- **出典・日付の構文検証**: 出典 URL は `http://` または `https://` で始まる有効な URL（空ラベルや先頭・末尾ドットのない有効なホスト名）であること、出典日はカレンダー上に実在する有効な ISO 8601 日付（`YYYY-MM-DD`、存在しない 2月31日等は拒否）であることを検証する。
- **マニフェストへの伝播**: 検証済みペアの出典情報（`source`, `sourceDate`, `notes`）は `manifest.json` の `provenance` 配列に記録され、成果物配布時にも追跡可能となる。

### 登録ペア（全 8 件）
- **ペア ID**: `bp:c1-outer:kandabashi-takaracho`
- **入口ランプ OSM ウェイ ID**: `92243921`（神田橋入口、一般道側始点: `n:1070862943`）
- **出口ランプ OSM ウェイ ID**: `297864314`（宝町出口、一般道側終点: `n:1130812252`）
- **本線周回基準点 OSM ノード ID**: `499831338`（C1 外回り神田橋合流点）
- **車種プロファイル**: `passenger-car-etc`
- **検証状態**: `verified`（`oneSectionAheadVerified: true`）
- **根拠と出典**:
  - 出典 URL: `https://www.shutoko.jp/use/network/map/`
  - 参照日: `2026-09-10`
- **通行料金**: 首都高速道路株式会社の公式料金表と 2026-10 改定資料に基づき、普通車 ETC 料金の 10 OD セルを期間付きで登録する。2025-04 版と 2026-10 版は別の `distanceEvidence` と `prices` を持ち、2026-10-01 JST 以降は PDF のセルと規則検算の両方が一致した `priced` とする。これにより `routing-core` が `pricingAt` と期間から正しい金額と observed distance を選べる。
  - 神田橋→宝町は 1.7km / 300円を両版で保持する。
  - 霞が関→代官町は両版とも 2.3km / 300円。570円 / 12.4km は霞が関→霞が関（対角セル）の値であり、この OD には使わない。
  - 2号目黒→天現寺は 2025-04 が 19.4km / 790円、2026-10 が 19.4km / 860円。
  - 2026-10 PDF は SHA-256 `1dd86cf7946deb28ca6e25d57f133f3acf1c4f5e4110d70d00d8055b3ee6d1e5`、C1 は P.3、2号は P.4 を確認した。PDF と画像は gitignore 済みの `.cache/official-fare/` に置く（正本は `data/od-tariffs.json` の `documents[]` の SHA-256 とページ・行・列。`documents[].cachePath` / `pendingResolution.cachePath` は参考情報）。
  - 出典: `https://www.shutoko.jp/ss/2026ryoukin-kaitei/gallery/ryoukin-kaitei_toll_rates.pdf`、`https://www.shutoko.co.jp/company/press/2026/data/07/31-toll/`

#### 新規登録 7 ペア（2026-09-10 登録、2026-09-25 料金表 v3 で再検証）

7 ペアの 2025-04 公式セルと、2026-10 公式 PDF の対応セルをそれぞれ登録する。金額は「2025-04 版 → 2026-10 版」の順で、期間ごとに別の `evidenceId` を持つ。1 区間先の隣接関係は公式路線図に基づく。表の金額は**現行の料金表 v3 の実測値**であり、`data/od-tariffs.json` の `assignments` と一致する。

| # | 方向 | 入口 → 出口 | 料金距離 | 普通車 ETC 基本料金 | ペア ID | 入口 way | 出口 way | 基準点（anchor）node | 状態 |
|---|------|------------|---------|-----------|---------|---------|---------|---------------------|------|
| 1 | 外回り | 霞が関入口 → 代官町出口 | 2.3km → 2.3km | 300 円 → 300 円 | `bp:c1-outer:kasumigaseki-daikancho` | `916571610` | `276920911` | `577255571` | `verified_one_section_ahead` |
| 2 | 外回り | 銀座入口 → 芝公園出口 | 3.4km → 3.4km | 300 円 → 300 円 | `bp:c1-outer:ginza-shibakoen` | `4848922` | `944671542` | `31254160` | `verified_one_section_ahead` |
| 3 | 外回り | 芝公園入口 → 飯倉出口 | 1.6km → 1.6km | 300 円 → 300 円 | `bp:c1-outer:shibakoen-iikura` | `4853801` | `203832842` | `31296971` | **`unverified`**（端点が `access:conditional`） |
| 4 | 内回り | 霞が関入口 → 芝公園出口 | 3.7km → 3.7km | 300 円 → 300 円 | `bp:c1-inner:kasumigaseki-shibakoen` | `916571615` | `203873821` | `264877748` | `verified_one_section_ahead` |
| 5 | 内回り | 代官町入口 → 霞が関出口 | 2.3km → 2.3km | 300 円 → 300 円 | `bp:c1-inner:daikancho-kasumigaseki` | `1091280541` | `1232166619` | `297945194` | `verified_one_section_ahead` |
| 6 | 内回り | 芝公園入口 → 汐留出口 | 2.4km → 2.4km | 300 円 → 300 円 | `bp:c1-inner:shibakoen-shiodome` | `4853797` | `45068171` | `31295430` | `verified_one_section_ahead` |
| 7 | 内回り | 宝町入口 → 神田橋出口 | 1.7km → 1.7km | 300 円 → 300 円 | `bp:c1-inner:takaracho-kandabashi` | `378284514` | `390441534` | `1891818143` | `verified_one_section_ahead` |

- 出典（共通）:
  - 料金距離・料金: `https://edge.sitecorecloud.io/metropolita84c2-shutokoeb0e-productionbcbd-eb79/media/Project/shutoko/docs/drivers/tolls/about/price/2504_pamphlet_fee_table.pdf`（旧 URL `https://www.shutoko.jp/-/media/pdf/responsive/customer/fee/fee-info/2504_pamphlet_fee_table.pdf` は 2026-09-10 時点で 404。首都高料金表 2025年4月改訂版 P.3「料金・距離表（ETC 普通車）」）
  - 路線図（1 区間先の隣接関係）: `https://www.shutoko.jp/use/network/map/`
  - 料金体系・下限料金: `https://www.shutoko.jp/tolls/about/price/`（旧 URL `https://www.shutoko.jp/fee/fee-info/about/` は 2026-09-10 時点で /tolls/about/price/ へ 301 リダイレクト）
  - 2026-10-01 改定発表および OD 表 PDF: `https://www.shutoko.co.jp/company/press/2026/data/07/31-toll/`、`https://www.shutoko.jp/ss/2026ryoukin-kaitei/gallery/ryoukin-kaitei_toll_rates.pdf`
  - 参照日: `2026-09-10`

> **注記（内回り銀座入口の 1 区間先について）**:
> 内回り銀座入口 → 新富町出口（0.4km、300 円）は公式資料上の 1 区間先だが、OSM の分流点・合流点の順序（内回り新富町出口の分流点が銀座入口の合流点より上流にある）により relation 制約つきの First Exit 検証が通らないため**商品ペアとしては未登録**のままである。京橋出口は 2 区間先なので登録しない。料金表 v3 には `assignment:c1-inner:ginza-shintomicho`（0.4km / 300円）を証拠つきで保持するが、これは公式セルに料金があることのみを意味し、1 区間先として公開できることを意味しない。隣接関係証跡 `data/billing-pair-adjacency.json` は `bp:c1-inner:ginza-shintomicho` を `reviewStatus: blocked` で保持し、自動導出レポート（`pair-candidates.json`）でも `hold` として出力する。

### 3.1 `schemaVersion: 2` の混在 seed（parser実装済み）

Issue #42 で C1 legacy と2号 radial pair を同じ seed ファイルへ混在させる。`data/billing-pairs-seed.json` は Issue #68 で `schemaVersion: 2` へ更新され、既存 C1 8要素の項目、値、意味は変更せず2件の radial pair を追加した。`pairKind` を持たない要素は legacy ring pair と解釈する。

schema 2では `pairKind: "radialReturn"` と `routePlanVersion: 1` を必須とし、route/membership、mandatory lap、return corridor、exact bindingを検証する。検証済みの4 resolved segmentはschema 4の`radialReturn`として公開し、未解決候補はschema 2とmanifestの診断記録に閉じる。

混在の規則は次のとおりである。

- parser は `schemaVersion` を明示的に `1` / `2` へ dispatch し、それ以外は parsing 前に拒否する。
- schema 1/2 の top-level、pair、provenance、price、endpoint、route plan、status の全 nested struct で unknown field を拒否する。
- legacy 要素は現行の `entryOsmWayId`、`exitOsmWayId`、`anchorOsmNodeId`、`status`、`oneSectionAheadVerified` を持つ。`pairKind` は必須ではない。
- radial 要素は `pairKind: "radialReturn"` と `routePlanVersion: 1` を必須とする。
- `pairKind` または `routePlanVersion` が未知なら fail-closed で拒否する。
- radial が `pairKind` / `routePlanVersion` のどちらかを欠く場合、または legacy が variant 必須フィールドを欠く場合も拒否する。
- seed 内に legacy と radial を何件ずつ含めてよい。ただし ID は重複させない。同じ array 内で endpoint support、pair eligibility、loop validation、tariff status を混ぜない。
- **金額の正本は `data/od-tariffs.json` の `assignments` だけである。** schema 2 の seed は `assignmentId` 参照だけを持ち、legacy の `prices[]` も radial の `tariff.amountYen` / `tariff.billingDistanceMeters` / `tariff.prices[]` も持たない。parser はいずれかが値として残る seed を build 時に拒否し、`graph-builder` の `OdTariffsFile` と `routing-core` の `OdTariffsFileV3` はともに `deny_unknown_fields` なので、同じ理由で legacy の top-level `rules` ブロックも戻せない。
- Issue #62 で、未知 version、各 variant の未知 field、variant 必須 field の欠落をそれぞれ fixture 化して検証した。

検証状態は次の軸で独立させ、1つの `status` に押し込まない。

| 軸 | 主な値 |
| --- | --- |
| endpoint `supportState` | `verified_bound`, `unsupported`, `unresolved` |
| `routingCapability` | `routable`, `structural_no_loop`, `unsupported` |
| `pairEligibility.status` | `verified_one_section_ahead`, `unverified`, `topology_only` |
| `loopValidation.status` | `declared_route_validated`, `unresolved`, `topology_only` |
| `tariff.status` / Candidate `tariffStatus` | `priced`, `unpriced`, `expired`, `not_applicable` |

以下は、同じ `billingPairs` array に置ける legacy 1件と radial 1件の wire-level 例である。長い `notes` を含む legacy 要素も、現行データから値を変えない。

```json
{
  "id": "bp:c1-outer:kandabashi-takaracho",
  "entryOsmWayId": 92243921,
  "entryName": "神田橋入口",
  "exitOsmWayId": 297864314,
  "exitName": "宝町出口",
  "anchorOsmNodeId": 499831338,
  "vehicleProfile": "passenger-car-etc",
  "status": "verified",
  "oneSectionAheadVerified": true,
  "provenance": {
    "source": "https://www.shutoko.jp/use/network/map/",
    "sourceDate": "2026-09-10",
    "notes": "Verified 1-section-ahead adjacency on C1 outer loop from Kandabashi entry to Takaracho exit (Gofukubashi and Edobashi exits decommissioned in 2021). Tariff sources (verified 2026-09-10): fee structure and minimum toll (https://www.shutoko.jp/tolls/about/price/ (旧 URL https://www.shutoko.jp/fee/fee-info/about/ は 2026-09-10 時点で /tolls/about/price/ へ 301 リダイレクト)); Kandabashi to Takaracho toll distance 1.7km with minimum toll 300 yen applied (https://edge.sitecorecloud.io/metropolita84c2-shutokoeb0e-productionbcbd-eb79/media/Project/shutoko/docs/drivers/tolls/about/price/2504_pamphlet_fee_table.pdf (旧 URL https://www.shutoko.jp/-/media/pdf/responsive/customer/fee/fee-info/2504_pamphlet_fee_table.pdf は 2026-09-10 時点で 404)); 2026-10-01 tariff revision maintaining 300 yen minimum toll (https://www.shutoko.co.jp/company/press/2026/data/07/31-toll/)."
  },
  "prices": [
    {
      "amountYen": 300,
      "effectiveFrom": "2022-03-31T15:00:00Z",
      "effectiveTo": "2026-09-30T15:00:00Z"
    },
    {
      "amountYen": 300,
      "effectiveFrom": "2026-09-30T15:00:00Z"
    }
  ]
}
```

```json
{
  "id": "bp:2-inbound:meguro:c1-inner:tengenji",
  "pairKind": "radialReturn",
  "routePlanVersion": 1,
  "vehicleProfile": "passenger-car-etc",
  "assignmentId": "assignment:2:meguro-tengenji",
  "entryEndpoint": {
    "rampId": "ramp:2-inbound:meguro-entry",
    "name": "目黒入口",
    "supportState": "verified_bound",
    "directedSegments": [
      {
        "segmentId": "ramp:2-inbound:meguro-entry:segment:0",
        "osmWayIds": [207535708],
        "osmNodeIds": [2177935837, 2177935839],
        "edgeIds": ["e:w207535708:0:f"],
        "fromNodeId": "n:2177935837",
        "toNodeId": "n:2177935839",
        "edgeIdsSha256": "6af7e1b129f1f8fdb3a48e829c809c8ea9c276d57c5e83481d7bac03c285db79"
      }
    ]
  },
  "exitEndpoint": {
    "rampId": "ramp:2-outbound:tengenji-exit",
    "name": "天現寺出口",
    "supportState": "verified_bound",
    "directedSegments": [
      {
        "segmentId": "ramp:2-outbound:tengenji-exit:segment:0",
        "osmWayIds": [172358461, 422023171, 931759044, 172358460],
        "osmNodeIds": [252175582, 1832672250, 1832672244, 1832672214, 1832672165, 1832672133, 1832672121, 1832672108, 1832672093, 1832672090, 1832672098, 8638072333, 1832672096, 1832672105, 1832672128, 1832672146, 1832672162],
        "edgeIds": [
          "e:w172358461:0:f",
          "e:w172358461:1:f",
          "e:w172358461:2:f",
          "e:w172358461:3:f",
          "e:w172358461:4:f",
          "e:w172358461:5:f",
          "e:w422023171:0:f",
          "e:w422023171:1:f",
          "e:w422023171:2:f",
          "e:w931759044:0:f",
          "e:w172358460:0:f",
          "e:w172358460:1:f",
          "e:w172358460:2:f",
          "e:w172358460:3:f",
          "e:w172358460:4:f",
          "e:w172358460:5:f"
        ],
        "fromNodeId": "n:252175582",
        "toNodeId": "n:1832672162",
        "edgeIdsSha256": "bb9114f49d64b952b58b5a2ef53679a6007bea48a51671ade34c56b0325fa7cd"
      }
    ]
  },
  "routePlan": {
    "entryCorridor": {
      "membershipId": "route:2:inbound",
      "terminalEdgeId": "e:w4853804:16:f",
      "mergeNodeId": "n:574460576"
    },
    "anchor": {
      "anchorKind": "directedJunction",
      "routeId": "C1",
      "direction": "inner",
      "mergeNodeId": "n:574460576",
      "branchNodeId": "n:574460605",
      "mergeTerminalEdgeId": "e:w4853804:16:f",
      "branchInitialEdgeId": "e:w45248411:0:f",
      "arcPolicy": "ordinaryLongArc",
      "excludedShortConnector": {
        "fromNodeId": "n:574460605",
        "toNodeId": "n:574460576",
        "osmWayId": "23297444",
        "edgeCount": 23,
        "distanceMeters": 493
      }
    },
    "mandatoryLap": {
      "membershipId": "route:C1:inner",
      "firstEdgeId": "e:w23297444:43:f",
      "lastEdgeId": "e:w23297444:19:f",
      "lapCount": 1
    },
    "returnCorridor": {
      "membershipId": "route:2:outbound",
      "startNodeId": "n:574460605",
      "initialEdgeId": "e:w45248411:0:f",
      "firstGeneralExit": {
        "rule": "firstGeneralExit",
        "expectedRampId": "ramp:2-outbound:tengenji-exit",
        "exactDirectedBinding": "verified_bound"
      }
    }
  },
  "routingCapability": "routable",
  "pairEligibility": {
    "status": "verified_one_section_ahead",
    "oneSectionAheadVerified": true
  },
  "loopValidation": {
    "status": "declared_route_validated"
  },
  "tariff": {
    "status": "priced",
    "amountYen": 790,
    "billingDistanceMeters": 19400,
    "prices": [
      {
        "amountYen": 790,
        "effectiveFrom": "2022-03-31T15:00:00Z",
        "effectiveTo": "2026-09-30T15:00:00Z"
      },
      {
        "amountYen": 860,
        "effectiveFrom": "2026-09-30T15:00:00Z"
      }
    ]
  },
  "provenance": {
    "source": "https://www.shutoko.jp/use/network/map/",
    "sourceDate": "2026-09-25",
    "notes": "目黒入口から一ノ橋JCTのC1 inner長弧を通り、2号下りへ戻った最初の一般Exit候補を天現寺とする。4 wayの有向鎖は2号下りのn:252175582から最初の一般道接続node n:1832672162へ到達し、groundWayIds=[258834790]としてverifiedにする。"
  }
}
```

endpoint は単一 OSM way を仮定しない。`supportState=verified_bound` では `directedSegments[]` に、解釈が確定した順に連続する `osmWayIds`、`osmNodeIds`、`edgeIds`、両端nodeとhashを必ず記録する。wayをまたぐ場合も1つのdirected segmentにまとめ、各要素の接続と順序を検証する。最初の接続nodeに複数のwayがある場合は `groundWayIds[]` に昇順でway IDを記録し、互換の `groundWayId` は先頭とする。天現寺は4 wayと17 node、16 Edgeの接続とhashを固定し、最後の node `n:1832672162` の `groundWayIds=[258834790]`（明治通り）で `verified_bound` にする。way `172358466` を含む未prune chainは後続node `n:1832672205` への接続があるため、診断候補としては unresolved として残すが昇格 evidenceには含めない。

上例は現行 `data/billing-pairs-seed.json` の実形状（`bp:2-inbound:meguro:c1-inner:tengenji`）と一致する。`bindingCandidates[]` は `supportState` が `unresolved` / `unsupported` のときだけ現れ、`verified_bound` の要素は空配列の `directedSegments[]` を持つ。

`edgeIdsSha256` は、順序を保った `edgeIds` を空白なしの JSON array へ直列化し、その UTF-8 バイト列を SHA-256 にした値とする。outer も同じ形を使い、`anchor.direction=outer`、M=`n:31297008`、B=`n:31297000`、mandatory lap の first / last Edge=`e:w24039737:24:f` / `e:w24039737:3:f`、除外 connector は way `24039737`、20 edges、461m、return initial Edge は `e:w4853805:0:f` とする。

### 3.2 正本 anchor と generated graph union

`anchorKind` の値は `sameNode` と `directedJunction` の2つだけとする。seed、generated graph、Candidate で同じ値を使い、`sameNodeLoopAnchor` / `directedJunctionLoopAnchor` のような別名を登場させない。

| `anchorKind` | 必須 field | 意味 |
| --- | --- | --- |
| `sameNode` | `nodeId`, `routeId`, `direction`, `arcPolicy` | 現行 C1。基準点へ戻り、同じノードから次へ進む。 |
| `directedJunction` | `mergeNodeId`, `branchNodeId`, `mergeTerminalEdgeId`, `branchInitialEdgeId`, `routeId`, `direction`, `arcPolicy`, `excludedShortConnector` | 放射線から環状線へ入り、別ノードから放射線へ戻る。`mergeTerminalEdgeId` は M の直前に来る Edge、`branchInitialEdgeId` は B の直後に出る Edge。 |

mandatory lap 自身の境界は `routePlan.mandatoryLap.firstEdgeId` / `lastEdgeId` に置く。anchor と lap の責務を混ぜない。

| 現行 schema 1 | schema 2 / graph schema 4 | 規則 |
| --- | --- | --- |
| `pairKind` なし | `pairKind="legacyRing"` | seed では省略を許す。graph schema 4 では明示する。 |
| `anchorOsmNodeId` | `anchor.anchorKind="sameNode"`, `anchor.nodeId` | 値と意味を変えない。 |
| schema 1 にない route / direction | `anchor.routeId`, `anchor.direction` | graph build で entry・anchor・exit の Edge 列として解ける一意な route membership から導出する。0件または複数候補なら graph 4 への昇格を拒否する。 |
| schema 1 にない arc policy | `anchor.arcPolicy="sameNodeLoop"` | legacy adapter の固定値。raw seed は変更しない。 |
| `entryOsmWayId` / `entryName` | `entryId` と `entryEndpoint` | graph Edge ID は build で解決し、way 変更として seed へ書き戻さない。 |
| `exitOsmWayId` / `exitName` | `exitId` と `exitEndpoint` | 同上。 |
| `status`, `oneSectionAheadVerified` | `pairEligibility.status`, `pairEligibility.oneSectionAheadVerified` | raw status は変更しない。`verified` は v2 の `verified_one_section_ahead` へ正規化する。 |
| `prices[]` | `tariff.prices[]`, `tariff.status` | **raw seed からは削除済み。** schema 2 の seed は `assignmentId` 参照だけを持ち、手書きの金額・距離・`prices[]` を持たない。parser は値が残る seed を fail-closed で拒否する。graph schema 4 の `tariff.prices[]` は v3 assignment から導出する。霞が関→代官町は両版とも2.3km / 300円、2号目黒→天現寺は19.4km / 790円と19.4km / 860円。v3 assignmentが唯一の正本。 |
| なし | `routePlanVersion`, `entryCorridor`, `anchor`, `mandatoryLap`, `returnCorridor` | radial variant だけを必須にする。 |
| なし | `pairEligibility`, `loopValidation`, `tariff` の独立 status | endpoint support、routing capability、loop validation、料金状態を混在させない。 |

generated graph の `billingPairs[]` も同じ判別 union とする。schema 2/3 の `pairKind` なしは legacy として読めるが、schema 4 の builder 出力では `pairKind` を必ず書く。`legacyRing` は `entryToAnchorEdgeIds` と `anchorToExitEdgeIds` を必須にし、`radialReturn` は `anchorNodeId` を省略する。未知の kind、anchor kind、route plan version は reader と builder の両方で拒否する。Issue #65 で schema 4 builder 出力の legacy pair を明示 union へ変換し、core reader、WASM 型、Web Worker の dispatch / fail-closed 契約を追加した。Issue #68の追加修正ではverified entry / exit bindingとFirst Exitを解決し、entry approach / mandatory lap / return corridor / exit approachの4 resolved segmentを検証して`radialReturn`を同じunionへ追加する。

以下はreader fixtureのwire fragmentである。IDとEdgeは実装テスト用の synthetic value であり、天現寺 binding が解けたこと、または公開可能な pair であることを表さない。`legacyRing` と `radialReturn` の判別、必須 field、status の配置を同じ例で確認する。

```json
{
  "billingPairs": [
    {
      "id": "fixture:legacy:c1",
      "pairKind": "legacyRing",
      "vehicleProfile": "passenger-car-etc",
      "entryId": "fixture:legacy:entry",
      "exitId": "fixture:legacy:exit",
      "anchor": {
        "anchorKind": "sameNode",
        "nodeId": "fixture:node:anchor",
        "routeId": "C1",
        "direction": "inner",
        "arcPolicy": "sameNodeLoop"
      },
      "entryToAnchorEdgeIds": ["fixture:edge:legacy:entry-anchor"],
      "anchorToExitEdgeIds": ["fixture:edge:legacy:anchor-exit"],
      "pairEligibility": {
        "status": "verified_one_section_ahead",
        "oneSectionAheadVerified": true
      },
      "loopValidation": {
        "status": "declared_route_validated"
      },
      "tariff": {
        "status": "priced",
        "amountYen": 300,
        "billingDistanceMeters": 1700,
        "prices": [
          {
            "amountYen": 300,
            "effectiveFrom": "2026-09-30T15:00:00Z",
            "effectiveTo": null
          }
        ]
      }
    },
    {
      "id": "fixture:radial",
      "pairKind": "radialReturn",
      "routePlanVersion": 1,
      "vehicleProfile": "passenger-car-etc",
      "entryId": "fixture:edge:entry:ramp",
      "exitId": "fixture:edge:exit:ramp",
      "entryEndpoint": {
        "rampId": "fixture:ramp:entry",
        "name": "fixture entry",
        "supportState": "verified_bound",
        "directedSegments": [
          {
            "segmentId": "fixture:binding:entry:0",
            "osmWayIds": [1001],
            "edgeIds": ["fixture:edge:entry:ramp"],
            "fromNodeId": "fixture:node:entry:ground",
            "toNodeId": "fixture:node:entry:ramp-end",
            "edgeIdsSha256": "1846afd5803d03982b1641d3f1c4c9816379eeede1e100a4c3e0a6c130248e97"
          }
        ]
      },
      "exitEndpoint": {
        "rampId": "fixture:ramp:exit",
        "name": "fixture exit",
        "supportState": "verified_bound",
        "directedSegments": [
          {
            "segmentId": "fixture:binding:exit:0",
            "osmWayIds": [2001],
            "edgeIds": ["fixture:edge:exit:ramp"],
            "fromNodeId": "fixture:node:exit:ramp-end",
            "toNodeId": "fixture:node:exit:ground",
            "edgeIdsSha256": "b275420fdbd9d55bd72a6b5f8e36cbf6218042d5afdd1400be3b8713b4b7a35c"
          }
        ]
      },
      "routePlan": {
        "entryCorridor": {
          "membershipId": "fixture:route:r1:inbound",
          "terminalEdgeId": "fixture:edge:entry:mainline",
          "mergeNodeId": "fixture:node:merge"
        },
        "anchor": {
          "anchorKind": "directedJunction",
          "routeId": "fixture:loop",
          "direction": "forward",
          "mergeNodeId": "fixture:node:merge",
          "branchNodeId": "fixture:node:branch",
          "mergeTerminalEdgeId": "fixture:edge:entry:mainline",
          "branchInitialEdgeId": "fixture:edge:return:mainline",
          "arcPolicy": "ordinaryLongArc",
          "excludedShortConnector": {
            "fromNodeId": "fixture:node:branch",
            "toNodeId": "fixture:node:merge",
            "osmWayId": 3001,
            "edgeCount": 1,
            "distanceMeters": 100
          }
        },
        "mandatoryLap": {
          "membershipId": "fixture:route:loop:forward",
          "firstEdgeId": "fixture:edge:lap:1",
          "lastEdgeId": "fixture:edge:lap:2",
          "lapCount": 1
        },
        "returnCorridor": {
          "membershipId": "fixture:route:r1:outbound",
          "startNodeId": "fixture:node:branch",
          "initialEdgeId": "fixture:edge:return:mainline",
          "firstGeneralExit": {
            "rule": "firstGeneralExit",
            "expectedRampId": "fixture:ramp:exit",
            "exactDirectedBinding": "verified_bound"
          }
        }
      },
      "resolvedRouteSegments": [
        {
          "resolvedSegmentId": "fixture:resolved:entry",
          "role": "entry_approach",
          "membershipId": "fixture:route:r1:inbound",
          "sourceSegmentIds": ["fixture:binding:entry:0", "fixture:relation:r1:inbound:main"],
          "edgeIds": ["fixture:edge:entry:ramp", "fixture:edge:entry:mainline"],
          "edgeIdsSha256": "26fae7c7d5e125ca9f0c5de847411ff1fdb1883e961b641eb4b05b6030a381be"
        },
        {
          "resolvedSegmentId": "fixture:resolved:lap",
          "role": "mandatory_lap",
          "membershipId": "fixture:route:loop:forward",
          "sourceSegmentIds": ["fixture:relation:loop:forward:main"],
          "edgeIds": ["fixture:edge:lap:1", "fixture:edge:lap:2"],
          "edgeIdsSha256": "99ddf4b7771edc3fe3c04034d2465719932ca1c2d2104c0d05bab41d6e462a43"
        },
        {
          "resolvedSegmentId": "fixture:resolved:return",
          "role": "return_corridor",
          "membershipId": "fixture:route:r1:outbound",
          "sourceSegmentIds": ["fixture:relation:r1:outbound:main"],
          "edgeIds": ["fixture:edge:return:mainline"],
          "edgeIdsSha256": "ba825c8c0f6236ca476824f7553aefe8bf09a94a5343c2470cde33b1bb443e1b"
        },
        {
          "resolvedSegmentId": "fixture:resolved:exit",
          "role": "exit_approach",
          "membershipId": "fixture:route:r1:outbound",
          "sourceSegmentIds": ["fixture:binding:exit:0"],
          "edgeIds": ["fixture:edge:exit:ramp"],
          "edgeIdsSha256": "b275420fdbd9d55bd72a6b5f8e36cbf6218042d5afdd1400be3b8713b4b7a35c"
        }
      ],
      "routingCapability": "routable",
      "pairEligibility": {
        "status": "verified_one_section_ahead",
        "oneSectionAheadVerified": true
      },
      "loopValidation": {
        "status": "declared_route_validated"
      },
      "tariff": {
        "status": "unpriced",
        "amountYen": null,
        "billingDistanceMeters": null,
        "prices": []
      }
    }
  ]
}
```

`resolvedRouteSegments` の role は `entry_approach`, `mandatory_lap`, `return_corridor`, `exit_approach` の4種類だけにする。Candidate では各 role を `edgeRouteLegs` の `startEdgeIndex`（含む）から `endEdgeIndexExclusive`（含まない）へ写す。一般道の surface access / return は Edge を持たないため、graph segment にも Candidate の Edge index にも入れない。

schema 4 の manifest は `billingPairsVersion=v3`、`tariffModelVersion=1`、`graphSchemaVersion=4`、`routePlanVersion=1`、決定論的な `routeMembershipsSha256`、導出ルールの `pairDerivation`（`rule=billingPairDerivation/v2` と 5 入力の SHA-256、候補集計）を記録し、`graph.json`、`od-tariffs.json`、`pair-candidates.json`、`ramps.json` と release ID を結び付ける。旧 manifest の schema 1/2 record は上書きしない。`all-real-v4` はこの契約で生成し、`all-real-v3` は rollback 用に残す。公開 release の切替は、consumer reader と Web/Workers の検証、および R2 の read-back が通った後に行う。

### 3.3 `RouteMembershipIndex` は本線 relation と ramp binding を別々に証明する

Issue #63 で graph-builder にこのデータ型と生成・検証処理を追加し、Issue #64 で `routePlanLapV1` の directed mandatory lap と return-corridor First Exit を同じ membership index 上で生成・検証する処理を追加した。Issue #66 で graph-builder の既定を schema 4 に切り替え、`graph.json` の top-level `routeMemberships[]` と manifest の route membership hash を公開する。`--graph-schema 2` を明示した場合だけ legacy schema 2 を出力する。OSM relation の `relationMainline` と、正規ランプ台帳の exact directed binding に由来する `boundRamp` は同じ route/direction の membership 内でも別 segment として保持する。各 segment の `orderedEdgeIdsSha256`、source snapshot hash、way/node/Edge 連続性を builder が検証する。所属の正本はrelationで、順序の正本はrelation所属way間の一意な要求方向的有向接続である。各接続で後続がちょうど1つであることを要求し、分岐・行き止まりはsegment境界として切り出すかfail-closedで拒否する。member順は保証されないため`memberIndexes`をprovenanceとして記録し、graph順序との一致を`memberOrderMatchesRelation`で診断する。way IDソート、member順、graph上の別pathによる並べ替えや補完は行わない。`directionMappingVersion=osm-relation-role/v1` を記録し、route 2 の OSM `forward` / `backward` を `outbound` / `inbound` に正規化する。CLI の schema 4 は現在の実 snapshot に C1 relation `4256008` と route 2 relation `4256339` が揃う場合だけ relation ID を固定し、合成 snapshot では入力中の route relation を処理する。固定対象以外の relation は bound ramp evidence としてのみ保持する。schema 4 reader は #65 で実装済みで、#66 で versioned release へ接続し、`all-real-v4` の `billingPairsVersion=v3` を受理する。`find_first_exits_from_anchor` は C1 legacy のまま保存し、membership 制約付きの `find_first_exit_on_corridor` は B と return corridor の initial edge から relationMainline の順序どおりに一般 Exit を探す。declared candidate の exact binding が `unresolved` / `unsupported` の場合は次の supported Exit へ進まず、その状態を返す。

OSM route relation は mainline を列挙し、一般入口・出口の ramp way を含まない。目黒 entry way `207535708` や天現寺 exit candidate way `172358461` / `422023171` / `931759044` / `172358460` / `172358466` を mainline relation の member として扱い続けると、正しい ramp binding を relation の連続 Edge 列へ不正に対応させる。したがって、graph schema 4 の top-level `routeMemberships[]` は次の二層構造にする。

| object | 必須 field | 証明する内容 |
| --- | --- | --- |
| `RouteMembershipIndex` | `membershipId`, `routeId`, `direction`, `directionMappingVersion`, `segments[]` | 路線・方向ごとに使う directed segment を束ねる。 |
| `RouteMembershipSegment` | `segmentId`, `sourceKind`, `sourceRelationId`, `sourceSnapshotSha256`, `bindingEvidenceId`, `orderedEdgeIds`, `orderedEdgeIdsSha256`, `memberIndexes`, `memberOrderMatchesRelation` | `sourceKind=relationMainline` なら relation所属way、有向接続順序、member provenanceと順序診断を記録し、`sourceKind=boundRamp` なら exact binding を由来にする。 |

`relationMainline` は `sourceRelationId` と `bindingEvidenceId=null` を要求し、そのrelationとroleの要求方向に属するwayだけを使う。順序の正本はrelation所属way間の一意な有向接続であり、member順はprovenanceと診断にだけ使う。各接続で後続がちょうど1つであることを要求し、分岐・行き止まりはsegmentを分けるかfail-closedで拒否し、onewayや要求directionに逆らうfallbackを拒否する。`memberIndexes`と`memberOrderMatchesRelation`を各segmentに記録する。`boundRamp` は `sourceRelationId=null` と非 null の `bindingEvidenceId` を要求し、正規ランプ台帳と exact directed binding の順序付き Edge 列、from/to endpoint、way順、Edge順、hash を使う。どちらも `orderedEdgeIdsSha256` を必須にする。天現寺は4-way / 16-Edgeの directed segment を `boundRamp` に昇格し、最初の一般道接続node `n:1832672162` と `groundWayIds=[258834790]` を保持する。

次の synthetic fragment は、1つの entry approach と return corridor が mainline segment と bound ramp segment を組み合わせた wire shape を示す。

```json
{
  "routeMemberships": [
    {
      "membershipId": "fixture:route:r1:inbound",
      "routeId": "fixture:R1",
      "direction": "inbound",
      "directionMappingVersion": "osm-relation-role/v1",
      "segments": [
        {
          "segmentId": "fixture:binding:entry:0",
          "sourceKind": "boundRamp",
          "sourceRelationId": null,
          "sourceSnapshotSha256": "b2a0b24aa896e9d92425ff81539194531e036bda0764aa0792f4cbadf61c044a",
          "bindingEvidenceId": "fixture:binding:entry",
          "orderedEdgeIds": ["fixture:edge:entry:ramp"],
          "orderedEdgeIdsSha256": "1846afd5803d03982b1641d3f1c4c9816379eeede1e100a4c3e0a6c130248e97"
        },
        {
          "segmentId": "fixture:relation:r1:inbound:main",
          "sourceKind": "relationMainline",
          "sourceRelationId": "fixture:relation:r1",
          "sourceSnapshotSha256": "b2a0b24aa896e9d92425ff81539194531e036bda0764aa0792f4cbadf61c044a",
          "bindingEvidenceId": null,
          "orderedEdgeIds": ["fixture:edge:entry:mainline"],
          "orderedEdgeIdsSha256": "b0ad47503d2604d5160411aba0d6b541ef2171fb1023ebc9bc398d750b5c6bb2"
        }
      ]
    },
    {
      "membershipId": "fixture:route:loop:forward",
      "routeId": "fixture:loop",
      "direction": "forward",
      "directionMappingVersion": "osm-relation-role/v1",
      "segments": [
        {
          "segmentId": "fixture:relation:loop:forward:main",
          "sourceKind": "relationMainline",
          "sourceRelationId": "fixture:relation:loop",
          "sourceSnapshotSha256": "e19978c5b7cdfc9bb595fb044fcbb6a1a27d1ff2281a9a3eca663d6cc8b7cf30",
          "bindingEvidenceId": null,
          "orderedEdgeIds": ["fixture:edge:lap:1", "fixture:edge:lap:2"],
          "orderedEdgeIdsSha256": "99ddf4b7771edc3fe3c04034d2465719932ca1c2d2104c0d05bab41d6e462a43"
        }
      ]
    },
    {
      "membershipId": "fixture:route:r1:outbound",
      "routeId": "fixture:R1",
      "direction": "outbound",
      "directionMappingVersion": "osm-relation-role/v1",
      "segments": [
        {
          "segmentId": "fixture:relation:r1:outbound:main",
          "sourceKind": "relationMainline",
          "sourceRelationId": "fixture:relation:r1",
          "sourceSnapshotSha256": "b2a0b24aa896e9d92425ff81539194531e036bda0764aa0792f4cbadf61c044a",
          "bindingEvidenceId": null,
          "orderedEdgeIds": ["fixture:edge:return:mainline"],
          "orderedEdgeIdsSha256": "ba825c8c0f6236ca476824f7553aefe8bf09a94a5343c2470cde33b1bb443e1b"
        },
        {
          "segmentId": "fixture:binding:exit:0",
          "sourceKind": "boundRamp",
          "sourceRelationId": null,
          "sourceSnapshotSha256": "b2a0b24aa896e9d92425ff81539194531e036bda0764aa0792f4cbadf61c044a",
          "bindingEvidenceId": "fixture:binding:exit",
          "orderedEdgeIds": ["fixture:edge:exit:ramp"],
          "orderedEdgeIdsSha256": "b275420fdbd9d55bd72a6b5f8e36cbf6218042d5afdd1400be3b8713b4b7a35c"
        }
      ]
    }
  ]
}
```

`orderedEdgeIdsSha256`と`edgeIdsSha256`は同じ規則で、順序を保ったEdge IDの空白なしJSON arrayをSHA-256化する。IDの並べ替えや重複除去はhash生成前に行わない。

route planのlegは`sourceSegmentIds[]`でmainlineとrampの由来を明示し、その順番に`edgeIds`を連結する。連続性の検証対象を分ける。`mandatory_lap` は必ず1つの `relationMainline` segment の連続部分列でなければならない。entry、return、exit は複数の `relationMainline` と `boundRamp` segment を連結できるが、各 segment 内部の順序・hash・binding 証拠を個別に満たす。relation を持たない ramp を「例外」として無検証で許さない。

`find_first_exit_on_corridor` は、Issue #64 の graph-builder 実装で次の条件をすべて満たす場合だけ一般 Exit を返す。

1. mandatory lap の B を出発点とし、return corridor の `initialEdgeId` から探索を始める。
2. B から Exit split までの mainline Edge は指定 relation の `relationMainline` segment に順番どおり所属する。
3. Exit split 以降は seed の `expectedRampId` と `verified_bound` の `boundRamp` segment の先頭 Edge だけが split node から直接続き、segment 内の全 Edge・接続・way順・hash を検証して `CorridorExit.edgeIds` と距離へ含める。
4. 候補は return corridor 内の一般 Exitだけで、C1 の Exit、entry approach 中の Exit、boundary JCT を数えない。
5. 禁止遷移を満たし、探索予算を明示して処理する。

B から全グラフの最短 Exit を選ぶ処理は使わない。実データでは B から C1 芝公園 Exit が1,306m、天現寺候補の開始点が1,972mであり、route constraint なしで Exit を選ぶと誤る。graph-builder の `resolve_diagnostic_radial_route_plan` は seed の declared candidate を検証し、relationMainlineの複数segmentとoffsetを横断して候補の`fromNodeId`まで必ず探索する。候補の exact binding が `unresolved` または `unsupported` の場合は、到達したmainline Edge列を保持したまま次の supported Exit へ skip しない。候補へ到達する前に経路が尽きた場合は`ExitNotFound`、探索予算が尽きた場合は`BudgetExceeded`を返す。First Exit の幾何探索が成功しても、端点 support や pair eligibility の証拠にはしない。

実装テストには次を含める。

- inner / outer の M→B長弧を選び、B→Mの0.493km / 0.461km connectorを拒否する。
- relation memberに目黒entryや天現寺exitを含めない現行snapshotで、対応するboundRamp segmentだけをevidence付きで許可する。
- multi-way rampのway順、node接続、Edge順、from/to endpoint、hashを検証し、最初の合法な一般道接続nodeより後の別node接続がある場合だけcandidateをunresolvedにする。同じnodeへの複数way接続は `groundWayIds[]` に記録してverifiedにする。
- entry corridorにC1 Exitがあっても、return corridorのExitと混同しない。
- 逆方向、同名JCT、relation非所属mainline way、別armへの近道を拒否する。
- segment内のEdge反復を拒否し、route planが宣言したsegment間反復を許す。
- 探索予算超過を「Exitなし」と読み替えない。
- 既存 C1 8 件の anchor、edge resolution、First Exit、7件のverified・1件のunverified、期間別料金、回帰テストで固定する。


### 3.4 2号計画の診断用データと公開 BillingPair を分ける

本節で定義した inner / outer object は、Issue #62 で `fixtures/seed-v2/diagnostic-radial-v2.json` と `diagnostic-radial-v2.snapshot.json` に固定し、Issue #68 で同じ2件を実 `data/billing-pairs-seed.json` にも追加した。parser test、snapshot、実 seed の graph-builder contract test で同じ wire shape を確認する。天現寺 exit は4 way / 16 Edgeで、最初の合法な一般道接続 node `n:1832672162` と `groundWayIds=[258834790]` を `verified_bound` として保持する。way `172358466` を通る未prune chain は、後続 node `n:1832672205` でも一般道へ接続するため、診断では `AMBIGUOUS_GROUND_ENDPOINT` として unresolved にし、昇格する4-way evidenceには含めない。graph-builder は両 route plan の M→B 長弧と return corridorを検証し、4 resolved segmentと未解決でないFirst Exitが一致した両pairをschema 4 unionの`radialReturn`として公開する。`--graph-schema 2`ではradial pairを診断unionへ出さず、manifestへpair固有の`rejected`記録を残して暗黙に落とさない。

料金も v3 assignment の期間別 evidenceを正本とする。2025-04 は19.4km / 790円、2026-10 は19.4km / 860円を保持し、OSM driven distanceで補完しない。

## 4. 成果物の決定論的再生成手順

### 再生成コマンド
```bash
./scripts/generate-fixtures.sh
```
`generate-fixtures.sh` は既定で `all-real-v4` を生成する。rollback 先など別の release ID を再現するときは第 7 引数または `SHUTOKO_RELEASE_ID` で release ID を渡す。

または直接 CLI を実行する（`scripts/generate-fixtures.sh` が内部で呼んでいる内容と同じ）。
```bash
cargo run --bin shutoko-graph-builder --locked -- \
  --osm fixtures/osm/shutoko-all.json \
  --seed data/billing-pairs-seed.json \
  --inventory data/ramp-inventory.json \
  --bindings data/osm-ramp-bindings.json \
  --tariffs data/od-tariffs.json \
  --adjacency data/billing-pair-adjacency.json \
  --support-decisions data/ramp-support-decisions.json \
  --out-dir fixtures/generated \
  --release-id "all-real-v4" \
  --built-at "2026-09-24T00:00:00Z" \
  --source-date "2026-09-16" \
  --vehicle-profile "passenger-car-etc" \
  --coverage-area "Metropolitan Expressway network (Tokyo, Kanagawa, Saitama)" \
  --graph-version "1.0.0"
```
`--graph-schema` の既定は 4 なので省略してよい（明示的に 4 を渡しても同じ）。`--graph-schema 2` を指定すると legacy schema 2 を生成するが、`all-real-v4` を指定した場合は schema 4 以外を拒否する。検証済み課金ペアが1件以上生成されていることを強制したい場合は `--strict` を付ける。検証済みペアが0件の場合に非ゼロで終了する。

### 現行 graph schema 4 が保持する情報

- **Node の地理座標 (`lat`, `lon`)**: 全ノードに必須fieldとして出力する。WASMの空間snapとGeoJSON LineStringの合成で使う。
- **Edge の日本語道路名 (`name`)**: OSMの`name`、なければ`name:ja`を`Edge.name: Option<String>`として伝播する。名前がないEdgeはJSON keyを省略する。
- **課金ペアの公式ランプ名 (`entryName`, `exitName`)**: `data/billing-pairs-seed.json`の値を`graph.json`の`billingPairs[]`へ伝播する。
- **RouteMembershipIndex**: `graph.json` の `routeMemberships[]` に relation mainline と bound ramp の順序付き Edge、source snapshot、binding evidence、各 hash を記録する。今回の `all-real-v4` は 53 memberships を出力する（双方向 46 件に、全 route relation cover の `forward` 7 件）。
- **料金カタログ**: `graph.json` は後方互換の `odTariffs` に加え、料金表 v3 の正本 `odTariffsV3`（`tariffRules` / `distanceEvidence` / `assignments` / `deprecatedAssignments`）と `billingPairsVersion`、`tariffModelVersion` を埋め込む。
- **schema 4 manifest**: `graphSchemaVersion=4`、`routePlanVersion=1`、`billingPairsVersion=v3`、`tariffModelVersion=1` と、決定論的な `routeMembershipsSha256`、および `pairDerivation`（導出ルールと 5 入力の SHA-256、候補集計）を `manifest.json` に記録する。

現行 `all-real-v4` の生成済みファイルは次のとおりである。数値は `fixtures/generated/` の実測値であり、C1限定fixtureの数値ではない。**ファイルサイズ列は `fixtures/generated/manifest.json` の `artifacts[].byteLength` と実ファイルから機械的に取り直した値**（`manifest.json` 自身は `fixtures/generated/manifest.json` の実サイズ）で、`cargo test -p shutoko-graph-builder --test release_v4_contract` の `documented_artifact_sizes_match_the_generated_files` が本表を実測値と照合するので、数値を手書きで写し直してずれると落ちる。

| 成果物 | schema | 内容 | ファイルサイズ |
| --- | ---: | --- | ---: |
| `graph.json` | 4 | 22,824 nodes / 22,987 edges（Shutoko 22,621、Entry 168、Exit 198）、billing pairs 10件（legacy 8 + radial 2）、route memberships 53件、ramps 236件 | 7,606,019 bytes |
| `od-tariffs.json` | 3 | 料金表 v3（規則 2 件、evidence 20 件、assignment 10 件、deprecated 2 件） | 48,482 bytes |
| `pair-candidates.json` | 2 | 導出レポート（候補 11 件 = eligible 9 / hold 2、relation coverage 26 件 = pass 11 / fail 15） | 75,737 bytes |
| `ramps.json` | 1 | 正規台帳399件、うちbound 236件 | 278,337 bytes |
| `snap-index.json` | 2 | Entryアクセス地点168件 | 15,244 bytes |
| `manifest.json` | 1 | release、schema/route-plan/tariff/hash、pairDerivation、artifact hash、byte length、unverified sections、provenance 9件 | 45,773 bytes |

`graph.json` 単体は10MiBの転送予算より小さい。`manifest.artifacts[]` は 5 成果物（`graph.json` / `od-tariffs.json` / `pair-candidates.json` / `ramps.json` / `snap-index.json`）の path・SHA-256・byte length を固定し、manifest 自身のサイズと schema は別情報として扱う。`all-real-v4` の `routeMembershipsSha256` は `6cb9b78af5cd556abae9b2285cb41f2e501d10a891e1845593fce8d4871aa6f5` であり、同一入力の2回の生成で一致する。3 世代分のバイト一致は `cargo test --release -p shutoko-graph-builder --test release_v4_contract --locked -- --ignored` の `all_real_v4_artifacts_are_byte_identical_across_three_generations` が担保する。

### `snap-index.json` の意味と `schemaVersion: 2`

`snap-index.json` は一般道ノード一覧から**入口アクセス地点（Entry エッジの from ノード）一覧**へ変わった。WASM はこのインデックスから出発座標に近い順に最大 `max_access_entries` 件のアクセス地点を選ぶ。

### 再現性・決定論的検証
同一入力から2回実行し、`diff -r`で成果物がバイト単位で一致することを確認する。

- `graph.json`: route memberships の hash と禁止遷移・BillingPair のソート順を決定論的に出力する（`schemaVersion: 4`）。
- `snap-index.json`: Entryアクセス地点を安定順序で出力する（`schemaVersion: 2`、168件）。
- `ramps.json`: 正規台帳とbinding結果を安定順序で出力する（`schemaVersion: 1`）。
- `manifest.json`: `graphSchemaVersion=4`、`routePlanVersion=1`、`billingPairsVersion=v3`、`tariffModelVersion=1`、`routeMembershipsSha256`、`pairDerivation` と 5 成果物の SHA-256、byte length、unverified sections、verified pair の provenance を記録する。

## 5. 未検証区間（Unverified Sections）

現時点で課金ペアとして検証されていない入出口ランプ区間は、グラフビルダーによって`manifest.json`の`unverifiedSections[]`へ自動列挙する。
- **自動列挙対象**: graph内のEntry / Exit Edgeのうち、verified billing pairのentry / exitへ採用されていないEdge。wayに`name`があれば「Edge ID（way name）」で記録する。
- **通行規制のskip注記**: 現行 `all-real-v4` は restriction relation 33 件のうち conditional 2 件、via 欠落 2 件、graph 外要素 16 件を skip する（`no_turn` 0 件、`only_turn` 13 件から禁止遷移 16 ペア、`via=way` 0 件）。`all` input のため「C1 以外の路線を除外した」という注記は出さない。
- **現状**: `all-real-v4` は legacy 課金ペア 8 件（`verified_one_section_ahead` 7 件、`unverified` 1 件）と、schema 4 の radial pair 2 件（`verified_one_section_ahead`）の合計 10 件を保持する。内回り銀座 → 新富町（#34）は商品ペアとして登録せず未検証のまま残す。天現寺は 4-way / 16 Edge の最初の一般道接続 node `n:1832672162` で `verified_bound` となり、`radialReturn` として公開集合に含まれる。active 一般ランプ 371 件のうち 236 件（単一 way binding 235 + multi-way candidate の天現寺 1）を exact directed segment へ bind し、134 件は根拠付き `unsupported`、1 件（`ramp:c1-outer:shibakoen-entry`）は reason code `CONDITIONAL_ACCESS_RESTRICTION` 付きの `unresolved` として graph 外へ隔離する。

## 6. 全24路線・正規ランプ台帳（Canonical Ramp Inventory）

首都高速道路全線（東京・神奈川・埼玉の公式24路線区分）を網羅する正規ランプ台帳を導入した。台帳の `route` はデータ上の運用識別子を保持するため25種類あり、1号線を `1H`（羽田）/`1U`（上野）に分ける。したがって「全24路線」は公式路線区分の被覆を表し、JSON値の distinct 数を表さない。

- **台帳ファイル**: `data/ramp-inventory.json`
- **対象路線（全24路線）**:
  - 都心・環状線: C1（都心環状線）、C2（中央環状線）、Y（八重洲線）
  - 放射線: 1号上野線、1号羽田線、2号目黒線、3号渋谷線、4号新宿線、5号池袋線、6号向島線、6号三郷線、7号小松川線、9号深川線、10号晴海線、11号台場線、B（湾岸線）
  - 神奈川エリア: K1（横羽線）、K2（三ツ沢線）、K3（狩場線）、K5（大黒線）、K6（川崎線）、K7（横浜北線・横浜北西線）、B（湾岸線神奈川区間）
  - 埼玉エリア: S1（川口線）、S2（埼玉新都心線）、S5（埼玉大宮線）
- **総ランプ数**: 399 ランプ（一般入口 182、一般出口 189、境界流入 JCT 12、境界流出 JCT 12、閉鎖 4）
- **ランプ種別（`RampKind`）**:
  - `general_entry`: 一般道から首都高速へ流入する一般入口
  - `general_exit`: 首都高速から一般道へ流出する一般出口
  - `boundary_in`: NEXCO（東名・中央・東北・常磐・関越・東関東・京葉・第三京浜・東京外環・東京湾アクアライン等）から首都高速へ流入する境界 JCT
  - `boundary_out`: 首都高速から他社高速道路へ流出する境界 JCT
- **方向・ハーフIC制限の明示**:
  - 各ランプには路線（`route`）、方向（`direction`: `inner`, `outer`, `inbound`, `outbound`, `east`, `west`, `north`, `south`）を付与。
  - 入口専用・出口専用のハーフ IC、ETC 専用ランプなどの制約を構造化。
- **事実と推定・導出値の厳格な分離（Provenance & Verification）**:
  - **公式確認事実（Verified Facts）**: 施設名（`facilityName`）、路線（`route`）、方向（`direction`）、ランプ種別（`kind`）、供用状態（`status`）は、首都高速道路公式検索データ（`https://search.shutoko.jp/`）と現行の路線・出入口案内（`https://www.shutoko.jp/driving/route/`）を 2026-09-16 に照合した正本事実である。
  - **位置座標の導出（Derived Coordinates）**: 公式サイトには緯度経度の数値データは掲載されていない。active 一般ランプの `lat`, `lon` は `data/osm-ramp-bindings.json` の OpenStreetMap 候補から導出した値であり、`coordinateSource: "osm"`, `coordinateStatus: "derived"` として公式事実と区別する。境界 JCT と閉鎖済みランプは公開選択対象外で、OSM binding を持たない。
  - **利用可能性（Support State）**: active 一般ランプの `supportState` は `verified_bound`（236件）、`unresolved`（1件）、`unsupported`（134件）のいずれかである。`unresolved` は恒久的な利用不可ではなく exact binding が未解決という状態で、`supportReasonCode` に理由（例: `CONDITIONAL_ACCESS_RESTRICTION`）を持つ。いずれも公式台帳から削除せず、個別の `supportReason` と `supportEvidence` を保持する。境界 JCT と閉鎖済み施設は `not_routable` とする。
  - **端点能力（Routing Capability）**: verified-bound 236件を、有向Shutoko実グラフ上で5km以上の循環SCCへ接続する `routable` 201件と、接続できない `structural_no_loop` 35件（入口13・出口22）へ全件分類する。分類と理由は台帳・`ramps.json`・manifestへ出力し、生成時に再計算値との完全一致をassertする。
  - **八重洲線の扱い**: 八重洲4件と丸の内1件は公式snapshotに保持する一方、現行fixtureのY線が construction/abandoned 状態でactive `motorway_link`を確認できず、公式liveページも再確認できなかったため `unsupported` とする。閉鎖を断定せず、宝町・C1・霞が関の近傍segmentを流用しない。
  - **利用制約の検証状況（Restriction Verification）**: 首都高では ETC 専用料金所の順次導入（35箇所以上）が進行中であるが、全ランプに対する制約調査は完了していない。そのため、未全数調査のランプは `restrictionStatus: "unverified"` として明示的にモデル化し、公式確認済みのランプ（神田橋、馬場等）のみ `restrictionStatus: "verified"` とする。制約が空配列 `[]` であることをもって「現金利用可能であることが公式確認された」と誤認させない。

### 6.2 公開成果物とグラフへのバインド（`ramps.json`）

`crates/graph-builder` は探索用グラフ `graph.json` に加え、正規ランプ台帳をグラフの各エッジ・ノードに紐付けた公開成果物 `fixtures/generated/ramps.json` を同時に生成する。

- `ramps.json`: 正規ランプ台帳全399件を保持し、`verified_bound` 236件を bound にする。そのうち `routable` 201件だけが周回候補端点であり、`structural_no_loop` 35件は disabled / 診断表示に使う。`unresolved` 1件、`unsupported` 134件、境界JCT 24件、閉鎖済み4件（合計 `not_routable` 28件）は unbound とする。

## 7. OSM ランプバインディング（`data/osm-ramp-bindings.json`）

正規ランプ台帳の active 一般ランプと OpenStreetMap 実データの要素（way / node）を決定論的に紐付ける。境界 JCT と閉鎖済みランプは台帳にのみ保持する。version 4の`bindings[]`は現行schema 2へ投影する単一way binding、`bindingCandidates[]`はpublic projectionから除外する順序付きmulti-way候補として分離する。

- **バインディングファイル**: `data/osm-ramp-bindings.json`
- **判断正本**: `data/ramp-support-decisions.json`。距離順位による fallback は使わず、未分類の公式レコードが現れた場合は生成を停止する。
- **各要素の定義**:
  - `rampId`: 正規ランプ ID（例: `ramp:c1-outer:kandabashi-entry`）
  - `osmWayId`: 現行binding recordの代表`motorway_link` way ID。graph schema 4のendpointは単一wayへ依存せず、`directedSegments[].osmWayIds[]`へwayをまたぐ順序列を保存する
  - `osmNodeId`: 一般道接続端点ノード（入口の乗込ノードまたは出口の流出ノード）
  - `motorwayNodeId`: 首都高本線（`motorway`）との分合流ノード ID
  - `sharedPhysicalOverrides`: 公式番号が異なる共有物理segmentである G15/G27/G53 の完全なメンバー集合、directed segment triplet、理由、証拠。同一facility・別directionも例外にせず、すべてのduplicate triplet集合とoverride集合の完全一致を強制する。方向一意性を立証できない旧22組は `unsupported` としbindingを削除した。
  - `bindingCandidates[]`: `candidateId`, `rampId`, `status`, `publicProjection`, `direction`, 理由・reason code・根拠。`directedSegments[]` は順序付き`osmWayIds`、`osmNodeIds`、`edgeIds`、両端node、`edgeIdsSha256`を保持し、wayTags、route relation role、ground way、公式施設順をsnapshotと照合する。最初の一般道接続nodeに複数のwayが接続する場合は `groundWayIds[]` を昇順で記録し、単数 `groundWayId` を先頭として保持する。最初の接続後に別nodeで一般道へ続く場合だけ `status=unresolved`、`publicProjection=excluded_unresolved` とする。
  - 天現寺出口の4 way は `172358461` → `422023171` → `931759044` → `172358460`。16 Edgeのhashは`bb9114f49d64b952b58b5a2ef53679a6007bea48a51671ade34c56b0325fa7cd`で、最初の接続nodeは `n:1832672162`、`groundWayIds=[258834790]`（明治通り）として `status=verified_bound`、`publicProjection=included_verified` にする。way `172358466` を含む未prune chainは後続接続があるため、診断上だけ `AMBIGUOUS_GROUND_ENDPOINT` とする。
- **Overpass クエリ戦略**:
  - 首都高速道路のリレーション（全 24 路線）および `network="首都高速道路"` タグを起点とし、関連する `motorway_link` を多ホップ展開（1〜4 ホップ）して抽出。
  - 一般道との接続判定は、地表コンテキストウェイ（車両通行可能な `highway` ウェイ）のノード集合との積集合により機械的・決定論的に特定。
  - 境界 JCT は一般道ウェイと接続せず公開入口・出口でもないため、一般ランプ用 binding へ流用せず unbound とする。

### バインディング距離監査の限界

5kmの座標距離監査は粗い変位ガードであり、施設帰属を独立に証明しない。active一般ランプの座標の多くは同じbindingから導出されるため比較が自己参照となり、現行 236 binding のうちランプ座標と OSM 端点が 0.5m 未満で一致するのは 209 件である（天現寺は multi-way candidate で context node の座標が snapshot に無いため照算対象外）。したがって、約1.4km離れた誤帰属も5km閾値だけでは検出できず、「全件を距離で独立検証済み」とは扱わない。

施設同一性の主なhard signalは、OSM wayの `name` / `ref` / `destination`、共有物理segmentの完全なdirected tripletとoverride、公式facility・route・directionとの整合である。ただし `入口` / `出口` を含むname signalがないwayではタグ比較だけでも帰属を証明できないため、graph componentの接続方向と各ランプの個別evidenceに依存する。信号が不足する候補は距離の近さで補わず `unsupported` とする。

## 8. 境界 JCT と一般出入口の分離モデリング

- **境界 JCT の課題**: 他社高速道路（NEXCO、東京外環等）との接続 JCT（例: 用賀・三郷・川口・大泉・東名東京・保土ヶ谷等）は、一般道との直接接続を持たない。これらを一般入口として扱うと、一般道座標スナップで高架下の地表から高速JCTへ直接ワープする誤ルーティングが生じる。
- **分離方式**:
  - `boundary_in` / `boundary_out` を `general_entry` / `general_exit` と明確に区別。
  - 出発地・帰着地の一般道スナップ対象ノードインデックス（`snap-index.json`）には `general_entry` のみを含め、`boundary_in` は地表スナップ候補から除外。
  - 境界 JCT、閉鎖、unsupported は明示的な `entryRampId` / `exitRampId` でも拒否し、公開ルーティング端点にしない。他社線乗り継ぎは現行契約の対象外。

## 9. OD 料金マトリクスと普通車 ETC 計算規則（`data/od-tariffs.json`）

首都高速道路の ETC 料金制度に基づく料金データおよび計算ロジック。

- **料金定義ファイル**: `data/od-tariffs.json`（version 3）
- **券種の前提**: 車種 `ordinary`、支払方法 `etc`、料金種別 `base_toll_excluding_discounts`（普通車 ETC 基本料金・割引適用前）。`fareLabel` は「普通車ETC基本料金（割引適用前）」。除外する割引は `midnight_discount` / `central_tokyo_inflow_discount` / `environmental_road_pricing_discount` / `etc2_discount` / `frequent_user_discount` の 5 種類で、券種や支払方法がこの組合せと食い違う要求は fail-closed とする。
- **公式普通車 ETC 料金体系（`TariffRuleV1` × 2 期間）**:
  - `shutoko-etc-ordinary-2022-04`（`[2022-03-31T15:00:00Z, 2026-09-30T15:00:00Z)`）: 料金距離 4.3 km 以下は下限料金 300 円。4.3 km を超える場合は `(料金距離 km × 29.52 円 + 150 円) × 1.10` を 10 円単位四捨五入し、1,950 円で上限を設ける。
  - `shutoko-etc-ordinary-2026-10`（`2026-09-30T15:00:00Z` から）: 料金距離 3.9 km 以下は下限料金 300 円。3.9 km を超える場合は `(料金距離 km × 32.472 円 + 150 円) × 1.10` を 10 円単位四捨五入し、2,130 円で上限を設ける。
  - 3.9 km は 0.1 km 単位の距離量子、改定後単価、1.10 の税率、10 円単位四捨五入から規則的に 300 円から 310 円へ切り替わる境界である。
  - 期間はすべて半開区間 `[effectiveFrom, effectiveTo)` で、UTC `2026-09-30T15:00:00Z`（JST 2026-10-01 00:00）を境に新規則へ即時切り替える。gap / overlap はビルドエラーとする。
  - 実装はマイクロ円単位の整数演算（`u128`）で行い、浮動小数点を使わない。`subtax = (distanceMeters / 100) * rateMicrosYenPerUnit + terminalChargeYen * 1_000_000`、`taxed = subtax * taxBasisPoints / 10000`、`finalFare = clamp(round_half_up_to_10(taxed / 1_000_000), minimumYen, maximumYen)` の順に評価する。
- **料金距離と実走行距離のスキーマ分離**:
  - `shutoko_distance_meters`: 首都高速上の実際の走行距離（エッジ長の積算値）。周回ループを含むため数十〜百キロ超になり得る。
  - `toll.billing_distance_meters`: 入口〜出口間の公称料金距離（OD テーブルまたはベースライン最短経路長）。
  - **OSM 幾何距離を公称料金距離として扱わない規律**: グラフ幾何から計算される実走距離（`shutoko_distance_meters`）を公称料金距離として勝手に流用しない。料金計算は `data/od-tariffs.json` の検証済み OD ペアまたは公式料金距離テーブルに明示された値のみを根拠とし、未定義区間では安易な幾何距離代用を行わず未計算（None）として誠実にモデル化する。
  - 周回走行を行っても、料金距離は入口と出口の組み合わせで決める。現行 10 assignment は料金表 v3 の期間別 evidence を使い、霞が関→代官町は両版とも 2.3km / 300円とする。2号 radial pair 2 件も 1 件の assignment を共有し、19.4km / 790円・19.4km / 860円を使う。「1区間先だから 300 円」という理由だけで金額を決めない。
  - 3号用賀と4号高井戸の同一地点折り返しは公式 OD セルが存在しないため `deprecatedAssignments` へ移し、`amountYen=null` / `billingDistanceMeters=null` / `tariffStatus=unpriced` のまま扱う（OSM 距離での補完はしない）。
- **検証済み OD ペア**:
  - フェーズ 1 で必要な 10 の一意 OD について、公式 OD セルと規則検算の両方が一致することを人手で確認したデータだけを保持する。料金表全体の OD マトリクス自動抽出は行わない。
  - 料金セル根拠のない OD は `amountYen=null`、`billingDistanceMeters=null`、`tariffStatus=unpriced` のまま残し、OSM 実走距離・周回距離・anchor 距離・最短経路を一切 fallback にしない。active 期間外は `expired` とする。

### 9.1 料金 v3 データと 2026-10 OD 表の検証済み状態

`data/od-tariffs.json` version 3 は `tariffRules`、期間ごとの `distanceEvidence`、10件の一意な OD `assignments`、および `deprecatedAssignments` を分離する。証拠となる文書は 4 つで、`documents[]` に版・URL・cache path・SHA-256・レビュー状態を保持する。

| documentId | 版 | 参照先 | SHA-256 | 用途 |
| --- | --- | --- | --- | --- |
| `shutoko-2025-04-od-fare-table` | 2025-04 | `2504_pamphlet_fee_table.pdf` | `dc2d80ee0c8f215ffcf544f1f2daf3b84ff036e636782be31e438a7113c3a889` | OD セル（P.3 C1、P.4 2号） |
| `shutoko-2026-04-fare-guide` | 2026-04 | `2604_pamphlet_guide.pdf` | `8221fca208884b7ab37d92c58dcb7bba2a09e416687588dd19d419bd312a1e45` | 距離量子・丸め・上下限の規則構造 |
| `shutoko-2026-10-revision-material` | 2026-10 | `31-toll-shiryo.pdf` | `f80126994b3deee36e198f947f3f4f4c3219dd16473bbd9bc9dd296115345702` | 改定後の単価・上下限・端数処理（P.5-6） |
| `shutoko-2026-10-od-fare-table` | 2026-10 | `ryoukin-kaitei_toll_rates.pdf` | `1dd86cf7946deb28ca6e25d57f133f3acf1c4f5e4110d70d00d8055b3ee6d1e5` | 改定後の OD セル（P.3 C1、P.4 2号） |

各 `distanceEvidence` レコードは PDF ページ、行見出し、列見出し、セル識別子、基本料金区分、距離（100m 刻み）、観測基本料金額、規則検算値、PDF の SHA-256、レビュー日時とレビュー方法（ページ画像と PDF テキスト層の 2 通り）を保持する。2025-04 版と 2026-10 版は別の `evidenceId` を持ち、1 つの price period はちょうど 1 つの版だけを指す。

2025-04 の普通車規則は `[2022-03-31T15:00:00Z, 2026-09-30T15:00:00Z)`、2026-10 の規則は `2026-09-30T15:00:00Z` から開始する。2026-10 のパラメータは1kmあたり32.472円、下限300円、上限2,130円、ターミナルチャージ150円、税率1.10、距離量子100m、10円単位の四捨五入、3.9kmの下限境界である。改定後パラメータの根拠資料は SHA-256 `f80126994b3deee36e198f947f3f4f4c3219dd16473bbd9bc9dd296115345702` の `31-toll-shiryo.pdf` P.5-6 であり、ODセルの確認には別の2026-10 PDFを使う。

2号線の inner・outer 計画は目黒入口→天現寺出口の 1 件の一意な assignment（`assignment:2:meguro-tengenji`）を共有する。2025-04 は P.4 の 19.4km / 790円、2026-10 は同じ距離で 860円である。旧設計値の 14.2km・630円は別の列の値であるため採用しない。C1 P.3 の 9 OD セルと 2号 P.4 の 1 セル（合計 10 OD）も含めて、PDF のセルと規則計算値が一致することを確認済みで、`pendingEvidence` は空、各 assignment の 2 期間とも `priced` である。`pendingResolution.status` は `completed_2026_10_pdf_review` で、PDF 自体とページ画像は gitignore 済み cache（`.cache/official-fare/`）に置き commit しない。legacy `verifiedOdPairs` は現行 reader との暫定互換投影であり、料金表 v3 の正本ではない。top-level の legacy `rules` ブロック（2026-10 の固定費・税率・上下限・距離下限・単価を `tariffRules` と重複して保持していたもの）は読み込む箇所が存在しないため削除済みで、`tariffRules` だけがパラメータの正本である。ブロックを戻すと `deny_unknown_fields` により catalog 読み込みが失敗する。

2026-09-26 にページ画像から 10 OD セルと 2025-04 P.4 の目黒→天現寺セルを読み直した独立照合を実施し、2026-10 の 10 セルと 2025-04 の 19.4km / 790円がすべて一致することを確認した。同じ照合で 2025-04 P.3 の霞が関→代官町が 12.4km / 570円ではなく 2.3km / 300円であることを発見し、evidence・assignment・`verifiedOdPairs`・seed・テスト・本節の記述を訂正した。12.4km / 570円は霞が関→霞が関の対角セルの値である。

`migration` は既存データの keep / replace / deprecate を明示する。keep は 4 件、added は 6 件、deprecate は 3号用賀と 4号高井戸の 2 件で、`verifiedOdPairs` には deprecate した OD を含まない。

## 10. 成果物公開アーティファクト（5 成果物 + manifest）

グラフビルダーは、ビルド時に 5 成果物と manifest を生成・出力する:
- `graph.json`: 道路ネットワークグラフ（ノード、エッジ、バインド済みランプ、route memberships、料金表 v3 内蔵）
- `od-tariffs.json`: 料金表 v3 のカタログ（規則・evidence・assignment・deprecated）。`data/od-tariffs.json` と同一内容
- `pair-candidates.json`: 課金ペアの自動導出レポート（`billingPairDerivation/v2`）。seed は変更しない
- `ramps.json`: 正規ランプ台帳全399件の属性・座標・support state・routing capability・グラフバインド状態を格納した公開成果物（`verified_bound` 236件のみ bound）
- `snap-index.json`: 地表スナップ用入口アクセス地点インデックス
- `manifest.json`: 5 成果物の SHA-256 と byte length、`routeMembershipsSha256`、`pairDerivation`、未検証区間、検証済みペア出典情報

## 11. 課金ペアの自動導出（`billingPairDerivation/v2`）

同じビルダーが、検証済みのシード定義から別の候補を **列挙だけして** 報告する導出機能を持ち、結果を `fixtures/generated/pair-candidates.json` に出力する。導出結果は seed を自動更新する仕組みではない（`automaticSeedWrite: false`）。

### 入力

| 入力 | 役割 |
| --- | --- |
| `fixtures/osm/shutoko-all.json` | 道路形状 |
| `data/ramp-inventory.json` | 正規ランプ台帳 |
| `data/ramp-support-decisions.json` | 端点 support 判定（`verified_bound` / `unresolved` / `unsupported`） |
| `data/osm-ramp-bindings.json` | exact directed binding と multi-way candidate |
| RouteMembershipIndex | relation mainline と bound ramp の順序付き Edge 列 |
| `data/billing-pair-adjacency.json` | 人手レビュー済みの公式路線順の隣接関係と directed route-plan 証跡 |
| `data/od-tariffs.json` | 料金表 v3 の assignment |
| `data/billing-pairs-seed.json` | 登録済みの商品ペア |

`pairDerivation.inputHashes` に記録されるのは上表 8 行の入力に対する 5 つのハッシュ（`osmSnapshotSha256` / `rampLedgerSha256` / `routeMembershipIndexSha256` / `billingPairAdjacencySha256` / `odTariffsSha256`）で、`pair-candidates.json` と `manifest.json` に同じ値を入れる。ただし 5 つのハッシュと 8 行の入力は 1 対 1 に対応しない。`rampLedgerSha256` は `data/ramp-inventory.json` 単体のハッシュではなく、台帳・`data/ramp-support-decisions.json`・`data/osm-ramp-bindings.json` の 3 つを 1 つの JSON 配列にまとめて直列化した合成ハッシュであり、`routeMembershipIndexSha256` はファイルではなく `RouteMembershipIndex` 列を直列化した値である。登録済み商品ペアの `data/billing-pairs-seed.json` も導出入力の 1 つだがハッシュは記録しないため、seed まで含めた完全な再現性は現状この 5 つのハッシュでは固定されない。seed のハッシュの追加は未実施であり、必要になれば `inputHashes` へのキー追加というコード変更を要する。

### ゲート構造

候補が `eligible_for_review` になるには 8 個のゲートをすべて通過する必要がある。

1. **official adjacency**: `billing-pair-adjacency.json` に `reviewStatus: reviewed` の隣接関係がある
2. **route**: 宣言された route の relation mainline が展開済み
3. **direction**: 要求方向の有向接続が要求どおりに存在する
4. **first exit**: relation 制約つきの First Exit（return corridor 上の最初の一般出口）が一致する
5. **mandatory lap**: M→B の通常長弧が `lapCount=1` で構成される
6. **entry binding**: entry ランプが `firstPublicRoadConnection/v1` で `verified_bound`
7. **exit binding**: exit ランプが `verified_bound`
8. **tariff assignment**: 有効な料金規則と公式 PDF セル証跡が存在する

いずれかを満たさない候補は `promotionDecision: hold` となり、`rejectionReasons` に gate ごとの理由コード（`CONDITIONAL_ACCESS_RESTRICTION`、`AMBIGUOUS_GROUND_ENDPOINT`、`ROUTE_RELATION_NO_MEMBERSHIP` など）を残す。**silent な skip は禁止**で、判定できなかった関係はすべて `relationCoverage[]` に `status` と reason code 付きで記録する。

### 現行の導出結果（実測）

| 指標 | 値 |
| --- | ---: |
| 候補総数 | 11 |
| `eligible_for_review` | 9 |
| `hold` | 2（`bp:c1-inner:ginza-shintomicho` と `bp:c1-outer:shibakoen-iikura`） |
| route relation 総数 | 26 |
| 展開できた relation | 11 |
| 展開できなかった relation | 15（すべて `ROUTE_MEMBERSHIP_RELATION_INVALID` などの reason code 付き） |

展開できた relation は 11 件で、`relationCoverage[]` の `status=pass` 11 レコードと distinct な `relationId` 11 件が一致する。内訳は C1（`4256008`）、C2（`4256077`）、Y（`4256119`）、2号（`4256339`）と、9号（`4257496`）、11号（`4257564`）、K2（`4258166`）、K5（`4259192`）、K6（`4259197`）、K7（`10355798` と `10732984` の 2 relation）である。routeId でまとめると 10 種類になる（K7 が 2 relation を持ち、どちらも `route:K7:forward` へ展開される）。C2（`4256077`）は C1 / C2 / Y / 2号 のグループにも `forward` グループにも現れる 1 本の relation であり、二重計上しない。Y（`4256119`）は `status=pass` だが `reasonCode=ROUTE_RELATION_NO_MEMBERSHIP` で `membershipIds` は 0 件のため、mainline membership は作られていない。展開できなかった 15 件はすべて `reasonCode=ROUTE_MEMBERSHIP_RELATION_INVALID` で、その detail には「way に Shutoko エッジが無い」「方向の無い mainline member が曖昧」「`motorway_link` を含む relation」など理由が残る。

### 人手レビューと seed 更新

導出レポートは seed を直接変更しない。レビュー担当者はレポートと公式路線図・料金表を突き合わせ、reviewed PR を通じて `data/billing-pairs-seed.json` と `data/billing-pair-adjacency.json` を更新する。OSM 形状が公式意味を修復・昇格させることはない。

## 12. 保守・更新ワークフロー（ランプ・路線・料金の追加手順）

路線拡張やランプの新設・改修、料金改定時は以下の手順で安全に更新を行う:

1. **公式snapshot更新**: `data/official-population-snapshot.json` を更新する。
2. **根拠付き分類**: `data/ramp-support-decisions.json` に `verified_bound` と exact directed segment、`unresolved` multi-way candidate、または `unsupported` と理由・証拠を追加する。未分類のまま生成しない。
3. **隣接関係**: `data/billing-pair-adjacency.json` に reviewed / blocked の隣接関係と directed route-plan 証跡を追加する。
4. **料金定義**: 必要に応じて `data/od-tariffs.json` に新 OD ペアの料金距離・料金額と期間別 evidence を追加する。
5. **自動バリデーション**: `cargo test -p shutoko-graph-builder` を実行。台帳・バインディング・隣接関係・料金の整合性検証（ID 参照整合性、座標範囲、料金範囲、10円丸め等）が自動的に走る。
6. **フィクスチャ再生成**: `bash scripts/generate-fixtures.sh` を実行し、`fixtures/generated/` の成果物を更新。`pair-candidates.json` の `eligible_for_review` / `hold` を確認する。
7. **回帰テスト**: `cargo test --workspace --locked`、`cargo test --release -p shutoko-routing-core --test real_graph_contract --locked -- --ignored`、`cargo test --release -p shutoko-graph-builder --test release_v4_contract --locked -- --ignored`、`cargo test --release -p shutoko-graph-builder --test route_relation_coverage --locked -- --ignored` で回帰がないことを確認。

## 13. CI における自動再生成検証

パイプラインの決定論的性質と成果物の整合性を担保するため、GitHub Actions ワークフロー（`.github/workflows/ci.yml`）で再生成チェックを自動実行している。
- コミット済みの `fixtures/osm/shutoko-all.json` を入力とし、外部 Overpass API にはアクセスしない（外部ネットワーク非依存）。
- 一時ディレクトリへの生成と in-place の生成を両方行い、`diff --no-dereference -r` で 5 成果物が一致すること、`git diff --exit-code` と `git status --porcelain` で作業ツリーが dirty のまま残らないことを検証する。
- 3 世代のバイト一致は `release_v4_contract` の `all_real_v4_artifacts_are_byte_identical_across_three_generations`、relation 展開の網羅は `route_relation_coverage` が担保する。
- グラフビルダーのロジックや課金シード・料金表の更新時は、再生成された `fixtures/generated/` を同一 PR でコミットする必要があり、意図しない出力の乖離やリグレッションを防ぐ。
