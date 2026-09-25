// 製品の料金スコープ。値の正本は crates/routing-core/src/tariff.rs の
// PRODUCT_* 定数と crates/routing-wasm/wasm-contract.json の productTariff。
// pipeline.ts（Worker 側の実行時検証）と ui/model.ts（画面表示）が同じ値を
// 共有し、エンジンも画面も別の車種・割引込みの金額へ変わらないようにする。

/** 製品の車種（普通車）。 */
export const PRODUCT_VEHICLE_CLASS = "ordinary";

/** 製品の支払方法（ETC）。 */
export const PRODUCT_PAYMENT_METHOD = "etc";

/** 製品の料金種別。割引適用前の基本料金だけを扱う。 */
export const PRODUCT_FARE_BASIS = "base_toll_excluding_discounts";

/** 画面に出す料金ラベル。UI はこの文言をそのまま表示する。 */
export const PRODUCT_FARE_LABEL = "普通車ETC基本料金（割引適用前）";

/** 距離基準の公定料金のみを可信とする出所。 */
export const OFFICIAL_DISTANCE_RULE_SOURCE = "official_distance_rule";

/** od-tariffs.json v3 の tariffModelVersion。 */
export const TARIFF_MODEL_VERSION = 1;
