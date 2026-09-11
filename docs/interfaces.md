# データ・インターフェース設計

以下は完成版に向けた契約案。初期 Rust/WASM コアは座標入力や外部連携を含まないため、現時点で呼べる契約は [Rust / WASM 開発](wasm-development.md)を参照する。実データ生成パイプラインの仕様と手順については [実データ生成パイプライン](data-pipeline.md) を参照。JSON の座標オブジェクトは `lat` / `lon`、GeoJSON の座標配列は `[経度, 緯度]`、距離は m、時間は秒、時刻は UTC の ISO 8601 を使う。

## 公開成果物

マニフェスト（`manifest.json`）は `schemaVersion`、`releaseId`、`engineVersion`、`graphVersion`、`builtAt`、`sourceDate`、`coverage`、`vehicleProfile`、`timeModelVersion`、`billingPairsVersion`、`attribution`（`© OpenStreetMap contributors`）、`odblLicenseUrl`、`unverifiedSections`、`provenance`、`artifacts` を持つ。`artifacts` は生成成果物（`graph.json`、`snap-index.json` 等）それぞれの相対パス、SHA-256、バイト数を持つ。`builtAt` は再現性を担保するため外部から与えられた固定値を用いる。`coverage` は対応領域（`area`）と検証済み入出口一覧（`verifiedEntries`、`verifiedExits`）。対応領域内でも経路接続の存在は別途確認する。

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

## Workers の HTTP 境界

| インターフェース | 入出力・挙動 |
| --- | --- |
| `GET /releases/{releaseId}/manifest.json` | 許可された公開版のマニフェスト（200 OK、`application/json; charset=utf-8`、`Cache-Control: public, max-age=300, stale-while-revalidate=60`、ETag 付与、`If-None-Match` 一致時 304、HEAD 対応）。未知版・manifest 未配置は 404 |
| `GET /releases/{releaseId}/{artifact}` | 成果物 allowlist に含まれるファイルのみ配信（`.wasm` は `application/wasm`、`.json` は `application/json; charset=utf-8`、`.js` は `text/javascript; charset=utf-8`、`.d.ts` は `text/plain; charset=utf-8`。`Cache-Control: public, max-age=31536000, immutable`、ETag 付与、`If-None-Match` 一致時 304、HEAD 対応）。未知版・allowlist 外・パストラバーサル（`..`、`%2e`）・manifest 未配置版はすべて 404 |
| `POST /api/geocode` | `{ "query": "住所" }` → `{ "candidates": [{ "label": "住所", "lat": 35.0, "lon": 139.0 }] }`。0 件ヒット時は 200 で `{ "candidates": [] }`。全応答（成功・エラー問わず）`Cache-Control: no-store` |

