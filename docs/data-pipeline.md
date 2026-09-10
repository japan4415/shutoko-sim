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
- **取得日**: 2026-09-10（UTC: `2026-09-10T05:08:05Z`）
- **Overpass API エンドポイント**:
  - 主系: `https://overpass-api.de/api/interpreter`
  - 副系: `https://overpass.kumi.systems/api/interpreter`
- **クエリ SHA-256**: `ae297db9599a41f5a56f3a270ab3d01032cca6c1164e0ced06c5c6a2399f4b3f`
- **出力先**: `fixtures/osm/shutoko-c1.json`
- **ファイルサイズ**: 1,780,598 bytes（約 1.78 MB）
- **要素数**: 合計 8,511 要素（ノード: 7,086、ウェイ: 1,273、リレーション: 152）

### 対象範囲
- **首都高速都心環状線（C1）**: リレーション ID `4256008`（首都高速都心環状線、`ref=C1`）
- **接続ランプ（motorway_link）**: C1 本線ノードから 2 ホップ以内で到達可能な進入・退出ランプウェイ
- **周辺主要一般道**: C1 領域のバウンディングボックス（緯度 35.65〜35.695、経度 139.735〜139.78）内の幹線道路（`highway` が `trunk`、`primary`、`secondary`）およびランプ端点に接続する道路（`tertiary`、`residential`、`unclassified`）
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
- **最初の出口（First Exit）検証**: 本線基準点（`anchorOsmNodeId`）から首都高速本線エッジを経由して最初に到達可能な出口エッジのみが検証済みペアとして採用可能。後続の出口（例: 神田橋入口・宝町出口に対してさらに下流の新富町出口）は `FIRST_EXIT_MISMATCH` として拒否される。探索アルゴリズムは経路依存の最良コスト状態（ノードと直前エッジ履歴）によって健全に枝刈りを行い、より近傍の合法な出口の見落としを防ぐ。
- **出典・日付の構文検証**: 出典 URL は `http://` または `https://` で始まる有効な URL（空ラベルや先頭・末尾ドットのない有効なホスト名）であること、出典日はカレンダー上に実在する有効な ISO 8601 日付（`YYYY-MM-DD`、存在しない 2月31日等は拒否）であることを検証する。
- **マニフェストへの伝播**: 検証済みペアの出典情報（`source`, `sourceDate`, `notes`）は `manifest.json` の `provenance` 配列に記録され、成果物配布時にも追跡可能となる。

### 登録ペア: 神田橋入口 〜 宝町出口
- **ペア ID**: `bp:c1-outer:kandabashi-takaracho`
- **入口ランプ OSM ウェイ ID**: `92243921`（神田橋入口、一般道側始点: `n:1070862943`）
- **出口ランプ OSM ウェイ ID**: `297864314`（宝町出口、一般道側終点: `n:1130812252`）
- **本線周回基準点 OSM ノード ID**: `499831338`（C1 外回り神田橋合流点）
- **車種プロファイル**: `passenger-car-etc`
- **検証状態**: `verified`（`oneSectionAheadVerified: true`）
- **根拠と出典**:
  - 出典 URL: `https://www.shutoko.jp/use/network/map/`
  - 参照日: `2026-09-10`
  - 根拠詳細: 首都高速都心環状線（外回り・時計回り）において、2021年5月10日に呉服橋出入口および江戸橋出入口が廃止されたため、神田橋入口から進行方向（外回り）で最初に出会う隣接出口は宝町出口である。
- **通行料金**: 出典 URL と参照日を伴って確認できる固定金額がないため、`prices: []`（空配列）とする（`routing-core` では `amountYen: null` として扱われ、時間順ランキングで候補提示可能）。

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
- **除外路線・通行規制スキップの注記**: C1 外の分岐路線（八重洲線、1号上野線、6号向島線等）や、静的道路グラフで適用外となった通行規制（conditional / no via / outside graph / disconnected 等のスキップカテゴリ）に関する注記も件数付きで同リストに収録。
- **現状**: 今回のリリース `c1-real-v1` では `bp:c1-outer:kandabashi-takaracho` のみを人手検証済み（`verified`）。将来追加予定のランプ区間（芝公園、霞が関、銀座、飯倉等）については、公式料金区間表または本線隣接導出の根拠とともに順次シードへ追加する。

## 6. CI における自動再生成検証

パイプラインの決定論的性質と成果物の整合性を担保するため、GitHub Actions ワークフロー（`.github/workflows/ci.yml`）で再生成チェックを自動実行している。
- コミット済みの `fixtures/osm/shutoko-c1.json` を入力とし、外部 Overpass API にはアクセスしない（外部ネットワーク非依存）。
- `scripts/generate-fixtures.sh` を実行後、`git diff --exit-code` および `git status --porcelain` でコミット済みの `fixtures/generated/` との差分が一切生じないことを検証する。
- グラフビルダーのロジックや課金シードの更新時は、再生成された `fixtures/generated/` を同一 PR でコミットする必要があり、意図しない出力の乖離やリグレッションを防ぐ。

