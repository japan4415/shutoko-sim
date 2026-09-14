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
///
/// Stores indices into the `graph.nodes` slice instead of string references,
/// enabling the index to be stored in [`crate::PreparedGraph`] without
/// creating a self-referential structure.
#[derive(Default)]
pub struct OwnedSnapGrid {
    /// Each cell stores node indices into `graph.nodes`, sorted by node ID for
    /// deterministic tie-breaking.
    cells: BTreeMap<(i64, i64), Vec<usize>>,
    /// Total number of nodes stored in the grid.
    total_nodes: usize,
}

impl OwnedSnapGrid {
    /// Build from an iterator of local-node indices into `all_nodes`.
    ///
    /// Each cell's entries are sorted by node ID for deterministic tie-breaking,
    /// matching the behaviour of [`SnapGrid`].
    pub fn build(local_node_indices: impl IntoIterator<Item = usize>, all_nodes: &[Node]) -> Self {
        let mut cells: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
        let mut total_nodes = 0usize;
        for idx in local_node_indices {
            let n = &all_nodes[idx];
            cells.entry(cell_key(n.lat, n.lon)).or_default().push(idx);
            total_nodes += 1;
        }
        // Sort each cell by node ID so tie-breaking is deterministic.
        for v in cells.values_mut() {
            v.sort_by(|&a, &b| all_nodes[a].id.cmp(&all_nodes[b].id));
        }
        Self { cells, total_nodes }
    }

    /// Return up to `k` nearest nodes sorted by distance (ascending).
    ///
    /// Tie-breaking: when two nodes are at exactly equal distance, the node
    /// with the **lexicographically smaller ID** comes first — identical to
    /// the contract of [`Self::nearest`].
    ///
    /// Unlike [`Self::nearest`], there is **no radius cap**: all nodes in the
    /// graph are candidates regardless of distance.  If the graph contains
    /// fewer than `k` nodes the full set is returned.
    ///
    /// # Algorithm
    ///
    /// Rings of cells (Chebyshev distance 0, 1, 2, …) are scanned outward from
    /// the cell that contains `(lat, lon)`.  Expansion stops as soon as:
    ///
    /// * we hold at least `k` candidates **and** the *k*-th closest distance is
    ///   smaller than `r × SNAP_RADIUS_METERS` (a provable lower bound on the
    ///   distance to any node outside the scanned frontier), **or**
    /// * every node in the grid has been collected.
    ///
    /// The lower bound `r × SNAP_RADIUS_METERS` is tight: at the cell boundary
    /// the query point is exactly `r` cell-heights away from the next ring.
    pub fn k_nearest(&self, lat: f64, lon: f64, k: usize, all_nodes: &[Node]) -> Vec<(f64, usize)> {
        if k == 0 || self.total_nodes == 0 {
            return vec![];
        }
        let want = k.min(self.total_nodes);

        let (ci, cj) = cell_key(lat, lon);
        let mut candidates: Vec<(f64, usize)> = Vec::new();

        // Expand rings outward.  Ring r contains all cells at Chebyshev
        // distance exactly r from (ci, cj).
        for r in 0i64.. {
            // Collect every node from cells newly reached at ring r.
            for di in -r..=r {
                for dj in -r..=r {
                    // Skip interior cells — they were processed in earlier rings.
                    if di.abs() != r && dj.abs() != r {
                        continue;
                    }
                    if let Some(idxs) = self.cells.get(&(ci + di, cj + dj)) {
                        for &idx in idxs {
                            let n = &all_nodes[idx];
                            let d = distance_meters(lat, lon, n.lat, n.lon);
                            candidates.push((d, idx));
                        }
                    }
                }
            }

            // All nodes in the grid are already in candidates — done.
            if candidates.len() >= self.total_nodes {
                break;
            }

            // We have enough candidates: check if any ring-(r+1) node could
            // beat the current k-th best.
            //
            // Lower bound on the minimum distance from (lat, lon) to any cell
            // at Chebyshev distance r+1:
            //   r × SNAP_RADIUS_METERS
            // (proof: the query is inside its center cell; the near edge of
            // ring r+1 is at least r cell-heights away in both lat and lon.)
            if candidates.len() >= want {
                candidates.sort_by(|a, b| {
                    a.0.partial_cmp(&b.0)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| all_nodes[a.1].id.cmp(&all_nodes[b.1].id))
                });
                let kth_dist = candidates[want - 1].0;
                if kth_dist < (r as f64) * SNAP_RADIUS_METERS {
                    break;
                }
            }
        }