### 成果物 allowlist
成果物配信（`/releases/{releaseId}/{artifact}`）で許可されるファイル名は以下の固定リストに限定される:
- `manifest.json`
- `graph.json`
- `snap-index.json`
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
  "minMinutes": 60,
  "maxMinutes": 90,
  "vehicleProfile": "passenger-car-etc"
}
```

Web Worker は `ready`、`result`、`error` を返し、各探索応答に request ID を付ける。WASM 呼出は `search(graph, request, limits)` に相当する純粋な境界とし、JS glue が型とメモリ管理を担う。UI の制限を信用せず Rust 側でも座標の有限性・範囲（`-90.0..=90.0`, `-180.0..=180.0`）、`origin` と `originNodeId` の排他性、分の整数・大小関係、版・車両の一致、`pricingAt` の有効な UTC 時刻を検証する。構造や値の不正は `Err(RoutingError { code: "INVALID_INPUT", message })` を返し、WASM 境界で JSON シリアライズされた `RoutingErrorPayload` として JS 例外をスローする。

### 空間スナップ契約
- スナップ対象: 一般道（`EdgeKind::Local`）に接する（from / to のいずれか）ノードのみ。
- スナップ距離計算: 等距円筒近似（Equirectangular approximation、東京付近 `cos(lat)` 補正）。
- スナップ半径: `SNAP_RADIUS_METERS = 200.0`（200m）。
- 200m 以内に Local ノードが存在しない場合: エラーとせず、`status: "no_candidates"`, `reason: "NO_CONNECTION"`, `candidates: []` を正常返却する。

### 探索結果の status と reason コード
結果は `requestId`、`releaseId`、`status`（`ok` / `no_candidates` / `truncated`）、`reason`、`candidates` を持つ。`reason` は該当時のみ以下のコードをとる:
- `NO_CONNECTION`: スナップ対象の一般道ノードが 200m 以内に存在しない、または一般道から入口・出口への接続経路がない
- `NO_BILLING_PAIR`: 有効な課金ペアが 1 件も存在しない
- `NO_LOOP`: 周回ループが見つからない、または進入不可
- `TIME_WINDOW`: 指定所要時間枠（minMinutes〜maxMinutes）に収まる候補がない
- `NO_HANDOFF`: Maps URL 長が 2,048 文字を超過したため候補が除外された
- `SEARCH_LIMIT`: 探索状態数・候補数の上限に達した（`status: "truncated"`）

### 候補の必須フィールド

| フィールド | 型 | 内容 |
| --- | --- | --- |
| `id`, `releaseId` | `string` | 条件と経路列から安定生成する検索内 ID と版。利用履歴として保存しない |
| `origin` | `LatLng \| null` | 入力座標（`{ lat, lon }`）。`originNodeId` 指定時は `null` |
| `snappedOrigin` | `SnappedOrigin` | 接続した一般道ノード（`{ nodeId, lat, lon, distanceMeters }`）。`originNodeId` 指定時はそのノードで `distanceMeters: 0` |
| `entry`, `exit` | `RampInfo` | 入出口情報（`{ edgeId, name }`。`name` は課金ペア公式ランプ名、無ければ `null`） |
| `entryId`, `exitId` | `string` | 入出口エッジ ID（後方互換用） |
| `roadNames` | `string[]` | 首都高部分で通過したエッジ名の重複除去済み順序付きリスト（名前のないエッジはスキップ） |
| `edgeIds` | `string[]` | 順序付き走行エッジ ID 列（access → loop → return） |
| `geometry` | `GeoJsonLineString` | 全経路の GeoJSON LineString（`{ type: "LineString", coordinates: [[lon, lat], ...] }`）。重複端点なし |
| `duration` | `Duration` | `accessSeconds`, `shutokoSeconds`, `returnSeconds`, `baseSeconds`, `bufferSeconds`, `planSeconds` |
| `distanceMeters`, `shutokoDistanceMeters` | `number` | 総距離と首都高部分の距離（メートル） |
| `toll` | `Toll` | `billingPairId`, `chargedSectionCount: 1`, `amountYen`（未確認なら `null`）, `pricingAt`, `effectiveFrom`, `effectiveTo` |
| `loop` | `Loop` | `anchorNodeId`, `edgeIds`, `durationSeconds`, `distanceMeters`, `validated: true` |
| `reasons` | `string[]` | 機械可読推薦理由コード（先頭候補: 料金確定時は `BEST_TIME_PER_YEN`、時間ソート時は `BEST_SHUTOKO_TIME`。全候補共通: `ONE_SECTION_TOLL`） |
| `warnings` | `string[]` | 警告コード（常時付与: `HANDOFF_WAYPOINTS_UNVERIFIED`（#8 実機検証未了）、`STATIC_TRAVEL_TIME`） |
| `handoff` | `Handoff` | Google Maps 引き継ぎ情報（`{ origin, destination, waypoints, mapsUrl, verificationSetVersion }`） |

### 推薦理由コード（`reasons`）一覧
- `BEST_TIME_PER_YEN`: 時間あたり料金効率が最も高い最優先候補（料金確定時）
- `BEST_SHUTOKO_TIME`: 首都高滞在時間が最も長い最優先候補（料金未確定時等の時間ソート時）
- `ONE_SECTION_TOLL`: 1区間料金（最低料金）が適用される周回経路

### 警告コード（`warnings`）一覧
- `HANDOFF_WAYPOINTS_UNVERIFIED`: Google Maps 引き継ぎ経由地選定ルールが暫定であり実機検証未了であることを示す（#8 完了まで常時付与）
- `STATIC_TRAVEL_TIME`: 渋滞・規制を含まない静的制限速度に基づく推定時間であることを示す

## Google マップへの引き継ぎ

公式 Maps URLs を使い、`https://www.google.com/maps/dir/` に `api=1`、`origin`、`destination`（出発地点）、`travelmode=driving`、`waypoints` を設定する。出発ボタンは経路の確認画面を開く意味とし、自動的にナビを開始する保証はしない。

