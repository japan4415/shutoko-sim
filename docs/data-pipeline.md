# 実データ道路グラフ・課金ペア生成パイプライン

本ドキュメントでは、OpenStreetMap（OSM）実データから首都高速道路都心環状線（C1）および接続ランプ・周辺一般道を抽出し、決定論的な道路ネットワーク成果物（`graph.json`、`snap-index.json`、`manifest.json`）を生成するオフラインパイプラインの仕様と手順を記録する。

## 1. ライセンスと帰属表示

- **データ提供元**: [OpenStreetMap](https://www.openstreetmap.org/)
- **著作権・帰属表示**: `© OpenStreetMap contributors`
- **ライセンス**: [Open Database License (ODbL) 1.0](https://opendatacommons.org/licenses/odbl/1-0/)
- **権利表記 URL**: [https://www.openstreetmap.org/copyright](https://www.openstreetmap.org/copyright)

本プロジェクトが生成する派生成果物は ODbL に準拠して配布・利用される。

## 2. OSM 実データ取得手順

### 取得仕様
- **取得日**: 2026-09-10（UTC: `2026-09-10T13:25:54Z`）
- **Overpass API エンドポイント**:
  - 主系: `https://overpass-api.de/api/interpreter`
  - 副系: `https://overpass.kumi.systems/api/interpreter`
- **クエリ SHA-256**: `d578e54ce4ba2d960b783ceaa0ef49de664280d2d1be035f5ec8a62b1cd80604`
- **出力先**: `fixtures/osm/shutoko-c1.json`
- **ファイルサイズ**: 2,298,787 bytes（約 2.30 MB）
- **要素数**: 合計 10,837 要素（ノード: 8,953、ウェイ: 1,688、リレーション: 196）

### 対象範囲
- **首都高速都心環状線（C1）**: リレーション ID `4256008`（首都高速都心環状線、`ref=C1`）
- **接続ランプ（motorway_link）**: C1 本線ノードから最大 5 ホップで到達可能な進入・退出ランプウェイ（芝公園・飯倉・霞が関・汐留・宝町等の多ホップランプを包含）
- **周辺主要一般道**: C1 領域のバウンディングボックス（緯度 35.645〜35.700、経度 139.730〜139.785）内の幹線道路（`highway` が `trunk`、`primary`、`secondary`）およびランプ端点に接続する道路・リンク（`tertiary`、`residential`、`unclassified`、`*_link`、`service`）
- **右左折・Uターン禁止制限**: 対象ウェイに関連する `type=restriction` リレーション
  - `no_*`（via=node）: from エッジから to エッジへの禁止遷移ペア（長さ 2）を生成。
  - `only_*`（via=node）: via ノードにおける to 以外の代替流出エッジを自動特定し、禁止遷移ペアとして生成。
  - `via=way`（Uターン制限等）: from エッジ、via エッジ列、to エッジを連結する長さ 3 以上の禁止エッジ列を生成。
  - `restriction:conditional`（時間帯・車種条件付き制限）: 静的道路グラフでは一意に評価できないためスキップし、標準エラー出力およびマニフェストへ記録。
  - `only_*`（via=way）: 静的道路グラフ生成では現時点で未サポートとし、該当関係が存在する場合はスキップしてマニフェストへ記録。

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

### 登録ペア（全 9 件）
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
    - 料金体系・下限料金: `https://www.shutoko.jp/fee/fee-info/about/`
    - 神田橋〜宝町 料金距離 1.7km・300 円: `https://www.shutoko.jp/-/media/pdf/responsive/customer/fee/fee-info/2504_pamphlet_fee_table.pdf`（首都高料金表 2025年4月改訂版 P.3「料金・距離表（ETC 普通車）」）
    - 2026-10-01 改定発表: `https://www.shutoko.co.jp/company/press/2026/data/07/31-toll/`
    - 参照日: `2026-09-10`

#### 新規登録 8 ペア（2026-09-10）

いずれも普通車 ETC で料金距離に応じた下限料金 300 円が適用される。料金は神田橋〜宝町と同一の考え方により、改定前後 2 レコード（`2022-03-31T15:00:00Z`〜`2026-09-30T15:00:00Z` / `2026-09-30T15:00:00Z`〜無期限、いずれも 300 円）を登録している。出典は共通で、料金距離・料金は首都高料金表 2025 年 4 月改訂版 P.3、1 区間先の隣接関係は公式路線図に基づく（参照日 `2026-09-10`）。

| # | 方向 | 入口 → 出口 | 料金距離 | 普通車 ETC | ペア ID | 入口 way | 出口 way | 基準点（anchor）node |
|---|------|------------|---------|-----------|---------|---------|---------|---------------------|
| 1 | 外回り | 霞が関入口 → 代官町出口 | 2.3km | 300 円 | `bp:c1-outer:kasumigaseki-daikancho` | `916571610` | `276920911` | `577255571` |
| 2 | 外回り | 銀座入口 → 芝公園出口 | 3.4km | 300 円 | `bp:c1-outer:ginza-shibakoen` | `4848922` | `944671542` | `31254160` |
| 3 | 外回り | 芝公園入口 → 飯倉出口 | 1.6km | 300 円 | `bp:c1-outer:shibakoen-iikura` | `4853801` | `203832842` | `31296971` |
| 4 | 内回り | 霞が関入口 → 芝公園出口 | 3.7km | 300 円 | `bp:c1-inner:kasumigaseki-shibakoen` | `916571615` | `203873821` | `264877748` |
| 5 | 内回り | 代官町入口 → 霞が関出口 | 2.3km | 300 円 | `bp:c1-inner:daikancho-kasumigaseki` | `1091280541` | `1232166619` | `297945194` |
| 6 | 内回り | 芝公園入口 → 汐留出口 | 2.4km | 300 円 | `bp:c1-inner:shibakoen-shiodome` | `4853797` | `45068171` | `31295430` |
| 7 | 内回り | 銀座入口 → 京橋出口 | 0.6km | 300 円 | `bp:c1-inner:ginza-kyobashi` | `4848936` | `4849052` | `31254341` |
| 8 | 内回り | 宝町入口 → 神田橋出口 | 1.7km | 300 円 | `bp:c1-inner:takaracho-kandabashi` | `378284514` | `390441534` | `1891818143` |

- 出典（共通）:
  - 料金距離・料金: `https://www.shutoko.jp/-/media/pdf/responsive/customer/fee/fee-info/2504_pamphlet_fee_table.pdf`（首都高料金表 2025 年 4 月改訂版 P.3「料金・距離表（ETC 普通車）」）
  - 路線図（1 区間先の隣接関係）: `https://www.shutoko.jp/use/network/map/`
  - 料金体系・下限料金: `https://www.shutoko.jp/fee/fee-info/about/`
  - 2026-10-01 改定発表（下限料金 300 円維持）: `https://www.shutoko.co.jp/company/press/2026/data/07/31-toll/`
  - 参照日: `2026-09-10`

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
  --release-id "c1-real-v1" \
  --built-at "2026-09-10T00:00:00Z" \
  --source-date "2026-09-10" \
  --vehicle-profile "passenger-car-etc" \
  --coverage-area "Tokyo Inner Circular Route (C1) and connecting ramps" \
  --graph-version "1.0.0"
```
※ 検証済み課金ペアが1件以上生成されていることを強制したい場合は `--strict` フラグを付与して実行可能（検証済みペアが0件の場合に非ゼロで終了）。

### 再現性・決定論的検証
同一入力から 2 回実行し、`diff -r` によりバイト完全一致（SHA-256 一致）が確認されている。
- `graph.json`: 禁止遷移（65件、`only_*` および `via=way` を含む）やソート順を決定論的に出力
- `snap-index.json`: 一般道ノードの空間投影インデックス
- `manifest.json`: 全成果物の SHA-256、未検証区間一覧、検証済みペア出典情報（`provenance`）を記録

## 5. 未検証区間（Unverified Sections）

現時点で課金ペアとして検証されていない入出口ランプ区間は、グラフビルダーによって `manifest.json` の `unverifiedSections` 配列に自動列挙される。
- **自動列挙対象**: グラフ内に存在するすべての入口・出口エッジのうち、検証済み課金ペアに採用されていないエッジ。OSM ウェイに `name` タグが存在する場合は「エッジID（ウェイ名）」の形式で可読性を担保。
- **除外路線・通行規制スキップの注記**: C1 外の分岐路線（八重洲線、1号上野線、6号向島線等）や、静的道路グラフで適用外となった通行規制（conditional / no via / outside graph / disconnected / unrecognized 等のスキップカテゴリ）に関する注記も件数付きで同リストに収録。
- **現状**: 今回のリリース `c1-real-v1` では 3 節の表に記載した 9 ペア（外回り 3・内回り 5・既存の神田橋〜宝町 1）すべてが人手検証済み（`verified`）で、`unverifiedSections` に `rejected:` は存在しない。将来追加予定のランプ区間については、公式料金区間表または本線隣接導出の根拠とともに順次シードへ追加する。

## 6. CI における自動再生成検証

パイプラインの決定論的性質と成果物の整合性を担保するため、GitHub Actions ワークフロー（`.github/workflows/ci.yml`）で再生成チェックを自動実行している。
- コミット済みの `fixtures/osm/shutoko-c1.json` を入力とし、外部 Overpass API にはアクセスしない（外部ネットワーク非依存）。
- `scripts/generate-fixtures.sh` を実行後、`git diff --exit-code` および `git status --porcelain` でコミット済みの `fixtures/generated/` との差分が一切生じないことを検証する。
- グラフビルダーのロジックや課金シードの更新時は、再生成された `fixtures/generated/` を同一 PR でコミットする必要があり、意図しない出力の乖離やリグレッションを防ぐ。

