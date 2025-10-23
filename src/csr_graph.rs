use std::fmt;

/// Edge list item: (from, to, rate)
pub type InputEdge = (usize, usize, f64);

/// Compact sparse-row adjacency encoding used across the crate.
///
/// Owns the edge list and keeps two helper arrays:
/// - `edge_offsets` marks, for every node, the start/end positions of its outgoing edges inside
///   `edge_indices`.
/// - `edge_indices` stores the indices of edges (relative to the original slice) laid out
///   contiguously per node.
///
/// `weights_in_neglog` caches the `-ln(rate)` value per edge which is the working cost for
/// arbitrage detection.
#[derive(Clone)]
pub struct CSRGraph {
    edge_offsets: Vec<usize>,
    edge_indices: Vec<usize>,
    edges: Vec<InputEdge>,
    pub weights_in_neglog: Vec<f64>,
    node_count: usize,
}

impl fmt::Debug for CSRGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CSRGraph")
            .field("node_count", &self.node_count)
            .field("edge_count", &self.edge_count())
            .finish()
    }
}

#[derive(Debug)]
pub enum UpdateError {
    IndexOutOfBounds(usize),
    InvalidRate(f64),
}

impl CSRGraph {
    /// Build a CSR graph from owned `edges` with `(from, to, rate)` triples.
    pub fn from_edges(node_count: usize, edges: Vec<InputEdge>) -> Self {
        let mut outgoing_edges_count_by_node = vec![0usize; node_count];
        for (node, _, _) in &edges {
            outgoing_edges_count_by_node[*node] += 1;
        }

        let mut edge_offsets = Vec::with_capacity(node_count + 1);
        edge_offsets.push(0);
        for (i, outgoing_edge_count) in outgoing_edges_count_by_node.iter().enumerate() {
            let previous_offset = edge_offsets[i];
            edge_offsets.push(previous_offset + outgoing_edge_count);
        }

        // Index edge indices to preserve the order from the row offsets
        let mut edge_indices = vec![0usize; edges.len()];
        let mut weights_in_neglog = Vec::with_capacity(edges.len());

        let mut offsets_so_far = vec![0usize; node_count];
        for (edge_index, (from_node, _to, rate)) in edges.iter().enumerate() {
            let slot = edge_offsets[*from_node] + offsets_so_far[*from_node];
            edge_indices[slot] = edge_index;
            offsets_so_far[*from_node] += 1;
            debug_assert!(
                edge_offsets[*from_node] + offsets_so_far[*from_node]
                    <= edge_offsets[*from_node + 1]
            );

            weights_in_neglog.push(-rate.ln());
        }

        Self {
            edge_offsets,
            edge_indices,
            edges,
            weights_in_neglog,
            node_count,
        }
    }

    #[inline]
    pub fn node_count(&self) -> usize {
        self.node_count
    }

    #[inline]
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Borrow neighbors of `from_node` as (edge_index, to, neg_log_weight)
    #[inline]
    pub fn neighbors(&self, from_node: usize) -> impl Iterator<Item = (usize, usize, f64)> + '_ {
        let start = self.edge_offsets[from_node];
        let end = self.edge_offsets[from_node + 1];
        self.edge_indices[start..end]
            .iter()
            .copied()
            .map(move |edge_index| {
                let (_, to_node, _rate) = self.edges[edge_index];
                (edge_index, to_node, self.weights_in_neglog[edge_index])
            })
    }

    #[inline]
    pub fn edge_src(&self, edge_index: usize) -> usize {
        let (src, _, _) = self.edges[edge_index];
        src
    }

    #[inline]
    pub fn edge_dst(&self, edge_index: usize) -> usize {
        let (_, dst, _) = self.edges[edge_index];
        dst
    }

    #[inline]
    pub fn edge_rate(&self, edge_index: usize) -> f64 {
        let (_, _, rate) = self.edges[edge_index];
        rate
    }

    #[inline]
    pub fn update_rate(&mut self, edge_index: usize, new_rate: f64) -> Result<(), UpdateError> {
        if edge_index >= self.edges.len() {
            return Err(UpdateError::IndexOutOfBounds(edge_index));
        }
        if new_rate <= 0.0 || !new_rate.is_finite() {
            return Err(UpdateError::InvalidRate(new_rate));
        }
        let (src, dst, _) = self.edges[edge_index];
        self.edges[edge_index] = (src, dst, new_rate);
        self.weights_in_neglog[edge_index] = -new_rate.ln();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbors_preserve_insertion_order() {
        let edges = vec![(0, 1, 1.2), (0, 2, 0.9), (1, 0, 1.1), (2, 1, 1.05)];
        let graph = CSRGraph::from_edges(3, edges);

        let neigh: Vec<_> = graph.neighbors(0).collect();
        assert_eq!(neigh.len(), 2);
        assert_eq!(neigh[0].0, 0);
        assert_eq!(neigh[0].1, 1);
        assert!((neigh[0].2 - (-1.2_f64.ln())).abs() < 1e-12);
        assert_eq!(neigh[1].0, 1);
        assert_eq!(neigh[1].1, 2);
    }

    #[test]
    fn nodes_with_no_outgoing_edges_have_empty_neighbors() {
        let edges = vec![(0, 1, 1.0)];
        let graph = CSRGraph::from_edges(3, edges);
        assert_eq!(graph.neighbors(0).count(), 1);
        assert_eq!(graph.neighbors(1).count(), 0);
        assert_eq!(graph.neighbors(2).count(), 0);
    }

    #[test]
    fn update_rate_mutates_rate_and_weight() {
        let edges = vec![(0, 1, 1.0), (1, 0, 2.0)];
        let mut graph = CSRGraph::from_edges(2, edges);
        let old_weight = graph.weights_in_neglog[1];
        graph.update_rate(1, 1.25).unwrap();
        assert!((graph.edge_rate(1) - 1.25).abs() < 1e-12);
        assert!((graph.weights_in_neglog[1] - (-1.25f64.ln())).abs() < 1e-12);
        assert_ne!(graph.weights_in_neglog[1], old_weight);
    }

    #[test]
    fn update_rate_rejects_invalid_inputs() {
        let edges = vec![(0, 1, 1.0)];
        let mut graph = CSRGraph::from_edges(2, edges);
        assert!(matches!(
            graph.update_rate(1, 1.0),
            Err(UpdateError::IndexOutOfBounds(_))
        ));
        assert!(matches!(
            graph.update_rate(0, 0.0),
            Err(UpdateError::InvalidRate(_))
        ));
        assert!(matches!(
            graph.update_rate(0, f64::NAN),
            Err(UpdateError::InvalidRate(_))
        ));
    }
}
