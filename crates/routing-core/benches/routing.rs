//! Criterion ベンチマーク: routing-core アルゴリズムのベースライン計測
//!
//! # 計測対象
//! - `search_small_kandabashi`: 小グラフ（8,803 ノード）神田橋起点で `search()` 全体を計測
//! - `search_small_all_pairs`: 小グラフ、8 課金ペアそれぞれのエントリーノードを起点に `search()` を 8 回回す
//! - `snap_small/*`: 小グラフ、座標スナップ経路（スナップ成功・失敗を含む）
//! - `search_large`: 大規模グラフ（環境変数 `BENCH_LARGE_GRAPH_PATH` 指定、未設定なら skip）
//! - `snap_large/*`: 大規模グラフ、座標スナップ経路
//!
//! # 大規模グラフの注意
//! `validate()` にノード 100,000 / エッジ 300,000 のハードコード上限があるため、
//! 23 区全域グラフ（約 602,549 ノード / 1,194,346 エッジ）は現時点で拒否される。
//! その場合はベンチを skip し、理由をコンソールに出力する（エラーにしない）。

use criterion::{criterion_group, criterion_main, Criterion};
use shutoko_routing_core::{search, Graph, LatLng, SearchLimits, SearchRequest};
use std::hint::black_box;
use std::time::Duration;

// 小グラフ（fixtures/generated/graph.json, 8,803 ノード / 9,726 エッジ）をコンパイル時に埋め込む。
// テスト側（tests/real_graph_contract.rs）と同じ手法。
static SMALL_GRAPH_JSON: &str = include_str!("../../../fixtures/generated/graph.json");

fn small_graph() -> Graph {
    serde_json::from_str(SMALL_GRAPH_JSON).expect("小グラフのデシリアライズに失敗")
}

// 8 課金ペアそれぞれのエントリーエッジ（kind=entry）の `from` ノード ID。
// これらはローカル道路上のノードであり、対応する課金ペアへの接続始点として機能する。
//
// | 課金ペア ID                              | from ノード（エントリーエッジの出発点）   |
// |------------------------------------------|-------------------------------------------|
// | bp:c1-inner:daikancho-kasumigaseki       | n:1866081909 （代官町入口付近）           |
// | bp:c1-inner:kasumigaseki-shibakoen       | n:573233927  （霞が関入口付近）           |
// | bp:c1-inner:shibakoen-shiodome           | n:254367256  （芝公園入口付近）           |
// | bp:c1-inner:takaracho-kandabashi         | n:1105125663 （宝町入口付近）             |
// | bp:c1-outer:ginza-shibakoen              | n:835996316  （銀座入口付近）             |
// | bp:c1-outer:kandabashi-takaracho         | n:1070862943 （神田橋入口付近）           |
// | bp:c1-outer:kasumigaseki-daikancho       | n:577255402  （霞が関入口付近・外回り）   |
// | bp:c1-outer:shibakoen-iikura             | n:940044988  （芝公園入口付近・外回り）   |
const PAIR_ENTRY_NODES: &[(&str, &str)] = &[
    ("bp:c1-inner:daikancho-kasumigaseki", "n:1866081909"),
    ("bp:c1-inner:kasumigaseki-shibakoen", "n:573233927"),
    ("bp:c1-inner:shibakoen-shiodome", "n:254367256"),
    ("bp:c1-inner:takaracho-kandabashi", "n:1105125663"),
    ("bp:c1-outer:ginza-shibakoen", "n:835996316"),
    ("bp:c1-outer:kandabashi-takaracho", "n:1070862943"),
    ("bp:c1-outer:kasumigaseki-daikancho", "n:577255402"),
    ("bp:c1-outer:shibakoen-iikura", "n:940044988"),
];

// --- 小グラフ ベンチ ---

/// `search_small_kandabashi`: 小グラフ・神田橋起点で `search()` 全体を計測。
/// 計測対象: validate（インデックス構築）+ Dijkstra（アクセス/リターン）+ ループ列挙 + 候補ランキング。
fn bench_search_small_kandabashi(c: &mut Criterion) {
    let g = small_graph();
    let limits = SearchLimits::default();

    c.bench_function("search_small_kandabashi", |b| {
        b.iter(|| {
            let req = SearchRequest {
                request_id: "bench-kandabashi".into(),
                release_id: "c1-real-v1".into(),
                origin_node_id: Some("n:1070862943".into()),
                origin: None,
                min_minutes: 15,
                max_minutes: 60,
                vehicle_profile: "passenger-car-etc".into(),
                pricing_at: "2026-09-10T00:00:00Z".into(),
            };
            black_box(search(black_box(&g), black_box(&req), black_box(&limits)))
        })
    });
}

