# 実データ道路グラフ・課金ペア生成パイプライン

本ドキュメントでは、OpenStreetMap（OSM）実データから首都高速道路の全24路線と接続ランプを抽出し、決定論的な道路ネットワーク成果物（`graph.json`、`ramps.json`、`snap-index.json`、`manifest.json`）を生成するオフラインパイプラインの仕様と手順を記録する。一般道はルーティンググラフのエッジには含めないが、入口・出口ランプの分類コンテキストとして取得・参照する。

## 1. ライセンスと帰属表示

- **データ提供元**: [OpenStreetMap](https://www.openstreetmap.org/)
- **著作権・帰属表示**: `© OpenStreetMap contributors`
- **ライセンス**: [Open Database License (ODbL) 1.0](https://opendatacommons.org/licenses/odbl/1-0/)
- **権利表記 URL**: [https://www.openstreetmap.org/copyright](https://www.openstreetmap.org/copyright)

本プロジェクトが生成する派生成果物は ODbL に準拠して配布・利用される。

## 2. OSM 実データ取得手順

### 現行 `all-real-v2` の取得仕様

- **Overpass API エンドポイント**:
  - 主系: `https://overpass-api.de/api/interpreter`
  - 副系: `https://overpass.kumi.systems/api/interpreter`
- **クエリの正本**: `scripts/fetch-osm.sh`。既定は全24路線の relation を取得し、ランプを4 hop、context way を1 hop展開する
- **出力先**: `fixtures/osm/shutoko-all.json`
- **source date**: `2026-09-16`
- **ファイル SHA-256**: `566f3d7910c3962600e05d0e9d442b0621ae2bcac817fd375b60267f8a22a4c9`
- **ファイルサイズ**: 4,203,540 bytes
- **要素数**: 合計 26,847 要素（ノード 23,661 / way 3,125 / リレーション 61）
- **生成 release**: `all-real-v2`（graph schema 2）

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
  - `restriction:conditional`（時間帯・車種条件付き制限）: 静的道路グラフでは一意に評価できないためスキップし、標準エラー出力およびマニフェストへ記録。
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

**現行 `all-real-v2` の結果**:

| 種別 | エッジ数 |
|------|---------:|
| Entry | 168 |
| Exit | 182 |
| Shutoko | 22,637 |
| Local | 0 |
| undecidable_ramp_edges | 0 |

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
- **通行料金**: 首都高速道路株式会社の公式料金表およびプレスリリースに基づき、普通車 ETC 料金（300 円、料金距離 1.7km・普通車下限料金適用）を有効期間付きで 2 レコード登録済み。2026-10-01 の料金改定（キロ単価引き上げ）後も下限料金 300 円は維持されるため、改定前後で期間を分割して登録している。これにより `routing-core` において有効期間に応じた `amountYen: 300` が解決され、`time_per_yen`（首都高時間 / 料金円）ソートが有効化されている。
  - 現行レコード: 金額 300 円、有効期間 `2022-03-31T15:00:00Z` 〜 `2026-09-30T15:00:00Z`（JST 2022-04-01 00:00 〜 2026-10-01 00:00）
  - 2026-10-01 改定後レコード: 金額 300 円、有効期間 `2026-09-30T15:00:00Z` 〜 期限なし（null）
  - 出典:
    - 料金体系・下限料金: `https://www.shutoko.jp/tolls/about/price/`（旧 URL `https://www.shutoko.jp/fee/fee-info/about/` は 2026-09-10 時点で /tolls/about/price/ へ 301 リダイレクト）
    - 神田橋〜宝町 料金距離 1.7km・300 円: `https://edge.sitecorecloud.io/metropolita84c2-shutokoeb0e-productionbcbd-eb79/media/Project/shutoko/docs/drivers/tolls/about/price/2504_pamphlet_fee_table.pdf`（旧 URL `https://www.shutoko.jp/-/media/pdf/responsive/customer/fee/fee-info/2504_pamphlet_fee_table.pdf` は 2026-09-10 時点で 404。首都高料金表 2025年4月改訂版 P.3「料金・距離表（ETC 普通車）」）
    - 2026-10-01 改定発表: `https://www.shutoko.co.jp/company/press/2026/data/07/31-toll/`
    - 参照日: `2026-09-10`

#### 新規登録 7 ペア（2026-09-10）

いずれも普通車 ETC で料金距離に応じた下限料金 300 円が適用される。料金は神田橋〜宝町と同一の考え方により、改定前後 2 レコード（`2022-03-31T15:00:00Z`〜`2026-09-30T15:00:00Z` / `2026-09-30T15:00:00Z`〜無期限、いずれも 300 円）を登録している。出典は共通で、料金距離・料金は首都高料金表 2025 年 4 月改訂版 P.3、1 区間先の隣接関係は公式路線図に基づく（参照日 `2026-09-10`）。

| # | 方向 | 入口 → 出口 | 料金距離 | 普通車 ETC | ペア ID | 入口 way | 出口 way | 基準点（anchor）node |
|---|------|------------|---------|-----------|---------|---------|---------|---------------------|
| 1 | 外回り | 霞が関入口 → 代官町出口 | 2.3km | 300 円 | `bp:c1-outer:kasumigaseki-daikancho` | `916571610` | `276920911` | `577255571` |
| 2 | 外回り | 銀座入口 → 芝公園出口 | 3.4km | 300 円 | `bp:c1-outer:ginza-shibakoen` | `4848922` | `944671542` | `31254160` |
| 3 | 外回り | 芝公園入口 → 飯倉出口 | 1.6km | 300 円 | `bp:c1-outer:shibakoen-iikura` | `4853801` | `203832842` | `31296971` |
| 4 | 内回り | 霞が関入口 → 芝公園出口 | 3.7km | 300 円 | `bp:c1-inner:kasumigaseki-shibakoen` | `916571615` | `203873821` | `264877748` |
| 5 | 内回り | 代官町入口 → 霞が関出口 | 2.3km | 300 円 | `bp:c1-inner:daikancho-kasumigaseki` | `1091280541` | `1232166619` | `297945194` |
| 6 | 内回り | 芝公園入口 → 汐留出口 | 2.4km | 300 円 | `bp:c1-inner:shibakoen-shiodome` | `4853797` | `45068171` | `31295430` |
| 7 | 内回り | 宝町入口 → 神田橋出口 | 1.7km | 300 円 | `bp:c1-inner:takaracho-kandabashi` | `378284514` | `390441534` | `1891818143` |

- 出典（共通）:
  - 料金距離・料金: `https://edge.sitecorecloud.io/metropolita84c2-shutokoeb0e-productionbcbd-eb79/media/Project/shutoko/docs/drivers/tolls/about/price/2504_pamphlet_fee_table.pdf`（旧 URL `https://www.shutoko.jp/-/media/pdf/responsive/customer/fee/fee-info/2504_pamphlet_fee_table.pdf` は 2026-09-10 時点で 404。首都高料金表 2025 年 4 月改訂版 P.3「料金・距離表（ETC 普通車）」）
  - 路線図（1 区間先の隣接関係）: `https://www.shutoko.jp/use/network/map/`
  - 料金体系・下限料金: `https://www.shutoko.jp/tolls/about/price/`（旧 URL `https://www.shutoko.jp/fee/fee-info/about/` は 2026-09-10 時点で /tolls/about/price/ へ 301 リダイレクト）
  - 2026-10-01 改定発表（下限料金 300 円維持）: `https://www.shutoko.co.jp/company/press/2026/data/07/31-toll/`
  - 参照日: `2026-09-10`

> **注記（内回り銀座入口の 1 区間先について）**:
> 内回り銀座入口 → 新富町出口（0.4km、300 円）は公式資料上の 1 区間先だが、OSM の分流点・合流点の順序（内回り新富町出口の分流点が銀座入口の合流点より上流にある）により First Exit 検証が通らないため未登録。京橋出口は 2 区間先なので登録しない。

### 3.1 `schemaVersion: 2` の混在 seed（parser実装済み）

Issue #42 で C1 legacy と2号 radial pair を同じ seed ファイルへ混在させる。既存の `schemaVersion: 1` は現行どおりに読み込める。同一ファイルを schema 2 へ更新する時も、既存 C1 8要素の項目、値、意味は変更しない。`pairKind` を持たない要素は legacy ring pair と解釈する。

Issue #62 で parser は `schemaVersion` を明示的に 1 / 2 へ dispatch し、全 nested struct の `deny_unknown_fields` を実装した。schema 2 では `pairKind` がない要素を legacy ring、`pairKind: "radialReturn"` と `routePlanVersion: 1` を持つ要素を diagnostic radial pair として読む。Issue #65 で graph schema 4 reader は両 variant と binding / membership / resolved segment を読むが、diagnostic radial pair は exact binding 完了まで `Graph.billingPairs` へ昇格させない。以下の fail-closed 規則は Issue #62 の fixture と unit test で検証済み。

混在の規則は次のとおりである。

- parser は `schemaVersion` を明示的に `1` / `2` へ dispatch し、それ以外は parsing 前に拒否する。
- schema 1/2 の top-level、pair、provenance、price、endpoint、route plan、status の全 nested struct で unknown field を拒否する。
- legacy 要素は現行の `entryOsmWayId`、`exitOsmWayId`、`anchorOsmNodeId`、`status`、`oneSectionAheadVerified` を持つ。`pairKind` は必須ではない。
- radial 要素は `pairKind: "radialReturn"` と `routePlanVersion: 1` を必須とする。
- `pairKind` または `routePlanVersion` が未知なら fail-closed で拒否する。
- radial が `pairKind` / `routePlanVersion` のどちらかを欠く場合、または legacy が variant 必須フィールドを欠く場合も拒否する。
- seed 内に legacy と radial を何件ずつ含めてよい。ただし ID は重複させない。同じ array 内で endpoint support、pair eligibility、loop validation、tariff status を混ぜない。
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
  "entryEndpoint": {
    "rampId": "ramp:2-inbound:meguro-entry",
    "name": "目黒入口",
    "supportState": "verified_bound",
    "directedSegments": [
      {
        "segmentId": "ramp:2-inbound:meguro-entry:segment:0",
        "osmWayIds": [207535708],
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
    "supportState": "unsupported",
    "directedSegments": [],
    "bindingCandidates": [
      {
        "candidateId": "tengenji:g21:172358461-422023171",
        "status": "unresolved",
        "directedSegments": [
          {
            "segmentId": "tengenji:g21:chain:0",
            "osmWayIds": [172358461, 422023171],
            "edgeIds": [
              "e:w172358461:0:f",
              "e:w172358461:1:f",
              "e:w172358461:2:f",
              "e:w172358461:3:f",
              "e:w172358461:4:f",
              "e:w172358461:5:f",
              "e:w422023171:0:f",
              "e:w422023171:1:f",
              "e:w422023171:2:f"
            ],
            "fromNodeId": "n:252175582",
            "toNodeId": "n:1832672090",
            "edgeIdsSha256": "ac97e5a464d46bc4dd5cfb641171da94639832516d8bc4876ef510dab2a7872a"
          }
        ]
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
        "exactDirectedBinding": "unsupported"
      }
    }
  },
  "routingCapability": "routable",
  "pairEligibility": {
    "status": "unverified",
    "oneSectionAheadVerified": false
  },
  "loopValidation": {
    "status": "declared_route_validated"
  },
  "tariff": {
    "status": "unpriced",
    "amountYen": null,
    "billingDistanceMeters": null,
    "prices": []
  },
  "provenance": {
    "source": "https://www.shutoko.jp/use/network/map/",
    "sourceDate": "2026-09-16",
    "notes": "目黒入口から一ノ橋JCTのC1 inner長弧を通り、2号下りへ戻った最初の一般Exit候補を天現寺とする。天現寺exitのexact directed bindingは未解決。"
  }
}
```

endpoint は単一 OSM way を仮定しない。`supportState=verified_bound` では `directedSegments[]` に、解釈が確定した順に連続する `osmWayIds` と `edgeIds` を必ず記録する。way をまたぐ場合も1つの directed segment にまとめ、各要素の接続と順序を検証する。`supportState=unresolved` / `unsupported` では `directedSegments` を空にし、監査した候補だけを `bindingCandidates[]` に置く。候補は `eligibilityStatus=verified_one_section_ahead` へ昇移できず、目黒出口や別施設 ID で補完しない。

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
| `prices[]` | `tariff.prices[]`, `tariff.status` | 300円→300円と有効期間を保持する。radial は #41 まで空。 |
| なし | `routePlanVersion`, `entryCorridor`, `anchor`, `mandatoryLap`, `returnCorridor` | radial variant だけを必須にする。 |
| なし | `pairEligibility`, `loopValidation`, `tariff` の独立 status | endpoint support、routing capability、loop validation、料金状態を混在させない。 |

generated graph の `billingPairs[]` も同じ判別 union とする。schema 2/3 の `pairKind` なしは legacy として読めるが、schema 4 の builder 出力では `pairKind` を必ず書く。`legacyRing` は `entryToAnchorEdgeIds` と `anchorToExitEdgeIds` を必須にし、`radialReturn` は `anchorNodeId` を省略する。未知の kind、anchor kind、route plan version は reader と builder の両方で拒否する。Issue #65 で schema 4 builder 出力の legacy pair を明示 union へ変換し、core reader、WASM 型、Web Worker の dispatch / fail-closed 契約を追加した。

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

schema 4 の manifest は `billingPairsVersion=v2`、graph schema 4、route plan version、route membership hash を記録し、`graph.json`、`ramps.json`、tariff 成果物と release ID を結び付ける。旧 manifest の schema 1/2 record は上書きしない。公開 release の切替は、consumer reader と Web/Workers の検証が通った後に行う。

### 3.3 `RouteMembershipIndex` は本線 relation と ramp binding を別々に証明する

Issue #63 で graph-builder にこのデータ型と生成・検証処理を追加し、Issue #64 で `routePlanLapV1` の directed mandatory lap と return-corridor First Exit を同じ membership index 上で生成・検証する処理を追加した。`--graph-schema 4` を明示した場合だけ `graph.json` の top-level に `routeMemberships[]` を出力し、既定の schema 2 / `fixtures/generated/*` は変更しない。OSM relation の `relationMainline` と、正規ランプ台帳の exact directed binding に由来する `boundRamp` は同じ route/direction の membership 内でも別 segment として保持する。各 segment の `orderedEdgeIdsSha256`、source snapshot hash、way/node/Edge 連続性を builder が検証する。relation member は graph 上の端点連続性から directed path として再構成し、並び替えを無検証な断片にしない。`directionMappingVersion=osm-relation-role/v1` を記録し、route 2 の OSM `forward` / `backward` を `outbound` / `inbound` に正規化する。CLI の schema 4 opt-in は現在の実 snapshot に C1 relation `4256008` と route 2 relation `4256339` が揃う場合だけ relation ID を固定し、合成 snapshot では入力中の route relation を処理する。固定対象以外の relation は bound ramp evidence としてのみ保持する。schema 4 reader は #65 で実装済み。manifest への route membership hash 統合と公開 release の切替は #66 の範囲である。`find_first_exits_from_anchor` は C1 legacy のまま保存し、membership 制約付きの `find_first_exit_on_corridor` は B と return corridor の initial edge から relationMainline の順序どおりに一般 Exit を探す。declared candidate の exact binding が `unresolved` / `unsupported` の場合は次の supported Exit へ進まず、その状態を返す。

OSM route relation は mainline を列挙し、一般入口・出口の ramp way を含まない。目黒 entry way `207535708` や天現寺 exit candidate way `172358461` / `422023171` を mainline relation の member として扱い続けると、正しい ramp binding を relation の連続 Edge 列へ不正に対応させる。したがって、graph schema 4 の top-level `routeMemberships[]` は次の二層構造にする。

| object | 必須 field | 証明する内容 |
| --- | --- | --- |
| `RouteMembershipIndex` | `membershipId`, `routeId`, `direction`, `directionMappingVersion`, `segments[]` | 路線・方向ごとに使う directed segment を束ねる。 |
| `RouteMembershipSegment` | `segmentId`, `sourceKind`, `sourceRelationId`, `sourceSnapshotSha256`, `bindingEvidenceId`, `orderedEdgeIds`, `orderedEdgeIdsSha256` | `sourceKind=relationMainline` なら relation と snapshot、`sourceKind=boundRamp` なら exact binding を由来にする。 |

`relationMainline` は `sourceRelationId` と `bindingEvidenceId=null` を要求し、relation の way member と graph 端点から再構成した directed path を使う。`boundRamp` は `sourceRelationId=null` と非 null の `bindingEvidenceId` を要求し、正規ランプ台帳と exact directed binding の順序付き Edge 列、from/to endpoint、way順、Edge順、hash を使う。どちらも `orderedEdgeIdsSha256` を必須にする。

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

B から全グラフの最短 Exit を選ぶ処理は使わない。実データでは B から C1 芝公園 Exit が1,306m、天現寺候補の開始点が1,972mであり、route constraint なしで Exit を選ぶと誤る。graph-builder の `resolve_diagnostic_radial_route_plan` は seed の declared candidate を検証し、天現寺候補が未解決なら `firstGeneralExit.exactDirectedBinding=unresolved` または `unsupported` を保持したまま次の supported Exit へ skip しない。候補の探索予算が尽きた場合は `ExitNotFound` ではなく `BudgetExceeded` を返す。First Exit の幾何探索が成功しても、端点 support や pair eligibility の証拠にはしない。

実装テストには次を含める。

- inner / outer の M→B長弧を選び、B→Mの0.493km / 0.461km connectorを拒否する。
- relation memberに目黒entryや天現寺exitを含めない現行snapshotで、対応するboundRamp segmentだけをevidence付きで許可する。
- multi-way rampのway順、node接続、Edge順、from/to endpoint、hashを検証し、候補をverified bindingへ昇格しない。
- entry corridorにC1 Exitがあっても、return corridorのExitと混同しない。
- 逆方向、同名JCT、relation非所属mainline way、別armへの近道を拒否する。
- segment内のEdge反復を拒否し、route planが宣言したsegment間反復を許す。
- 探索予算超過を「Exitなし」と読み替えない。
- 既存C1 8件のanchor、edge resolution、First Exit、300円→300円、verified 2件・unverified 6件を回帰testで固定する。


### 3.4 2号計画の診断用データと公開 BillingPair を分ける

本節で定義した inner / outer object は、Issue #62 で `fixtures/seed-v2/diagnostic-radial-v2.json` と `diagnostic-radial-v2.snapshot.json` に固定し、parser test と snapshot で同じ wire shape を確認している。Issue #64 では同じ fixture を graph-builder の diagnostic route-plan resolver に渡し、inner / outer の M→B 長弧と return corridor の状態を検証する。Issue #65 では synthetic graph / wire fragment を reader と WASM/Web consumer へ通し、完全な exact binding のみ graph schema 4 の radial pair として受理する。天現寺 exact directed binding が未解決の間は、plan を `Graph.billingPairs` へ入れて公開候補にしない。

binding issue では、multi-way ramp の全 way、ground ↔ mainline の接続、ramp ID の逆引き、公式施設順を同じ support evidence として扱う。binding が解けた後に、route membership、First Exit、全 segment の完全分割を再検証し、graph schema 4 の `radialReturn` として昇格する。昇格後も Issue #41 までは `amountYen=null`、`billingDistanceMeters=null`、`tariffStatus=unpriced` を維持する。

## 4. 成果物の決定論的再生成手順

### 再生成コマンド
```bash
./scripts/generate-fixtures.sh
```
または直接 CLI を実行する。
```bash
cargo run --bin shutoko-graph-builder --locked -- \
  --osm fixtures/osm/shutoko-all.json \
  --seed data/billing-pairs-seed.json \
  --inventory data/ramp-inventory.json \
  --bindings data/osm-ramp-bindings.json \
  --tariffs data/od-tariffs.json \
  --out-dir fixtures/generated \
  --release-id "all-real-v2" \
  --built-at "2026-09-17T00:00:00Z" \
  --source-date "2026-09-16" \
  --vehicle-profile "passenger-car-etc" \
  --coverage-area "Metropolitan Expressway network (Tokyo, Kanagawa, Saitama)" \
  --graph-version "1.0.0"
```
検証済み課金ペアが1件以上生成されていることを強制したい場合は `--strict` を付ける。検証済みペアが0件の場合に非ゼロで終了する。

### 現行 graph schema 2 が保持する情報

issue #10 の探索コア・WASM 境界拡張に伴い、`graph.json` には次を加えた。

- **Node の地理座標 (`lat`, `lon`)**: 全ノードに必須fieldとして出力する。WASMの空間snapとGeoJSON LineStringの合成で使う。
- **Edge の日本語道路名 (`name`)**: OSMの`name`、なければ`name:ja`を`Edge.name: Option<String>`として伝播する。名前がないEdgeはJSON keyを省略する。
- **課金ペアの公式ランプ名 (`entryName`, `exitName`)**: `data/billing-pairs-seed.json`の値を`graph.json`の`billingPairs[]`へ伝播する。

現行 `all-real-v2` の生成済みファイルは次のとおりである。数値は `fixtures/generated/` の実測値であり、C1限定fixtureの数値ではない。

| 成果物 | schema | 内容 | ファイルサイズ |
| --- | ---: | --- | ---: |
| `graph.json` | 2 | 22,824 nodes / 22,987 edges（Shutoko 22,637、Entry 168、Exit 182）、billing pairs 8件、bound ramps 232件 | 7,089,932 bytes |
| `ramps.json` | 1 | 正規台帳399件、うちbound 232件 | 277,569 bytes |
| `snap-index.json` | 2 | Entryアクセス地点168件 | 15,244 bytes |
| `manifest.json` | 1 | release、hash、byte length、unverified sections、provenance | 41,578 bytes |

`graph.json` 単体は10MiBの転送予算より小さい。`manifest.artifacts[]` は `graph.json`、`ramps.json`、`snap-index.json` のpath・SHA-256・byte lengthを固定し、manifest自身のサイズとschemaは別情報として扱う。

### `snap-index.json` の意味と `schemaVersion: 2`

`snap-index.json` は一般道ノード一覧から**入口アクセス地点（Entry エッジの from ノード）一覧**へ変わった。WASM はこのインデックスから出発座標に近い順に最大 `max_access_entries` 件のアクセス地点を選ぶ。

### 再現性・決定論的検証
同一入力から2回実行し、`diff -r`で成果物がバイト単位で一致することを確認する。

- `graph.json`: 禁止遷移とソート順を決定論的に出力する（`schemaVersion: 2`）。
- `snap-index.json`: Entryアクセス地点を安定順序で出力する（`schemaVersion: 2`、168件）。
- `ramps.json`: 正規台帳とbinding結果を安定順序で出力する（`schemaVersion: 1`）。
- `manifest.json`: 各公開成果物のSHA-256、byte length、unverified sections、verified pairのprovenanceを記録する。

## 5. 未検証区間（Unverified Sections）

現時点で課金ペアとして検証されていない入出口ランプ区間は、グラフビルダーによって`manifest.json`の`unverifiedSections[]`へ自動列挙する。
- **自動列挙対象**: graph内のEntry / Exit Edgeのうち、verified billing pairのentry / exitへ採用されていないEdge。wayに`name`があれば「Edge ID（way name）」で記録する。
- **通行規制のskip注記**: 現行`all-real-v2`はconditional 2件、via欠落2件、graph外要素16件を数える。`all` inputのため「C1以外の路線を除外した」という注記は出さない。
- **現状**: `all-real-v2`は監査用課金ペア8件を保持し、端点と公式施設名を照合できた2件だけを`verified`とする。残る6件は`unverified`としてpair検索から除外する。active一般ランプ371件のうち232件をexact directed segmentへbindし、139件は根拠付き`unsupported`としてgraph外へ隔離する。

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
  - **利用可能性（Support State）**: active 一般ランプは `supportState` が `verified_bound`（232件）または `unsupported`（139件）のどちらか一方である。後者も公式台帳から削除せず、個別の `supportReason` と `supportEvidence` を保持する。境界 JCT と閉鎖済み施設は `not_routable` とする。
  - **端点能力（Routing Capability）**: verified-bound 232件を、有向Shutoko実グラフ上で5km以上の循環SCCへ接続する `routable` 197件と、接続できない `structural_no_loop` 35件（入口13・出口22）へ全件分類する。分類と理由は台帳・`ramps.json`・manifestへ出力し、生成時に再計算値との完全一致をassertする。
  - **八重洲線の扱い**: 八重洲4件と丸の内1件は公式snapshotに保持する一方、現行fixtureのY線が construction/abandoned 状態でactive `motorway_link`を確認できず、公式liveページも再確認できなかったため `unsupported` とする。閉鎖を断定せず、宝町・C1・霞が関の近傍segmentを流用しない。
  - **利用制約の検証状況（Restriction Verification）**: 首都高では ETC 専用料金所の順次導入（35箇所以上）が進行中であるが、全ランプに対する制約調査は完了していない。そのため、未全数調査のランプは `restrictionStatus: "unverified"` として明示的にモデル化し、公式確認済みのランプ（神田橋、馬場等）のみ `restrictionStatus: "verified"` とする。制約が空配列 `[]` であることをもって「現金利用可能であることが公式確認された」と誤認させない。

### 6.2 公開成果物とグラフへのバインド（`ramps.json`）

`crates/graph-builder` は探索用グラフ `graph.json` に加え、正規ランプ台帳をグラフの各エッジ・ノードに紐付けた公開成果物 `fixtures/generated/ramps.json` を同時に生成する。

- `ramps.json`: 正規ランプ台帳全399件を保持し、`verified_bound` 232件を bound にする。そのうち `routable` 197件だけが周回候補端点であり、`structural_no_loop` 35件はdisabled/診断表示に使う。`unsupported` 139件、境界JCT 24件、閉鎖済み4件は unbound とする。

## 7. OSM ランプバインディング（`data/osm-ramp-bindings.json`）

正規ランプ台帳の active 一般ランプと OpenStreetMap 実データの要素（way / node）を決定論的に紐付ける。境界 JCT と閉鎖済みランプは台帳にのみ保持し、このファイルには含めない。

- **バインディングファイル**: `data/osm-ramp-bindings.json`
- **判断正本**: `data/ramp-support-decisions.json`。距離順位による fallback は使わず、未分類の公式レコードが現れた場合は生成を停止する。
- **各要素の定義**:
  - `rampId`: 正規ランプ ID（例: `ramp:c1-outer:kandabashi-entry`）
  - `osmWayId`: 現行binding recordの代表`motorway_link` way ID。graph schema 4のendpointは単一wayへ依存せず、`directedSegments[].osmWayIds[]`へwayをまたぐ順序列を保存する
  - `osmNodeId`: 一般道接続端点ノード（入口の乗込ノードまたは出口の流出ノード）
  - `motorwayNodeId`: 首都高本線（`motorway`）との分合流ノード ID
  - `sharedPhysicalOverrides`: 公式番号が異なる共有物理segmentである G15/G27/G53 の完全なメンバー集合、directed segment triplet、理由、証拠。同一facility・別directionも例外にせず、すべてのduplicate triplet集合とoverride集合の完全一致を強制する。方向一意性を立証できない旧22組は `unsupported` としbindingを削除した。
- **Overpass クエリ戦略**:
  - 首都高速道路のリレーション（全 24 路線）および `network="首都高速道路"` タグを起点とし、関連する `motorway_link` を多ホップ展開（1〜4 ホップ）して抽出。
  - 一般道との接続判定は、地表コンテキストウェイ（車両通行可能な `highway` ウェイ）のノード集合との積集合により機械的・決定論的に特定。
  - 境界 JCT は一般道ウェイと接続せず公開入口・出口でもないため、一般ランプ用 binding へ流用せず unbound とする。

### バインディング距離監査の限界

5kmの座標距離監査は粗い変位ガードであり、施設帰属を独立に証明しない。active一般ランプの座標の多くは同じbindingから導出されるため比較が自己参照となり、現行232 bindingのうち211件はランプ座標とOSM端点が0.5m未満で一致する。したがって、約1.4km離れた誤帰属も5km閾値だけでは検出できず、「全件を距離で独立検証済み」とは扱わない。

施設同一性の主なhard signalは、OSM wayの `name` / `ref` / `destination`、共有物理segmentの完全なdirected tripletとoverride、公式facility・route・directionとの整合である。ただし `入口` / `出口` を含むname signalがないwayではタグ比較だけでも帰属を証明できないため、graph componentの接続方向と各ランプの個別evidenceに依存する。信号が不足する候補は距離の近さで補わず `unsupported` とする。

## 8. 境界 JCT と一般出入口の分離モデリング

- **境界 JCT の課題**: 他社高速道路（NEXCO、東京外環等）との接続 JCT（例: 用賀・三郷・川口・大泉・東名東京・保土ヶ谷等）は、一般道との直接接続を持たない。これらを一般入口として扱うと、一般道座標スナップで高架下の地表から高速JCTへ直接ワープする誤ルーティングが生じる。
- **分離方式**:
  - `boundary_in` / `boundary_out` を `general_entry` / `general_exit` と明確に区別。
  - 出発地・帰着地の一般道スナップ対象ノードインデックス（`snap-index.json`）には `general_entry` のみを含め、`boundary_in` は地表スナップ候補から除外。
  - 境界 JCT、閉鎖、unsupported は明示的な `entryRampId` / `exitRampId` でも拒否し、公開ルーティング端点にしない。他社線乗り継ぎは現行契約の対象外。

## 9. OD 料金マトリクスと普通車 ETC 計算規則（`data/od-tariffs.json`）

首都高速道路の ETC 料金制度に基づく料金データおよび計算ロジック。

- **料金定義ファイル**: `data/od-tariffs.json`
- **公式普通車 ETC 料金体系**:
  - 下限料金: 300 円（料金距離 ≤ 4.3 km）
  - 距離制料金（> 4.3 km）: `(料金距離 km × キロ単価 + ターミナルチャージ) × 1.10`（10 円単位四捨五入）
    - キロ単価: 29.52 円/km（2026 年時点改定後 34.50 円/km）
    - ターミナルチャージ: 150 円
  - 上限料金: 1,950 円（普通車 ETC 上限）
- **料金距離と実走行距離のスキーマ分離**:
  - `shutoko_distance_meters`: 首都高速上の実際の走行距離（エッジ長の積算値）。周回ループを含むため数十〜百キロ超になり得る。
  - `toll.billing_distance_meters`: 入口〜出口間の公称料金距離（OD テーブルまたはベースライン最短経路長）。
  - **OSM 幾何距離を公称料金距離として扱わない規律**: グラフ幾何から計算される実走距離（`shutoko_distance_meters`）を公称料金距離として勝手に流用しない。料金計算は `data/od-tariffs.json` の検証済み OD ペアまたは公式料金距離テーブルに明示された値のみを根拠とし、未定義区間では安易な幾何距離代用を行わず未計算（None）として誠実にモデル化する。
  - 周回走行を行っても、料金距離は入口と出口の組み合わせで決める。現行 C1 8件の legacy price record は300円→300円だが、2号 radial pair の金額を「1区間先だから300円」という理由だけで決めない。
- **検証済み OD ペア**:
  - 頻出・代表的な OD ペア（C1 各ランプ、八重洲線接続、主要放射線連絡等）について公式料金距離および料金額を検証済みデータとして保持。

## 10. 成果物公開アーティファクト（`ramps.json`）

グラフビルダーは、ビルド時に以下のアーティファクトを生成・出力する:
- `graph.json`: 道路ネットワークグラフ（ノード、エッジ、バインド済みランプ、OD 料金）
- `snap-index.json`: 地表スナップ用入口アクセス地点インデックス
- `ramps.json`: 正規ランプ台帳全399件の属性・座標・support state・routing capability・グラフバインド状態を格納した公開成果物（`verified_bound` 232件のみ bound）
- `manifest.json`: 全成果物の SHA-256、未検証区間、検証済みペア出典情報

## 11. 保守・更新ワークフロー（ランプ・路線・料金の追加手順）

路線拡張やランプの新設・改修、料金改定時は以下の手順で安全に更新を行う:

1. **公式snapshot更新**: `data/official-population-snapshot.json` を更新する。
2. **根拠付き分類**: `data/ramp-support-decisions.json` に `verified_bound` と exact directed segment、または `unsupported` と理由・証拠を追加する。未分類のまま生成しない。
3. **料金定義**: 必要に応じて `data/od-tariffs.json` に新 OD ペアの料金距離・料金額を追加。
4. **自動バリデーション**: `cargo test -p shutoko-graph-builder` を実行。台帳・バインディング・料金の整合性検証（ID 参照整合性、座標範囲、料金範囲、10円丸め等）が自動的に走る。
5. **フィクスチャ再生成**: `bash scripts/generate-fixtures.sh` を実行し、`fixtures/generated/` の成果物を更新。
6. **回帰テスト**: `cargo test --workspace --locked` および `cargo test --release -p shutoko-routing-core --test real_graph_contract --locked -- --ignored` で回帰がないことを確認。

## 12. CI における自動再生成検証

パイプラインの決定論的性質と成果物の整合性を担保するため、GitHub Actions ワークフロー（`.github/workflows/ci.yml`）で再生成チェックを自動実行している。
- コミット済みの `fixtures/osm/shutoko-all.json` を入力とし、外部 Overpass API にはアクセスしない（外部ネットワーク非依存）。
- `scripts/generate-fixtures.sh` を実行後、`git diff --exit-code` および `git status --porcelain` でコミット済みの `fixtures/generated/` との差分が一切生じないことを検証する。
- グラフビルダーのロジックや課金シードの更新時は、再生成された `fixtures/generated/` を同一 PR でコミットする必要があり、意図しない出力の乖離やリグレッションを防ぐ。
