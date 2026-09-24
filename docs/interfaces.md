# データ・インターフェース設計

以下は完成版に向けた契約案。初期 Rust/WASM コアは座標入力や外部連携を含まないため、現時点で呼べる契約は [Rust / WASM 開発](wasm-development.md)を参照する。実データ生成パイプラインの仕様と手順については [実データ生成パイプライン](data-pipeline.md) を参照。JSON の座標オブジェクトは `lat` / `lon`、GeoJSON の座標配列は `[経度, 緯度]`、距離は m、時間は秒、時刻は UTC の ISO 8601 を使う。

## 公開成果物

マニフェスト（`manifest.json`）は `schemaVersion`、`releaseId`、`engineVersion`、`graphVersion`、`builtAt`、`sourceDate`、`coverage`、`vehicleProfile`、`timeModelVersion`、`billingPairsVersion`、`attribution`（`© OpenStreetMap contributors`）、`odblLicenseUrl`、`unverifiedSections`、`provenance`、`artifacts` を持つ。`artifacts` は生成成果物（`graph.json`、`snap-index.json` 等）それぞれの相対パス、SHA-256、バイト数を持つ。`builtAt` は再現性を担保するため外部から与えられた固定値を用いる。`coverage` は対応領域（`area`）、検証済み課金端点（`verifiedEntries`、`verifiedExits`）、および全verified-boundランプの `endpointCapabilities` を持つ。後者は `routable` と `structuralNoLoop` の入口・出口ID一覧と件数を機械可読に公開する。

グラフにはノード座標、エッジ ID、始終点、距離、時間、道路種別、路線名、形状、車両制限、遷移制限、入口/出口区分、引き継ぎ検証済み経由地点を格納する。生成元 OSM スナップショット、追加の人手検証情報とその出典・日付を追跡できるようにする。

R2 での格納形式はサイズ計測後に決める。スキーマと WASM の互換性がない場合は探索前に停止する。ネットワークの Content-Length だけに依存せず取得後のサイズとハッシュを照合する。

## 課金対象1区間のデータ

課金ペアは OSM から自動判別せず、人手で検証した宣言的シード入力（`data/billing-pairs-seed.json`）からビルダーによって生成される。シードの各要素は以下を持つ:

- `id`: 課金ペアの一意識別子（例: `bp:c1-inner:shibakoen-kasumigaseki`）
- `entryOsmWayId`: 入口ランプの OSM ウェイ ID
- `exitOsmWayId`: 出口ランプの OSM ウェイ ID
- `anchorOsmNodeId`: 本線上の周回基準点となる OSM ノード ID
- `vehicleProfile`: 対象車種プロファイル（例: `passenger-car-etc`）
- `status`: 検証状態（`verified` / `unverified`）
- `oneSectionAheadVerified`: 「1区間先の出口」であることが人手検証済みであるフラグ（boolean）
- `provenance`: 出典情報（`source`、`sourceDate`、`notes`）
- `prices[]`: 料金レコード配列（`amountYen`、`effectiveFrom`、`effectiveTo`）

生成された `Graph.billingPairs` は `id`、`entryId`、`exitId`、`anchorNodeId`、実経路探索により導出された `entryToAnchorEdgeIds` と `anchorToExitEdgeIds`、`status`、`vehicleProfile`、`prices` を持つ。`status=verified` のペアのみ候補に利用する。「1区間先」は地図上で最も近い出口ではなく、この対応表が指定する方向付き出口である。金額未確認でもペアの成立条件が確認済みなら候補を提示できる。

`anchorNodeId` は入口の合流後から直接区間へ進む本線上の基準状態（ノード）。ここへ一周後に戻り、出口へ進む道路列を定義できるペアを登録する。料金規則の前提は原案に従い、個別ペアの登録ではその適用条件とデータ根拠を確認する。

本節は現行 seed schema 1 / generated graph schema 2 の legacy ring pair について記載する。Issue #62 で seed schema 2 の混在 parser と diagnostic radial 型を実装し、Issue #63 で graph-builder の opt-in schema 4 に `RouteMembershipIndex` を追加した。schema 4 の membership は `directionMappingVersion` を持ち、relation mainline と bound ramp の endpoint・順序・hash を個別に検証する。`legacyRing` / `radialReturn` の graph reader、diagnostic plan から公開 BillingPair への昇移、core/WASM/Web の schema 4 consumer は #65/#66 の範囲で未実装である。仕様の正本は[実データ生成パイプライン](data-pipeline.md)とし、既存 C1 8要素の raw seed は変更しない。

## Workers の HTTP 境界

