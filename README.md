# shutoko-sim

「今からこの時間で、首都高をどれくらい楽しめるだろう？」を考えるためのドライブルート提案サービスです。
首都高を一周したうえで入口の1区間先で降りると、1区間分の料金になるという原案の前提を活かし、時間予算内で長く楽しめるルートを探します。出発地点と最小・最大時間から候補を比較し、Google マップへ引き継ぐ体験を目指します。

現在は Rust/WASM の探索コア（`crates/routing-core`, `crates/routing-wasm`）に加え、実 OSM データから探索用道路グラフを決定論的に生成するオフラインビルダー（`crates/graph-builder`）を実装しています。座標入力による空間スナップ、GeoJSON LineString 幾何データ合成、日本語道路名・ランプ名、Google マップ引き継ぎ URL 生成、および首都高速都心環状線（C1）の実データ fixture を用いた探索の成立性検証が完了しています。Web アプリ、公開サービスは今後の段階で開発予定です。

## 想定する使い方

1. 住所または現在地から出発地点を指定します。
2. ドライブに使える時間を分単位で指定します。
3. 首都高を周回して入口の1区間先で降りる候補を、実走行時間と課金対象区間を見ながら比較します。
4. 候補を選び、「出発」から Google マップを開いて経路を確認し、ナビを設定します。

「実際に走る周回経路」と「料金の対象となる1区間」を分けることが設計の中心です。一般道で出発地点に戻ることは今回の企画案です。所要時間は予測で、Google マップでの周回再現は先行検証が必要です。神田橋入口〜宝町出口の普通車 ETC 料金（300 円、2026-10-01 改定対応）が登録済みで、コスパ順（time_per_yen）ソートが利用可能です。

## 実データ生成パイプラインと graph-builder

`crates/graph-builder`（`shutoko-graph-builder`）は、OpenStreetMap（OSM）実データと宣言的課金シードから、方向付きの探索用道路グラフ（`graph.json`）、一般道スナップインデックス（`snap-index.json`）、および各成果物の SHA-256 チェックサムを含むマニフェスト（`manifest.json`）を決定論的に生成するオフライン CLI ツールです。通行規制（`no_*`、`only_*`、`via=way`）の抽出、本線からの最初の出口（First Exit）検証、および出典情報（`provenance`）の記録をビルド時に自動検証します。

### 再現手順

1. **OSM 実データの取得**（Overpass API 経由）:
   ```bash
   ./scripts/fetch-osm.sh
   ```
   ※ 通常の開発や CI ではリポジトリにコミット済みの `fixtures/osm/shutoko-c1.json` を使用するため、外部ネットワーク呼び出しは不要です。
2. **決定論的成果物の再生成**:
   ```bash
   ./scripts/generate-fixtures.sh
   ```
   コミット済みの OSM データと課金シードから `fixtures/generated/` 以下の成果物をバイト完全一致（SHA-256 一致）で再生成します。CI でもこの再生成チェックを実行し、差分がないことを自動検証しています。

## Cloudflare Workers（成果物配信・住所検索プロキシ）

`workers/` は、R2 バケットからのバージョン付き静的成果物配信（`GET /releases/{releaseId}/...`）および国土地理院 住所検索 API を用いた住所検索プロキシ（`POST /api/geocode`）を提供する Cloudflare Workers プロジェクトです。

### 起動・テスト手順

1. **依存関係のインストール**:
   ```bash
   cd workers
   npm ci
   ```
2. **型検査と自動テスト**:
   ```bash
   npm run typecheck   # tsc --noEmit
   npm test            # vitest run（@cloudflare/vitest-pool-workers）
   ```
3. **ローカル R2 エミュレータへの成果物シードと開発サーバー起動**:
   ```bash
   npm run seed:local  # fixtures/generated と dist/wasm のハッシュ照合・R2 投入
   npx wrangler dev    # ローカル開発サーバー起動（http://localhost:8787）
   ```

### 本番デプロイ手順

1. **R2 バケットの作成**（初回のみ。既存なら再利用）:
   ```bash
   npx wrangler r2 bucket create shutoko-artifacts
   npx wrangler r2 bucket create shutoko-artifacts-preview
   ```
2. **成果物のビルドと本番 R2 への投入**（`sha256`・`byteLength` を `manifest.json` と照合してからアップロード）:
   ```bash
   bash scripts/build-wasm.sh                # dist/wasm/ を生成
   cd workers && node scripts/seed-local-r2.mjs --remote
   ```
