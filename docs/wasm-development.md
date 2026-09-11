# Rust / WASM 開発

## 今回の実装範囲

初回の探索コアは、人工グラフを使って「首都高を一周し、入口の1区間先で退出する」経路を生成するためのもの。実在の入出口・料金の検証や Google マップへの引き継ぎを済ませた公開ルート検索ではない。

`crates/routing-core` は Rust の純粋な探索処理、`crates/routing-wasm` は JSON 文字列を受け渡す JavaScript 向け境界。ネットワーク、DOM、住所検索には依存しない。`fixtures` は架空の道路・料金データで、実走行案内には使わない。

出発地点は `origin: { lat, lon }` または `originNodeId` のどちらかで指定する（排他）。座標指定時は WASM 内部で 200m 以内の一般道ノードへ空間スナップされる。OSM 実データからのグラフ生成、幾何データ合成、Google マップ引き継ぎ URL 生成に対応している。ブラウザ Web Worker の制御や実機検証（#8）は後続とする。[インターフェース設計](interfaces.md)に詳細な契約表を記載している。

## 準備

Rust の stable toolchain、rustfmt、clippy、Node.js 22 を使用する。WASM の JS glue を生成する CLI は crate と同じバージョンに固定する。

```sh
rustup target add wasm32-unknown-unknown
rustup component add rustfmt clippy
cargo install wasm-bindgen-cli --version 0.2.128 --locked
```

## 検証とビルド

リポジトリのルートで実行する。

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
bash scripts/build-wasm.sh
node scripts/test-wasm.mjs
```

`dist/wasm/` に `.wasm`、ES module の JS glue、および TypeScript 型定義を生成する。
- TypeScript 正典型定義: `crates/routing-wasm/types/index.d.ts`
- ビルドスクリプト（`scripts/build-wasm.sh`）がビルド完了時に `dist/wasm/index.d.ts` へコピーし、npm パッケージ / Web Worker から直接型参照可能にする。
- 定義される主要型: `SearchRequest`, `SearchLimits`, `SearchResult`, `Candidate`, `Handoff`, `Toll`, `Loop`, `Duration`, `GeoJsonLineString`, `RoutingErrorPayload`

`test-wasm.mjs` は配信用と同じ `--target web` の glue と WASM を Node.js でロードし、以下を自動検証する:
1. 合成グラフに対する `originNodeId` 探索および期待されるエッジ列・時間・料金の算出
2. 決定論性（同一入力による連続実行でバイト完全一致）
3. 候補の新フィールド構造（GeoJSON `LineString` 幾何、`mapsUrl` 形式および長さ ≤ 2,048、`snappedOrigin`、`warnings` への `HANDOFF_WAYPOINTS_UNVERIFIED` の包含）
4. 座標入力（`origin: { lat, lon }`）による空間スナップ探索
5. 200m 超過座標における接続不可（`status: "no_candidates"`, `reason: "NO_CONNECTION"`）
6. 異常入力の拒否と JavaScript Error（Error の `message` に `RoutingErrorPayload { code: "INVALID_INPUT", message }` の JSON 文字列）のスロー検証

## 呼び出し

生成された ES module を使う例:

```javascript
import init, { search } from './shutoko_routing.js';
await init();
const result = JSON.parse(search(graphJson, requestJson, '{}'));
```

引数は順にグラフ JSON、検索条件 JSON、探索上限 JSON の文字列。`{}` は既定の探索上限を選ぶ。入力不備は JavaScript `Error`（`message` に `RoutingErrorPayload` JSON 文字列）としてスローされるため、呼び出し元で捕捉する。戻り値も JSON 文字列で、候補なしや探索打ち切りは正常な探索結果として扱う。

Rust からは `shutoko_routing_core::search`、JSON 境界の確認には `search_json` を利用できる。動作する入力例は [人工グラフ](../fixtures/synthetic-graph.json) と [検索条件](../fixtures/synthetic-request.json) を参照する。

## 初期データ契約

グラフには `schemaVersion: 2`、`releaseId`、`vehicleProfile`、ノード、エッジ、課金ペア、禁止エッジ列を格納する。エッジ種別は `local` / `entry` / `shutoko` / `exit`。上下線は異なるノード・エッジで表す。料金ペアは入口から基準点への経路、基準点から出口への経路を明示し、間に非空の一周を挿入する。`entryId` / `exitId` はこの初期契約では実際の入退出エッジ ID と一致させる。二つの接続路を結んだ直接経路は単純路とし、そこに追加の周回を埋め込めない。一方、挿入する一周と接続路のエッジ共有は許す。

時間と距離は正の整数で秒・mを用いる。料金は実走行距離から計算せず、ペアに登録された有効期間の金額を使う。期間は開始を含み終了を含まない。検索入力の `pricingAt` に固定して判定する。入力検証はデータの構造を検証するもので、実際の道路や課金関係を認定しない。

出力の周回候補は、外部ナビ引き継ぎ未検証の探索コア結果である。完成版で予定する GeoJSON、住所、経由地点、料金根拠の表示データを補った後に公開候補へ変換する。

## 探索の上限と完成版との差分

| 設定 | 既定値 | 許容範囲 |
| --- | --- | --- |
| `maxExpandedStates` | 100,000 | 1〜1,000,000 |
| `beamWidth` | 200 | 1〜200 |
| `maxLoopEdges` | 2,000 | 1〜2,000 |
| `maxLocalEdges` | 200 | 1〜2,000 |
| `maxPairs` | 10 | 1〜100 |
| `maxCandidates` | 3 | 1〜3 |

各経路列挙で保存する成功パスは `beamWidth` 件かつ合計20,000エッジまで。候補集合も `beamWidth` 件かつ合計20,000エッジまでとし、上限で候補を省略すると `status: "truncated"` / `reason: "SEARCH_LIMIT"` を返す。展開数は一般道・閉路・経路結合で共有する。ノード/エッジ等の ID は256バイト以内、固定接続路は各2,000エッジ以内。大きな入力を無制限に探索する API ではない。

現段階の列挙順は安定したエッジ ID 順、課金ペアの絞り込みはペア ID 順。完成版で予定する一般道往復時間順のペア選択と、帰還時間の下界による枝刈りは未実装。前後の一般道も幅制限付きの単純パス列挙を用いるため、大規模道路網では候補欠落・打ち切りが起きやすい。上限内に保存できた集合を評価する初期実装で、全経路の最良を保証しない。

全候補で料金が分かる場合の `rankingMode` は `time_per_yen`、未確認額を含む場合は `shutoko_time`。`loop.validated` は入力グラフ内で非空の一周が成立した意味で、実際の道路やナビの検証済みを意味しない。候補に `HANDOFF_WAYPOINTS_UNVERIFIED` と `STATIC_TRAVEL_TIME` の警告コードを付ける。

実道路網に対するブラウザの処理時間・ピークメモリ目標は未検証で、人工グラフのテスト成功から実道路網での性能を推定しない。同期呼び出しの10秒キャンセルはこのコアではなく、今後の Web Worker 呼び出し元で実装する。
