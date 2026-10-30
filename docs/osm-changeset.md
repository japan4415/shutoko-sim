# relation 4256077（首都高速中央環状線 C2）の上流 changeset 提案

このドキュメントは、`data/osm-relation-repairs.json` にまとめた relation 4256077 の修正を
OpenStreetMap へ提案するまでの手順と、その根拠を記録する。このリポジトリはネットワークへ
接続せず、OSM へのアップロードも行わない。ここに書く作業はすべて人手で行う。

## 1. 提案の内容

relation 4256077 は 115 member すべてが role 空欄で、JCT 連絡路の一部が member に入っている。
graph-builder は role が無い relation に対して、verified_bound な C2 ランプの接続点を錨として
内回り／外回りの分割を推定する（`crates/graph-builder/src/route_membership.rs`）。推定は
幾何だけでは決まらない区間（トンネル内で上下 2 段が重なる大井JCT、link way が折り返す大橋JCT）
を持つため、上流が role を持てば graph 側の推定は不要になる。

提案は 3 種類の編集からなる。

| 編集 | 件数 | 内容 |
| --- | ---: | --- |
| `setMemberRole` | 111 | member way に `Inner directions` / `Outer directions` を付与する（C1 relation 4256008 と同じ書式） |
| `removeMemberWay` | 4 | どちらの carriageway にも辺が無い member（78334075 / 541983769 / 766677827 / 805171062）を削除する |
| `splitMemberWayAtNode` | 2 | 891237687（`n:8283617223`）と 44130240（`n:370270234`）を内部ノードで分割し、member 鎖が継続できるようにする |

`setMemberRole` の role は、公開 release の分割（`fixtures/generated/graph.json` の
`routeMemberships`）を書き出したもので、独立した証拠で再現できることを
`scripts/audit-c2-carriageway-labels.py` が確認する。監査の根拠は次の 3 つ。

1. **公式ランプ方向**: `data/ramp-inventory.json` の方向（公式路線図由来）と
   `data/osm-ramp-bindings.json` の合流／分流点から、verified_bound な C2 ランプ 25 件のうち
   20 件に錨を置く。
2. **有向鎖**: 共有ノードで one-way が連続する member 辺は同じ carriageway である。距離の
   しきい値を使わない厳密な証拠で、2 次元幾何が縮退する区間（大井JCT のトンネル、大橋JCT の
   ループ）をこれで解く。
3. **幾何**: 60 m 以内の同方向ペアは同じ carriageway、逆平行ペアは反対側。鎖が沈黙している
   区間だけに使う。layer が異なる逆平行ペアは「上下 2 段の stacked pair」として別に報告する。

監査は 20 錨 / 2,449 member 辺を分類し、公開 release の分割と一致しない member way を
1 件（768600220、大橋JCT で 2 本の carriageway が接する merge ノード上）だけ報告する。
この 1 件は role 無しだと relation 全体が fail-closed になるため、提案では公開 release の
ラベル（Outer directions）を付けたうえで `humanReviewRequired` に残す。

## 2. 生成物

```
python3 scripts/build-osm-changeset.py --out-dir /tmp/osm-changeset
```

| ファイル | 用途 |
| --- | --- |
| `plan.json` | 適用する編集の機械可読な一覧（role、削除、分割の node 列、member 数の前後） |
| `relation-4256077.osm` | 提案後の relation を OSM XML で表した参照用ファイル（アップロード用ではない） |
| `SPOT-CHECK.md` | 人手チェックリスト（区間、OSM の URL、changeset の comment/source/tags、手順） |
| `changeset-tags.json` | changeset に設定するタグ |

`plan.json` の member 列は `scripts/apply-osm-relation-repairs.py` の適用結果と
機械的に照合される。両者が食い違えばスクリプトは非ゼロで終了するので、レビュー用紙と
検証済みの修正内容がずれたまま出ていくことはない。

`relation-4256077.osm` は `version` を持たない。スナップショット（Overpass の `[out:json]`）に
オブジェクトの version が無いためで、このまま OSM API へ送ると版の不一致で拒否される。
アップロード用の版番号は手順 3 で取得する。

## 3. アップロード手順（人手）

1. **最新の relation を取得する**。`https://api.openstreetmap.org/api/0.6/relation/4256077/full.json`
   を `out meta;` 相当（要素に `version` / `timestamp` が付く形）で取得する。スナップショットの
   取得方法は `scripts/fetch-osm.sh` と同じでよい。
2. **提案がまだ有効か確認する**。relation が既に role を持っていれば、この提案は古い。
   member の増減も確認する（`plan.json` の `membersBefore` と一致するか）。
