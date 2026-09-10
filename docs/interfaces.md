# データ・インターフェース設計

以下は完成版に向けた契約案。初期 Rust/WASM コアは座標入力や外部連携を含まないため、現時点で呼べる契約は [Rust / WASM 開発](wasm-development.md)を参照する。実データ生成パイプラインの仕様と手順については [実データ生成パイプライン](data-pipeline.md) を参照。JSON の座標オブジェクトは `lat` / `lon`、GeoJSON の座標配列は `[経度, 緯度]`、距離は m、時間は秒、時刻は UTC の ISO 8601 を使う。

## 公開成果物

マニフェスト（`manifest.json`）は `schemaVersion`、`releaseId`、`engineVersion`、`graphVersion`、`builtAt`、`sourceDate`、`coverage`、`vehicleProfile`、`timeModelVersion`、`billingPairsVersion`、`attribution`（`© OpenStreetMap contributors`）、`odblLicenseUrl`、`unverifiedSections`、`artifacts` を持つ。`artifacts` は生成成果物（`graph.json`、`snap-index.json` 等）それぞれの相対パス、SHA-256、バイト数を持つ。`builtAt` は再現性を担保するため外部から与えられた固定値を用いる。`coverage` は対応領域（`area`）と検証済み入出口一覧（`verifiedEntries`、`verifiedExits`）。対応領域内でも経路接続の存在は別途確認する。

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
| `GET /releases/{releaseId}/manifest.json` | 許可された公開版のマニフェスト |
| `GET /releases/{releaseId}/{artifact}` | マニフェスト記載の成果物のみ。未知版・ファイルは404 |
| `POST /api/geocode` | `{ "query": "住所" }` → `{ "candidates": [{ "label": "住所", "lat": 35.0, "lon": 139.0 }] }` |

住所検索は本文4KiB以下、query は空白を除いて1〜200文字、応答は最大5候補。確定検索に限定し、ブラウザと API 応答は `Cache-Control: no-store` とする。プロバイダーが要求する内部キャッシュの扱いは選定時に確定し、保存方針と矛盾するサービスは採用しない。400は入力不備、429は制限超過、502は上流失敗、504は上流5秒タイムアウト。エラー本文は `{ "error": { "code": "GEOCODER_UNAVAILABLE", "retryable": true } }` の形とし、検索語や上流の本文は含めない。

初期の住所検索制限案は送信元ごとに毎分10回に加え、サービス全体でプロバイダー契約の上限以下に制限する。具体値は提供元選定時に更新する。公開住所検索 API の無制限な代理にはしない。

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

Web Worker は `ready`、`result`、`error` を返し、各探索応答に request ID を付ける。WASM 呼出は `search(graph, request, limits)` に相当する純粋な境界とし、JS glue が型とメモリ管理を担う。UI の制限を信用せず Rust 側でも座標の有限性・範囲、分の整数・大小関係、版・車両の一致、`pricingAt` の有効な UTC 時刻を検証する。UI は検索開始時刻を一度取得して `pricingAt` として渡し、探索中の時計の変化を参照しない。

結果は `requestId`、`releaseId`、`status`（`ok` / `no_candidates` / `truncated`）、`reason`、`candidates` を持つ。`reason` は該当時のみ `TIME_WINDOW` / `NO_CONNECTION` / `NO_HANDOFF` / `NO_BILLING_PAIR` / `NO_LOOP` / `SEARCH_LIMIT`。複数の除外要因がある場合は件数内訳を別に持ち、経路が存在しないと断言しない。ロード失敗等は `error` メッセージの `code` で返す。

候補の必須フィールド:

