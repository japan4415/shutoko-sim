/// CI が実行する extended limits の非回帰。
///
/// `max_expanded_states` は validation が 1,000,000 まで許すが、その上限値で prepare と
/// search が通る経路を誰も通していなかった（大規模グラフを要求する手動スモークは
/// `LARGE_GRAPH_PATH` が無ければ skip され、CI からは実行されない。Issue #33 で削除した）。
/// ここでは同じ経路を小さな固定入力グラフで 1 度だけ通し、CI が上限値と探索の戻り方を
/// 監視できるようにする。
use shutoko_routing_core::{prepare, search_prepared, Graph, SearchLimits, SearchRequest};

#[test]
fn extended_limits_path_is_covered_without_the_large_graph() {
    let graph: Graph = serde_json::from_reader(std::io::Cursor::new(include_str!(
        "../../../fixtures/synthetic-graph.json"
    )))
    .expect("the synthetic graph must parse through the public reader");
    let limits = SearchLimits {
        max_expanded_states: 1_000_000, // validation が許す最大値
        ..SearchLimits::default()
    };
    let request: SearchRequest =
        serde_json::from_str(include_str!("../../../fixtures/synthetic-request.json"))
            .expect("the synthetic request must parse through the public reader");
    let prepared = prepare(graph, &limits).expect("prepare must accept the maximum limit");
    let result = search_prepared(&prepared, &request).expect("search must succeed");
    assert_eq!(result.status, "ok");
    assert!(
        !result.candidates.is_empty(),
        "the extended-limit search must still return candidates"
    );
    assert!(
        result.expanded_states <= limits.max_expanded_states,
        "expanded states {} exceeded the limit {}",
        result.expanded_states,
        limits.max_expanded_states
    );
}
