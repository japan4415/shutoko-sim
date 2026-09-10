//! JSON boundary for the experimental routing core. No DOM or networking.
use wasm_bindgen::prelude::*;

/// Search a prepared graph. Invalid inputs throw a JavaScript error.
#[wasm_bindgen(js_name = search)]
pub fn search_json(
    graph_json: &str,
    request_json: &str,
    limits_json: &str,
) -> Result<String, JsValue> {
    shutoko_routing_core::search_json(graph_json, request_json, limits_json)
        .map_err(|error| JsValue::from_str(&error.to_json_string()))
}
