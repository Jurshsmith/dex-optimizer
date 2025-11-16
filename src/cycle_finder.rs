use crate::csr_graph::CSRGraph;

pub use crate::csr_graph::InputEdge;

const EPS: f64 = 1e-12;

#[derive(Debug, Clone)]
pub struct Cycle {
    /// s -> ... -> s  (length = edge_indexes.len() + 1)
    pub vertices: Vec<usize>,
    /// indices into the `edges` slice, in the *cycle order*
    pub edge_indexes: Vec<usize>,
    /// product of rates along the cycle
    pub profit: f64,
    /// sum of -ln(rate) along the cycle (negative ⇒ profitable)
    pub neg_log_sum: f64,
}

/// Bellman–Ford with a hop cap (no super-source).
/// For each start node, we run exact-hop DP up to `hop_cap`, relaxing in place and
/// reusing buffers (swap) to minimize allocations. A cycle exists at hop `h` iff
/// best_cost[h][start] < 0 (with costs in -ln space).
pub fn find_profitable_cycle(
    n: usize,
    edges: &[(usize, usize, f64)],
    hop_cap: usize,
) -> Option<Cycle> {
    if n == 0 || edges.is_empty() || hop_cap == 0 {
        return None;
    }
    if edges
        .iter()
        .any(|&(u, v, r)| u >= n || v >= n || r <= 0.0 || !r.is_finite())
    {
        return None;
    }

    let graph = CSRGraph::from_edges(n, edges.to_vec());
    find_profitable_cycle_with_graph(&graph, hop_cap)
}

/// Variant accepting a pre-built CSR graph to avoid rebuilding adjacency data on every call.
pub fn find_profitable_cycle_with_graph(graph: &CSRGraph, hop_cap: usize) -> Option<Cycle> {
    let n = graph.node_count();
    if n == 0 || graph.edge_count() == 0 || hop_cap == 0 {
        return None;
    }

    // Try each start node separately (no virtual super-source).
    for start in 0..n {
        // hop 0: only `start` reachable with cost 0; others are ∞
        let mut best_previous = vec![f64::INFINITY; n];
        best_previous[start] = 0.0;

        // Preallocate next-hop buffer and the predecessor-edge buffer (reused each hop).
        let mut best_current = vec![f64::INFINITY; n];
        let mut predecessor_at_hop = vec![None; n];

        // History of per-hop predecessors for backtracking (snapshot per hop).
        // At hop 0 there is no incoming edge.
        let mut predecessors_by_hop: Vec<Vec<Option<usize>>> = Vec::with_capacity(hop_cap + 1);
        predecessors_by_hop.push(vec![None; n]);

        for hop in 1..=hop_cap {
            relax_hop_inplace(
                graph,
                &best_previous,
                &mut best_current,
                &mut predecessor_at_hop,
            );

            // Detect cycle: cost to return to `start` after exactly `hop` hops is negative.
            let cost_to_start = best_current[start];
            if cost_to_start.is_finite() && cost_to_start < -EPS {
                // Reconstruct the cycle of exactly `hop` edges ending at `start`.
                let used_edges = reconstruct_edge_path(
                    hop,
                    start,
                    &predecessors_by_hop,
                    &predecessor_at_hop,
                    graph,
                )?;
                let (vertices, neg_log_sum, profit) = assemble_cycle_metrics(&used_edges, graph)?;

                debug_assert_eq!(vertices.first(), vertices.last());

                return Some(Cycle {
                    vertices,
                    edge_indexes: used_edges,
                    profit,
                    neg_log_sum,
                });
            }

            // Snapshot predecessors for this hop (for backtracking later).
            predecessors_by_hop.push(predecessor_at_hop.clone());

            // Reuse allocations next round:
            // - swap best_current <-> best_previous (so `best_previous` holds the latest),
            // - reset current buffers in place.
            std::mem::swap(&mut best_previous, &mut best_current);
            best_current.fill(f64::INFINITY);
            predecessor_at_hop.fill(None);
        }
    }

    None
}