| インターフェース | 入出力・挙動 |
| --- | --- |
| `GET /releases/{releaseId}/manifest.json` | 許可された公開版のマニフェスト（200 OK、`application/json; charset=utf-8`、`Cache-Control: public, max-age=300, stale-while-revalidate=60`、ETag 付与、`If-None-Match` 一致時 304、HEAD 対応）。未知版・manifest 未配置は 404 |
| `GET /releases/{releaseId}/engine.json` | 同版の wasm / JS glue の期待値（`schemaVersion`、`releaseId`、`artifacts`: `{ path, sha256, byteLength }[]`）。Content-Type と Cache-Control は `manifest.json` と同一（`application/json; charset=utf-8`、`public, max-age=300, stale-while-revalidate=60`）。投入時に実ファイルから計算するため、環境をまたいでも一致する。未知版・R2 未配置は 404 |
| `GET /releases/{releaseId}/{artifact}` | 成果物 allowlist に含まれるファイルのみ配信（`.wasm` は `application/wasm`、`.json` は `application/json; charset=utf-8`、`.js` は `text/javascript; charset=utf-8`、`.d.ts` は `text/plain; charset=utf-8`。`Cache-Control: public, max-age=31536000, immutable`、ETag 付与、`If-None-Match` 一致時 304、HEAD 対応）。未知版・allowlist 外・パストラバーサル（`..`、`%2e`）・manifest 未配置版はすべて 404 |
| `POST /api/geocode` | `{ "query": "住所" }` → `{ "candidates": [{ "label": "住所", "lat": 35.0, "lon": 139.0 }] }`。0 件ヒット時は 200 で `{ "candidates": [] }`。全応答（成功・エラー問わず）`Cache-Control: no-store` |

### 成果物 allowlist
成果物配信（`/releases/{releaseId}/{artifact}`）で許可されるファイル名は以下の固定リストに限定される:
- `manifest.json`
- `engine.json`
- `graph.json`
- `snap-index.json`
- `ramps.json`
- `od-tariffs.json`
- `shutoko_routing_bg.wasm`
- `shutoko_routing.js`
- `shutoko_routing.d.ts`
- `index.d.ts`

### 住所検索仕様と制約
- **対象データ**: 国土地理院 住所検索 API をプロバイダーとして利用。行政地名・街区・住居表示レベルの**住所・地名検索専用**であり、駅名・施設名（POI）の検索には非対応。該当なしの場合は 404 ではなく 200 で空配列 `{ "candidates": [] }` を返す。
- **入力バリデーション**: 本文 4,096 バイト以下、`query` は空白を除いて 1〜200 文字、確定検索に限定。
- **レート制限**: Cloudflare Rate Limiting binding により、送信元 IP ごとに毎分 10 回（`IP_RATE_LIMITER`、超過時 429 と `Retry-After: 60`）、サービス全体で毎分 600 回（`GLOBAL_RATE_LIMITER`）に制限。バインディング不在時または例外発生時は fail-closed とし、上流を呼ばずに 503 `RATE_LIMITER_UNAVAILABLE` を返す。
- **タイムアウト**: 上流呼び出しは 5 秒でタイムアウト（AbortController 連携、超過時 504）。
- **プライバシー・秘密保護**: 全応答に `Cache-Control: no-store` を設定。エラー本文・レスポンスヘッダ・アクセスログに検索クエリ、座標、上流エラー本文、上流 URL、API キーを含めない。

### エラーコード一覧
すべてのエラー応答はステータスコードに応じた HTTP レスポンスとともに、以下の JSON スキーマ `{ "error": { "code": string, "retryable": boolean } }` を返す:

| HTTP ステータス | エラーコード (`code`) | `retryable` | 発生条件 |
| --- | --- | --- | --- |
| 400 Bad Request | `INVALID_QUERY` | `false` | JSON デコード失敗、`query` フィールド欠落/非文字列、空白除去後 1〜200 文字の範囲外 |
| 400 Bad Request | `PAYLOAD_TOO_LARGE` | `false` | リクエスト本文が 4,096 バイト (4KiB) を超過 |
| 404 Not Found | `NOT_FOUND` | `false` | 未知の URL パス、未知の releaseId、成果物 allowlist 外のファイル要求、パストラバーサル検出、または R2 上に `manifest.json` が存在しない版への要求 |
| 405 Method Not Allowed | `METHOD_NOT_ALLOWED` | `false` | `/releases/...` に対する GET/HEAD 以外のメソッド、または `/api/geocode` に対する POST 以外のメソッド |
| 429 Too Many Requests | `RATE_LIMITED` | `true` | IP 毎分 10 回、または全体毎分 600 回のレート制限超過（`Retry-After: 60` ヘッダ付与） |
| 502 Bad Gateway | `GEOCODER_UNAVAILABLE` | `true` | 上流ジオコーダーの非 2xx 応答、ネットワーク通信失敗、または不正 JSON 応答 |
| 503 Service Unavailable | `RATE_LIMITER_UNAVAILABLE` | `true` | レート制限バインディング不在、またはレート制限呼び出しの例外発生（上流ジオコーダーを呼ばず即時返却） |
| 504 Gateway Timeout | `GEOCODER_TIMEOUT` | `true` | 上流ジオコーダー呼び出しが 5 秒以内に完了せずタイムアウト |


## ブラウザの探索境界

UI → Web Worker のリクエスト例（値は形式を示す架空例）:

```json
{
  "type": "search",
  "requestId": "request-1",
  "releaseId": "sample-release",
  "pricingAt": "2026-09-10T00:00:00Z",
  "origin": { "lat": 35.68, "lon": 139.76 },
  "entryRampId": "ramp:c1-outer:kandabashi-entry",
  "exitRampId": "ramp:c1-outer:takaracho-exit",
  "minMinutes": 60,
  "maxMinutes": 90,
  "vehicleProfile": "passenger-car-etc"
}
```