URL は座標のみの固定形式を `format!` で組み立て（区切りは `%7C`）、2,048文字以内にする。モバイルブラウザの上限に合わせ最大3経由地点とする。経由地点が非対応の製品もある。[Google Maps URLs](https://developers.google.com/maps/documentation/urls/get-started)

本線上の座標が別道路や停車地点に解釈される可能性を先行検証する。一周を省略せず入口・主要通過点・出口を最大3点で表現でき、代表端末で確認済みの経路系列だけを初期の公開候補とする。入口と1区間先の出口だけでは周回を省略した短い経路になり得るため、周回の再現を必ず確認する。上限超過時に黙って地点を削除したり、走行中の手動区間切替を要求したりしない。

任意出発地点について外部経路の完全一致を事前保証できないため、検証済み系列でも利用者に最終確認を促す。Google の再計算結果を取得して自動比較する機能は含めない。再現不能な系列を除外すると企画価値を満たせない場合は、公開を進めず連携方式を再設計する。

## 地図・住所検索のデータ利用

OSM のデータライセンスとタイルサーバーの利用条件は別に扱う。地図に OpenStreetMap contributors の帰属と著作権ページへのリンクを表示する。派生グラフの公開時は ODbL に基づく帰属・提供義務を確認し、元データと生成方法を追える形にする。[OpenStreetMap copyright](https://www.openstreetmap.org/copyright)

標準タイルサーバーを本番用の無制限基盤とはみなさない。大量先読み・オフライン取得を行わず、必要な帰属とキャッシュ条件を守る。実際のタイル提供元は利用量・可用性・条件を確認して選ぶ。[OSMF Tile Usage Policy](https://operations.osmfoundation.org/policies/tiles/)

公開 Nominatim は絶対上限1リクエスト/秒で、クライアント側オートコンプリートは禁止されている。本番はプロバイダー契約または自前運用を選定し、この制約を前提としない構成を確定するまで公開しない。[Nominatim Usage Policy](https://operations.osmfoundation.org/policies/nominatim/)

## 料金データの有効期間

`billingPairs` の料金レコードは `effectiveFrom` / `effectiveTo` と車種・支払い方法を持つ。リクエストの `pricingAt` に有効な版で比較し、金額不明や期限外は未確認として扱う。期間は `[effectiveFrom, effectiveTo)`（終了 null は期限なし）とし、重複期間は成果物検証で拒否する。将来出発日時の入力は初期版に含めない。料金期間をまたいだ状態で出発する場合は UI で再検索を案内する。2026-10-01 の料金改定が告知されているため、固定額をコードへ埋め込まず有効期間を切り替える。[首都高の料金改定発表](https://www.shutoko.co.jp/company/press/2026/data/07/31-toll/)

ETC を料金前提とする場合は入口から出口まで同じカードを利用する条件を明示する。[首都高 ETC 利用案内](https://www.shutoko.jp/fee/fee-info/pay_etc/attention/)
