//! Criterion ベンチマーク: routing-core アルゴリズムのベースライン計測
//!
//! # 計測対象
//! - `search_small_kandabashi`: 小グラフ（8,803 ノード）神田橋起点で `search()` 全体を計測
//! - `search_small_all_pairs`: 小グラフ、8 課金ペアそれぞれのエントリーノードを起点に `search()` を 8 回回す
//! - `snap_small/*`: 小グラフ、座標スナップ経路（スナップ成功・失敗を含む）
//! - `prepared_small/*`: 小グラフ、prepare() を 1 回 + search_prepared() を N 回計測
//!   - `prepared_small/prepare_only`: prepare() のコスト（インデックス構築）
//!   - `prepared_small/search_prepared_kandabashi`: search_prepared() のコスト（探索のみ）
//!   - `prepared_small/search_prepared_all_pairs`: search_prepared() × 8 課金ペア
//! - `search_large`: 大規模グラフ（環境変数 `BENCH_LARGE_GRAPH_PATH` 指定、未設定なら skip）
//! - `snap_large/*`: 大規模グラフ、座標スナップ経路
//! - `prepared_large/*`: 大規模グラフ、prepare() + search_prepared() 分離計測

use criterion::{criterion_group, criterion_main, Criterion};
use shutoko_routing_core::{prepare, search, Graph, LatLng, SearchLimits, SearchRequest};
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

// --- 小グラフ PreparedGraph ベンチ ---

