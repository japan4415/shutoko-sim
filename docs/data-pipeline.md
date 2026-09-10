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

### 再現実行手順
```bash
./scripts/fetch-osm.sh [OUTPUT_PATH] [ENDPOINT]
```
引数を省略した場合は既定値（`fixtures/osm/shutoko-c1.json`、`https://overpass-api.de/api/interpreter`）で実行される。

## 3. 宣言的課金シード（`data/billing-pairs-seed.json`）

料金適格性および「1区間先」の出口関係は OSM 幾何のみから自動判別できないため、人手検証と公式資料に基づく宣言的シード定義を行う。

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

### 再現性・決定論的検証
同一入力から 2 回実行し、`diff -r` によりバイト完全一致（SHA-256 一致）が確認されている。
- `graph.json`: SHA-256 `e630a52fd9149a86bcfc15d0eb85f46cef9b4bc21dfd12f7aae3843a7925b1e5`（1,618,062 bytes）
- `snap-index.json`: SHA-256 `a3aea1c5799026926d5bf536c887f012e131f971e4999165f6e74b4f2dcbce8c`（495,332 bytes）
- `manifest.json`: 全成果物の SHA-256 およびサイズを記録

## 5. 未検証区間（Unverified Sections）

現時点で登録・検証されていない入出口ランプ区間（未検証区間）はマニフェストおよびシードで追跡される。
- 今回のリリース `c1-real-v1` では `bp:c1-outer:kandabashi-takaracho` を公式路線図に基づき人手検証済み（`verified`）。
- 将来追加予定のランプ区間（芝公園、霞が関、銀座、飯倉等）については、公式料金区間表または本線隣接導出の根拠とともに順次シードへ追加する。