Web Worker は `ready`、`result`、`error` を返し、各探索応答に request ID を付ける。`ready` の payload は `{ "type": "ready", "releaseId": "all-real-v2" }` で、初期化（取得・照合・WASM init）完了時に 1 度だけ送る。`error` の `code` 一覧は次のとおり。

| `error.code` | 発生箇所 | 意味 |
| --- | --- | --- |
| `ARTIFACT_MISMATCH` | Worker（パイプライン） | manifest / engine.json の期待値との sha256・バイト長不一致、または engine.json の形式不正（`schemaVersion`・`releaseId`・必須エントリ）。部分データでは探索せず停止 |
| `FETCH_FAILED` | Worker（パイプライン） | 成果物取得の HTTP 失敗・通信失敗 |
| `INVALID_INPUT` | WASM 境界（`RoutingErrorPayload`） | 座標・時間・版などの入力検証失敗。理由を入力欄付近に表示 |
| `WASM_ERROR` | WASM 境界 | `RoutingErrorPayload` JSON でない例外メッセージのフォールバック |
| `RESULT_CONTRACT_MISMATCH` | Worker（パイプライン） | 探索結果 JSON が実行時契約に合わない。必須診断フィールド（`nearestAccess`・`minPlanSeconds`）の欠落や、値の型・有限性・非負性の違反を検出した。旧エンジンと新 UI の取り違えを部分データとして表示せず停止する |
| `USE_AFTER_FREE` | Worker（パイプライン） | 解放済みの `PreparedGraph`（`LoadedRelease`）に対する呼び出しの検出。設計上到達しない防御的ガード |
| `TIMEOUT` | UI 側 | 探索押下から 10 秒以内に `result` / `error` が返らず、UI が Worker を terminate した |

`TIMEOUT` は Worker が送る応答ではなく UI 側が生成する状態であり、次回検索時に新しい Worker を再生成して `ready` を待ってから再送する。ブート時の初期化失敗の `error` は `requestId: ""` で送られ、UI は再読み込みを案内する。WASM 呼出は `search(graph, request, limits)` に加え、グラフのパース・インデックス構築を 1 度だけ行い再利用する `prepare(graph, limits)` と `searchPrepared(pg, request)` の 2 段境界を提供する。`prepare` が返す `WasmPreparedGraph` は WASM リニアメモリ上に保持され、JS glue の `free()` により明示的に解放される。Worker パイプライン（`ReleaseStore` / `LoadedRelease`）は参照カウント（`retain` / `release`）と世代管理によりこのライフサイクルを一元管理し、検索実行中の安全な再利用とリリース切替・破棄時のメモリ解放を両立する。UI の制限を信用せず Rust 側でも座標の有限性・範囲（`-90.0..=90.0`, `-180.0..=180.0`）、`origin` と `originNodeId` の排他性、分の整数・大小関係、版・車両の一致、`pricingAt` の有効な UTC 時刻を検証する。構造や値の不正は `Err(RoutingError { code: "INVALID_INPUT", message })` を返し、WASM 境界で JSON シリアライズされた `RoutingErrorPayload` として JS 例外をスローする。Worker は検索結果 JSON を受け取った時点で必須診断フィールド（`nearestAccess`・`minPlanSeconds`）を実行時検証し、欠落や型・有限性・非負性の違反は `RESULT_CONTRACT_MISMATCH` で停止する（旧エンジンの応答を部分データとして UI に流さない）。

### 計測フック（`bench`、任意・#13）

性能計測ページ（`web/bench.html`）だけが付ける任意フィールド。**通常 UI は付けない**ため、`bench` が無いときの取得順・照合・エラーの挙動は一切変わらない。

- `search` の `bench.cacheBust?: string`: 指定された時だけ、5 成果物（manifest / engine / graph / wasm / glue）の URL に `?bench=<nonce>` を付けて取得する。配信側ルータ（`workers/src/index.ts` の `getRawPath`）はクエリを落とすため同一成果物が返り、sha256 照合はバイト列に対して行われるので成立する。cold 計測（ブラウザ HTTP キャッシュ `immutable` の迂回）専用。
- `result` の `bench`（`msg.bench` があるときだけ載る）:
  - `marks`: `loadStartEpochMs` / `loadEndEpochMs` / `searchStartEpochMs` / `searchEndEpochMs`。すべて `performance.timeOrigin + performance.now()` の epoch ms で、ページと Worker で同じ土台の差分を取れる。
  - `resources`: Worker 内で収集した成果物 URL の Resource Timing（`name` / `transferSize` / `encodedBodySize` / `decodedBodySize` / `deliveryType` / `responseStatus` / `duration`）。Worker の fetch はメイン document の Resource Timing に出ないため Worker 側で収集して返す。
  - `memory`: `performance.memory` の探索直前・直後サンプル（MiB）。取得できない環境では `null`。
