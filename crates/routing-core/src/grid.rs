//! Uniform-grid spatial index for fast nearest-local-node snap.
//!
//! Replaces the O(n) linear scan over `local_nodes` with O(1) amortised lookup
//! by partitioning nodes into fixed-size cells and querying only the 3×3
//! neighbourhood around the query point.
//!
//! **Tie-breaking contract**: when two nodes are at exactly equal distance from
//! a query coordinate, the node with the **lexicographically smaller ID** is
//! returned — identical to the original `BTreeSet` iteration order.
use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::{distance_meters, Node, SNAP_RADIUS_METERS};

/// Approximate degrees of latitude per metre (constant everywhere).
const LAT_DEG_PER_M: f64 = 1.0 / 111_320.0;

/// Conservative degrees of longitude per metre. Using cos 0.70 (≈ arccos 45.6°N)
/// keeps cells wide enough for latitudes up to 45.6 °N, covering all of Japan's
/// metropolitan areas. A larger cell (lower cos) means more candidates per query
/// but never misses a within-radius node.
const LON_DEG_PER_M: f64 = 1.0 / (111_320.0 * 0.70);

#[inline]
fn cell_size_lat() -> f64 {
    SNAP_RADIUS_METERS * LAT_DEG_PER_M
}

#[inline]
fn cell_size_lon() -> f64 {
    SNAP_RADIUS_METERS * LON_DEG_PER_M
}

/// Grid cell key: (row, col) in integer cell coordinates.
#[inline]
fn cell_key(lat: f64, lon: f64) -> (i64, i64) {
    let row = (lat / cell_size_lat()).floor() as i64;
    let col = (lon / cell_size_lon()).floor() as i64;
    (row, col)
}

/// Uniform-grid spatial index over local-road nodes.
#[derive(Default)]
pub struct SnapGrid<'a> {
    /// Each cell stores its node IDs in **lexicographic order** (preserved from
    /// the `BTreeSet` iteration used during construction).
    cells: BTreeMap<(i64, i64), Vec<&'a str>>,
}

/// Owned version of `SnapGrid` — no lifetime parameter.
///
/// Stores indices into the `graph.nodes` slice instead of string references,
/// enabling the index to be stored in [`crate::PreparedGraph`] without
/// creating a self-referential structure.
#[derive(Default)]
pub struct OwnedSnapGrid {
    /// Each cell stores node indices into `graph.nodes`, sorted by node ID for
    /// deterministic tie-breaking (identical to [`SnapGrid`] behaviour).
    cells: BTreeMap<(i64, i64), Vec<usize>>,
}

impl<'a> SnapGrid<'a> {
    /// Build from the set of local-node IDs and the full node coordinate map.
    ///
    /// `local_nodes` is a `BTreeSet`, so iteration is already in lexicographic
    /// order. Each cell therefore receives its nodes in lex order automatically,
    /// with no additional sort needed.
    pub fn build(local_nodes: &BTreeSet<&'a str>, nodes: &BTreeMap<&'a str, &'a Node>) -> Self {
        let mut cells: BTreeMap<(i64, i64), Vec<&'a str>> = BTreeMap::new();
        for &id in local_nodes {
            let n = nodes[id];
            cells.entry(cell_key(n.lat, n.lon)).or_default().push(id);
        }
        Self { cells }
    }

    /// Find the nearest local node within [`SNAP_RADIUS_METERS`] of `(lat, lon)`.
    ///
    /// Returns `Some((distance_meters, node))` or `None` if no node qualifies.
    ///
    /// Tie-breaking: among nodes at equal distance, the one with the
    /// lexicographically smaller node ID is returned, matching the `BTreeSet`
    /// linear-scan behaviour.
    pub fn nearest(
        &self,
        lat: f64,
        lon: f64,
        nodes: &BTreeMap<&'a str, &'a Node>,
    ) -> Option<(f64, &'a Node)> {
        let (ci, cj) = cell_key(lat, lon);

        // Track (distance, node_id, node_ref) for the best candidate so far.
        let mut best: Option<(f64, &'a str, &'a Node)> = None;

        for di in -1i64..=1 {
            for dj in -1i64..=1 {
                let key = (ci + di, cj + dj);
                let Some(ids) = self.cells.get(&key) else {
                    continue;
                };
                // `ids` are in lex order within the cell.
                for &id in ids {
                    let n = nodes[id];
                    let d = distance_meters(lat, lon, n.lat, n.lon);
                    let update = match best {
                        None => true,
                        // Update if strictly closer, or equally close but lex-smaller ID.
                        // This replicates the BTreeSet `d < bd` rule: the first node
                        // encountered in lex order at the minimum distance wins.
                        Some((bd, bid, _)) => d < bd || (d == bd && id < bid),
                    };
                    if update {
                        best = Some((d, id, n));
                    }
                }
            }
        }

        best.filter(|(d, _, _)| *d <= SNAP_RADIUS_METERS)
            .map(|(d, _, n)| (d, n))
    }
}

impl OwnedSnapGrid {
    /// Build from an iterator of local-node indices into `all_nodes`.
    ///
    /// Each cell's entries are sorted by node ID for deterministic tie-breaking,
    /// matching the behaviour of [`SnapGrid`].
    pub fn build(local_node_indices: impl IntoIterator<Item = usize>, all_nodes: &[Node]) -> Self {
        let mut cells: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
        for idx in local_node_indices {
            let n = &all_nodes[idx];
            cells.entry(cell_key(n.lat, n.lon)).or_default().push(idx);
        }
        // Sort each cell by node ID so tie-breaking is deterministic.
        for v in cells.values_mut() {
            v.sort_by(|&a, &b| all_nodes[a].id.cmp(&all_nodes[b].id));
        }
        Self { cells }
    }

    /// Find the nearest local node within [`SNAP_RADIUS_METERS`].
    ///
    /// Returns `Some((distance_meters, node_index))` or `None`.
    /// Tie-breaking: lexicographically smaller node ID wins, identical to
    /// the borrowed [`SnapGrid::nearest`].
    pub fn nearest(&self, lat: f64, lon: f64, all_nodes: &[Node]) -> Option<(f64, usize)> {
        let (ci, cj) = cell_key(lat, lon);
        // Track (distance, index) of the best candidate.
        let mut best: Option<(f64, usize)> = None;

        for di in -1i64..=1 {
            for dj in -1i64..=1 {
                let key = (ci + di, cj + dj);
                let Some(idxs) = self.cells.get(&key) else {
                    continue;
                };
                // `idxs` are already in lex order within each cell.
                for &idx in idxs {
                    let n = &all_nodes[idx];
                    let d = distance_meters(lat, lon, n.lat, n.lon);
                    let update = match best {
                        None => true,
                        Some((bd, bidx)) => d < bd || (d == bd && n.id < all_nodes[bidx].id),
                    };
                    if update {
                        best = Some((d, idx));
                    }
                }
            }
        }

        best.filter(|(d, _)| *d <= SNAP_RADIUS_METERS)
    }
}
