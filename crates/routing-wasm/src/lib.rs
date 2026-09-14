//! JSON boundary for the experimental routing core. No DOM or networking.
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsError;

/// An opaque handle wrapping a [`PreparedGraph`] for use from JavaScript.
///
/// Obtain one with [`prepare`] and pass it to [`search_prepared`] for fast
/// repeated route search without rebuilding the index each time.
#[wasm_bindgen]
pub struct WasmPreparedGraph(shutoko_routing_core::PreparedGraph);

/// Build a prepared graph from JSON strings.
///
/// This is the expensive step (index construction); call it once per graph
/// and reuse the returned handle across many [`search_prepared`] calls.
///
/// @param graphJson  Serialized Graph JSON string
/// @param limitsJson Serialized SearchLimits JSON string (use "{}" for defaults)
/// @returns An opaque WasmPreparedGraph handle
/// @throws JavaScript Error with JSON-serialized RoutingErrorPayload on invalid input
#[wasm_bindgen]
pub fn prepare(graph_json: &str, limits_json: &str) -> Result<WasmPreparedGraph, JsError> {
    shutoko_routing_core::prepare_json(graph_json, limits_json)
        .map(WasmPreparedGraph)
        .map_err(|e| JsError::new(&e.to_json_string()))
}

/// Execute a route search on an already-prepared graph.
///
/// Skips all index rebuilding; only validates the request and runs the
/// search algorithm.
///
/// @param pg          A WasmPreparedGraph handle obtained from [`prepare`]
/// @param requestJson Serialized SearchRequest JSON string
/// @returns Serialized SearchResult JSON string
/// @throws JavaScript Error with JSON-serialized RoutingErrorPayload on invalid input
#[wasm_bindgen(js_name = searchPrepared)]
pub fn search_prepared(pg: &WasmPreparedGraph, request_json: &str) -> Result<String, JsError> {
    shutoko_routing_core::search_prepared_json(&pg.0, request_json)
        .map_err(|e| JsError::new(&e.to_json_string()))
}

/// Search a graph by parsing all three JSON arguments on every call.
///
/// Convenience API that preserves the original single-call signature.
/// For repeated searches on the same graph, prefer [`prepare`] +
/// [`search_prepared`] to avoid rebuilding the index each time.
///
/// @param graphJson   Serialized Graph JSON string
/// @param requestJson Serialized SearchRequest JSON string
/// @param limitsJson  Serialized SearchLimits JSON string (use "{}" for defaults)
/// @returns Serialized SearchResult JSON string
/// @throws JavaScript Error with JSON-serialized RoutingErrorPayload on invalid input
#[wasm_bindgen(js_name = search)]
pub fn search_json(
    graph_json: &str,
    request_json: &str,
    limits_json: &str,
) -> Result<String, JsError> {
    shutoko_routing_core::search_json(graph_json, request_json, limits_json)
        .map_err(|error| JsError::new(&error.to_json_string()))
}