/// In-place relaxation from hop-1 → hop.
/// - `best_previous` is read-only (costs for exactly h-1 hops).
/// - `best_current` is overwritten with costs for exactly h hops.
/// - `predecessor_at_hop[v]` becomes the winning predecessor edge index for (hop, v), or None.
#[inline]
fn relax_hop_inplace(
    graph: &CSRGraph,
    best_previous: &[f64],
    best_current: &mut [f64],
    predecessor_at_hop: &mut [Option<usize>],
) {
    // assume caller already did: best_current.fill(∞), predecessor_at_hop.fill(None)
    for (u, &du) in best_previous.iter().enumerate() {
        if !du.is_finite() {
            continue;
        }
        for (ei, v, w) in graph.neighbors(u) {
            let d = du + w;
            if d < best_current[v] {
                best_current[v] = d;
                predecessor_at_hop[v] = Some(ei); // predecessor (argmin) for (hop, v)
            }
        }
    }
}

/// Backtrack exactly `hop` steps along predecessor edges to recover the edge sequence
/// (in forward order) that ends at `end_node` after `hop` hops.
#[inline]
fn reconstruct_edge_path(
    mut hop: usize,
    mut end_node: usize, // here `end_node` is the start node (cycle end/start)
    predecessors_by_hop: &[Vec<Option<usize>>],
    predecessor_at_hop: &[Option<usize>],
    graph: &CSRGraph,
) -> Option<Vec<usize>> {
    let mut used = Vec::with_capacity(hop);
    while hop > 0 {
        let ei = predecessor_edge_at_hop(predecessors_by_hop, predecessor_at_hop, hop, end_node)?;
        used.push(ei);
        end_node = graph.edge_src(ei);
        hop -= 1;
    }
    used.reverse();
    Some(used)
}

/// Access the single predecessor edge chosen for (hop, node).
#[inline]
fn predecessor_edge_at_hop(
    predecessors_by_hop: &[Vec<Option<usize>>],
    predecessor_at_hop: &[Option<usize>],
    hop: usize,
    node: usize,
) -> Option<usize> {
    // While computing hop H, historical hops are in predecessors_by_hop[0..=H-1],
    // and the current hop’s choices are in predecessor_at_hop.
    if hop == predecessors_by_hop.len() {
        predecessor_at_hop[node]
    } else {
        predecessors_by_hop[hop][node]
    }
}

