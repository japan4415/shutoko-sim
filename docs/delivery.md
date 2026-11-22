# 実装・検証計画

## 公開版とローカル生成物を区別する

公開中の [all-real-v5 マニフェスト](https://shutoko-sim-workers.raiden000discord.workers.dev/releases/all-real-v5/manifest.json) は graph schema 4。現在のローカル生成手順は `all-real-v5` を既定 ID として残しているが、出力は graph schema 5。名前が同じでも、以下の2つを同じ成果物として扱わない。

| 確認対象 | `releaseId` | graph schema | `graph.json` の SHA-256 |
| --- | --- | --- | --- |
| 公開配信 | `all-real-v5` | 4 | `e245c7a8dabefe0cef6b4fb2e88d036f2bdd9936242975806990b46f2713fbd7` |
| リポジトリの [ローカル生成物](../fixtures/generated/manifest.json) | `all-real-v5` | 5 | `09f3e6871be81397ae6933191d9a8f541ff2bdc22bdd70d658e3689a83d09855` |

両グラフとも課金ペアは14件（`legacyRing` は検証済み11件・未検証1件、`radialReturn` は検証済み2件）。これらはデータ上の検証であり、実走後の ETC 請求や Google マップの再現性を示さない。探索結果の比較は[東京駅の実測と最近接入口の契約](routing.md#東京駅の実測と最近接入口の契約)、生成手順は[実データ生成パイプライン](data-pipeline.md)を参照する。

## 新しい成果物を本番へ切り替える手順

以下は公開を承認された担当者の手順。今回の変更では公開済みのファイルを変更せず、R2 への投入、Cloudflare 設定の変更、本番デプロイは実行しない。ユーザー向けの検証項目は[神田橋入口から宝町出口までの手順](#神田橋入口から宝町出口まで実機と実走を別に検証する)を先に読む。

1. **未使用の release ID を割り当てる。** R2 の prefix に既存の成果物がないことも確認する。グラフビルダーの `FULL_RELEASE_IDS`、Web の `KNOWN_RELEASES`、Worker の `ALLOWED_RELEASES`（配信設定とテスト用設定）をそろえ、公開中の `all-real-v5` を切り戻し用に残す。`SHUTOKO_RELEASE_ID` の指定だけでは、ビルダーのフルリリース検証や各読込側の許可が揃わない。
2. **新 ID で別々の作業用出力に2回生成し、バイト比較する。** `scripts/generate-fixtures.sh` は第1引数で出力先を、第7引数または `SHUTOKO_RELEASE_ID` で ID を指定できる。使用予定の WASM と Web をビルドし、5 成果物の SHA-256・バイト数、graph schema、`routeMembershipsSha256`、料金契約、候補の表示と警告をローカルまたは分離したプレビューで確認する。投入用コマンドのバイト照合だけで合格にせず、schema 5 の graph と manifest が一致し、Web Worker が実 WASM で読めることを確認する。同じ成果物で実機検証の結果と未検証範囲を記録する。
3. **承認後に R2 へ投入し、全件を読み戻す。** `workers/scripts/seed-local-r2.mjs` は作業ツリーの `fixtures/generated/` と `dist/wasm/` を読むため、実行前にそこにある manifest が新 ID であり、ローカル検証に使った成果物と同一であることを再確認する。本番投入は固定版の cf CLI を使う（`--remote`）。単一の担当が payload と `engine.json` を先に置き、全件の読み戻しとハッシュ・バイト数の照合に成功してから、最後に `manifest.json` を置いて再照合する。公開済みの ID を再投入しない。
4. **R2 の確認後にだけ Web の既定を切り替える。** main へのマージが配信を起動する構成では、R2 の読み戻し完了までマージしない。切り替え後に公開 URL で manifest、engine、graph、WASM の ID・schema・ハッシュを照合し、代表検索と未検証の引き継ぎ制限を確認する。旧版の URL も読み取り可能であることを確かめる。
5. **異常時は直前の公開版へ戻す。** Web の `DEFAULT_RELEASE_ID` を `all-real-v5`（公開中の schema 4）に戻して再デプロイする。Worker の許可リストや既存 R2 オブジェクトは変更しない。戻した版のハッシュ、読み込み、代表検索を再確認する。

## `all-real-v5` の atomic release runbook

> **当時の公開手順の記録（schema 4）。以下のコマンドは現在の手順として実行しない。** 同じ ID で生成される現在のローカル成果物は schema 5。次の公開は[新しい成果物を本番へ切り替える手順](#新しい成果物を本番へ切り替える手順)を使う。

**当時の手順: 順序は (1) 2026-10 版 PDF の人手レビュー（`all-real-v4` で完了済み・記録済み。v5 は同じ入力を読む） → (2) 同一入力で成果物を再生成して決定性を確認し、`all-real-v5` を R2 へ投入して read-back する（payload と `engine.json` が先、`manifest.json` は最後） → (3) PR を merge する（これで本番デプロイと allowlist・既定 release の切り替えが起きる） → (4) 本番を確認する → (5) 異常があれば `all-real-v4` へ切り戻す。R2 への投入と read-back が終わるまで merge も production deploy もしない。マージの前に allowlist を切り替える工程は無い（切り替えは merge 後の deploy で起きる）。**

`all-real-v4` と `all-real-v3` の runbook も当時の記録として下に残す。

### 1. 2026-10 版 PDF の OD セルと金額の人手レビュー（完了済み）

**このレビューは完了しており、残条件ではない。** 記録は `data/od-tariffs.json` にあり、`documents[shutoko-2026-10-od-fare-table]` が `status=verified` / `reviewedAt=2026-09-25` / `documentSha256=1dd86cf7946deb28ca6e25d57f133f3acf1c4f5e4110d70d00d8055b3ee6d1e5`、`pendingEvidence` は空、`pendingResolution.status` は `completed_2026_10_pdf_review` である。2026-09-26 に 10 OD セル（page 3 の C1 9 セル、page 4 の 2 号目黒→天現寺 1 セル）を再読取し、**10 件すべてが PDF のセルと一致する**ことを確認した。同じ照合で 2025-04 の霞が関→代官町が 12.4km / 570 円ではなく 2.3km / 300 円であることも発見し、evidence・assignment・`verifiedOdPairs`・seed・テストを訂正済みである。写真ではなくページ画像と PDF テキスト層の 2 通りで読んでおり、レビュー方法として `reviewMethods` に記録している。

置き場所は 2 つを区別する。

| 用途 | パス | gitignore |
| --- | --- | --- |
| 作業者のキャッシュ（リポジトリの外） | `/Users/discord4415/Documents/hobby/shutoko-sim-worktrees/phase1-research-cache/` | リポジトリ外なので無関係 |
| worktree 内の作業用コピー | `.cache/official-fare/`（`/.cache/` と `*.pdf` で無視される） | あり |

`data/od-tariffs.json` に記録するのは `documentSha256` と `documents[].cachePath`・ページ・行・列であり、**パスは参考情報**である。PDF とページ画像はどちらも commit しない。

併せて `fixtures/generated/pair-candidates.json` を開き、候補 14 件の内訳が想定どおりかを確認する。内訳は `eligible_for_review` 13 件、`hold` 1 件（`bp:c1-outer:shibakoen-iikura`。public way が `access:conditional` のため）で、`relationCoverage[]` に 26 relation のうち 11 件が pass・15 件が reason code 付きで fail していることが分かる。`hold` 1 件を人手レビューで強制的に昇格させず、理由付きの未検証として残す。導出レポートは seed を変更しないので、`data/billing-pairs-seed.json` と `data/billing-pair-adjacency.json` の更新はレビュー済みの PR で行う。C2 のように relation の run が分断されている路線では、mandatory lap が「M→B の通常長弧」として一意に取れる OD だけが候補になり、弧の上に別の一般出口がある OD は first exit ゲートで落ちる（`docs/c2-route-design.md` 段8・段9）。

### 2. 同一入力で成果物を再生成し、決定性を確認してから R2 へ投入して read-back する

2-1. 決定性を確認する:
   ```bash
   bash scripts/generate-fixtures.sh
   snapshot_dir="$(mktemp -d)"
   SHUTOKO_RELEASE_ID=all-real-v5 bash scripts/generate-fixtures.sh "${snapshot_dir}"
   diff --no-dereference -r "${snapshot_dir}" fixtures/generated
   git diff --exit-code -- fixtures/generated
   ```
   `manifest.json` の `releaseId=all-real-v5`、`graphSchemaVersion=4`、`routePlanVersion=1`、`billingPairsVersion=v3`、`tariffModelVersion=1`、`routeMembershipsSha256`、および 5 成果物（graph / od-tariffs / pair-candidates / ramps / snap-index）の SHA-256 と byteLength を確認する。上の 2 回の生成と一時ディレクトリの差分ゼロ（`diff` と `git diff --exit-code`）で決定性を確認する。3 世代分のバイト一致は `cargo test --release -p shutoko-graph-builder --test release_contract --locked -- --ignored` の `release_artifacts_are_byte_identical_across_three_generations` が担保する。

2-2. WASM と release metadata を用意する:
   ```bash
   bash scripts/build-wasm.sh
   npm --prefix workers ci
   npm --prefix web ci
   npm --prefix workers run typecheck
   npm --prefix workers test
   npm --prefix web run typecheck
   npm --prefix web test
   ```
   `engine.json` の hash は実ファイルから生成し、変更されうる固定 hash をソースコードへ追加しない。

2-3. マージ前に検証できるソースレベルの切替を確認する（deploy はまだ無い）:
   `web/src/worker/artifact-hashes.ts` の `DEFAULT_RELEASE_ID` と `workers/wrangler.toml` の `ALLOWED_RELEASES` に `all-real-v5` が含まれ、`all-real-v4` も許可されたままであることを確認する（`web/test/release-rollback.test.ts` が 2 つの allowlist の一致、既定と公開 fixture の通過、rollback 先の残存を検査する。`DEFAULT_RELEASE_ID` を 1 行戻す rollback commit でもこのテストは green のままになる）。**この時点では本番の Worker はまだ `all-real-v5` を 404 する**（`workers/src/releases.ts` が新しい allowlist を配るまで）ので、実行時の確認は 4 に置く。

2-4. cf CLI で本番 R2 へ投入して read-back する:
   ```bash
   node workers/scripts/seed-local-r2.mjs --remote
   ```
   本番への公開投入は wrangler ではなく cf CLI で行う（Issue #98）。seed script は投入前に `cf --version` を実行して版をログへ出す。cf は `workers/package.json` で `1.0.0-beta.14` に固定してある（npm 運用へ切り替え済み。`npm --prefix workers run` 経由では `node_modules/.bin/cf` が解決され、直接呼び出しでも `CF_BIN` → PATH の順で解決し `npx` フォールバックは無い）。cf の OAuth トークンは `~/Library/Preferences/cloudflare/config/<profile>.json` にあり、`expiration_time` を過ぎると `cf auth whoami` が `authenticated: false` を返す。その場合は `cf auth login`（`--no-browser` で device flow）で再ログインしてから投入する。投入先は未使用の `releases/all-real-v5/` とし、既存の `manifest.json` があれば上書きを拒否する（`cf r2 objects get` が既存 key を取得できた時点で停止し、取得エラーが「key が存在しない」以外なら fail-closed で停止する）。bucket は `--bucket-name shutoko-artifacts` として渡り、object key は `releases/all-real-v5/<artifact>` になる。投入順は manifest の `artifacts[]` 順（graph / od-tariffs / pair-candidates / ramps / snap-index）に WASM 4 点と `engine.json` を先に上げ、各 payload と `engine.json` を `cf r2 objects get`（本文は stdout に流れる）で読み戻して bytes・length・SHA-256 を照合し、**全件成功した後の最後に `manifest.json` を上げて read-back する**。`manifest.json` の投入前は Worker が新 release を公開しない。

   **cf の版**: リポジトリは npm の `cf` パッケージを `1.0.0-beta.14` に固定した（Issue #98 の継続実装）。初回に R2 操作を実測したのは `1.0.0-beta.10`（`/usr/local/bin/cf`、2026-10-11）で、seed のログに出る `cf --version` が package.json の固定と一致することを確認してから投入する。新設定系のスキーマは beta 間で動くため、cf と wrangler（`4.149.0`。cf dev / cf deploy が要求する 4.136.0 以上）は両方とも package.json で固定し、更新は意図的な作業にする。

   **ローカル投入と E2E の dev サーバ（Issue #98 の継続実装で cf dev に移行済み）**: E2E の dev サーバは `cf dev --port 8787`（新設定系の `cloudflare.config.ts` と `wrangler.config.ts` を読む）に切り替えた。`npm --prefix workers run seed:local`（`--remote` 無し）は wrangler `--local` で Miniflare のローカル R2（`.wrangler/state`）へ書き、cf dev は同じ状態を読む（wrangler と cf はローカル状態のパスも形式も同じ）。ローカル投入先のバケット名は cf dev の r2 バインディングと同じ `shutoko-artifacts` である（旧 wrangler dev がバインドしていた preview バケット名は新設定系に存在しない）。seed の wrangler は `WRANGLER_BIN` → PATH の順で解決し `npx` フォールバックは無く、`SHUTOKO_REQUIRE_PINNED_WRANGLER=1` で `workers/package-lock.json` の固定版（4.149.0）と完全一致を要求する。Workers Builds の deploy command は 2026-10-16 に `cf builds workers update` で `cd workers && npx cf deploy` へ切り替えた（`--dry-run` 検証済み）。`wrangler.toml` は workers の vitest-pool-workers だけが読むため残し、新設定との一致を `workers/test/cloudflare-config.test.ts` が検査する。

### 3. PR を merge する

2 の全工程が成功した後に限り PR を merge する。**merge が本番デプロイと、allowlist・既定 release の切り替えを起こす。** `web/src/worker/artifact-hashes.ts` の `DEFAULT_RELEASE_ID` と `workers/wrangler.toml` の `ALLOWED_RELEASES` はこの PR で既に `all-real-v5` を含むため、merge 後の deploy で切り替わる（マージ前に allowlist を切り替える工程は無い）。main への merge/push が Cloudflare Builds の自動 deploy を誘発する構成では、`releases/all-real-v5/manifest.json` の存在と read-back 済みを manual approval / feature gate として確認する。radial の Google Maps handoff は実機検証（Issue #72）が完了するまで `enabled=false` を維持する。

### 4. 本番を確認する

**マージと deploy の後**に、本番の Worker と Web に対して次を確認する。2-3 はマージ前にできるソースレベルの確認、4 はマージと deploy が終わってから実行する実行時確認である。

4-1. 取得と hash / schema 4 の照合: manifest → `engine.json` → `graph.json` → WASM/glue の取得・hash・schema 4 照合、`billingPairsVersion=v3` と `tariffModelVersion=1` の受理を確認する。

4-2. C1 legacy 回帰と 13 件の verified pair: 13 件の `verified_one_section_ahead`（legacy 11 + radial 2）と未確定 1 件（`bp:c1-outer:shibakoen-iikura`、入口ランプは `supportState=unresolved`）が想定どおりであること、2026-10 改定をまたぐ単価（目黒→天現寺 790 円 → 860 円、C1 は 300 円のまま）を確認する。

4-3. 4 地点の探索結果: 期待値は**最近接入口 tier 優先（Issue #57）の実測**であり、`fixtures/representative-locations.json` の値をそのまま使う（UI の既定 15〜60 分は目黒駅の窓と一致しない）。同 fixture は `web/test/integration-wasm.test.ts` と `crates/routing-core/tests/real_graph_contract.rs` が実 WASM / 実 engine で再計算して照合する。

   | 地点 | 最寄りの入口（距離） | 時間窓（fixture） | 候補 | 区分 |
   | --- | --- | --- | --- | --- |
   | 東京駅 | `ramp:c1-inner:takaracho-entry`（746 m） | 15〜60 分 | `bp:c1-inner:takaracho-kandabashi` 1 件 | 料金付き（300 円・改定前後とも）。`time_per_yen`、推薦 1 件 |
   | 目黒駅 | `ramp:2-inbound:meguro-entry`（313 m） | **15〜120 分** | `bp:2-inbound:meguro:c1-outer:tengenji`（推薦）と `bp:2-inbound:meguro:c1-inner:tengenji` の 2 件 | 料金付き（790 円 → 860 円）。`time_per_yen`、推薦は最大効率の 1 件だけ |
   | 銀座 | `ramp:c1-inner:ginza-entry`（511 m） | 15〜60 分 | `bp:c1-inner:ginza-shintomicho` 1 件 | 料金付き（300 円・改定前後とも、区間距離 0.4 km）。`time_per_yen`、推薦 1 件 |
   | 六本木 | `ramp:c1-inner:iikura-entry`（663 m） | 15〜60 分 | `od:ramp:c1-inner:iikura-entry:ramp:c1-outer:iikura-exit` 1 件 | **未価格の topologyOnly**。商品対象外・推薦なし・`shutoko_time` |

   六本木は最寄りの入口が**内回りランプ**（飯倉入口）で、そこからの 1 区間先に検証済みペアが無いため、動的 OD の未価格候補しか返らない。**この地点では基本料金（割引適用前）の表示を検証できない**ので、未価格（金額なし）・商品対象外・推薦なしのまま表示されることを確認するのが正しい期待である。六本木にも料金付き候補が出ることを rollout 前の gate として求めるなら、Issue #57 の最近接 tier 制限か検証済みペアの登録を変更する必要がある（本 PR のスコープ外）。

   銀座は内回り銀座入口 → 新富町出口（`bp:c1-inner:ginza-shintomicho`）が登録済みのため料金付き候補が返る（Issue #34 はアンカーの再設定で解決済み。経緯は `data-pipeline.md` の注記）。3 地点すべてで金額ラベルと帰属表示が出ることを確認する。

### 5. 異常があれば `all-real-v4` へ切り戻す

4 のいずれかが期待どおりでなかった場合は下の「`all-real-v5` の rollback」手順に従う。**実行時に release pointer を切り替える運用はしない。** 戻すのは Web の `DEFAULT_RELEASE_ID` 1 行だけで、Worker の `ALLOWED_RELEASES` は `all-real-v4` を残したままにする（allowlist を狭めない）。radial の公開 handoff は Issue #72 の実機検証が完了するまで `enabled=false` を保つ。

### `all-real-v5` の rollback

`all-real-v5` の公開後に異常を確認した場合は、R2 上の旧 release manifest を上書き・削除せず、Web の `DEFAULT_RELEASE_ID` を `all-real-v4` へ戻す rollback commit を作成して再 deploy する。Worker の `ALLOWED_RELEASES` は `all-real-v4` を残したままにする（allowlist を狭めない）ため、戻すのは Web の既定 1 行である。`web/test/release-rollback.test.ts` のアサーションはすべて rollback 不変に書いてあるので（公開 fixture は新しい版のままであるため「既定 = fixture の releaseId」は rollback 後に成立しない）、**この 1 行だけの変更で web の test は green を保つ**。実行時に release pointer を切り替える運用は行わない。rollback 後も `all-real-v4` の manifest → engine → graph → WASM/glue の hash/schema、C1 legacy 回帰、4 地点の探索結果を再確認し、古い Web/Worker cache が新しい release 参照を保持していないことを確認する。Web の reader は `billingPairsVersion` の v2（`all-real-v3`）と v3（`all-real-v4` / `all-real-v5`）のどちらでも読むため、既定を戻すだけで確実に前の版へ戻る。

## `all-real-v4` の atomic release runbook（当時の記録）

`all-real-v4` は `all-real-v5` より前の公開版である。現在の生成コマンドで過去の公開バイト列を再現できるとは限らない。
手順・検証項目・rollback は v5 と同一で、投入先を `releases/all-real-v4/`、release ID を
`all-real-v4` に読み替える。v4 公開時の実測記録（2026-10 PDF のレビュー、決定性の確認、
代表 4 地点の実測、`all-real-v3` への切り戻し手順）は v4 を公開した commit の本文にある。
`all-real-v4` からさらに `all-real-v3` へ戻す場合は、下の `all-real-v3` の runbook に従う。

## `all-real-v3` の atomic release runbook

> **当時の手順の記録。現在の公開作業には使わない。**

**当時の前提: `all-real-v3` の R2 投入とread-backが完了するまで、v3をmergeしない。manifestは全artifactのread-back後に最後に投入する。**

Issue #66 のコード変更だけでは本番の release は切り替わらない。`all-real-v3` の公開を承認済みの Manager が、次の順序で単一の R2 seed プロセスを実行する。今回の実装作業では R2 への upload、Wrangler deploy、Cloudflare への書き込みを行わない。release artifactの投入完了・read-back・manifestの最終投入がmergeとproduction deployの前提となる。

1. **同一入力で成果物を生成する**:
   ```bash
   bash scripts/generate-fixtures.sh
   git diff --exit-code -- fixtures/generated
   ```
   `graph.json` の schema/version、manifest の `graphSchemaVersion=4`、`routePlanVersion=1`、`billingPairsVersion=v2`、`routeMembershipsSha256`、artifact の SHA-256/byteLength を確認する。生成差分がゼロであることを確認する。
2. **WASM と release metadata を用意する**:
   ```bash
   bash scripts/build-wasm.sh
   npm --prefix workers ci
   npm --prefix web ci
   npm --prefix web run typecheck
   npm --prefix web test
   npm --prefix workers run typecheck
   npm --prefix workers test
   ```
   `engine.json` の hash は実ファイルから生成し、変更されうる固定 hash をソースコードへ追加しない。
3. **未使用の versioned ID に本番 R2 を投入する**:
   ```bash
   node workers/scripts/seed-local-r2.mjs --remote
   ```
   本番投入は cf CLI で行う（手順と前提は上の v5 runbook 2-4 と同じ。投入前に `cf --version` がログへ出る）。seed script は既存の `releases/all-real-v3/manifest.json` を拒否し、manifestの`artifacts[]`順に`graph.json`、`ramps.json`、`snap-index.json`、WASM、glue、型定義、`engine.json`を先に投入する。各 payload と `engine.json` を read-back して bytes、length、SHA-256 を照合し、全件成功した後の最後に `manifest.json` を投入して read-back する。`manifest.json` の投入前は Worker が新 release を公開しない。**この工程とread-backが終わるまでPRをmergeしない。**
4. **公開後の切替を確認する**: Worker と Web の release allowlist が `all-real-v3` を含み、`all-real-v2` も許可されていることを確認し、manifest → engine → graph → WASM/glue の取得・hash・schema 4 照合、build-time device manifestのbinding、C1 legacy 回帰を再実行する。異常時は Worker/Web の参照を `all-real-v2` に戻し、旧 release の manifest を上書き・削除しない。
5. **merge と production deploy を行う**: R2 artifactとmanifestのread-back、以及Web/Workerのrelease検証が成功した後に限り、PRをmergeし、続けてCloudflare WorkersとWebのproduction deployを別Managerが行う。R2のread-back確認前にmergeまたはproduction deployを実行しない。mainへのmerge/pushがCloudflare Buildsの自動deployを誘発する構成では、R2 v3 manifestの存在確認とread-backをmanual approval / feature gateとして確認する。

### Rollback

`all-real-v3` の公開後に異常を確認した場合は、R2の旧releaseを上書き・削除せず、WebとWorkerのrelease source定数を`all-real-v2`へ戻すrollback commitを作成して再deployする。実行時にrelease pointerを切り替える運用は行わない。rollback後も`all-real-v2` のmanifest → engine → graph → WASM/glue のhash/schemaとC1 legacy回帰を再確認し、古いWeb/Worker cacheが新しいrelease参照を保持していないことを確認する。



| 段階 | 実施内容 | 次へ進む条件 |
| --- | --- | --- |
| 0: 企画判断 | 原案の周回・1区間課金を具体化し、帰着、車両、限定範囲を確認 | 未決事項に決定と理由を記録 |
| 1: 成立性検証 | 少数の一周経路と1区間先の入出口ペア、入口アクセス地点の概算アクセス、Google 引き継ぎ、WASM 配信を検証 | 経路再現性・データ利用条件・端末性能に公開を妨げる問題がない |
| 2: データと Rust | グラフ生成、制限処理、探索・評価、再現可能な成果物ビルド | 人工グラフの正しさと代表実経路の検証が通る |
| 3: 一連の体験 | Workers 配信、住所検索、現在地、地図、比較、出発を接続 | 正常・異常の双方でユーザーが復帰できる |
| 4: 限定公開準備 | 実機確認、利用者テスト、監視、版更新と復旧 | 下記公開条件を満たし、対応範囲を明記 |

工数と公開日は段階1の結果から見積もる。先に広域の UI や全道路データを作り込まない。

## 検証項目

| 領域 | 確認すること |
| --- | --- |
| 入力 | 座標異常、時間の上下限・等値・小数、住所候補なし、GPS 拒否 |
| 探索 | 一周成立、1区間先での退出、課金対象と実走行の分離、方向、禁止遷移、帰着、時間余裕、最小時間を満たす迂回、重複排除、候補不足 |
| 料金 | 有効期間の開始・終了境界、同一 pricingAt の再現性、金額不明、重複期間拒否、出発前の期間切替 |
| 打ち切り | 幅・展開数・深さの上限、10秒終了、キャンセル、古い検索応答の破棄 |
| 地図 | 全経路の描画、入出口、候補選択との一致、帰属、タイル障害 |
| 成果物 | WASM の実ロード、版不一致・ハッシュ不一致拒否、新旧版共存、ロールバック |
| 外部サービス | 住所検索のタイムアウト・制限、秘密情報非露出、外部 URL の構成 |
| 操作性 | スマートフォン縦画面、キーボード、読み上げ、色に依存しない候補識別 |
| 情報管理 | 座標・住所・経路 URL がログや解析に残らないこと、送信先説明 |

Google マップは iPhone の Safari と Android の Chrome を対象に、アプリあり・なしの両方で確認する。出発＝帰着、経由地点0〜3、近接した上下線、入口・出口、URL 長上限を含める。OS・ブラウザ・アプリの版、検証日、経路系列、期待と実結果を記録する。対象経路で意図しない退出や入口変更があれば、その系列を除外して再検証する。本検証は部分的に実施済みである。Android Chrome と Google マップアプリ「あり」の組み合わせで、神田橋入口 → C1 外回り → 宝町出口の経路系列において周回が短絡せず維持されることを確認した。Google マップアプリ「なし」（モバイル Web 版）と iOS Safari は未実施であり、経由地点ごとの系列・URL 長上限・OS/ブラウザ/アプリの版・検証日は未記録である。プロトコル全文の充足には残条件の検証を要する。検証記録の詳細は「Google マップ実機引き継ぎの検証記録」を参照する。

## 性能目標と測定

暫定の目標は、成果物取得済みの探索 p95 が2秒以内、初回ロードから候補表示まで p95 が8秒以内。探索のみ10秒で終了する。圧縮転送量10MiB以内、探索ピークメモリ128MiB以内をブラウザ案の初期予算とする。これはプラットフォームの上限ではなく設計上の目標で、未測定である。

段階1で対象の中程度のスマートフォン機種とネットワーク条件を固定し、初回・キャッシュ済みを分け、代表出発地点10件×時間条件3件を各3回測定する。住所検索は別計測し、初回総時間には含める。検索条件、成果物版、転送量、p50/p95、最大メモリ、上限到達率を記録する。予算超過時は範囲縮小を試し、それでも成立しなければ[サーバー実行の代替案](architecture.md)を評価する。

計測手順・計測値の定義・限界・レポート雛形は[性能計測（bench）](bench/README.md)に集約している。2026-09-11 の代理計測（Pixel 5 相当 / Chromium 153 + CDP の回線エミュレーション、代表 30 パターン × cold 3 回・warm 3 回、計 360 試行）では、Fast 4G で探索 p95 222 ms・初回ロード p95 1,999 ms・cold 転送量最大 405.7 KiB、Slow 4G で探索 p95 221 ms・初回ロード p95 6,572 ms・cold 転送量最大 405.7 KiB、10 秒上限到達 0 %、ピークメモリ 9.5 MiB（`performance.memory`、10 MB 量子化）で、**4 目標すべてを満たした**（実測は[代理計測 2026-09-11](bench/2026-09-11-proxy-pixel5-emulation/summary.md)）。ただし代理計測は Chromium 限定で iOS Safari を再現せず、CDP の CPU スロットルは探索が走る Web Worker に効かないため、この探索時間はスロットルなしのデスクトップ CPU の値である。Slow 4G の初回ロードは 8 秒に対して余裕が約 1.4 秒しかなく、逐次取得の RTT が支配している（並列化の候補は bench/README.md を参照）。**実機計測（特に iOS）は未実施**で、これが唯一の正である。全 26 route relation を含む `all-real-v5` の代理計測（2026-10-12、Slow 4G、10 パターン × cold 2 回 = 40 試行）では、成果物 5 件を 1 往復で取得する並列化（Issue #43）の前後で cold の初回ロード p95 が 10,836 ms → **8,691 ms**、10 秒到達率が **15.0 % → 0 %** になった。ただし予算 8,000 ms には 691 ms 届いていない（残差は転送 1.06 MiB ≈ 5.2 秒と wasm コンパイル / Rust 側 JSON parse の CPU。対策の候補は [bench/README.md](bench/README.md) の 6-2/6-3 で、いずれも成果物の版を 上げる判断が要る。実測は [代理計測 2026-10-12](bench/2026-10-12-proxy-pixel5-c2-payload/summary.md)）。この残差に対し、配布 wasm からデバッグ用の `name` セクションを除き（#114、−48,884 B ≈ −0.24 秒）、graph schema 5 でノード ID を差分エンコーディングにして整形を外し（#115、−123,939 B ≈ −0.62 秒）した再計測では、cold 転送量が 1.06 MiB → **882.2 KiB**、初回ロード p95（cold）が **8,691 ms → 7,495 ms** となり予算内に収まった（実測は [代理計測 2026-10-15](bench/2026-10-15-proxy-pixel5-schema5/summary.md)。残余予算 ≈505 ms ≈ 4,400 edge ≈ C1 約 2.6 本分で、これを超える路線追加には分割配信か部分読みの実装が要る）。

## 公開条件

料金付きの商品候補は、周回・入出口の対応・公式料金の根拠を確認できること。参考候補を同じ推薦や順位に入れないこと。対応範囲、データ基準日、時間の前提、料金の確認状況が読め、位置拒否や候補なしから再操作できること。外部ナビは代表端末での引き継ぎが成立して初めて「実機確認済み」とできる。未検証の C1 legacy リンクには注意を表示し、放射線（radial）のリンクは専用ゲートが開くまで無効にする。配信と住所検索の利用条件・費用を確定し、監視・直前版への復旧を確認する。

### 公開条件の充足状況（Issue #16）

条件ごとに、どの実装・テストが担保しているかを記録する。実機や外部設定に依存する項目は、このリポジトリ内のテストだけで合格にしない。

| 条件 | 状態 | 担保・根拠 |
| --- | --- | --- |
| 地図領域の ODbL 帰属表示とライセンスへのリンクが常時見える | 充足 | `web/src/map/tiles.ts` の `OSM_ATTRIBUTION` / 国土地理院帰属（どちらも `openstreetmap.org/copyright` と地理院の利用規約へリンク）。E2E `(6)` がテキストとリンクを実測 |
| フッターへの帰属表示 | 充足 | `web/index.html` の `<footer class="site-footer">` が OSM・国土地理院へのリンク付き帰属を常時表示する。E2E `(75)` が探索前の初期状態で帰属リンク・ODbL 表記・料金/所要時間の前提・免責・対応範囲を検証する |
| HTTP セキュリティヘッダー（CSP / nosniff / HSTS 等） | 充足 | 静的アセットは `web/public/_headers`、Worker 応答は `workers/src/security.ts`。`workers/test/security-headers.test.ts` が両者の一致と全経路のヘッダーを、E2E `(74)` が実測を担う（[インターフェース](interfaces.md) の「セキュリティヘッダー」） |
| CSP 違反なく WASM・タイル・API が動く | 充足（Chromium のみ） | E2E `(74)` がコンソールの CSP 違反ゼロと、ヘッダー付きで読み込んだアプリの実探索成立を確認する。Safari / Firefox は未計測 |
| アプリ由来のログに座標・住所・検索語を残さない | 充足 | Worker の構造化ログはイベント名・status・duration・releaseId・成果物名だけを持つ（クエリ文字列を含めない）。`workers/test/security-headers.test.ts` が検索語・緯度経度・`q=` を含まないことを固定 |
| プラットフォームのアクセスログに座標・検索語が出ないこと | 未確認 | Cloudflare 側のログ設定（Logpush / Workers Logs）はダッシュボードで確認する必要がある |
| 住所検索 API の IP 単位レート制限と月間上限アラート | 一部充足 | `IP_RATE_LIMITER`（10 req/60 s）と `GLOBAL_RATE_LIMITER`（600 req/60 s）は `workers/cloudflare.config.ts` の binding で設定済み・`workers/test/geocode.test.ts` が 429 を検証。月間上限アラートは Cloudflare の通知設定（外部作業） |
| releaseId 切り替えによるロールバック | 手順は充足・実演は未実施 | 切り戻しは `web/src/worker/artifact-hashes.ts` の `DEFAULT_RELEASE_ID` 1 行で、`web/test/release-rollback.test.ts` が allowlist 同期と既定の隣接関係を固定する。本番での切り戻し実演はデプロイ作業後 |
| 外部ナビ（Google マップ）の実機引き継ぎ | 一部のみ・公開条件は未充足 | C1 legacy の旧 Android サンプル1件のみ。対象版での再検証と、放射線の4環境での検証（Issue #72）が必要 |
| 周回後の ETC 実請求 | 未確認 | 公式料金表と周回経路のデータ検証は済んでいるが、実走後の利用明細は別途確認する |
| データ基準日・前提が読める | 充足 | フッターが manifest の `sourceDate`（`YYYY-MM-DD` を検証、不正なら日付行を出さない）と料金・時間の前提を常時表示する。E2E `(75)` が実 worker 経由で `datetime="2026-09-16"` を検証する |
| 商品推薦に未検証ペアを含めない | 充足 | `hold` の芝公園入口 → 飯倉出口を商品推薦に含めない。動的 OD は `topology_only` の参考候補とし、料金根拠がある場合も「参考料金」として商品比較から外す |

## 神田橋入口から宝町出口まで実機と実走を別に検証する

対象は `bp:c1-outer:kandabashi-takaracho`（神田橋入口 → C1 外回りを一周 → 宝町出口）。東京駅の既定検索で出る逆向きの宝町入口 → 神田橋出口とは別のペアなので、対象を ID または入口・出口・方向で照合する。検証には「神田橋」プリセット（緯度 `35.689673`、経度 `139.764425`）と既定の 15〜60 分を使う。出発地点を変更する場合も自宅などの私的な地点は使わない。実機の担当者は下の順に確認し、結果を公開担当者に渡す。

| 確認項目 | 現在わかっていること |
| --- | --- |
| 公式の入口・出口間料金 | 普通車 ETC 基本料金（割引適用前）は **1.7 km / 300 円**。料金距離は入口・出口間の料金計算用で、地図に描く線の長さではない。[料金カタログ](../data/od-tariffs.json)の `assignment:c1-outer:kandabashi-takaracho` を参照。 |
| アプリ内の首都高周回 | 確認した神田橋 → 宝町の表示例は **約 16.1 km**。これは地図に描いたランプ・周回・退出を含む首都高の経路長で、一般道の往復や実測走行距離ではない。ローカルの schema 5 では推定 1,418 秒、余裕 300 秒、計画時間 1,718 秒。実走にかかった時間ではない。 |
| Google マップへの引き継ぎ | 2026年9月に Android Chrome から Google マップアプリへ渡した旧版の1例で、周回維持を確認した。版や当日の条件の記録が足りず、今回の版や他の環境の合格には使えない。 |
| 実走後の ETC 請求 | **未確認**。料金表、リンクを開いた結果、アプリの距離のいずれも、実走後の利用明細の代わりにはならない。 |

### 停車中に Google マップの表示を確認する

1. 検証対象の配信元、`releaseId`、graph schema・SHA-256、Web ビルド識別子を記録する。公開地点・時間条件で検索し、対象ペアの入口、出口、方向、周回距離、公式料金の表示と、実機検証の注意を確認する。2D・3D とも、料金表の距離から推定した料金区間の線を描いていないことも確かめる。
2. Android Chrome と iOS Safari で、Google マップアプリあり／なし（モバイル Web）の4通りを別々に試す。端末の OS、ブラウザ・アプリの版、検証日時とタイムゾーン、利用した経由地点数・URL 長、入口・方向・出口と帰着先の期待／実結果を記録する。実機に届く URL を使い、到達した先が Web かアプリかも確認する。
3. Google マップで入口から C1 外回りの一周を維持し、途中退出・再入場なしで宝町出口に達し、出発地点へ向かう場合だけ、その組み合わせを合格とする。地図が開いただけでは合格にしない。最短経路への短絡、向きや出入口の変更は不合格。未試行、目的の表示先で開けない、挙動を確認できない場合も合格にせず、理由を記録する。検証した版・端末・条件の範囲でのみ結果を報告する。

### 実走できた場合は利用明細を別に照合する

実走は任意。交通規制や案内標識に従い、経路や支払い条件に疑問があれば実施しない。運転中に画面を操作したり記録を取ったりせず、走行の確認と明細の照合は停車後に行う。普通車 ETC、実際の入口・出口、C1 外回りを一周したか、途中退出・再入場の有無、適用された割引の有無、確定後の利用明細の金額を確認する。ルートを変更した、内訳がわからない、明細が未確定、割引のため基本料金と比較できない場合は、公式料金どおりだったとは判定しない。条件が一致しても、確認した1件の観測値として報告する。

記録は「対象版／実機環境／入口・方向・出口の期待と実結果／引き継ぎの判定／実走の有無と ETC 照合の判定／差異の理由」に絞る。個人住所、正確な GPS 履歴・出発時刻、アカウント名、車両番号、ETC カード・車載器・取引の識別番号、未加工の URL・画面・明細画像は、リポジトリや共有記録に載せない。端末と検証の日時は記録しても、走行や支払いの個人記録と結び付けて公開しない。

## Google マップ実機引き継ぎの検証記録

Issue #8（Google マップ引き継ぎの実機成立性検証）に関する記録。検証プロトコルは「検証項目」を参照する。

### 実施済み（2026-09、Android）

| 項目 | 内容 |
| --- | --- |
| 端末 | Android（OS 版は未記録） |
| ブラウザ | Android Chrome（版は未記録） |
| Google マップアプリ | あり |
| 経路系列 | 神田橋入口 → C1 外回り → 宝町出口 |
| 結果 | 周回を維持し、意図しない退出・入口変更なし（短絡なし） |
| 配信元 | `https://shutoko-sim-workers.raiden000discord.workers.dev`（Cloudflare Workers 本番、Version `0062c139-9a5e-4d09-8f4c-3fa04e6bd3f5`） |
| 未記録項目 | 経由地点数、Maps URL 長、検証日、アプリの版 |

### 未実施（残条件）

| 条件 | 状態 |
| --- | --- |
| Android Chrome + Google マップアプリ「なし」（モバイル Web 版） | 未実施 |
| iPhone Safari + アプリ「あり」 | 未検証（端末未所有） |
| iPhone Safari + アプリ「なし」 | 未検証（端末未所有） |
| 経由地点 0〜3 点の系列網羅 | 未実施（本記録は代表1系列のみ） |
| 近接した上下線・入口・出口の各系列 | 未実施 |
| URL 長上限（2,048文字）到達条件 | 未測定 |

残条件の検証が完了するまで、候補の `warnings` に `HANDOFF_WAYPOINTS_UNVERIFIED` を付与し続ける（[インターフェース設計](interfaces.md) 参照）。

### Issue #72: 放射線 split Maps の4環境検証

上のIssue #8記録はC1 legacyの単一URL検証であり、#71で追加した3 leg handoffの証拠とは分ける。リポジトリ内の未検証記録は`data/device-verification-manifest.json`で、現状はschema 4 fixtureのroute plan / release / leg hashへ対応付けた4系列とも`missing`である。これは実機検証済みではなく、gateを閉じるための記録である。実際の放射線release候補では、同じcontractを検証対象の`routePlanId`と`releaseId`へ更新し、loop transfer legのURLとSHA-256を照合する。surface legのoriginを含むURLはbinding hashに含めず、構造と3 legの実測を確認する。`fixtures/device-verification/valid.json`とinvalid fixtureはcontract test用なので、実測値の上書き先にはしない。

| 環境 | client | 必須手順 | 現行状態 |
| --- | --- | --- | --- |
| Android × Web | Chrome | 3 legを順に開き、各legの`expectedRoad` / `expectedDirection`、originへの帰着、意図しない入口・出口変更がないことを確認する | `missing` |
| Android × app | Google Mapsアプリ | 同じ3 legと同一route planをアプリから開き、Webと同じ道路・向き・帰着を確認する | `missing` |
| iOS × Web | Safari | 同じ3 legをSafariで開き、Webと同じ道路・向き・帰着を確認する | `missing` |
| iOS × app | Google Mapsアプリ | 同じ3 legをアプリから開き、Webと同じ道路・向き・帰着を確認する | `missing` |

各系列で以下を手動確認し、manifestのverification recordへ記録する。

1. 端末のOS version、Webならbrowser名とversion、appならapp名とversionを、画面表示のAbout等地から控える。`passed`のrecordへ`unverified`を残さない。
2. `surface_access`、`loop_transfer`、`surface_return`をこの順で手動継続する。各Maps URLの構造・長さ・waypoint数を検査し、loop transfer legのSHA-256がmanifestの値と一致し、画面が`expectedRoad`と`expectedDirection`を満たすことを確認する。短絡、反対方向、誤ったarm、意図しない入口・出口への変更があれば`failed`とする。
3. UTC RFC3339の`verifiedAt`と、それより後の`expiresAt`を記録する。未着手または未検証は`verifiedAt=null`、`expiresAt=null`、`result=missing`とする。期限切れを検出した系列は再検証し、新しい`verifiedAt`と`expiresAt`を記録して古い`passed`を残さない。
4. release gateの判定時刻をUTCで固定し、`prepare` / `search`のリリース設定`deviceVerification.evaluatedAt`へ渡す。検索要求の`pricingAt`はgate判定には使わない。4系列がすべて`passed`で、判定時刻が各`[verifiedAt, expiresAt)`にあり、route plan、release、builder version、loop transfer hashのbindingも一致し、surface legの構造検証も通った場合だけ公開handoffを許可する。1件でもmissing、failed、未開始、期限切れ、manifest欠落・不正、binding不一致なら`enabled=false`、`legUrls=[]`を維持する。
5. C1 legacyについて既存のURL生成、handoff、`HANDOFF_WAYPOINTS_UNVERIFIED`が変わらないことを既存contract testで確認する。

Issue #72のコード実装、release設定、物理端末の4系列検証、公開handoffの有効化は分離して扱う。現時点では明示設定で検証済みmanifestを接続できるが、物理検証と公開handoffの有効化はユーザー作業待ちである。

## 引き継ぎ（このリポジトリの作業範囲外の事項）

コードと fixture の実装は完了しているが、公開を伴う外部操作は別の Manager / ユーザーが行う必要がある。状態を混同しないよう、実装済み・未実施・保留の 3 種を分けて記す。

### 実装済み（このリポジトリで検証している）

- 料金表 v3（`data/od-tariffs.json`、`tariffModelVersion=1`）: 規則 2 期間、evidence 20 件、assignment 10 件、deprecated 2 件。2025-04 / 2026-10 の両版を人手レビュー済みで `pendingEvidence` は空。
- 端点解決 `firstPublicRoadConnection/v1`: 天現寺を 4 way / 16 Edge・hash `bb9114f49d...` で `verified_bound` にし、芝公園入口外回りの `access:conditional` は reason code `CONDITIONAL_ACCESS_RESTRICTION` 付きの `unresolved` として fail-closed にした（`supportState` は `unsupported` ではなく `unresolved`）。
- 課金ペアの自動導出 `billingPairDerivation/v2`: `pair-candidates.json`（候補 11 / eligible 9 / hold 2、relation 11 展開・15 失敗）、seed の自動変更なし。
- ローカルの5成果物と manifest: 決定論的な再生成、route relation coverage、C1 legacy 回帰（神田橋・霞が関）、[代表4地点](../fixtures/representative-locations.json)をテストする。実機での再現性を検証するテストではない。
- Web / Workers の release 許可リスト: `DEFAULT_RELEASE_ID` と `ALLOWED_RELEASES` は `all-real-v5` を含み、`all-real-v4` / `all-real-v3` を残したまま運用する。rollback は Web の既定 1 行だけで成立する。
- 本番 R2 投入（`seed:local -- --remote`）は cf CLI（npm パッケージ `cf` を `1.0.0-beta.14` に固定。`CF_BIN` 上書き可、`npx` フォールバックなし、版は seed ログへ出力）。
- ローカル投入（E2E / `cf dev` が読む Miniflare 状態）は wrangler 照合を維持（4.149.0、`WRANGLER_BIN` 上書き可、`npx` フォールバックなし、`SHUTOKO_REQUIRE_PINNED_WRANGLER=1` で完全一致要求）。E2E の dev サーバは `cf dev` に移行済み（詳細は v5 runbook 2-4）。

### 未実施（外部実施が必要）

- ローカル生成物を公開する場合に、未使用 ID での R2 投入・読み戻しと参照切り替えを行う。この作業では未実施。公開中の `all-real-v5` は上書きしない。
- 神田橋入口 → 宝町出口の対象版での実機検証と、実走後の ETC 利用明細の確認。
- 2026-10 版 PDF の再取得（hash 検証と監査トレースの再現用。`data/od-tariffs.json` のレビュー済み値はすでに固定済みで、10 OD セルは一致済みなので残条件ではない。再取得した PDF の SHA-256 が `documents[shutoko-2026-10-od-fare-table].documentSha256` と一致することだけを確認する）。
- 放射線（radial）3 leg split URL の実機 4 環境検証（Android / iOS × Web / app）。完了まで `data/device-verification-manifest.json` の 4 レコードは `missing` のままで、radial の公開 handoff は `enabled=false` を保つ。

### 保留（根拠 evidence が揃うまで昇格しない）

- `bp:c1-outer:shibakoen-iikura`: 接続一般道 way `40969792` の `access:conditional=no @ (08:00-20:00)`。入口ランプは `supportState=unresolved`（reason code `CONDITIONAL_ACCESS_RESTRICTION`）、導出レポートの entry gate は `Unresolved` / `ENTRY_BINDING_UNRESOLVED`。時間帯モデルが導入されるまで `unverified` のまま商品推薦から外す。
- 銀座入口 → 新富町出口（#34）はアンカーの再設定で解決し、商品ペアに登録済み。六本木の代表検索は同じ意味ではなく、未価格の `topology_only` を返す。
- 未展開の 15 route relation: reason code 付きで記録され、無言でスキップはしない。relation ごとに proof と manifest を要求してから公開を広げる。

## 設計の決定事項と残る検証

| 論点 | 現在の提案 | 判断時点・判断材料 |
| --- | --- | --- |
| 帰着先 | 出発地点へ戻る | 決定済み。一般道の帰路を時間予算に含める |
| コスパの順位 | 1区間分の料金で得られる首都高走行時間 | 段階0、金額確認済み同士の比較と未確認の扱い |
| 課金対象の入出口 | 一周後に1区間先で降りる検証済みペア | 段階1、方向・車両・支払い条件と根拠 |
| 車両 | 普通乗用車・ETC | 段階0、対象利用者と入口制約 |
| 対応範囲 | 検証済みの限定道路網（一般道除外） | 段階1、アクセス時間概算の精度 |
| 実行場所 | ブラウザ Web Worker | 段階1、R2 配信と端末性能 |
| Google 引き継ぎ | 最大3点で表現できる検証済み系列 | 段階1、実機の再現性。成立しなければ連携を再設計 |
| 地図・住所検索 | 地図ライブラリは Leaflet、タイルは国土地理院（GSI）標準地図、住所検索は国土地理院ジオコーディング（Workers プロキシ）に決定 | 段階1で決定済み（#15）。GSI は無償・公共で利用規約が明確。OSM 標準タイルは本番の無制限基盤とみなさず既定にしない |
| 時間モデル・余裕 | 静的区間時間と20%または5分 | 段階2、代表経路との比較。交通予測の保証にしない |
| 探索上限・入力上限 | 探索設計の仮値、最大240分 | 段階2、正しさと性能測定 |
| 月間費用上限 | 未定 | 段階1、想定検索数×配信量と外部 API 単価。予算と警告閾値を設定 |

対応料金ペアの拡充、交通情報、景観などの好み、別地点への到着は、初期版の有用性を確認してから再企画する。

## ドキュメントの保守

原案と異なる要望が出た場合は、人間による原案更新と設計文書の更新を分ける。仕様を変更する PR では対応する企画・要件・インターフェース・検証条件も同時に見直す。README は人間向けの概要と目次を維持し、詳細仕様は docs 以下に置く。
