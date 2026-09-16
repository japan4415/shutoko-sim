# 実データ道路グラフ・課金ペア生成パイプライン

本ドキュメントでは、OpenStreetMap（OSM）実データから首都高速道路都心環状線（C1）および接続ランプを抽出し、決定論的な道路ネットワーク成果物（`graph.json`、`snap-index.json`、`manifest.json`）を生成するオフラインパイプラインの仕様と手順を記録する。一般道はルーティンググラフのエッジには含めないが、入口/出口ランプの分類コンテキストとして取得・参照している（詳細は「対象範囲」参照）。

## 1. ライセンスと帰属表示

- **データ提供元**: [OpenStreetMap](https://www.openstreetmap.org/)
- **著作権・帰属表示**: `© OpenStreetMap contributors`
- **ライセンス**: [Open Database License (ODbL) 1.0](https://opendatacommons.org/licenses/odbl/1-0/)
- **権利表記 URL**: [https://www.openstreetmap.org/copyright](https://www.openstreetmap.org/copyright)

本プロジェクトが生成する派生成果物は ODbL に準拠して配布・利用される。

## 2. OSM 実データ取得手順

### 取得仕様
- **Overpass API エンドポイント**:
  - 主系: `https://overpass-api.de/api/interpreter`
  - 副系: `https://overpass.kumi.systems/api/interpreter`
- **クエリ SHA-256**: `ae8754b341bce5bd0acb79f70fb8c71175b332679b44d6f3425b6e1a00b1510e`
- **出力先**: `fixtures/osm/shutoko-c1.json`
- **最終取得日時（UTC）**: `2026-09-15T04:38:52Z`（副系エンドポイント使用）
- **ファイルサイズ**: 369,367 bytes（約 361 KB）
- **要素数**: 合計 2,051 要素（ノード 1,745 / way 299 / リレーション 7）

### 対象範囲

一般道（`trunk`、`primary`、`secondary`、`residential`、`service` 等）は**ルーティンググラフには含めない**。ルーティングモデルが「直線距離の近い入口から乗る」前提であるため、一般道経路探索は不要である。ただし入口・出口ランプの**分類コンテキスト**として一般道ウェイを取得する（後述「入口/出口ランプの分類方式」参照）。路線追加は Overpass クエリのリレーション ID を足すだけで可能で、地理的 bbox を指定する必要もない。**旧方式で必要だった路線名リスト（「首都高 + ＜数字＞号」パターンマッチ）のメンテナンスも不要になった**（詳細は「入口/出口ランプの分類方式」参照）。

- **首都高速都心環状線（C1）**: リレーション ID `4256008`（首都高速都心環状線、`ref=C1`）
- **接続ランプ（motorway_link）**: C1 本線ノードから最大 4 ホップで到達可能な進入・退出ランプウェイ（芝公園・飯倉・霞が関・汐留・宝町等の多ホップランプを包含）
- **コンテキストウェイ（surface-road context）**: 4 ホップ展開後、取得済みの motorway / motorway_link ウェイのいずれかのノードを共有するすべての `highway` ウェイ（motorway / motorway_link を除く）。`trunk`・`primary`・`secondary`・`tertiary`・`unclassified`・`residential`・`service`・`living_street` など、出口ランプが降りる先になりうるあらゆる型を対象とする。グラフビルダーはこれらを**ルーティンググラフに含めず**、入口・出口ランプの分類（ランプ終端ノードが一般道と接するか否か）にのみ使用する。**コンテキストウェイは way 要素のみ（`nodes` 配列と `tags`）が出力され、そのノードの座標は出力されない。** 座標の代わりにノード ID の包含チェック（motorway_link のノード ID セットとの積集合）で一般道接続を判定するためである。本データセットでは motorway_link のノード ID と重複するコンテキストウェイのノード ID は 57 件。highway 値ごとの way 件数の内訳: motorway 105 / motorway_link 101 / secondary 24 / footway 19 / tertiary 16 / unclassified 11 / secondary_link 6 / primary 5 / primary_link 2 / residential 2 / pedestrian 2 / service 1（highway タグなし 5）。
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
1. **地表接続シグナル（一次）**: ランプ端点ノードが、車両通行可能な一般道 way のノード集合に含まれるか。`is_vehicle_highway(highway) -> Option<bool>` が道路種別の構造的区分を担い、歩道・歩行者専用路（footway / pedestrian / cycleway / steps 等）は共有ノードがあっても地表接続の証拠に数えない（本データセットで 3 ノードが該当）
2. **OSM ノードタグシグナル（二次）**: `highway=traffic_signals` → 入口/出口の証拠、`highway=motorway_junction` → 本線側の証拠
3. 一次シグナルが決定的ならそれを採用。二次と矛盾する場合は警告を記録
4. どちらでも決まらない場合は `undecidable_ramp_edges` を加算し警告を出したうえで保守的に Shutoko に分類

**結果（`c1-real-v1` データセット）**:

| 種別 | エッジ数 |
|------|---------|
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
引数を省略した場合は既定値（`fixtures/osm/shutoko-c1.json`、`https://overpass-api.de/api/interpreter`）で実行される。

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

## 4. 成果物の決定論的再生成手順

### 再生成コマンド
```bash
./scripts/generate-fixtures.sh
```
または直接 CLI を実行:
```bash
cargo run --bin shutoko-graph-builder --locked -- \
  --osm fixtures/osm/shutoko-c1.json \
  --seed data/billing-pairs-seed.json \
  --out-dir fixtures/generated \
  --release-id "c1-real-v2" \
  --built-at "2026-09-10T00:00:00Z" \
  --source-date "2026-09-10" \
  --vehicle-profile "passenger-car-etc" \
  --coverage-area "Tokyo Inner Circular Route (C1) and connecting ramps" \
  --graph-version "1.0.0"
```
※ 検証済み課金ペアが1件以上生成されていることを強制したい場合は `--strict` フラグを付与して実行可能（検証済みペアが0件の場合に非ゼロで終了）。

### 成果物スキーマの拡張（Node 座標・Edge 名称・ランプ名）
issue #10 の探索コア・WASM 境界拡張に伴い、以下のデータが `graph.json` に追加された:
- **Node の地理座標 (`lat`, `lon`)**: `graph.json` 内の全ノードに f64 の `lat` および `lon` を必須フィールドとして出力。WASM 内部での空間スナップおよび GeoJSON LineString 幾何データ合成に使用される。
- **Edge の日本語道路名 (`name`)**: OSM ウェイの `name`（存在しない場合は `name:ja`）を `Edge.name: Option<String>` として伝播。名前のないエッジは `serde(skip_serializing_if = "Option::is_none")` により JSON 出力からキーが省略される。
- **課金ペアの公式ランプ名 (`entryName`, `exitName`)**: `data/billing-pairs-seed.json` の各ペアに公式ランプ名（例: `"神田橋入口"`, `"宝町出口"`）が定義され、グラフビルダーにより `graph.json` の `billingPairs[]` へそのまま伝播される。
- **ファイルサイズと転送量予算**: 一般道エッジを除外したことで `fixtures/generated/graph.json` は実測 563 KB（1,717 ノード / 1,719 エッジ: shutoko 1,687 / entry 15 / exit 17）となり、プロジェクトのネットワーク転送量上限である 10MiB に対して十分に安全な範囲に収まっている。`schemaVersion` は 2。

### `snap-index.json` の意味と `schemaVersion: 2`

`snap-index.json` は一般道ノード一覧から**入口アクセス地点（Entry エッジの from ノード）一覧**に変わった。`schemaVersion` が 1 → 2 に更新されている。現行データには 15 件の入口アクセス地点が登録されており、ファイルサイズは約 1.5 KB である。WASM はこのインデックスを使って出発座標から近い順に最大 `max_access_entries` 件の入口アクセス地点を選択する。

### 再現性・決定論的検証
同一入力から 2 回実行し、`diff -r` によりバイト完全一致（SHA-256 一致）が確認されている。
- `graph.json`: 禁止遷移（`only_*` および `via=way` を含む）やソート順を決定論的に出力（`schemaVersion: 2`）
- `snap-index.json`: 入口アクセス地点（Entry エッジの from ノード）の空間インデックス（`schemaVersion: 2`、15 ノード、約 1.5 KB）
- `manifest.json`: 全成果物の SHA-256、未検証区間一覧、検証済みペア出典情報（`provenance`）を記録

## 5. 未検証区間（Unverified Sections）

現時点で課金ペアとして検証されていない入出口ランプ区間は、グラフビルダーによって `manifest.json` の `unverifiedSections` 配列に自動列挙される。
- **自動列挙対象**: グラフ内に存在するすべての入口・出口エッジのうち、検証済み課金ペアに採用されていないエッジ。OSM ウェイに `name` タグが存在する場合は「エッジID（ウェイ名）」の形式で可読性を担保。
- **除外路線・通行規制スキップの注記**: C1 外の分岐路線（八重洲線、1号上野線、6号向島線等）や、静的道路グラフで適用外となった通行規制（conditional / no via / outside graph / disconnected / unrecognized 等のスキップカテゴリ）に関する注記も件数付きで同リストに収録。
- **現状**: 現行リリース `c1-real-v2` では 3 節の表に記載した 8 ペア（外回り 4・内回り 4・既存の神田橋〜宝町 1 を含む）すべてが人手検証済み（`verified`）で、`unverifiedSections` に `rejected:` は存在しない。将来追加予定のランプ区間については、公式料金区間表または本線隣接導出の根拠とともに順次シードへ追加する。

## 6. 全24路線・正規ランプ台帳（Canonical Ramp Inventory）

首都高速道路全線（東京・神奈川・埼玉の全 24 路線）を網羅する正規ランプ台帳を導入した。

- **台帳ファイル**: `data/ramp-inventory.json`
- **対象路線（全24路線）**:
  - 都心・環状線: C1（都心環状線）、C2（中央環状線）、Y（八重洲線）
  - 放射線: 1号上野線、1号羽田線、2号目黒線、3号渋谷線、4号新宿線、5号池袋線、6号向島線、6号三郷線、7号小松川線、9号深川線、10号晴海線、11号台場線、B（湾岸線）
  - 神奈川エリア: K1（横羽線）、K2（三ツ沢線）、K3（狩場線）、K5（大黒線）、K6（川崎線）、K7（横浜北線・横浜北西線）、B（湾岸線神奈川区間）
  - 埼玉エリア: S1（川口線）、S2（埼玉新都心線）、S5（埼玉大宮線）
- **総ランプ数**: 339 ランプ（一般入口 156、一般出口 159、境界流入 JCT 12、境界流出 JCT 12）
- **ランプ種別（`RampKind`）**:
  - `general_entry`: 一般道から首都高速へ流入する一般入口
  - `general_exit`: 首都高速から一般道へ流出する一般出口
  - `boundary_in`: NEXCO（東名・中央・東北・常磐・関越・東関東・京葉・第三京浜・東京外環・東京湾アクアライン等）から首都高速へ流入する境界 JCT
  - `boundary_out`: 首都高速から他社高速道路へ流出する境界 JCT
- **方向・ハーフIC制限の明示**:
  - 各ランプには路線（`route`）、方向（`direction`: `inner`, `outer`, `inbound`, `outbound`, `eastbound`, `westbound`, `northbound`, `southbound`, `both` 等）を付与。
  - 入口専用・出口専用のハーフ IC、ETC 専用ランプ（`restrictions.etcOnly`）、大型車通行禁止（`restrictions.largeVehicleBanned`）などの制約を構造化。

## 7. OSM ランプバインディング（`data/osm-ramp-bindings.json`）

正規ランプ台帳の各ランプと OpenStreetMap 実データの要素（way / node）を決定論的に紐付ける。

- **バインディングファイル**: `data/osm-ramp-bindings.json`
- **各要素の定義**:
  - `rampId`: 正規ランプ ID（例: `ramp:c1-outer:kandabashi-entry`）
  - `osmWayId`: ランプを表す OSM `motorway_link` ウェイ ID
  - `osmNodeId`: 一般道接続端点ノード（入口の乗込ノードまたは出口の流出ノード）
  - `motorwayNodeId`: 首都高本線（`motorway`）との分合流ノード ID
- **Overpass クエリ戦略**:
  - 首都高速道路のリレーション（全 24 路線）および `network="首都高速道路"` タグを起点とし、関連する `motorway_link` を多ホップ展開（1〜4 ホップ）して抽出。
  - 一般道との接続判定は、地表コンテキストウェイ（車両通行可能な `highway` ウェイ）のノード集合との積集合により機械的・決定論的に特定。
  - 境界 JCT は一般道ウェイと接続しないため、路線外接続リンク（他社高速リレーション接続ノード）を境界ノードとしてバインド。

## 8. 境界 JCT と一般出入口の分離モデリング

- **境界 JCT の課題**: 他社高速道路（NEXCO、東京外環等）との接続 JCT（例: 用賀・三郷・川口・大泉・東名東京・保土ヶ谷等）は、一般道との直接接続を持たない。これらを一般入口として扱うと、一般道座標スナップで高架下の地表から高速JCTへ直接ワープする誤ルーティングが生じる。
- **分離方式**:
  - `boundary_in` / `boundary_out` を `general_entry` / `general_exit` と明確に区別。
  - 出発地・帰着地の一般道スナップ対象ノードインデックス（`snap-index.json`）には `general_entry` のみを含め、`boundary_in` は地表スナップ候補から除外。
  - 境界 JCT 発着の広域シミュレーションや他社線乗り継ぎ経路は、明示的なランプ ID クエリ（`entryRampId` / `exitRampId`）によってルーティング可能。

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
  - `toll.billing_distance_meters`: 入口〜出口間の最短料金距離（OD テーブルまたはベースライン最短経路長）。
  - 周回走行を行っても、料金距離は入口と出口の組み合わせによって決まるため、1区間先退出時は下限 300 円で周回が可能。
- **検証済み OD ペア**:
  - 頻出・代表的な OD ペア（C1 各ランプ、八重洲線接続、主要放射線連絡等）について公式料金距離および料金額を検証済みデータとして保持。

## 10. 成果物公開アーティファクト（`ramps.json`）

グラフビルダーは、ビルド時に以下のアーティファクトを生成・出力する:
- `graph.json`: 道路ネットワークグラフ（ノード、エッジ、バインド済みランプ、OD 料金）
- `snap-index.json`: 地表スナップ用入口アクセス地点インデックス
- `ramps.json`: 正規ランプ台帳全 339 ランプの属性・座標・グラフバインド状態を格納した公開成果物
- `manifest.json`: 全成果物の SHA-256、未検証区間、検証済みペア出典情報

## 11. 保守・更新ワークフロー（ランプ・路線・料金の追加手順）

路線拡張やランプの新設・改修、料金改定時は以下の手順で安全に更新を行う:

1. **台帳追加**: `data/ramp-inventory.json` に新ランプ（`rampId`, `facilityName`, `route`, `direction`, `kind`, `restrictions`）を追加。
2. **OSM バインディング**: `data/osm-ramp-bindings.json` に対応する `osmWayId`, `osmNodeId`, `motorwayNodeId` を追加。
3. **料金定義**: 必要に応じて `data/od-tariffs.json` に新 OD ペアの料金距離・料金額を追加。
4. **自動バリデーション**: `cargo test -p shutoko-graph-builder` を実行。台帳・バインディング・料金の整合性検証（ID 参照整合性、座標範囲、料金範囲、10円丸め等）が自動的に走る。
5. **フィクスチャ再生成**: `bash scripts/generate-fixtures.sh` を実行し、`fixtures/generated/` の成果物を更新。
6. **回帰テスト**: `cargo test --workspace --locked` および `cargo test --release -p shutoko-routing-core --test real_graph_contract --locked -- --ignored` で回帰がないことを確認。

## 12. CI における自動再生成検証

パイプラインの決定論的性質と成果物の整合性を担保するため、GitHub Actions ワークフロー（`.github/workflows/ci.yml`）で再生成チェックを自動実行している。
- コミット済みの `fixtures/osm/shutoko-c1.json` を入力とし、外部 Overpass API にはアクセスしない（外部ネットワーク非依存）。
- `scripts/generate-fixtures.sh` を実行後、`git diff --exit-code` および `git status --porcelain` でコミット済みの `fixtures/generated/` との差分が一切生じないことを検証する。
- グラフビルダーのロジックや課金シードの更新時は、再生成された `fixtures/generated/` を同一 PR でコミットする必要があり、意図しない出力の乖離やリグレッションを防ぐ。