- `ready` の payload は従来どおり。bench Worker（`new Worker(url, { name: "bench" })`）は起動時の先読みを行わず `search` メッセージ駆動で取得する（先読みが先に走ると `cacheBust` 付きの取得が先読み済みリリースに相乗りして cold 計測が成立しないため）。

### 空間スナップ契約
- スナップ対象: Entry エッジの from ノード（入口アクセス地点）のみ。一般道ノードはスナップ対象に含まない。また境界 JCT（`boundary_in`）は地表スナップ対象から除外。
- スナップ距離計算: 等距円筒近似（Equirectangular approximation、東京付近 `cos(lat)` 補正）。
- 選択件数: 近い順に最大 `SearchLimits.max_access_entries` 件（デフォルト 0 = 無制限、グラフ内の全 Entry アクセス地点）。スナップ半径（200m 固定）の概念は廃止。
- 距離キャップ: `SearchLimits.max_access_distance_meters`（デフォルト 30,000 m、0 は無制限）を超える最寄り入口は探索対象外とし、`status: "no_candidates"`, `reason: "NO_CONNECTION"` を返す。このときも最近接の入口アクセス地点を `nearestAccess` として返す。
- 入口アクセス地点が1件も得られない場合: エラーとせず、`status: "no_candidates"`, `reason: "NO_CONNECTION"`, `candidates: []`, `nearestAccess: null` を正常返却する。

### 探索制限パラメータ（`SearchLimits`）
- `max_expanded_states`: 全体展開状態数上限（デフォルト 100,000）
- `beam_width`: 小規模グラフの全単純閉路列挙で各深さに保持する状態数（デフォルト 200）。全線グラフはSCC/逆Dijkstra閉路カタログを使う
- `max_loop_edges`: 本線閉路探索エッジ数上限（デフォルト 2,000、入力として受理する上限 5,000）
- `min_loop_meters`: 最小ループ距離（デフォルト 5,000m、マイクロループ排除）
- `max_pairs`: 課金ペア探索対象上限（デフォルト 10）。座標検索（入口 tier 探索）では各 tier 内で評価する検証済み課金ペア数の上限として tier 単位に適用される。従来のノード検索（`originNodeId`）や、`origin` 座標を伴わない片側ランプ指定では全体の課金ペア探索対象上限として機能する。`origin` 座標と片側ランプ指定を併用した場合は座標検索として tier 単位に適用される。入口・出口双方を指定する明示OD探索には適用しない
- `max_access_distance_meters`: 最大アクセス距離（デフォルト 30,000m）
- `max_access_entries`: 最大アクセス入口数（デフォルト 0 = 無制限）

### 探索結果の status と reason コード
結果は `requestId`、`releaseId`、`status`（`ok` / `no_candidates` / `truncated`）、`reason`、`rankingMode`、`expandedStates`、`candidates`、`nearestAccess`、`minPlanSeconds` を持つ。`reason` は該当時のみ以下のコードをとる:
- `NO_CONNECTION`: 入口アクセス地点（Entry エッジ from ノード）が1件も得られない、または最寄りの入口アクセス地点が `SearchLimits.max_access_distance_meters` を超える場合に発生する。座標検索では `entry_tiers` が空（距離キャップ超過または一般入口ランプ不在）のときにのみ発生し、検証済み課金ペアが無くても動的 OD tier が評価されるため「課金ペア不在による NO_CONNECTION」は生じない。一方、従来のノード検索（`originNodeId`）や、`origin` 座標を伴わない片側ランプ指定では、`SearchLimits.max_access_entries` により検証済み課金ペアの入口がアクセス候補に含まれない場合にも本理由が返る（`origin` 座標と片側ランプ指定を併用した場合は座標検索として扱われるため、本理由は上記の `entry_tiers` が空の場合に限る）。先頭2つ（入口アクセス地点0件・距離キャップ超過）は原点がデータ被覆の外側であることを意味し、`nearestAccess` は cap 超過の距離になる（cap を 0 = 無制限にした場合はこの限りでない）。`max_access_entries` 制限による場合は `nearestAccess` が近距離でも生じ得るため、UI は距離キャップ超過を確認してから「到達不能」を断定する。
- `NO_BILLING_PAIR`: 有効な課金ペアが 1 件も存在しない
- `NO_LOOP`: 周回ループが見つからない、または進入不可
- `TIME_WINDOW`: 指定所要時間枠（minMinutes〜maxMinutes）に収まる候補がない
- `NO_HANDOFF`: Maps URL 長が 2,048 文字を超過したため候補が除外された
- `SEARCH_LIMIT`: 探索状態数・候補数の上限に達した（`status: "truncated"`）。列挙が完走していないため `minPlanSeconds` は `null` になり得る（真の最小を証明しない）

診断情報（候補の有無にかかわらず返す）:

