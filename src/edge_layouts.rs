#[derive(Debug, Clone, PartialEq)]
pub struct EdgeAoS {
    pub from: usize,
    pub to: usize,
    pub rate: f64,
}

impl EdgeAoS {
    #[inline]
    pub fn new(from: usize, to: usize, rate: f64) -> Self {
        Self { from, to, rate }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeSoA {
    pub from: Vec<usize>,
    pub to: Vec<usize>,
    pub rate: Vec<f64>,
}

impl EdgeSoA {
    #[inline]
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            from: Vec::with_capacity(cap),
            to: Vec::with_capacity(cap),
            rate: Vec::with_capacity(cap),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.from.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.from.is_empty()
    }

    #[inline]
    pub fn push(&mut self, from: usize, to: usize, rate: f64) {
        self.from.push(from);
        self.to.push(to);
        self.rate.push(rate);
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (usize, usize, f64)> + '_ {
        self.from
            .iter()
            .copied()
            .zip(self.to.iter().copied())
            .zip(self.rate.iter().copied())
            .map(|((u, v), r)| (u, v, r))
    }
}

impl From<Vec<EdgeAoS>> for EdgeSoA {
    fn from(edges: Vec<EdgeAoS>) -> Self {
        let mut soa = EdgeSoA::with_capacity(edges.len());
        for edge in edges {
            soa.push(edge.from, edge.to, edge.rate);
        }
        soa
    }
}

impl From<&[EdgeAoS]> for EdgeSoA {
    fn from(edges: &[EdgeAoS]) -> Self {
        let mut soa = EdgeSoA::with_capacity(edges.len());
        for edge in edges {
            soa.push(edge.from, edge.to, edge.rate);
        }
        soa
    }
}

impl From<EdgeSoA> for Vec<EdgeAoS> {
    fn from(soa: EdgeSoA) -> Self {
        let EdgeSoA { from, to, rate } = soa;
        debug_assert!(from.len() == to.len() && to.len() == rate.len());

        from.into_iter()
            .zip(to)
            .zip(rate)
            .map(|((u, v), r)| EdgeAoS {
                from: u,
                to: v,
                rate: r,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_aos_to_soa_and_back() {
        let input = vec![
            EdgeAoS::new(1, 2, 1.1),
            EdgeAoS::new(2, 3, 0.9),
            EdgeAoS::new(3, 1, 1.05),
        ];

        let soa = EdgeSoA::from(input.clone());
        assert_eq!(soa.len(), input.len());

        let round_trip: Vec<EdgeAoS> = soa.into();
        assert_eq!(round_trip, input);
    }

    #[test]
    fn soa_iteration_matches_aos() {
        let edges = vec![
            EdgeAoS::new(0, 1, 1.01),
            EdgeAoS::new(1, 2, 1.02),
            EdgeAoS::new(2, 0, 0.99),
        ];

        let soa = EdgeSoA::from(edges.as_slice());
        let iterated: Vec<_> = soa.iter().collect();
        let expected: Vec<_> = edges.iter().map(|e| (e.from, e.to, e.rate)).collect();

        assert_eq!(iterated, expected);
    }

    #[test]
    fn with_capacity_preallocates_and_starts_empty() {
        let cap = 8;
        let soa = EdgeSoA::with_capacity(cap);

        assert_eq!(soa.len(), 0);
        assert!(soa.is_empty());
        assert!(soa.from.capacity() >= cap);
        assert!(soa.to.capacity() >= cap);
        assert!(soa.rate.capacity() >= cap);
    }

    #[test]
    fn push_adds_edges_in_order() {
        let mut soa = EdgeSoA::with_capacity(2);
        soa.push(4, 5, 1.5);
        soa.push(6, 7, 0.5);

        assert_eq!(soa.len(), 2);
        let collected: Vec<_> = soa.iter().collect();
        assert_eq!(collected, vec![(4, 5, 1.5), (6, 7, 0.5)]);
    }

    #[test]
    fn from_slice_copies_data() {
        let mut edges = vec![EdgeAoS::new(10, 11, 1.11), EdgeAoS::new(11, 12, 0.91)];
        let soa = EdgeSoA::from(edges.as_slice());

        // Mutate the source slice after conversion; the SoA snapshot should not change.
        edges[0].rate = 42.0;

        let collected: Vec<_> = soa.iter().collect();
        assert_eq!(collected, vec![(10, 11, 1.11), (11, 12, 0.91)]);
    }
}
