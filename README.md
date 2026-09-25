# shutoko-sim

「今からこの時間で、首都高をどれくらい楽しめるだろう？」を考えるためのドライブルート提案サービスです。
首都高を一周したうえで入口の1区間先で降りると、1区間分の料金になるという原案の前提を活かし、時間予算内で長く楽しめるルートを探します。出発地点と最小・最大時間から候補を比較し、Google マップへ引き継ぐ体験を目指します。

現在は Rust/WASM の探索コア（`crates/routing-core`, `crates/routing-wasm`）に加え、実 OSM データから探索用道路グラフを決定論的に生成するオフラインビルダー（`crates/graph-builder`）、Cloudflare Workers による成果物配信・住所検索プロキシ、および製品 UI（`web/`）を実装しています。製品 UI は住所検索（国土地理院ジオコーディング）または現在地取得による出発地点指定、分単位の時間範囲、Leaflet + 国土地理院タイルによる地図描画、最大3件の候補比較、Google マップへの引き継ぎ、例外系からの復帰を備えます。

## 想定する使い方

1. 住所または現在地から出発地点を指定します。
2. ドライブに使える時間を分単位で指定します。
3. 首都高を周回して入口の1区間先で降りる候補を、実走行時間と課金対象区間を見ながら比較します。
4. 候補を選び、「出発」から Google マップを開いて経路を確認し、ナビを設定します。

「実際に走る周回経路」と「料金の対象となる1区間」を分けることが設計の中心です。一般道で出発地点に戻ることは今回の企画案です。所要時間は予測で、Google マップでの周回再現は先行検証が必要です。

表示する料金は **普通車 ETC 基本料金（割引適用前）** に固定しています（車種 `ordinary` / 支払方法 `etc` / 料金種別 `base_toll_excluding_discounts`、適用する割引は列挙して除外）。深夜割引・都心流入割引・環境道路料金割引・ETC2.0 割引・ETC フリート割引は含みません。検証済みの商品課金ペアは 9 件（legacy 7 + radial 2）で、2026-10-01 の改定をまたぐ期間を保持し、1 件が未検証（`bp:c1-outer:shibakoen-iikura`）として商品推薦から外れています。金額（`amountYen`）が確認済みの候補集合では時間効率（`time_per_yen`）順に並びます。

## 実データ生成パイプラインと graph-builder

`crates/graph-builder`（`shutoko-graph-builder`）は、OpenStreetMap（OSM）実データと宣言的課金シード・料金カタログ・隣接関係証跡から、方向付きの探索用道路グラフ（`graph.json`）、一般道スナップインデックス（`snap-index.json`）、料金カタログ（`od-tariffs.json`）、課金ペアの導出レポート（`pair-candidates.json`）、および各成果物の SHA-256 チェックサムを含むマニフェスト（`manifest.json`）を決定論的に生成するオフライン CLI ツールです。ビルド時には次を自動検証します。

- 通行規制（`no_*`、`only_*`、`via=way`）の抽出と、`*:conditional` / `oneway:conditional` / `reversible` / `alternating` の fail-closed 判定
- 路線 relation の所属（有向接続の一意性）と exact directed binding によるランプ端点の確定（`firstPublicRoadConnection/v1`）
- relation 制約つきの First Exit 検証（return corridor 上で最初に到達する一般出口。全体グラフでの最短出口は使わない）
- route membership の hash、料金表 v3 の期間別 evidence と規則検算の整合
- 出典情報（`provenance`）と、5 入力（OSM snapshot / ランプ台帳 / route membership index / 隣接関係 / 料金カタログ）の SHA-256

公開既定は graph schema 4、release ID は `all-real-v4`（`billingPairsVersion=v3`、`tariffModelVersion=1`）です。`all-real-v3` は rollback 先として allowlist に残します。

### 再現手順

1. **OSM 実データの取得**（Overpass API 経由）:
   ```bash
   ./scripts/fetch-osm.sh
   ```
   ※ 通常の開発や CI ではリポジトリにコミット済みの `fixtures/osm/shutoko-all.json`（全 24 路線）を使用するため、外部ネットワーク呼び出しは不要です。C1 限定の診断スナップショットが必要なときだけ `ROUTES_FILTER=c1 ./scripts/fetch-osm.sh` を明示します。