| フィールド | 型 | 内容 |
| --- | --- | --- |
| `nearestAccess` | `SnappedOrigin \| null` | 座標入力（`origin`）における最近接の入口アクセス地点（`{ nodeId, lat, lon, distanceMeters }`）。距離キャップ超過の `NO_CONNECTION` でも返す。`originNodeId` 入力および Entry アクセス地点が0件のときは `null` |
| `minPlanSeconds` | `number \| null` | ループ時間が製品上限 240 分以内にある合法（禁止遷移を満たす）周回の `planSeconds`（`baseSeconds + bufferSeconds`）の最小値。指定時間枠で棄却した周回も含む。座標検索では最近接入口 tier が診断（`TIME_WINDOW` / `NO_HANDOFF`）を確定する場合、値は評価済み tier 内の最小値となる（合法周回を持つ最近接 tier で診断を確定し遠方入口へフォールスルーしないため）。従来のノード検索（`originNodeId`）や、`origin` 座標を伴わない片側ランプ指定では評価された検証済み課金ペア全体における最小値となる。`origin` 座標と片側ランプ指定を併用した場合は座標検索と同様に評価済み tier 内の最小値となる。列挙は「ループ部分の秒数 ≤ 240 分」で打ち切られるため、その値は列挙範囲内の最小値であり真の全周回最小を上回り得る。**`minPlanSeconds > 240 * 60` を「240 分以内に収まる合法周回が無い」の根拠として使えるのは、列挙が資源上限で打ち切られていない場合に限る**。`SearchLimits.beamWidth`・`maxExpandedStates`・`maxPairs`（または候補側の上限）が列挙を打ち切ったときは真の最小が証明できないため `null` を返す（この場合 UI は「最短でも N 分」「最大 4 時間でも無理」を断定してはならない）。値が non-null でも `240 * 60` を超えるときは列挙範囲内の最小にすぎず列挙外のより長い周回がより小さい `planSeconds` を持ち得るため、UI は絶対的な「最短」と断定せず出所（確認できた範囲）を明示する。`240 * 60` 以下の値は「240 分以内に収まる周回が存在する」ことの根拠として使える。合法な周回が1件も無い場合も `null`。`TIME_WINDOW` の数値根拠（最寄り入口までの距離は `nearestAccess.distanceMeters`）として使う |

### 現行 pre-v2 Candidate の必須フィールド

| フィールド | 型 | 内容 |
| --- | --- | --- |
| `id`, `releaseId` | `string` | 条件と経路列から安定生成する検索内 ID と版。利用履歴として保存しない |
| `origin` | `LatLng \| null` | 入力座標（`{ lat, lon }`）。`originNodeId` 指定時は `null` |
| `snappedOrigin` | `SnappedOrigin` | 選択した入口アクセス地点（`{ nodeId, lat, lon, distanceMeters }`）。候補ごとに異なる入口アクセス地点を持ちうる。`originNodeId` 指定時はそのノードで `distanceMeters: 0` |
| `entry`, `exit` | `RampInfo` | 入出口情報（`{ edgeId, name, rampId?, route?, direction? }`。`rampId` は正規ランプ ID、`name` はランプ名、`route` は路線記号、`direction` は方向） |
| `entryId`, `exitId` | `string` | 入出口エッジ ID（後方互換用） |
| `roadNames` | `string[]` | 首都高部分で通過したエッジ名の重複除去済み順序付きリスト（名前のないエッジはスキップ） |
| `edgeIds` | `string[]` | 順序付き走行エッジ ID 列（首都高上の entry → loop → exit） |
| `geometry` | `GeoJsonLineString` | 全経路の GeoJSON LineString（`{ type: "LineString", coordinates: [[lon, lat], ...] }`）。重複端点なし |
| `duration` | `Duration` | `accessSeconds`, `shutokoSeconds`, `returnSeconds`, `baseSeconds`, `bufferSeconds`, `planSeconds` |
| `distanceMeters`, `shutokoDistanceMeters` | `number` | 現行pre-v2ではどちらも首都高Edge距離（m）として同じ値を返す。一般道のaccess / return距離はDurationだけを持ち、総距離ではない |
| `toll` | `Toll` | `billingPairId`, `chargedSectionCount: 1`（現行pre-v2全outputの互換field）, `amountYen`（未確認なら`null`）, `pricingAt`, `effectiveFrom`, `effectiveTo`, `billingDistanceMeters?`, `tollSource?`（`"table" \| "od_tariff" \| "calculated"`） |
| `loop` | `Loop` | 現行 C1 の `anchorNodeId`, `edgeIds`, `durationSeconds`, `distanceMeters`, `validated: true` |
| `reasons` | `string[]` | 機械可読推薦理由コード。現行pre-v2ではC1 pairだけでなくdynamic ODにも`ONE_SECTION_TOLL`を付け、`BEST_TIME_PER_YEN`または`BEST_SHUTOKO_TIME`を先頭に置く。これはeligibilityの証拠ではない |
| `warnings` | `string[]` | 警告コード（常時付与: `HANDOFF_WAYPOINTS_UNVERIFIED`（#8 実機検証未了）、`STATIC_TRAVEL_TIME`） |
| `handoff` | `Handoff` | Google Maps引き継ぎ情報（`{ origin, destination, waypoints, mapsUrl, verificationSetVersion }`） |