3. **編集を適用する**。取得したスナップショットに
   `python3 scripts/apply-osm-relation-repairs.py --osm <取得した snapshot> --repairs data/osm-relation-repairs.json --out <編集後>`
   を実行すると、`version` を保ったまま role・削除・分割が適用される。分割で作られる 2 way は
   新しい id を持つので、アップロード時は新規オブジェクト（負の id）として送る。
4. **スポットチェック**。`/tmp/osm-changeset/SPOT-CHECK.md` の 3 区間をエディタ（JOSM 推奨）で
   目視する。
   - 大井JCT（lat 35.608, lon 139.755）: way 331692346（layer 2）が外回り、way 250494182
     （layer 3）が内回りで、上（layer 3）が内回りになっているか。2 次元幾何では分離できない。
   - 王子〜江北（lon 139.732–139.745）: way 44209387 / 44211094 / 780870438 が外回りか。
     飛鳥山トンネルは上下 2 段ではない例外区間。
   - 大橋JCT（lat 35.6518, lon 139.6878）: way 768600220。ここは link way が折り返す区間で、
     鎖の証拠（inner）と公開 release のラベル（outer）が食い違う唯一の member。
5. **アップロードする**。`changeset-tags.json` の comment / source / tags を設定する。
   変更内容の要約を changeset の discussion に書き、`humanReviewRequired` の未解決点
   （768600220 の扱い、削除 4 件を削除でよいか）を明記する。
6. **読み返す**。relation 4256077 を再度取得し、role が `plan.json` と一致することを確認する。
   一致しない場合はリバートせず、差分を `data/osm-relation-repairs.json` の
   `humanReviewRequired` に追記して再提案する。

## 4. 提案を適用した場合の効果

スナップショットのコピーに提案を適用し、同じ入力で graph-builder を回した実測
（`all-real-v5` の入力、relation だけ差し替え。2026-10-30 に再測）:

| 指標 | 現行（role 無し） | 提案適用後 |
| --- | --- | --- |
| relation member | 115 | 113（削除 4・分割 +2） |
| `route:C2:inner` relationMainline | 7 segment / 1,197 辺 | 7 segment / 1,201 辺 |
| `route:C2:outer` relationMainline | 9 segment / 1,252 辺 | 9 segment / 1,264 辺 |
| pair 候補 | 14 件（review 13 / hold 1） | 14 件（review 13 / hold 1） |

内回り・外回りの辺数が数十辺変わるのは、role 展開が分割の推定ではなく relation の記載順を
使うためである。**商品の候補（課金ペアと料金）は変わらない**。pair 候補は件数だけでなく
`pairId` の集合も現行と一致するので、3 ペアの mandatory lap と代表 4 地点の実測は現行の
まま使える。上流が role を持つまでの間、graph 側の推定
（`fill_carriageway_labels_by_pairing`）は現行のまま残す。

### 再現手順

`all-real-v5` の relation 4256077 だけを差し替えて build し、表の値を確かめる。

```sh
python3 scripts/apply-osm-relation-repairs.py --osm fixtures/osm/shutoko-all.json \
    --repairs data/osm-relation-repairs.json --out /tmp/applied-snapshot.json
bash scripts/generate-fixtures.sh /tmp/applied-build /tmp/applied-snapshot.json
python3 - <<'PY'
import json

with open("/tmp/applied-build/graph.json") as f:
    graph = json.load(f)
for membership in graph["routeMemberships"]:
    if membership["routeId"] != "C2":
        continue
    mainline = [s for s in membership["segments"] if s["sourceKind"] == "relationMainline"]
    print(membership["membershipId"], len(mainline), sum(len(s["orderedEdgeIds"]) for s in mainline))

with open("/tmp/applied-build/pair-candidates.json") as f:
    print(json.load(f)["summary"])
PY
```

`route:C2:inner 7 1201`、`route:C2:outer 9 1264`、`candidateTotal 14`（review 13 / hold 1）が
出れば表と一致する。build の出力は `/tmp/applied-build` に書き、`fixtures/generated` は
変更しない。

## 5. 更新のしかた

- role の一覧は手で編集しない。`python3 scripts/derive-osm-relation-repairs.py` が
  公開 release の分割と監査結果から `data/osm-relation-repairs.json` を作り直す
  （`--check` で checked-in の内容が最新かを検査できる）。
- 監査の根拠を変えた場合（錨の置き方、鎖の規則、幾何のしきい値）は、先に
  `scripts/audit-c2-carriageway-labels.py` の出力を確認し、`humanReviewRequired` の文言も
  あわせて更新する。
- 提案の適用結果は `crates/graph-builder` の診断メッセージで確認する。member 鎖が切れて
  いれば `relation 4256077 ... would traverse a way outside the relation after member ...`
  のように、次の修正対象（node・edge・member way）を指すエラーで止まる。