2. **決定論的成果物の再生成**:
   ```bash
   ./scripts/generate-fixtures.sh
   ```
   コミット済みの OSM スナップショットと `data/*.json` の入力から `fixtures/generated/` 以下の 5 成果物（`graph.json` / `od-tariffs.json` / `pair-candidates.json` / `ramps.json` / `snap-index.json`）と `manifest.json` をバイト完全一致（SHA-256 一致）で再生成します。CI でもこの再生成チェックを実行し、差分がないことを自動検証しています。release ID を変える場合は `SHUTOKO_RELEASE_ID=...` を渡します（既定は `all-real-v4`）。

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
   npm run seed:local  # fixtures/generated と dist/wasm のハッシュ照合・R2 投入（engine.json も生成して投入）
   npx wrangler dev    # ローカル開発サーバー起動（http://localhost:8787）
   ```

### 本番デプロイ手順

Cloudflare Workers Builds では、Git リポジトリを接続して次の値を設定する。ビルドが失敗した場合、deploy command は実行されない。

| 設定 | 値 |
| --- | --- |
| Root directory | `/` |
| Build command | `bash scripts/cloudflare-build.sh` |
| Deploy command | `cd workers && npx wrangler deploy` |
| Preview deploy command | `cd workers && npx wrangler versions upload` |
| Node version | `NODE_VERSION=22` |
| Automatic dependency install | `SKIP_DEPENDENCY_INSTALL=1` |
| Non-production branch builds | OFF |

`SKIP_DEPENDENCY_INSTALL=1` でプラットフォーム側の自動インストールを止め、`scripts/cloudflare-build.sh` が `workers` と `web` の lockfile 固定依存を `npm ci` で導入して `web/dist` を生成する。これにより deploy command が使う pinned Wrangler も `workers/node_modules` に用意される。

Workers Builds は Rust/WASM の生成や R2 への投入を行わず、既存 release を上書きしない。R2 成果物を更新する場合は、別の versioned release ID で WASM・graph・manifest・engine を先にすべて投入して検証し、その後に Worker と Web の参照先を新しい release へ切り替える。公開済み release ID へ再投入すると、複数オブジェクトの非原子的な上書きによって新旧ファイルが一時的に混在するため、自動 CI/CD では実行しない。

1. **R2 バケットの作成**（初回のみ。既存なら再利用）:
   ```bash
   npx wrangler r2 bucket create shutoko-artifacts
   npx wrangler r2 bucket create shutoko-artifacts-preview
   ```
2. **新しい versioned release のビルドと本番 R2 への投入**（`sha256`・`byteLength` を `manifest.json` と照合してからアップロード）:
   ```bash
   bash scripts/generate-fixtures.sh
   bash scripts/build-wasm.sh
   SHUTOKO_REQUIRE_PINNED_WRANGLER=1 npm --prefix workers run seed:local -- --remote
   ```
   詳細手順と manifest を最後に置く atomic release の runbook は [`docs/delivery.md`](docs/delivery.md#all-real-v4-の-atomic-release-runbook) を参照する。先に未使用の `all-real-v4` へ client allowlist を更新し、payload と `engine.json` を投入して全件を read-back 検証した後、公開条件となる `manifest.json` を最後に投入・再検証する。既存の本番 release ID があれば上書きを拒否する。`seed:local -- --remote` は `npm --prefix workers run` が `workers/node_modules/.bin` を PATH へ足すため、`workers/package-lock.json` の固定版 wrangler（4.131.0）が選ばれる。`SHUTOKO_REQUIRE_PINNED_WRANGLER=1` を付けると `wrangler --version` が固定版と完全一致することを要求し、`npx` フォールバックは無い。Web Worker は `engine.json` で wasm / glue を照合するため、**engine.json が無い版はブラウザ側で `ARTIFACT_MISMATCH` になる**。このリポジトリの変更作業では R2 upload / Wrangler deploy / Cloudflare 書き込みを実行しない。
3. **デプロイ**:
   ```bash
   cd workers && npx wrangler deploy
   ```
   デプロイ完了時に表示される `https://<worker>.<subdomain>.workers.dev` が配信 URL です。