3. **デプロイ**:
   ```bash
   cd workers && npx wrangler deploy
   ```
   デプロイ完了時に表示される `https://<worker>.<subdomain>.workers.dev` が配信 URL です。
4. **疎通確認**: `GET /releases/{releaseId}/manifest.json` が `200`・`application/json`・`Cache-Control: max-age=300` で返ること、`GET /releases/{releaseId}/graph.json` が `immutable` キャッシュと `ETag` 付きで返り本文の `sha256` が `manifest.json` と一致すること、存在しない release / 二重スラッシュが `404` になること、`POST /api/geocode` が正常クエリで `200`、空クエリで `400 INVALID_QUERY` を返すことを確認する。

現在の配信 URL: `https://shutoko-sim-workers.raiden000discord.workers.dev`（2026-09-11 デプロイ, wrangler 4.131.0）。

> **レート制限の本番挙動に関する注記**: `wrangler.toml` の `[[ratelimits]]`（IP: 10 req/60s、Global: 600 req/60s）は Cloudflare Workers Rate Limiting binding のベストエフォート仕様であり、`wrangler dev --local` の決定論的シミュレーションと異なり本番環境では正確な即時遮断を保証しない（同一 IP から短時間に 15 リクエストを送っても `429` が発生しない場合がある）。アプリケーション側の防御としては機能するが、厳密なレート保証が必要な用途には追加の対策を検討すること。

### ライセンスとデータ帰属（ODbL）

- 本プロジェクトで利用している実道路データは OpenStreetMap から提供されています。
- **著作権・帰属表示**: `© OpenStreetMap contributors`
- **ライセンス**: [Open Database License (ODbL) 1.0](https://opendatacommons.org/licenses/odbl/1-0/)
- **権利表記 URL**: [https://www.openstreetmap.org/copyright](https://www.openstreetmap.org/copyright)
- 本プロジェクトが生成する派生成果物（`fixtures/generated/` 配下等）は ODbL に準拠して取り扱われます。

### 対象範囲と未検証事項

- **対象範囲**: 首都高速道路 都心環状線（C1）および接続ランプ（進入・退出）、ならびに周辺主要一般道（神田橋〜宝町周辺）。
- **対応車両**: 普通乗用車・ETC（`passenger-car-etc`）。
- **検証済み課金区間**: C1 外回り 4 区間・内回り 4 区間の合計 8 ペア（`bp:c1-outer:kandabashi-takaracho` ほか。公式路線図および公式料金表・改定発表に基づき、いずれも普通車 ETC 300 円の実料金・有効期間を登録済み。出典情報は `manifest.json` に記録、一覧は `docs/data-pipeline.md` 参照）。
- **未検証事項**:
  - C1 の他ランプ区間（新富町、京橋、北の丸等）や他の首都高速路線（湾岸線・羽田線等）は未検証であり、`manifest.json` の `unverifiedSections` に未検証エッジおよび除外路線注記として自動列挙されます。
  - Google マップへの経由地引き継ぎ（経由地3点による周回再現）の実機検証は未完了です。
  - リアルタイム渋滞情報、交通規制、天候による所要時間変動、中型・大型車等の料金区分は対象外です。

## ドキュメント

| 読みたいこと | ドキュメント |
| --- | --- |
| 発案者の意図と技術上の前提 | [原案](docs/original.md) — 人間のみが編集する原文 |
| 誰の何を解決するか、初期版で作るもの | [プロダクト企画](docs/product.md) |
| 入力・比較・出発の流れと例外時の体験 | [機能要件とユーザー体験](docs/requirements.md) |
| Cloudflare・Rust/WASM・地図の役割 | [システム設計](docs/architecture.md) |
| 実データ道路グラフ・課金ペア生成パイプライン | [実データ生成パイプライン](docs/data-pipeline.md) |
| 時間予算に合わせた探索と候補の評価方法 | [ルート探索設計](docs/routing.md) |
| データ形式と外部サービスへの引き継ぎ | [データ・インターフェース設計](docs/interfaces.md) |
| Rust コアのテスト、WASM ビルド、実装済みの範囲 | [Rust / WASM 開発](docs/wasm-development.md) |
| 実装順序、検証条件、未決事項 | [実装・検証計画](docs/delivery.md) |

最初に企画と要件を読み、実装時にはシステム設計、探索、インターフェースの順に参照してください。設計は原案を具体化した提案であり、現在の実装範囲と動かし方は Rust / WASM 開発に記載しています。外部仕様の参照確認日は 2026-09-10 です。