/// `prepared_small`: 小グラフで prepare() + search_prepared() を分離して計測。
/// - `prepare_only`: グラフのロードとインデックス構築コスト
/// - `search_prepared_kandabashi`: prepare 済みグラフへの探索コスト（神田橋起点）
/// - `search_prepared_all_pairs`: prepare 済みグラフへの探索 × 8 課金ペア
fn bench_prepared_small(c: &mut Criterion) {
    let g = small_graph();
    let limits = SearchLimits::default();

    let mut group = c.benchmark_group("prepared_small");

    // prepare() のみを計測（インデックス構築コスト）
    group.bench_function("prepare_only", |b| {
        b.iter(|| black_box(prepare(black_box(g.clone()), black_box(&limits)).unwrap()))
    });

    // search_prepared() を計測（prepare は 1 回だけ実行）
    let pg = prepare(g.clone(), &limits).expect("prepare に失敗");

    group.bench_function("search_prepared_kandabashi", |b| {
        b.iter(|| {
            let req = SearchRequest {
                request_id: "bench-prepared-kandabashi".into(),
                release_id: "c1-real-v1".into(),
                origin_node_id: Some("n:1070862943".into()),
                origin: None,
                min_minutes: 15,
                max_minutes: 60,
                vehicle_profile: "passenger-car-etc".into(),
                pricing_at: "2026-09-10T00:00:00Z".into(),
            };
            black_box(
                shutoko_routing_core::search_prepared(black_box(&pg), black_box(&req)).unwrap(),
            )
        })
    });

    group.bench_function("search_prepared_all_pairs", |b| {
        b.iter(|| {
            for (i, (_pair_id, node_id)) in PAIR_ENTRY_NODES.iter().enumerate() {
                let req = SearchRequest {
                    request_id: format!("bench-prepared-pair-{i}"),
                    release_id: "c1-real-v1".into(),
                    origin_node_id: Some((*node_id).into()),
                    origin: None,
                    min_minutes: 5,
                    max_minutes: 90,
                    vehicle_profile: "passenger-car-etc".into(),
                    pricing_at: "2026-09-10T00:00:00Z".into(),
                };
                let _ = black_box(
                    shutoko_routing_core::search_prepared(black_box(&pg), black_box(&req)).unwrap(),
                );
            }
        })
    });

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

/// 大規模グラフが prepare() で受け入れられるか確認し、第 1 ノード ID を返す。
/// 拒否された場合はエラーメッセージを出力して `None` を返す。
fn prepare_large_graph_or_skip(g: &Graph) -> Option<String> {
    let first_node_id = g.nodes.first().map(|n| n.id.clone())?;
    let limits = SearchLimits::default();
    match prepare(g.clone(), &limits) {
        Ok(_) => Some(first_node_id),
        Err(e) => {
            eprintln!(
                "[bench] 大規模グラフは prepare() で拒否されました（スキップ）: {e}\n\
                 ノード数={}, エッジ数={}",
                g.nodes.len(),
                g.edges.len(),
            );
            None
        }
    }
}

/// `search_large`: 大規模グラフで `search()` 全体（prepare 込み）を計測。
/// `BENCH_LARGE_GRAPH_PATH` 未設定またはファイル不在 → skip。
fn bench_search_large(c: &mut Criterion) {
    let Some((g, _json)) = load_large_graph() else {
        eprintln!("[bench] search_large: BENCH_LARGE_GRAPH_PATH が未設定のためスキップします。");
        return;
    };
    let Some(origin_node_id) = prepare_large_graph_or_skip(&g) else {
        return;
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
fn bench_snap_large(c: &mut Criterion) {
    let Some((g, _json)) = load_large_graph() else {
        eprintln!("[bench] snap_large: BENCH_LARGE_GRAPH_PATH が未設定のためスキップします。");
        return;
    };
    if prepare_large_graph_or_skip(&g).is_none() {
        return;
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

/// `prepared_large`: 大規模グラフで prepare() と search_prepared() を分離して計測。
/// - `prepare_only`: インデックス構築コスト（1 回の計測で大規模グラフを丸ごと処理）
/// - `search_prepared_origin_first`: **失敗探索**（グラフ先頭ノード起点、status=no_candidates/NO_CONNECTION）のコスト
/// - `search_prepared_origin_first_success`: **成功探索**（max_expanded_states=1_000_000 で候補が返る 8 ペア中先頭ペアのエントリーノード起点）
/// - `search_prepared_snap_hit`: 座標スナップ成功経路の探索コスト
/// - `search_prepared_snap_miss`: 座標スナップ失敗（即 return）のコスト
fn bench_prepared_large(c: &mut Criterion) {
    let Some((g, _json)) = load_large_graph() else {
        eprintln!("[bench] prepared_large: BENCH_LARGE_GRAPH_PATH が未設定のためスキップします。");
        return;
    };
    let Some(origin_node_id) = prepare_large_graph_or_skip(&g) else {
        return;
    };
    let limits = SearchLimits::default();

    let mut group = c.benchmark_group("prepared_large");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));

    // prepare() のみを計測（インデックス構築、グラフ clone 込み）
    group.bench_function("prepare_only", |b| {
        b.iter(|| black_box(prepare(black_box(g.clone()), black_box(&limits)).unwrap()))
    });

    // 以降は prepare を 1 回だけ実行し search_prepared() のコストだけを計測
    let pg = prepare(g.clone(), &limits).expect("prepare に失敗");

    // NOTE: このベンチはグラフ先頭ノード（通常は周辺部ノード）を起点とするため
    // status=no_candidates / reason=NO_CONNECTION を返す失敗探索を計測している。
    // 「候補が返る成功探索」のコストは search_prepared_origin_first_success を参照。
    group.bench_function("search_prepared_origin_first", |b| {
        b.iter(|| {
            let req = SearchRequest {
                request_id: "bench-prepared-large".into(),
                release_id: pg.graph().release_id.clone(),
                origin_node_id: Some(origin_node_id.clone()),
                origin: None,
                min_minutes: 15,
                max_minutes: 60,
                vehicle_profile: pg.graph().vehicle_profile.clone(),
                pricing_at: "2026-09-10T00:00:00Z".into(),
            };
            black_box(shutoko_routing_core::search_prepared(
                black_box(&pg),
                black_box(&req),
            ))
        })
    });

    group.bench_function("search_prepared_snap_hit", |b| {
        b.iter(|| {
            let req = SearchRequest {
                request_id: "bench-prepared-snap-hit".into(),
                release_id: pg.graph().release_id.clone(),
                origin_node_id: None,
                origin: Some(LatLng {
                    lat: 35.6896727,
                    lon: 139.7644248,
                }),
                min_minutes: 15,
                max_minutes: 60,
                vehicle_profile: pg.graph().vehicle_profile.clone(),
                pricing_at: "2026-09-10T00:00:00Z".into(),
            };
            black_box(shutoko_routing_core::search_prepared(
                black_box(&pg),
                black_box(&req),
            ))
        })
    });

    group.bench_function("search_prepared_snap_miss", |b| {
        b.iter(|| {
            let req = SearchRequest {
                request_id: "bench-prepared-snap-miss".into(),
                release_id: pg.graph().release_id.clone(),
                origin_node_id: None,
                origin: Some(LatLng {
                    lat: 35.0000000,
                    lon: 138.0000000,
                }),
                min_minutes: 15,
                max_minutes: 60,
                vehicle_profile: pg.graph().vehicle_profile.clone(),
                pricing_at: "2026-09-10T00:00:00Z".into(),
            };
            black_box(shutoko_routing_core::search_prepared(
                black_box(&pg),
                black_box(&req),
            ))
        })
    });

    // 成功探索ベンチ: max_expanded_states=1_000_000 に引き上げて候補が返る探索を計測する。
    // 起点は bp:c1-outer:kasumigaseki-daikancho のエントリーノード (n:577255402)。
    // large_graph_smoke_search_extended_limits で status=ok (expanded≈856k) が確認済み。
    // このノードが大規模グラフに存在しない場合は「no node」エラーになりベンチをスキップする。
    {
        let success_origin = "n:577255402";
        let limits_ext = SearchLimits {
            max_expanded_states: 1_000_000,
            ..SearchLimits::default()
        };
        match prepare(g.clone(), &limits_ext) {
            Ok(pg_ext) => {
                let release_id = pg_ext.graph().release_id.clone();
                let vehicle_profile = pg_ext.graph().vehicle_profile.clone();
                // このベンチは 1 回あたり数秒かかるため sample_size を最小にする
                group.sample_size(10);
                group.measurement_time(Duration::from_secs(60));
                group.bench_function("search_prepared_origin_first_success", |b| {
                    b.iter(|| {
                        let req = SearchRequest {
                            request_id: "bench-prepared-large-success".into(),
                            release_id: release_id.clone(),
                            origin_node_id: Some(success_origin.into()),
                            origin: None,
                            min_minutes: 15,
                            max_minutes: 60,
                            vehicle_profile: vehicle_profile.clone(),
                            pricing_at: "2026-09-10T00:00:00Z".into(),
                        };
                        let result = shutoko_routing_core::search_prepared(
                            black_box(&pg_ext),
                            black_box(&req),
                        );
                        // 成功しなかった場合にベンチ実行時に気づけるよう status を出力する
                        if let Ok(ref r) = result {
                            if r.status != "ok" {
                                eprintln!(
                                    "[bench] search_prepared_origin_first_success: \
                                     status={} (expected ok) expanded_states={}",
                                    r.status, r.expanded_states
                                );
                            }
                        }
                        black_box(result)
                    })
                });
            }
            Err(e) => {
                eprintln!(
                    "[bench] search_prepared_origin_first_success: \
                     extended prepare() failed (スキップ): {e}"
                );
            }
        }
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_search_small_kandabashi,
    bench_search_small_all_pairs,
    bench_snap_small,
    bench_prepared_small,
    bench_search_large,
    bench_snap_large,
    bench_prepared_large,
);
criterion_main!(benches);