4. **疎通確認**: `GET /releases/{releaseId}/manifest.json` と `GET /releases/{releaseId}/engine.json` が `200`・`application/json`・`Cache-Control: max-age=300` で返ること、`GET /releases/{releaseId}/graph.json` が `immutable` キャッシュと `ETag` 付きで返り本文の `sha256` が `manifest.json` と一致すること、存在しない release / 二重スラッシュが `404` になること、`POST /api/geocode` が正常クエリで `200`、空クエリで `400 INVALID_QUERY` を返すことを確認する。

現在の配信 URL: `https://shutoko-sim-workers.raiden000discord.workers.dev`（2026-09-17 `all-real-v2` デプロイ, wrangler 4.131.0）。`all-real-v3` / `all-real-v4` の R2 投入と production deploy はこのリポジトリの作業範囲外であり、`docs/delivery.md` の runbook の read-back 確認と本番デプロイが完了するまで本番参照は切り替えない。Web（`DEFAULT_RELEASE_ID`）と Worker（`ALLOWED_RELEASES`）の許可リストはすでに `all-real-v4` を含んでおり、異常時は Worker の許可リストを狭めずに Web の既定 1 行を `all-real-v3` へ戻すだけで前の版へ切り戻せる。

> **レート制限の本番挙動に関する注記**: `wrangler.toml` の `[[ratelimits]]`（IP: 10 req/60s、Global: 600 req/60s）は Cloudflare Workers Rate Limiting binding のベストエフォート仕様であり、`wrangler dev --local` の決定論的シミュレーションと異なり本番環境では正確な即時遮断を保証しない（同一 IP から短時間に 15 リクエストを送っても `429` が発生しない場合がある）。アプリケーション側の防御としては機能するが、厳密なレート保証が必要な用途には追加の対策を検討すること。

### ライセンスとデータ帰属（ODbL）