/// `search_small_all_pairs`: 小グラフ、8 課金ペアのエントリーノードを起点に
/// それぞれ `search()` を呼ぶ（合計 8 回）。1 イテレーションは 8 回の search() 合計。
fn bench_search_small_all_pairs(c: &mut Criterion) {
    let g = small_graph();
    let limits = SearchLimits::default();

    c.bench_function("search_small_all_pairs", |b| {
        b.iter(|| {
            for (i, (_pair_id, node_id)) in PAIR_ENTRY_NODES.iter().enumerate() {
                let req = SearchRequest {
                    request_id: format!("bench-pair-{i}"),
                    release_id: "c1-real-v1".into(),
                    origin_node_id: Some((*node_id).into()),
                    origin: None,
                    // 広い時間窓でグラフ探索を十分に走らせる
                    min_minutes: 5,
                    max_minutes: 90,
                    vehicle_profile: "passenger-car-etc".into(),
                    pricing_at: "2026-09-10T00:00:00Z".into(),
                };
                let _ = black_box(search(black_box(&g), &req, black_box(&limits)));
            }
        })
    });
}

/// `snap_small`: 座標スナップ経路の計測。`search()` の origin に LatLng を渡す。
/// - `snap_hit_exact`: 神田橋ノードとほぼ同一座標 → スナップ成功、フル探索を実行
/// - `snap_hit_near` : 神田橋から約 10m の座標  → スナップ成功、フル探索を実行
/// - `snap_miss_far` : グラフから離れた座標（200m 圏外）→ スナップ失敗、即座に no_candidates を返す
fn bench_snap_small(c: &mut Criterion) {
    let g = small_graph();
    let limits = SearchLimits::default();

    // (ラベル, lat, lon)
    let cases: &[(&str, f64, f64)] = &[
        // 神田橋ノード (n:1070862943) の正確な座標
        ("snap_hit_exact", 35.6896727, 139.7644248),
        // 同ノードから約 10m ずれた座標（SNAP_RADIUS_METERS=200m 以内）
        ("snap_hit_near", 35.6897500, 139.7645000),
        // 遠方（皇居北西部）- どのローカルノードからも 200m 超
        ("snap_miss_far", 35.5000000, 139.5000000),
    ];

    let mut group = c.benchmark_group("snap_small");
    for (label, lat, lon) in cases {
        let lat = *lat;
        let lon = *lon;
        group.bench_function(*label, |b| {
            b.iter(|| {
                let req = SearchRequest {
                    request_id: "bench-snap".into(),
                    release_id: "c1-real-v1".into(),
                    origin_node_id: None,
                    origin: Some(LatLng { lat, lon }),
                    min_minutes: 15,
                    max_minutes: 60,
                    vehicle_profile: "passenger-car-etc".into(),
                    pricing_at: "2026-09-10T00:00:00Z".into(),
                };
                black_box(search(black_box(&g), black_box(&req), black_box(&limits)))
            })
        });
    }
    group.finish();
}

// --- 大規模グラフ ベンチ ---

/// 環境変数 `BENCH_LARGE_GRAPH_PATH` からパスを読み、ファイルを読み込む。
/// 未設定またはファイル不在なら `None` を返す（エラーにしない）。
fn load_large_graph() -> Option<(Graph, String)> {
    let path = std::env::var("BENCH_LARGE_GRAPH_PATH").ok()?;
    let json = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "[bench] BENCH_LARGE_GRAPH_PATH={path} の読み込みに失敗しました（スキップ）: {e}"
            );
            return None;
        }
    };
    let g: Graph = match serde_json::from_str(&json) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("[bench] 大規模グラフのデシリアライズに失敗しました（スキップ）: {e}");
            return None;
        }
    };
    Some((g, json))
}