/// Convert used edge IDs into vertex ring and metrics (neg_log_sum, profit).
#[inline]
fn assemble_cycle_metrics(
    used_edges: &[usize],
    graph: &CSRGraph,
) -> Option<(Vec<usize>, f64, f64)> {
    if used_edges.is_empty() {
        return None;
    }
    let mut vertices = Vec::with_capacity(used_edges.len() + 1);
    vertices.push(graph.edge_src(used_edges[0]));

    let mut neg_log_sum = 0.0_f64;
    for &ei in used_edges {
        let v2 = graph.edge_dst(ei);
        vertices.push(v2);
        neg_log_sum += graph.weights_in_neglog[ei];
    }
    let profit = (-neg_log_sum).exp();
    if !profit.is_finite() {
        return None;
    }
    Some((vertices, neg_log_sum, profit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_profitable_3_cycle_with_hop_cap() {
        let n = 3;
        let edges = [
            (0, 1, 1.02),
            (1, 2, 1.02),
            (2, 0, 0.98), // product ≈ 1.0196 > 1
        ];

        let cyc = find_profitable_cycle(n, &edges, 8).expect("should find");
        assert!(cyc.profit > 1.0);
        assert!(cyc.neg_log_sum < 0.0);
        assert_eq!(cyc.vertices.first(), cyc.vertices.last());
    }

    #[test]
    fn respects_hop_cap() {
        // Only a 4-hop profitable cycle exists; cap=3 should fail, cap=4 should pass.
        let n = 4;
        let edges = [
            (0, 1, 1.01),
            (1, 2, 1.01),
            (2, 3, 1.01),
            (3, 0, 1.01), // product ≈ 1.0406
        ];

        assert!(find_profitable_cycle(n, &edges, 3).is_none());
        assert!(find_profitable_cycle(n, &edges, 4).is_some());
    }

    #[test]
    fn returns_none_when_no_arbitrage() {
        let n = 3;
        let edges = [
            (0, 1, 1.01),
            (1, 2, 0.99),
            (2, 0, 1.0), // product = 0.9999
        ];

        assert!(find_profitable_cycle(n, &edges, 8).is_none());
    }

    #[test]
    fn returns_none_when_hop_cap_zero() {
        let n = 2;
        let edges = [(0, 1, 1.1), (1, 0, 1.1)];

        assert!(find_profitable_cycle(n, &edges, 0).is_none());
    }

    #[test]
    fn returns_none_on_invalid_edge_data() {
        let n = 3;
        let out_of_bounds = [(0, 3, 1.1)];
        assert!(find_profitable_cycle(n, &out_of_bounds, 5).is_none());

        let non_positive_rate = [(0, 1, 0.0), (1, 0, 1.1)];
        assert!(find_profitable_cycle(n, &non_positive_rate, 5).is_none());

        let nan_rate = [(0, 1, f64::NAN), (1, 0, 1.01)];
        assert!(find_profitable_cycle(n, &nan_rate, 5).is_none());
    }

    #[test]
    fn accepts_hop_cap_exceeding_node_count() {
        let n = 3;
        let edges = [(0, 1, 1.02), (1, 2, 1.02), (2, 0, 0.98)];

        let cyc = find_profitable_cycle(n, &edges, n + 10)
            .expect("hop cap larger than node count should still find cycle");
        assert!(cyc.profit > 1.0);
        assert_eq!(cyc.vertices.first(), cyc.vertices.last());
    }

    #[test]
    fn finds_shortest_profitable_cycle_when_multiple_exist() {
        let n = 4;
        let edges = [
            (0, 1, 1.1),
            (1, 0, 1.05),
            (0, 2, 1.03),
            (2, 3, 1.03),
            (3, 0, 1.03),
        ];
        let cyc = find_profitable_cycle(n, &edges, 4).expect("should find");
        assert_eq!(cyc.vertices.len(), 3); // 2-hop cycle: x -> y -> x
        assert!(cyc.profit > 1.0);
    }

    #[test]
    fn finds_profitable_cycle_from_dataset_slice() {
        let n = 101; // accommodates the highest token index in the slice
        let edges = [(83, 40, 1.011538), (40, 22, 1.006524), (22, 83, 1.00674)];

        let cyc =
            find_profitable_cycle(n, &edges, 4).expect("dataset slice should contain arbitrage");
        assert_eq!(cyc.vertices.len(), 4);
        assert!(cyc.profit > 1.0);
        assert!(cyc.neg_log_sum < 0.0);
    }

    #[test]
    fn finds_alt_dataset_cycle() {
        let n = 101;
        let edges = [(90, 71, 1.003291), (71, 88, 1.008421), (88, 90, 1.013105)];

        let cyc =
            find_profitable_cycle(n, &edges, 4).expect("dataset slice should contain arbitrage");
        assert_eq!(cyc.vertices.len(), 4);
        assert!(cyc.profit > 1.0);
    }

    #[test]
    fn finds_profitable_cycle_with_prebuilt_graph() {
        let n = 3;
        let edges = vec![(0, 1, 1.02), (1, 2, 1.02), (2, 0, 0.98)];
        let graph = CSRGraph::from_edges(n, edges);

        let cyc = find_profitable_cycle_with_graph(&graph, 8).expect("should find");
        assert!(cyc.profit > 1.0);
    }
}