現行pre-v2のdynamic ODはseed由来かどうかにかかわらず`status=ok`、`rankingMode=shutoko_time`で返り、`toll.amountYen=null`でも`chargedSectionCount=1`と`ONE_SECTION_TOLL`を持つ。Web UIも全Candidateへ「1区間料金」と表示する。現行の`distanceMeters`と`shutokoDistanceMeters`はどちらもhighway Edge距離で、UIの距離表示と名称が完全には一致しない。これらは移行前の互換contractであり、C1 legacyだけ、または実装済みのradial / total distance契約ではない。

### 推薦理由コード（`reasons`）一覧
- `BEST_TIME_PER_YEN`: 時間あたり料金効率が最も高い最優先候補（料金確定時）
- `BEST_SHUTOKO_TIME`: 首都高滞在時間が最も長い最優先候補（料金未確定時等の時間ソート時）
- `ONE_SECTION_TOLL`: 現行pre-v2ではC1 pairとdynamic ODの双方に付ける後方互換コード。routing v2では`legacyRing` adapterだけが付け、radialと`topology_only`には生成しない。Web UIも同じvariant境界で「1区間料金」を表示しない

### 警告コード（`warnings`）一覧
- `HANDOFF_WAYPOINTS_UNVERIFIED`: Google Maps 引き継ぎ経由地選定ルールが暫定であり実機検証未了であることを示す（#8 完了まで常時付与）。2026-09 に Android Chrome + Google マップアプリ「あり」で代表1系列の周回維持を確認したが、アプリ「なし」・iOS Safari・経由地点0〜3点の系列網羅・URL 長上限は未検証のため引き続き付与する（[検証記録](delivery.md) 参照）
- `STATIC_TRAVEL_TIME`: 渋滞・規制を含まない静的制限速度に基づく推定時間であることを示す

### Issue #42 後の Candidate v2（設計・未実装）

graph schema 4 / routing v2のCandidateは、次の点で現行C1 legacy outputと区別する。

- `pairKind="radialReturn"`、`routePlanVersion=1`、`anchor.anchorKind="directedJunction"`とM / Bを持つ。`anchorNodeId`と旧`loop`objectは不要。
- `routePlan.membershipIds[]`と`routePlan.resolvedRouteSegments[]`で、mainline relationとbound rampの由来を保つ。
- `eligibilityStatus`は`verified_one_section_ahead`、`unverified`、`topology_only`のいずれか。endpoint support、loop validation、routing capabilityと独立させる。
- `loopValidationStatus`は`declared_route_validated`、`unresolved`、`topology_only`のいずれか。route planの解決状態だけを表し、商品eligibilityへ代用しない。
- `tariffStatus`は`priced`、`unpriced`、`expired`、`not_applicable`のいずれか。#41前の2号radialは`amountYen=null`、`billingDistanceMeters=null`、`tariffStatus=unpriced`とする。
- `toll.chargedSectionCount`と`ONE_SECTION_TOLL`を出さない。`legacyRing` adapterだけが両者を維持する。
- `time_per_yen`は商品比較対象のtariffが全件`priced`のときだけ使う。unpricedが混在する集合は`shutoko_time`、`topology_only`は商品推薦から外す。
- `distanceMeters`はhighway + surface access + surface returnの推定距離、`shutokoDistanceMeters`はhighway Edge距離とする。
- radialの`handoff`は`{ enabled: false, legUrls: [] }`で返し、device verification gate通過後だけ`enabled=true`と検証済みleg URLを持たせる。

`edgeRouteLegs`のroleは`entry_approach`、`mandatory_lap`、`return_corridor`、`exit_approach`の4種類だけとする。`startEdgeIndex`は含み、`endEdgeIndexExclusive`は含まない。各legは`resolvedSegmentId`で`routePlan.resolvedRouteSegments[]`を参照し、参照先の`edgeIdsSha256`がCandidateの`edgeIds`スライスと一致することを確認する。4区間は`[0, edgeIds.length)`を重複も欠落もなく覆う。一般道のsurface access / returnは`estimatedLegs`に置き、`estimated=true`、`distanceMeters`、`durationSeconds`を持たせ、Edge indexとgeometryを持たない。

以下はreader fixture用のwire-level Candidateである。IDと座標はsynthetic valueで、公開可能な2号bindingや料金を表さない。`toll`に`chargedSectionCount`はなく、4 legのindex、segment hash、距離式が一致する。