- 本プロジェクトで利用している実道路データは OpenStreetMap から提供されています。
- **著作権・帰属表示**: `© OpenStreetMap contributors`
- **ライセンス**: [Open Database License (ODbL) 1.0](https://opendatacommons.org/licenses/odbl/1-0/)
- **権利表記 URL**: [https://www.openstreetmap.org/copyright](https://www.openstreetmap.org/copyright)
- 本プロジェクトが生成する派生成果物（`fixtures/generated/` 配下等）は ODbL に準拠して取り扱われます。

### 対象範囲と未検証事項

- **対象範囲**: 首都高速道路 都心環状線（C1）8 区間と 2 号目黒線（目黒入口 → 天現寺出口）の 2 区間、およびそれぞれの接続ランプ（進入・退出）。路線グラフ自体は全 24 路線を収録していますが、公開する商品課金ペアは上記 10 件だけです。
- **対応車両**: 普通乗用車・ETC（`passenger-car-etc`）。券種は普通車 ETC 基本料金（割引適用前）です。
- **検証済み課金区間**: 9 ペア。legacy 7 ペア（`bp:c1-outer:kandabashi-takaracho`、`bp:c1-outer:kasumigaseki-daikancho`、`bp:c1-outer:ginza-shibakoen`、`bp:c1-inner:kasumigaseki-shibakoen`、`bp:c1-inner:daikancho-kasumigaseki`、`bp:c1-inner:shibakoen-shiodome`、`bp:c1-inner:takaracho-kandabashi`）と radial 2 ペア（`bp:2-inbound:meguro:c1-inner:tengenji`、`bp:2-inbound:meguro:c1-outer:tengenji`）。いずれも公式路線図と公式料金表のセル証跡（版ごとに別の `evidenceId`）を保持し、2026-10-01 改定の期間別金額を区別します。出典はペアごとに異なり、legacy 7 ペアは `fixtures/generated/manifest.json` の `provenance`、radial 2 ペアは `data/billing-pairs-seed.json` の `billingPairs[].provenance` と `data/od-tariffs.json` の `assignmentId: assignment:2:meguro-tengenji`（`prices[].evidenceId` / `prices[].distanceEvidenceId`）と `documents[]` にあります。radial 2 ペアの出典を `manifest.json` の `provenance` へ伝播させる実装はまだないため、manifest だけを読む検証者には該当 2 件の出典が伝わりません。一覧と値は [`docs/data-pipeline.md`](docs/data-pipeline.md) を参照してください。
- **未検証のまま残す項目**:
  - `bp:c1-outer:shibakoen-iikura`（芝公園入口 → 飯倉出口）は、接続する一般道 way `40969792` に `access:conditional` があるため端点が unresolved で、**商品推薦から外したうえで未検証を保持**します。時間帯モデルが導入されるまで昇格は凍結です。
  - `bp:c1-inner:ginza-shintomicho`（内回り銀座入口 → 新富町出口）も、OSM 上の分流点が銀座入口の合流点より上流にあり relation 制約つきの First Exit 検証が通らないため**未検証のまま**です。導出レポートでも `hold` として出力されます。
  - 上記以外の C1 ランプ区間（新富町、京橋、北の丸等）と他の首都高速路線（湾岸線・羽田線等）は未検証であり、`manifest.json` の `unverifiedSections` に未検証エッジとして自動列挙されます。ルート relation は 26 件のうち 11 件のみ展開済みで、残り 15 件は理由付きで `fail` として記録され、無言でスキップされません。
  - 検証済みペアに到達できない動的 OD は `topology_only` として道路形状だけを返し、金額も商品推薦も付けません。
  - 放射線（radial）候補の Google マップ引き継ぎ（3 leg split URL）は**実機検証が完了していないため `enabled=false`** です。Web は出発ボタンを出さず「実機検証待ち」の理由を表示します。C1 legacy の単一 URL は従来どおり動作します。
  - リアルタイム渋滞情報、交通規制、天候による所要時間変動、中型・大型車等の料金区分は対象外です。

## Web アプリ（`web/`）

Vite + Vanilla TypeScript の製品 UI。探索はブラウザの専用 Web Worker 内で WASM を実行する。地図は Leaflet、タイルは国土地理院（GSI）標準地図を既定とし（提供元は差し替え可能）、経路データの帰属として `© OpenStreetMap contributors` を地図上に常時表示する。住所検索は同一オリジンの `POST /api/geocode`（国土地理院ジオコーディングのプロキシ）を確定時のみ呼ぶ。`workers/wrangler.toml` の `[assets]` により、`wrangler dev` 1 台（ポート 8787）で静的ファイルと `/releases`・`/api` を同一オリジン配信する。

### 起動手順（ローカル）

1. **WASM 成果物と依存の準備**（リポジトリルートで実行）:
   ```bash
   bash scripts/build-wasm.sh          # dist/wasm/ を生成（wasm-bindgen 0.2.128 が必要）
   npm --prefix workers ci
   npm --prefix workers run seed:local # Miniflare のローカル R2 へ成果物を投入
   ```
2. **配信サーバー起動**（静的 + API、ポート 8787）:
   ```bash
   cd workers && npx wrangler dev --port 8787
   ```
   `http://localhost:8787/` を直接開くとビルド済み UI が表示される。
3. **開発用 Vite サーバー**（任意。5173 から `/releases`・`/api` を 8787 へ proxy 中継）:
   ```bash
   cd web && npm ci && npm run dev
   ```
4. **単体テスト・型検査**（`dist/wasm/` が必要。未生成なら先に 1 の `bash scripts/build-wasm.sh` を実行する）:
   ```bash
   bash scripts/build-wasm.sh           # dist/wasm/ が無いと typecheck/test は失敗する
   cd web && npm ci && npm run typecheck && npm test
   ```
   `web/test/integration-wasm.test.ts` は `dist/wasm/shutoko_routing.js` を import するため、
   `dist/` が git 管理外（`.gitignore`）のクリーンチェックアウトでは WASM ビルドが先に必要になる。
   CI の `web` job も同じ理由で、`bash scripts/build-wasm.sh` を `npm ci` / `npm run typecheck` / `npm test` より前に置いている。
5. **E2E（Playwright + chromium）**: 上記 1 の準備が終わっている状態で、
   ```bash
   cd web && npm run e2e   # vite build → wrangler dev(8787) 起動 → 全シナリオ
   ```

UI の操作: 住所・地名で検索して候補を選ぶ、または「現在地を使う」で出発地点を確定する（座標の直接入力・神田橋プリセットも折りたたみ内に用意）→ 最小/最大分（既定 15〜60、`1 ≤ 最小 ≤ 最大 ≤ 240`、プリセットボタンあり）→「ルートを探す」→ 地図と候補カード（計画時間・総所要時間・時間内訳・料金・実走行距離・課金対象1区間・推薦理由・警告）が同じ候補 ID で連動し、カード選択で該当経路を強調 →「出発する（Google マップを開く）」。探索は 10 秒でタイムアウトし、キャンセルも可能。候補なし・GPS 拒否・住所検索失敗・地図取得失敗・成果物不整合の各状態から、リロードなしで再操作・再検索できる。

### 性能計測ページ（`bench.html`、#13）

モバイル性能計測ハーネス。上記 1 の準備（`bash scripts/build-wasm.sh` → `npm --prefix workers ci` → `npm --prefix workers run seed:local`）と `cd web && npm run build` を済ませ、`wrangler dev`（8787）を起動して `http://localhost:8787/bench.html` を開く（`npm run e2e` も同じ `vite build` を行うため、E2E 実行後なら `web/dist` は生成済み）。

- 代表出発地点 10 件 × 時間条件 3 件 = 30 パターン。各パターンを cold 3 回 → warm 3 回の順に自動実行する（既定 180 試行）。1 試行ごとに新しい Worker を生成し、終了時に terminate する。
- 計測値: `tTransfer`（Worker 生成〜成果物取得・照合・WASM init 完了）/ `tSearch`（`search` 呼び出し直前〜直後、Worker 内計測）/ `tFirstCandidate`（Worker 生成〜候補または status 文言の描画を二重 `requestAnimationFrame` で確定した後）/ 10 秒上限到達 / 成果物の `transferSize`・`encodedBodySize`・`decodedBodySize`・`deliveryType` / ピークメモリ。時刻は `performance.timeOrigin + performance.now()` の epoch ms で統一する。
- 画面に集計表（全体 / cold / warm / パターン別の p50・p95、10 秒到達率、cold 転送量最大、メモリ最大）、目標合否（探索 p95 2 秒 / 初回 p95 8 秒 / 転送 10 MiB / メモリ 128 MiB）、JSON 全文とダウンロードリンクを表示する。`window.__benchResult` に envelope、完了時に `window.__benchDone = true` を立て、console に `BENCH_RESULT` を出す（Playwright からの回収用）。
- 縮小実行: `?patterns=0,5`（パターン index）/ `?repeats=1`（cold・warm 各回数）/ `?auto=1`（表示直後に自動開始）。既定は計測開始ボタン押下で開始。
- 端末名・OS・ブラウザ・ネットワーク条件・ピークメモリ（MiB）の手入力欄があり、記入すると envelope の `device` と `memoryManualMiB`（`memorySource: "manual"`）に反映される。`performance.memory` を持たないブラウザ（Safari）ではこの欄が唯一のメモリ計測手段になる（COOP/COEP による cross-origin isolation と `measureUserAgentSpecificMemory()` は使わない）。
- 転送量は `transferSize` / `encodedBodySize` で判定する。`decodedBodySize` は伸長後サイズ（graph.json では約 7.6 倍）であり転送量ではない。
- フル計測は CI に入れない（試行数が多く、スロットル下では不安定なため）。CI では `web/e2e/bench-smoke.spec.ts` が 2 パターン × cold/warm 各 1 回だけを検証する。`wrangler dev` は本番 Cloudflare と圧縮・ヘッダの挙動が異なるため、ローカルの転送量を本番値として扱わないこと。

### 代理計測（`npm run bench`、#13）

Playwright で Pixel 5 相当のコンテキストを作り、CDP で回線（Fast 4G / Slow 4G）と CPU 倍率をエミュレートして `bench.html` を自動実行する。上記の準備（`build-wasm.sh` → `workers ci` → `seed:local` → `npm run build` → `wrangler dev` 8787）を済ませてから実行する。

```bash
cd web
npm run bench -- --network fast4g --repeats 3 --out ../docs/bench/<date>-proxy/fast4g.json
npm run bench -- --network slow4g --repeats 3 --out ../docs/bench/<date>-proxy/slow4g.json
npm run bench:summarize -- ../docs/bench/<date>-proxy/fast4g.json \
  ../docs/bench/<date>-proxy/slow4g.json --out ../docs/bench/<date>-proxy/summary.md
```

- オプション: `--network fast4g|slow4g`（既定 fast4g）/ `--cpu 4` / `--patterns 0,1,...` / `--repeats 3` / `--base-url http://localhost:8787` / `--out` / `--timeout-ms` / `--headed`。既定の出力は `web/bench/results/<network>.json`（`.gitignore` 済み）。
- cold は試行ごとに `browser.newContext()` を作り直してブラウザキャッシュを分離し、warm は同一コンテキストで cold の直後に回収する。`--repeats 3` は cold 3 回 + warm 3 回になる。
- `npm run bench:summarize` は 1 つ以上の結果 JSON を読み、計測ページと同じ `web/src/bench/summarize.ts` の集計・判定で Markdown の表を stdout に出す。追加依存は無い。
- **CDP の CPU スロットルはレンダラのメインスレッドにのみ作用し、探索（WASM）が走る Web Worker には効かない**（`--cpu 1` と `--cpu 20` で `tSearch` が変わらないことを実測）。したがって代理計測の探索時間は実機の下限にはならない。Chromium 限定でもあり、iOS Safari の代理にはならない（`devices['Pixel 5']` は UA / viewport / touch の模擬にすぎない）。
- 計測値の定義・限界・実機手順・レポート雛形は [docs/bench/README.md](docs/bench/README.md) に集約している。
- フル代理計測は CI に入れない。CI では Vitest（集計・検証の純粋関数）と `bench-smoke`（2 パターン（神田橋 15〜30 = 候補 0 件 / 神田橋 15〜60 = 候補あり）× cold/warm 各 1 回 = 4 試行）の二段でハーネスの腐敗だけを検知する。

## ドキュメント

| 読みたいこと | ドキュメント |
| --- | --- |
| 発案者の意図と技術上の前提 | [原案](docs/original.md) — 人間のみが編集する原文 |
| 誰の何を解決するか、初期版で作るもの | [プロダクト企画](docs/product.md) |
| 入力・比較・出発の流れと例外時の体験 | [機能要件とユーザー体験](docs/requirements.md) |
| Cloudflare・Rust/WASM・地図の役割 | [システム設計](docs/architecture.md) |
| 実データ道路グラフ・課金ペア自動導出・料金表 v3 のパイプライン | [実データ生成パイプライン](docs/data-pipeline.md) |
| 時間予算に合わせた探索と候補の評価方法 | [ルート探索設計](docs/routing.md) |
| データ形式と外部サービスへの引き継ぎ | [データ・インターフェース設計](docs/interfaces.md) |
| Rust コアのテスト、WASM ビルド、実装済みの範囲 | [Rust / WASM 開発](docs/wasm-development.md) |
| 実装順序、検証条件、未決事項 | [実装・検証計画](docs/delivery.md) |
| 性能目標の計測手順・計測値の定義・実測レポート | [性能計測（bench）](docs/bench/README.md) |

最初に企画と要件を読み、実装時にはシステム設計、探索、インターフェースの順に参照してください。設計は原案を具体化した提案であり、現在の実装範囲と動かし方は Rust / WASM 開発に記載しています。公開中の versioned release は `all-real-v4` で、外部仕様（料金表・改定発表・路線図）の参照確認日は 2026-09-25 です。