        // Final sort and truncate (no-op when the loop already sorted).
        candidates.sort_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| all_nodes[a.1].id.cmp(&all_nodes[b.1].id))
        });
        candidates.truncate(k);
        candidates
    }

    /// Find the nearest local node within [`SNAP_RADIUS_METERS`].
    ///
    /// Returns `Some((distance_meters, node_index))` or `None`.
    /// Tie-breaking: lexicographically smaller node ID wins, identical to
    /// Tie-breaking: lexicographically smaller node ID wins.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: &str, lat: f64, lon: f64) -> Node {
        Node {
            id: id.to_string(),
            lat,
            lon,
        }
    }

    fn build_grid(nodes: &[Node]) -> OwnedSnapGrid {
        OwnedSnapGrid::build(0..nodes.len(), nodes)
    }

    // -----------------------------------------------------------------------
    // Test 1: k items returned in strict distance-ascending order.
    // -----------------------------------------------------------------------
    #[test]
    fn k_nearest_returns_k_items_in_distance_order() {
        let q_lat = 35.0_f64;
        let q_lon = 139.0_f64;
        // Five nodes placed half-a-cell-height apart, all within the 3×3 window.
        let step = cell_size_lat() * 0.4;
        let nodes: Vec<Node> = (0..5)
            .map(|i| make_node(&format!("n{i}"), q_lat + step * (i as f64 + 1.0), q_lon))
            .collect();
        let grid = build_grid(&nodes);

        let result = grid.k_nearest(q_lat, q_lon, 3, &nodes);

        assert_eq!(result.len(), 3, "must return exactly k=3 items");
        for w in result.windows(2) {
            assert!(w[0].0 <= w[1].0, "distances must be non-decreasing");
        }
        // Nearest three are n0, n1, n2 (indices 0, 1, 2).
        assert_eq!(result[0].1, 0);
        assert_eq!(result[1].1, 1);
        assert_eq!(result[2].1, 2);
    }

    // -----------------------------------------------------------------------
    // Test 2: when the graph has fewer nodes than k, all are returned.
    // -----------------------------------------------------------------------
    #[test]
    fn k_nearest_returns_all_when_fewer_than_k_nodes() {
        let q_lat = 35.0_f64;
        let q_lon = 139.0_f64;
        let nodes = vec![
            make_node("a", q_lat + 0.001, q_lon),
            make_node("b", q_lat - 0.001, q_lon),
            make_node("c", q_lat, q_lon + 0.001),
        ];
        let grid = build_grid(&nodes);

        let result = grid.k_nearest(q_lat, q_lon, 5, &nodes);
        assert_eq!(
            result.len(),
            3,
            "all 3 nodes returned when k=5 > node count"
        );
    }

    // -----------------------------------------------------------------------
    // Test 3: equal-distance tie-breaking by lex-smaller node ID, including
    //         nodes that span different cells.
    // -----------------------------------------------------------------------
    #[test]
    fn k_nearest_same_distance_tie_breaking_node_id_order() {
        // Nodes placed symmetrically north/south: distance formula gives an
        // identical value for both (dx=0, |dy| identical).
        // d > cell_size_lat ensures the two land in different cells, covering
        // cross-cell tie-breaking.
        let q_lat = 35.0_f64;
        let q_lon = 139.0_f64;
        let d = cell_size_lat() * 1.5;

        // "z_far" > "a_far" lexicographically; "a_far" must come first.
        let nodes = vec![
            make_node("z_far", q_lat + d, q_lon), // index 0
            make_node("a_far", q_lat - d, q_lon), // index 1
        ];
        let grid = build_grid(&nodes);
        let result = grid.k_nearest(q_lat, q_lon, 2, &nodes);

        assert_eq!(result.len(), 2);
        // Distances are equal (floating-point symmetric placement).
        let diff = (result[0].0 - result[1].0).abs();
        assert!(diff < 1e-9, "distances should be exactly equal: {result:?}");
        // "a_far" (index 1) must precede "z_far" (index 0).
        assert_eq!(result[0].1, 1, "a_far should be first");
        assert_eq!(result[1].1, 0, "z_far should be second");
    }

    // -----------------------------------------------------------------------
    // Test 4: ring-expansion fires when the only node lies outside the 3×3
    //         initial window (> 1 cell-height from the query cell).
    // -----------------------------------------------------------------------
    #[test]
    fn k_nearest_expands_beyond_3x3_when_node_is_outside_window() {
        let q_lat = 35.0_f64;
        let q_lon = 139.0_f64;
        // Place node ≥ 4 cell-heights north: Chebyshev distance from centre
        // cell is guaranteed to be ≥ 4 regardless of intra-cell offset.
        let far_lat = q_lat + cell_size_lat() * 4.5;

        let nodes = vec![make_node("far", far_lat, q_lon)];
        let grid = build_grid(&nodes);

        // nearest() must NOT find it — it is well beyond SNAP_RADIUS_METERS.
        assert!(
            grid.nearest(q_lat, q_lon, &nodes).is_none(),
            "nearest() must not reach a node that is > 4 cell-heights away"
        );
        // k_nearest must find it via ring expansion.
        let result = grid.k_nearest(q_lat, q_lon, 1, &nodes);
        assert_eq!(result.len(), 1, "k_nearest must find the far node");
        assert_eq!(result[0].1, 0, "far node is index 0");
        // Sanity: distance is substantially larger than SNAP_RADIUS_METERS.
        assert!(
            result[0].0 > SNAP_RADIUS_METERS,
            "far node distance must exceed SNAP_RADIUS_METERS"
        );
    }

    // -----------------------------------------------------------------------
    // Test 5: k_nearest(.., 1) head matches nearest() for an in-radius node.
    // -----------------------------------------------------------------------
    #[test]
    fn k_nearest_1_head_matches_nearest_within_snap_radius() {
        let q_lat = 35.0_f64;
        let q_lon = 139.0_f64;
        // Node ~50 m north — well within the 200 m snap radius.
        let near_lat = q_lat + 50.0 / 111_320.0;

        let nodes = vec![make_node("n", near_lat, q_lon)];
        let grid = build_grid(&nodes);

        let kn = grid.k_nearest(q_lat, q_lon, 1, &nodes);
        let sn = grid.nearest(q_lat, q_lon, &nodes);

        let (sn_dist, sn_idx) = sn.expect("nearest() must find the 50 m node");
        assert_eq!(kn.len(), 1);
        let (kn_dist, kn_idx) = kn[0];
        assert_eq!(kn_idx, sn_idx, "both must identify the same node");
        assert!((kn_dist - sn_dist).abs() < 1e-9, "distances must be equal");
    }
}