```json
{
  "id": "fixture:candidate:radial",
  "releaseId": "radial-fixture-v1",
  "pairKind": "radialReturn",
  "routePlanVersion": 1,
  "origin": null,
  "originNodeId": "fixture:node:entry:ground",
  "snappedOrigin": {
    "nodeId": "fixture:node:entry:ground",
    "lat": 35.0,
    "lon": 139.0,
    "distanceMeters": 0
  },
  "entry": {
    "edgeId": "fixture:edge:entry:ramp",
    "rampId": "fixture:ramp:entry",
    "name": "fixture entry",
    "route": "fixture:R1",
    "direction": "inbound"
  },
  "exit": {
    "edgeId": "fixture:edge:exit:ramp",
    "rampId": "fixture:ramp:exit",
    "name": "fixture exit",
    "route": "fixture:R1",
    "direction": "outbound"
  },
  "entryId": "fixture:edge:entry:ramp",
  "exitId": "fixture:edge:exit:ramp",
  "roadNames": [],
  "edgeIds": [
    "fixture:edge:entry:ramp",
    "fixture:edge:entry:mainline",
    "fixture:edge:lap:1",
    "fixture:edge:lap:2",
    "fixture:edge:return:mainline",
    "fixture:edge:exit:ramp"
  ],
  "geometry": {
    "type": "LineString",
    "coordinates": [[139.0, 35.0], [139.001, 35.001], [139.002, 35.002]]
  },
  "anchor": {
    "anchorKind": "directedJunction",
    "mergeNodeId": "fixture:node:merge",
    "branchNodeId": "fixture:node:branch",
    "mergeTerminalEdgeId": "fixture:edge:entry:mainline",
    "branchInitialEdgeId": "fixture:edge:return:mainline",
    "routeId": "fixture:loop",
    "direction": "forward",
    "arcPolicy": "ordinaryLongArc",
    "excludedShortConnector": {
      "fromNodeId": "fixture:node:branch",
      "toNodeId": "fixture:node:merge",
      "osmWayId": 3001,
      "edgeCount": 1,
      "distanceMeters": 100
    }
  },
  "routePlan": {
    "membershipIds": [
      "fixture:route:r1:inbound",
      "fixture:route:loop:forward",
      "fixture:route:r1:outbound"
    ],
    "resolvedRouteSegments": [
      {
        "resolvedSegmentId": "fixture:resolved:entry",
        "role": "entry_approach",
        "membershipId": "fixture:route:r1:inbound",
        "sourceSegmentIds": ["fixture:binding:entry:0", "fixture:relation:r1:inbound:main"],
        "edgeIdsSha256": "26fae7c7d5e125ca9f0c5de847411ff1fdb1883e961b641eb4b05b6030a381be"
      },
      {
        "resolvedSegmentId": "fixture:resolved:lap",
        "role": "mandatory_lap",
        "membershipId": "fixture:route:loop:forward",
        "sourceSegmentIds": ["fixture:relation:loop:forward:main"],
        "edgeIdsSha256": "99ddf4b7771edc3fe3c04034d2465719932ca1c2d2104c0d05bab41d6e462a43"
      },
      {
        "resolvedSegmentId": "fixture:resolved:return",
        "role": "return_corridor",
        "membershipId": "fixture:route:r1:outbound",
        "sourceSegmentIds": ["fixture:relation:r1:outbound:main"],
        "edgeIdsSha256": "ba825c8c0f6236ca476824f7553aefe8bf09a94a5343c2470cde33b1bb443e1b"
      },
      {
        "resolvedSegmentId": "fixture:resolved:exit",
        "role": "exit_approach",
        "membershipId": "fixture:route:r1:outbound",
        "sourceSegmentIds": ["fixture:binding:exit:0"],
        "edgeIdsSha256": "b275420fdbd9d55bd72a6b5f8e36cbf6218042d5afdd1400be3b8713b4b7a35c"
      }
    ]
  },
  "edgeRouteLegs": [
    {
      "role": "entry_approach",
      "resolvedSegmentId": "fixture:resolved:entry",
      "startEdgeIndex": 0,
      "endEdgeIndexExclusive": 2
    },
    {
      "role": "mandatory_lap",
      "resolvedSegmentId": "fixture:resolved:lap",
      "startEdgeIndex": 2,
      "endEdgeIndexExclusive": 4
    },
    {
      "role": "return_corridor",
      "resolvedSegmentId": "fixture:resolved:return",
      "startEdgeIndex": 4,
      "endEdgeIndexExclusive": 5
    },
    {
      "role": "exit_approach",
      "resolvedSegmentId": "fixture:resolved:exit",
      "startEdgeIndex": 5,
      "endEdgeIndexExclusive": 6
    }
  ],
  "estimatedLegs": [
    {
      "role": "surface_access",
      "estimated": true,
      "distanceMeters": 1200,
      "durationSeconds": 240
    },
    {
      "role": "surface_return",
      "estimated": true,
      "distanceMeters": 800,
      "durationSeconds": 160
    }
  ],
  "duration": {
    "accessSeconds": 240,
    "shutokoSeconds": 960,
    "returnSeconds": 160,
    "baseSeconds": 1360,
    "bufferSeconds": 300,
    "planSeconds": 1660
  },
  "distanceMeters": 6000,
  "shutokoDistanceMeters": 4000,
  "eligibilityStatus": "verified_one_section_ahead",
  "loopValidationStatus": "declared_route_validated",
  "tariffStatus": "unpriced",
  "toll": {
    "billingPairId": "fixture:radial",
    "amountYen": null,
    "pricingAt": "2026-09-16T00:00:00Z",
    "effectiveFrom": null,
    "effectiveTo": null,
    "billingDistanceMeters": null
  },
  "reasons": [],
  "warnings": ["STATIC_TRAVEL_TIME", "HANDOFF_WAYPOINTS_UNVERIFIED"],
  "handoff": {
    "enabled": false,
    "legUrls": []
  }
}
```