/// 大規模グラフの validate() を事前チェックし、サイズ上限で拒否されるかを確認する。
/// 現行の validate() はノード 100,000 / エッジ 300,000 のハードコード上限を持つ。
/// 拒否された場合はエラーメッセージを出力して `None` を返す。
fn validate_large_graph_or_skip(g: &Graph) -> Option<String> {
    // 最小限のリクエストで validate() を通して上限チェックを実行する。
    // validate() は内部関数なので、search() ごしに上限チェックを行う。
    let first_node_id = g.nodes.first().map(|n| n.id.clone())?;
    let req = SearchRequest {
        request_id: "bench-large-probe".into(),
        release_id: g.release_id.clone(),
        origin_node_id: Some(first_node_id.clone()),
        origin: None,
        min_minutes: 1,
        max_minutes: 5,
        vehicle_profile: g.vehicle_profile.clone(),
        pricing_at: "2026-09-10T00:00:00Z".into(),
    };
    let limits = SearchLimits::default();

    match search(g, &req, &limits) {
        Ok(_) => Some(first_node_id),
        Err(e) => {
            eprintln!(
                "[bench] 大規模グラフは search() で拒否されました（スキップ）: {e}\n\
                 ノード数={}, エッジ数={} — validate() のハードコード上限 \
                 (ノード 100,000 / エッジ 300,000) を超えている可能性があります。",
                g.nodes.len(),
                g.edges.len(),
            );
            None
        }
    }
}

/// `search_large`: 大規模グラフで `search()` 全体を計測。
/// `BENCH_LARGE_GRAPH_PATH` 未設定またはファイル不在 → skip。
/// validate() のサイズ上限で拒否 → skip（理由をコンソールに出力）。
fn bench_search_large(c: &mut Criterion) {
    let Some((g, _json)) = load_large_graph() else {
        eprintln!("[bench] search_large: BENCH_LARGE_GRAPH_PATH が未設定のためスキップします。");
        return;
    };
    let Some(origin_node_id) = validate_large_graph_or_skip(&g) else {
        return; // 拒否された場合 — 理由は validate_large_graph_or_skip 内で出力済み
    };
    let limits = SearchLimits::default();

    let mut group = c.benchmark_group("search_large");
    // 大規模グラフはサンプル数を絞る
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));

    group.bench_function("origin_first_node", |b| {
        b.iter(|| {
            let req = SearchRequest {
                request_id: "bench-large".into(),
                release_id: g.release_id.clone(),
                origin_node_id: Some(origin_node_id.clone()),
                origin: None,
                min_minutes: 15,
                max_minutes: 60,
                vehicle_profile: g.vehicle_profile.clone(),
                pricing_at: "2026-09-10T00:00:00Z".into(),
            };
            black_box(search(black_box(&g), black_box(&req), black_box(&limits)))
        })
    });
    group.finish();
}

/// `snap_large`: 大規模グラフで座標スナップ経路を計測。
/// `BENCH_LARGE_GRAPH_PATH` 未設定またはファイル不在 → skip。
/// validate() のサイズ上限で拒否 → skip（理由をコンソールに出力）。
fn bench_snap_large(c: &mut Criterion) {
    let Some((g, _json)) = load_large_graph() else {
        eprintln!("[bench] snap_large: BENCH_LARGE_GRAPH_PATH が未設定のためスキップします。");
        return;
    };
    if validate_large_graph_or_skip(&g).is_none() {
        return; // 拒否された場合 — 理由は validate_large_graph_or_skip 内で出力済み
    }
    let limits = SearchLimits::default();

    // 大規模グラフのスナップ計測（グラフの中心付近を仮定した座標）
    // スナップ成功: 東京都心付近（大規模グラフがカバーする 23 区の中心部）
    // スナップ失敗: 日本海上など確実にグラフ外の座標
    let cases: &[(&str, f64, f64)] = &[
        ("snap_hit_center", 35.6896727, 139.7644248), // 神田橋付近
        ("snap_miss_far", 35.0000000, 138.0000000),   // 静岡沖（グラフ外）
    ];

    let mut group = c.benchmark_group("snap_large");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));

    for (label, lat, lon) in cases {
        let lat = *lat;
        let lon = *lon;
        group.bench_function(*label, |b| {
            b.iter(|| {
                let req = SearchRequest {
                    request_id: "bench-snap-large".into(),
                    release_id: g.release_id.clone(),
                    origin_node_id: None,
                    origin: Some(LatLng { lat, lon }),
                    min_minutes: 15,
                    max_minutes: 60,
                    vehicle_profile: g.vehicle_profile.clone(),
                    pricing_at: "2026-09-10T00:00:00Z".into(),
                };
                black_box(search(black_box(&g), black_box(&req), black_box(&limits)))
            })
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_search_small_kandabashi,
    bench_search_small_all_pairs,
    bench_snap_small,
    bench_search_large,
    bench_snap_large,
);
criterion_main!(benches);