| フィールド | 内容 |
| --- | --- |
| `id`, `releaseId` | 条件と経路列から安定生成する検索内 ID と版。利用履歴として保存しない |
| `origin`, `snappedOrigin` | 入力座標と接続した一般道上の点 |
| `entry`, `exit`, `roadNames` | 入出口 ID・名称と主な通過路線 |
| `edgeIds`, `geometry` | 順序付き走行エッジ、全経路の GeoJSON LineString |
| `duration` | `accessSeconds`, `shutokoSeconds`, `returnSeconds`, `baseSeconds`, `bufferSeconds`, `planSeconds` |
| `distanceMeters`, `shutokoDistanceMeters` | 総距離と首都高部分 |
| `toll` | `billingPairId`, `chargedSectionCount: 1`, `amountYen`（未確認なら null）, `basis`, `verifiedAt`, `pricingAt`, `effectiveFrom`, `effectiveTo`。実走行距離で算出しない |
| `loop` | 基準点、一周部分の順序付きエッジ、周回距離・時間、成立検証結果 |
| `reasons`, `warnings` | 推薦理由と推定条件 |
| `handoff` | 出発・帰着点、順序付き最大3経由地点、検証セットの版 |

## Google マップへの引き継ぎ

公式 Maps URLs を使い、`https://www.google.com/maps/dir/` に `api=1`、`origin`、`destination`（出発地点）、`travelmode=driving`、`waypoints` を設定する。出発ボタンは経路の確認画面を開く意味とし、自動的にナビを開始する保証はしない。

URL は標準の URL ビルダーでエンコードし、2,048文字以内にする。モバイルブラウザの上限に合わせ最大3経由地点とする。経由地点が非対応の製品もある。[Google Maps URLs](https://developers.google.com/maps/documentation/urls/get-started)

本線上の座標が別道路や停車地点に解釈される可能性を先行検証する。一周を省略せず入口・主要通過点・出口を最大3点で表現でき、代表端末で確認済みの経路系列だけを初期の公開候補とする。入口と1区間先の出口だけでは周回を省略した短い経路になり得るため、周回の再現を必ず確認する。上限超過時に黙って地点を削除したり、走行中の手動区間切替を要求したりしない。

任意出発地点について外部経路の完全一致を事前保証できないため、検証済み系列でも利用者に最終確認を促す。Google の再計算結果を取得して自動比較する機能は含めない。再現不能な系列を除外すると企画価値を満たせない場合は、公開を進めず連携方式を再設計する。

## 地図・住所検索のデータ利用

OSM のデータライセンスとタイルサーバーの利用条件は別に扱う。地図に OpenStreetMap contributors の帰属と著作権ページへのリンクを表示する。派生グラフの公開時は ODbL に基づく帰属・提供義務を確認し、元データと生成方法を追える形にする。[OpenStreetMap copyright](https://www.openstreetmap.org/copyright)

標準タイルサーバーを本番用の無制限基盤とはみなさない。大量先読み・オフライン取得を行わず、必要な帰属とキャッシュ条件を守る。実際のタイル提供元は利用量・可用性・条件を確認して選ぶ。[OSMF Tile Usage Policy](https://operations.osmfoundation.org/policies/tiles/)

公開 Nominatim は絶対上限1リクエスト/秒で、クライアント側オートコンプリートは禁止されている。本番はプロバイダー契約または自前運用を選定し、この制約を前提としない構成を確定するまで公開しない。[Nominatim Usage Policy](https://operations.osmfoundation.org/policies/nominatim/)

## 料金データの有効期間

`billingPairs` の料金レコードは `effectiveFrom` / `effectiveTo` と車種・支払い方法を持つ。リクエストの `pricingAt` に有効な版で比較し、金額不明や期限外は未確認として扱う。期間は `[effectiveFrom, effectiveTo)`（終了 null は期限なし）とし、重複期間は成果物検証で拒否する。将来出発日時の入力は初期版に含めない。料金期間をまたいだ状態で出発する場合は UI で再検索を案内する。2026-10-01 の料金改定が告知されているため、固定額をコードへ埋め込まず有効期間を切り替える。[首都高の料金改定発表](https://www.shutoko.co.jp/company/press/2026/data/07/31-toll/)

ETC を料金前提とする場合は入口から出口まで同じカードを利用する条件を明示する。[首都高 ETC 利用案内](https://www.shutoko.jp/fee/fee-info/pay_etc/attention/)