`edgeIdsSha256`は順序を保ったEdge IDの空白なしJSON arrayをSHA-256化した値で、generated graphの`resolvedRouteSegments`とCandidateの`routePlan`で同じ値を使う。reader / Workerは全hash、membership / binding参照、role、index範囲、各status、不正または欠落した`chargedSectionCount`を検証してからCandidateを返す。

core、WASM型、Web Workerはgraph schema 2 / 3 / 4を読む。schema 2 / 3の`pairKind`なしは`legacyRing`、schema 4のbuilder出力は`pairKind`を必須とし、未知のkind / versionは部分データを返さず停止する。builderの既定schema 4への切替は、reader、WASM、Web pipeline、Workers allowlist、release ID、manifest hashを同時に更新する独立issueで行う。

## Google マップへの引き継ぎ

公式 Maps URLs を使い、`https://www.google.com/maps/dir/` に `api=1`、`origin`、`destination`（出発地点）、`travelmode=driving`、`waypoints` を設定する。出発ボタンは経路の確認画面を開く意味とし、自動的にナビを開始する保証はしない。

URL は座標のみの固定形式を `format!` で組み立て（区切りは `%7C`）、2,048文字以内にする。モバイルブラウザの上限に合わせ最大3経由地点とする。経由地点が非対応の製品もある。[Google Maps URLs](https://developers.google.com/maps/documentation/urls/get-started)

本線上の座標が別道路や停車地点に解釈される可能性を先行検証する。一周を省略せず入口・主要通過点・出口を最大3点で表現でき、代表端末で確認済みの経路系列だけを初期の公開候補とする。入口と1区間先の出口だけでは周回を省略した短い経路になり得るため、周回の再現を必ず確認する。上限超過時に黙って地点を削除したり、走行中の手動区間切替を要求したりしない。

任意出発地点について外部経路の完全一致を事前保証できないため、検証済み系列でも利用者に最終確認を促す。Google の再計算結果を取得して自動比較する機能は含めない。再現不能な系列を除外すると企画価値を満たせない場合は、公開を進めず連携方式を再設計する。

Google Maps URL は waypoint の順序を示しても、近接 JCT の正しい arm、首都高の道路、radial の往路・復路を強制できない。放射線の公開 handoff は、URL 分割だけでは有効にしない。device verification manifest に `routePlanId`、`releaseId`、URL builder version、leg URL hash、期待する道路・向き、OS / browser / app version、検証日時、結果、期限を記録し、Android / iOS × Web / app の必要条件が1件でも `missing`、`failed`、`expired` なら public departure を無効にする。manifest と release gate の詳細は[ルート探索設計](routing.md)を正本とする。

## 地図・住所検索のデータ利用

OSM のデータライセンスとタイルサーバーの利用条件は別に扱う。地図に OpenStreetMap contributors の帰属と著作権ページへのリンクを表示する。派生グラフの公開時は ODbL に基づく帰属・提供義務を確認し、元データと生成方法を追える形にする。[OpenStreetMap copyright](https://www.openstreetmap.org/copyright)

標準タイルサーバーを本番用の無制限基盤とはみなさない。大量先読み・オフライン取得を行わず、必要な帰属とキャッシュ条件を守る。実際のタイル提供元は利用量・可用性・条件を確認して選ぶ。[OSMF Tile Usage Policy](https://operations.osmfoundation.org/policies/tiles/)

地図の表示範囲は、出発地点の確定（現在地・住所・地図タップ・座標）および候補表示への追従で移動し、移動後の範囲のタイルを既存の提供元（国土地理院）へ要求する。新しい提供元は追加しない。現在地取得を契機とする位置依存のタイル要求が発生し得るため、UI は地図操作・出発地点確定時の外部通信を説明する文言を表示する。位置情報そのもの（緯度経度）を成果物・ログ・成果物 URL に含めない方針は従来どおりで、現在地は検索リクエストの座標として WASM に渡すだけである。

公開 Nominatim は絶対上限1リクエスト/秒で、クライアント側オートコンプリートは禁止されている。本番はプロバイダー契約または自前運用を選定し、この制約を前提としない構成を確定するまで公開しない。[Nominatim Usage Policy](https://operations.osmfoundation.org/policies/nominatim/)

## 料金データの有効期間

`billingPairs` の料金レコードは `effectiveFrom` / `effectiveTo` と車種・支払い方法を持つ。リクエストの `pricingAt` に有効な版で比較し、金額不明や期限外は未確認として扱う。期間は `[effectiveFrom, effectiveTo)`（終了 null は期限なし）とし、重複期間は成果物検証で拒否する。将来出発日時の入力は初期版に含めない。料金期間をまたいだ状態で出発する場合は UI で再検索を案内する。2026-10-01 の料金改定が告知されているため、固定額をコードへ埋め込まず有効期間を切り替える。[首都高の料金改定発表](https://www.shutoko.co.jp/company/press/2026/data/07/31-toll/)

ETC を料金前提とする場合は入口から出口まで同じカードを利用する条件を明示する。[首都高 ETC 利用案内](https://www.shutoko.jp/fee/fee-info/pay_etc/attention/)
