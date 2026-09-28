//! Shared helpers for the graph tests: a plaintext reference graph with the
//! oblivious graph's semantics, random inputs, and comparison drivers.
#![allow(dead_code)]

use rand::{rngs::StdRng, Rng, SeedableRng};
use sam_model::{
    GraphBackend, GraphInput, ObliviousGraph, PageRankResult, SamError, ShortestPathResult,
    SingleAccessMachine, SpanningTreeResult, StepKind, TriangleCountResult, WeightedEdge,
};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque},
};

/// Settled keys and tree edges of a best-first search.
type Settled = (BTreeMap<usize, i64>, Vec<(usize, usize, i64)>);

/// Plaintext graph with the oblivious graph's semantics: ids in insertion
/// order, edges appended, `delete_edge` moves the last edge into the removed
/// slot, and `delete_vertex` removes the vertex's incoming edges in place.
#[derive(Clone, Debug, Default)]
pub struct Reference {
    pub ids: BTreeMap<u64, u64>,
    pub adjacency: BTreeMap<u64, Vec<(u64, i64)>>,
    pub next_id: u64,
}

impl Reference {
    pub fn from_input(input: &GraphInput) -> Self {
        let mut graph = Self::default();
        for name in &input.vertices {
            graph.add_vertex(*name);
        }
        for edge in &input.edges {
            graph.add_edge(edge.source, edge.destination, edge.weight);
        }
        graph
    }

    pub fn add_vertex(&mut self, name: u64) -> u64 {
        assert!(!self.ids.contains_key(&name));
        let id = self.next_id;
        self.next_id += 1;
        self.ids.insert(name, id);
        self.adjacency.insert(id, Vec::new());
        id
    }

    pub fn add_edge(&mut self, source: u64, destination: u64, weight: i64) {
        let destination = self.ids[&destination];
        self.adjacency
            .get_mut(&self.ids[&source])
            .unwrap()
            .push((destination, weight));
    }

    pub fn delete_edge(&mut self, source: u64, destination: u64) -> bool {
        let destination = self.ids[&destination];
        let edges = self.adjacency.get_mut(&self.ids[&source]).unwrap();
        match edges.iter().position(|(target, _)| *target == destination) {
            Some(index) => {
                edges.swap_remove(index);
                true
            }
            None => false,
        }
    }

    pub fn delete_vertex(&mut self, name: u64) {
        let id = self.ids.remove(&name).unwrap();
        self.adjacency.remove(&id);
        for edges in self.adjacency.values_mut() {
            edges.retain(|(target, _)| *target != id);
        }
    }

    pub fn names(&self) -> Vec<u64> {
        self.ids.keys().copied().collect()
    }

    pub fn neighbors(&self, name: u64) -> Vec<(u64, i64)> {
        self.adjacency[&self.ids[&name]].clone()
    }

    fn out(&self, id: u64) -> &[(u64, i64)] {
        &self.adjacency[&id]
    }

    pub fn traverse(&self, start: u64, max_steps: Option<usize>, depth_first: bool) -> Vec<usize> {
        let mut frontier = VecDeque::from([self.ids[&start]]);
        let mut visited = BTreeSet::new();
        let mut path = Vec::new();
        loop {
            if max_steps.is_some_and(|limit| path.len() >= limit) {
                break;
            }
            let next = if depth_first {
                frontier.pop_back()
            } else {
                frontier.pop_front()
            };
            let Some(vertex) = next else { break };
            if !visited.insert(vertex) {
                continue;
            }
            path.push(vertex as usize);
            frontier.extend(self.out(vertex).iter().map(|(target, _)| *target));
        }
        path
    }

    pub fn best_first(&self, start: u64, max_steps: Option<usize>, path_cost: bool) -> Settled {
        let start = self.ids[&start];
        let mut heap = BinaryHeap::from([Reverse((0_i64, start, start))]);
        let mut keys = BTreeMap::new();
        let mut edges = Vec::new();
        loop {
            if max_steps.is_some_and(|limit| keys.len() >= limit) {
                break;
            }
            let Some(Reverse((key, vertex, parent))) = heap.pop() else {
                break;
            };
            if keys.contains_key(&(vertex as usize)) {
                continue;
            }
            keys.insert(vertex as usize, key);
            if vertex != start {
                edges.push((parent as usize, vertex as usize, key));
            }
            for (target, weight) in self.out(vertex) {
                let key = if path_cost { key + weight } else { *weight };
                heap.push(Reverse((key, *target, vertex)));
            }
        }
        (keys, edges)
    }

    pub fn dijkstra(&self, start: u64, max_steps: Option<usize>) -> ShortestPathResult {
        let (costs, edges) = self.best_first(start, max_steps, true);
        ShortestPathResult {
            total_cost: costs.values().sum(),
            costs,
            edges,
        }
    }

    pub fn prim(&self, start: u64, max_steps: Option<usize>) -> SpanningTreeResult {
        let (_, edges) = self.best_first(start, max_steps, false);
        SpanningTreeResult {
            total_cost: edges.iter().map(|(_, _, weight)| weight).sum(),
            edges,
        }
    }

    pub fn random_walk(&self, start: u64, length: usize, rng: &mut StdRng) -> Vec<usize> {
        let mut current = self.ids[&start];
        let mut trace = vec![current as usize];
        for _ in 0..length {
            let edges = self.out(current);
            if edges.is_empty() {
                break;
            }
            current = edges[rng.gen_range(0..edges.len() as u64) as usize].0;
            trace.push(current as usize);
        }
        trace
    }

    pub fn contact_discovery(&self, first: u64, second: u64) -> BTreeSet<usize> {
        let set = |name: u64| -> BTreeSet<usize> {
            self.neighbors(name)
                .iter()
                .map(|(target, _)| *target as usize)
                .collect()
        };
        set(first).intersection(&set(second)).copied().collect()
    }

    pub fn triangles(&self, source: u64, limit: Option<usize>) -> TriangleCountResult {
        let source = self.ids[&source];
        let capped = |vertex: u64| -> Vec<u64> {
            let edges = self.out(vertex);
            let count = limit.map_or(edges.len(), |limit| limit.min(edges.len()));
            edges[..count].iter().map(|(target, _)| *target).collect()
        };
        let full = |length: usize| limit.is_none_or(|limit| length == limit);
        let mut complete = true;
        let mut triangles = Vec::new();
        let firsts = capped(source);
        complete &= full(firsts.len());
        for first in firsts {
            let seconds = capped(first);
            complete &= full(seconds.len());
            for second in seconds {
                let thirds = capped(second);
                complete &= full(thirds.len());
                if thirds.contains(&source) {
                    triangles.push((source as usize, first as usize, second as usize));
                }
            }
        }
        TriangleCountResult {
            triangles,
            complete,
        }
    }

    pub fn pagerank(
        &self,
        length: usize,
        damping: f64,
        rng: &mut StdRng,
    ) -> BTreeMap<usize, usize> {
        let mut visits = BTreeMap::new();
        let mut current: Option<u64> = None;
        for _ in 0..length {
            let probability = rng.gen::<f64>();
            if probability <= damping {
                if let Some(vertex) = current {
                    let edges = self.out(vertex);
                    current = (!edges.is_empty())
                        .then(|| edges[rng.gen_range(0..edges.len() as u64) as usize].0);
                }
            }
            if current.is_none() || probability > damping {
                current = Some(loop {
                    let id = rng.gen_range(0..self.next_id);
                    if self.adjacency.contains_key(&id) {
                        break id;
                    }
                });
            }
            *visits.entry(current.unwrap() as usize).or_insert(0) += 1;
        }
        visits
    }
}

/// A random input with `n` vertices whose names are not their ids. Vertex 0
/// has out-degree `hub_degree`, vertex 1 out-degree `fanout`, vertex 2 one
/// edge, vertex 3 none; the rest have 0..6 edges, plus a self-loop and a
/// parallel edge.
pub fn random_input(seed: u64, n: u64, fanout: usize, hub_degree: usize) -> GraphInput {
    let mut rng = StdRng::seed_from_u64(seed);
    let name = |index: u64| 1000 + 7 * index;
    let mut edges = Vec::new();
    let mut edge = |source: u64, destination: u64, rng: &mut StdRng| {
        edges.push(WeightedEdge {
            source: name(source),
            destination: name(destination),
            weight: rng.gen_range(1..10),
        });
    };
    for vertex in 0..n {
        let degree = match vertex {
            0 => hub_degree,
            1 => fanout,
            2 => 1,
            3 => 0,
            _ => rng.gen_range(0..6),
        };
        for _ in 0..degree {
            let mut target = rng.gen_range(0..n);
            if target == vertex {
                target = (target + 1) % n;
            }
            edge(vertex, target, &mut rng);
        }
    }
    edge(5, 5, &mut rng);
    edge(4, 2, &mut rng);
    edge(4, 2, &mut rng);
    GraphInput::new((0..n).map(name), edges)
}

/// Checks every algorithm from several starts against the reference.
pub fn assert_algorithms_match<P, B, S>(
    graph: &mut ObliviousGraph<P>,
    reference: &Reference,
    backend: &mut B,
    sam: &mut S,
    seed: u64,
) where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    let names = reference.names();
    let mut rng = StdRng::seed_from_u64(seed);
    let mut starts = vec![names[0], names[names.len() - 1]];
    starts.extend((0..4).map(|_| names[rng.gen_range(0..names.len())]));
    for (round, start) in starts.into_iter().enumerate() {
        let context = format!("start {start} round {round}");
        for limit in [None, Some(1), Some(5)] {
            graph.step_marks = Some(Vec::new());
            let path = graph.bfs(start, limit, backend, sam).unwrap();
            assert_eq!(
                path,
                reference.traverse(start, limit, false),
                "bfs {context}"
            );
            let marks = graph.step_marks.take().unwrap();
            let steps = marks
                .iter()
                .filter(|mark| mark.kind == StepKind::Step)
                .count();
            assert_eq!(steps, path.len(), "bfs step marks {context}");
            assert_eq!(
                marks
                    .iter()
                    .map(|mark| mark.kind)
                    .skip(steps)
                    .collect::<Vec<_>>(),
                vec![StepKind::Tail, StepKind::Cleanup]
            );
            assert_eq!(
                graph.dfs(start, limit, backend, sam).unwrap(),
                reference.traverse(start, limit, true),
                "dfs {context}"
            );
            assert_eq!(
                graph.dijkstra(start, limit, backend, sam).unwrap(),
                reference.dijkstra(start, limit),
                "dijkstra {context}"
            );
            assert_eq!(
                graph.prim(start, limit, backend, sam).unwrap(),
                reference.prim(start, limit),
                "prim {context}"
            );
            assert_eq!(
                graph
                    .directed_triangle_count_detailed(
                        start,
                        limit.map(|limit| limit + 1),
                        backend,
                        sam
                    )
                    .unwrap(),
                reference.triangles(start, limit.map(|limit| limit + 1)),
                "dtc {context} {limit:?}"
            );
        }
        let walk_seed = seed * 31 + round as u64;
        assert_eq!(
            graph
                .random_walk(
                    start,
                    30,
                    &mut StdRng::seed_from_u64(walk_seed),
                    backend,
                    sam
                )
                .unwrap(),
            reference.random_walk(start, 30, &mut StdRng::seed_from_u64(walk_seed)),
            "random walk {context}"
        );
        let other = names[rng.gen_range(0..names.len())];
        assert_eq!(
            graph.contact_discovery(start, other, backend, sam).unwrap(),
            reference.contact_discovery(start, other),
            "contact discovery {context}"
        );
        let PageRankResult { visits, .. } = graph
            .pagerank(40, 0.8, &mut StdRng::seed_from_u64(walk_seed), backend, sam)
            .unwrap();
        assert_eq!(
            visits,
            reference.pagerank(40, 0.8, &mut StdRng::seed_from_u64(walk_seed)),
            "pagerank {context}"
        );
    }
}

/// Checks every live vertex's neighbor list against the reference.
pub fn assert_neighbors_match<P, B, S>(
    graph: &mut ObliviousGraph<P>,
    reference: &Reference,
    backend: &mut B,
    sam: &mut S,
    context: &str,
) where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    assert_eq!(
        graph.vertex_count(),
        reference.ids.len() as u64,
        "{context}"
    );
    for name in reference.names() {
        assert_eq!(
            graph.neighbors(name, backend, sam).unwrap(),
            reference.neighbors(name),
            "neighbors of {name} {context}"
        );
    }
}

/// Applies `count` random operations to both graphs, comparing every
/// neighbor list after each one.
pub fn random_dynamic_ops<P, B, S>(
    graph: &mut ObliviousGraph<P>,
    reference: &mut Reference,
    backend: &mut B,
    sam: &mut S,
    seed: u64,
    count: usize,
) -> Result<(), SamError>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    let mut rng = StdRng::seed_from_u64(seed);
    let mut next_name = 5_000;
    for step in 0..count {
        let names = reference.names();
        let pick = |rng: &mut StdRng| names[rng.gen_range(0..names.len())];
        let operation = rng.gen_range(0..10);
        let description = match operation {
            0..=3 => {
                let (source, destination) = (pick(&mut rng), pick(&mut rng));
                let weight = rng.gen_range(1..10);
                graph.add_edge(source, destination, weight, backend, sam)?;
                reference.add_edge(source, destination, weight);
                format!("add_edge {source} {destination}")
            }
            4..=6 => {
                let source = pick(&mut rng);
                let edges = reference.neighbors(source);
                let destination = if edges.is_empty() || rng.gen_bool(0.2) {
                    pick(&mut rng)
                } else {
                    let target = edges[rng.gen_range(0..edges.len())].0;
                    *reference
                        .ids
                        .iter()
                        .find(|(_, id)| **id == target)
                        .unwrap()
                        .0
                };
                let found = graph.delete_edge(source, destination, backend, sam)?;
                assert_eq!(found, reference.delete_edge(source, destination));
                format!("delete_edge {source} {destination}")
            }
            7 => {
                graph.add_vertex(next_name, backend, sam)?;
                reference.add_vertex(next_name);
                next_name += 1;
                format!("add_vertex {}", next_name - 1)
            }
            8 if names.len() > 3 => {
                let name = pick(&mut rng);
                graph.delete_vertex(name, backend, sam)?;
                reference.delete_vertex(name);
                format!("delete_vertex {name}")
            }
            _ => {
                // Errors leave the graph unchanged.
                let name = pick(&mut rng);
                assert!(graph.add_vertex(name, backend, sam).is_err());
                assert!(graph.add_edge(name, 999_999, 1, backend, sam).is_err());
                format!("rejected duplicate {name}")
            }
        };
        assert_neighbors_match(
            graph,
            reference,
            backend,
            sam,
            &format!("after op {step}: {description}"),
        );
    }
    Ok(())
}

/// Runs every algorithm once from one start and compares with the reference.
pub fn assert_algorithms_match_once<P, B, S>(
    graph: &mut ObliviousGraph<P>,
    reference: &Reference,
    backend: &mut B,
    sam: &mut S,
) where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    let names = reference.names();
    let (start, other) = (names[0], names[names.len() / 2]);
    assert_eq!(
        graph.bfs(start, None, backend, sam).unwrap(),
        reference.traverse(start, None, false)
    );
    assert_eq!(
        graph.dfs(start, Some(4), backend, sam).unwrap(),
        reference.traverse(start, Some(4), true)
    );
    assert_eq!(
        graph.dijkstra(start, None, backend, sam).unwrap(),
        reference.dijkstra(start, None)
    );
    assert_eq!(
        graph.prim(start, None, backend, sam).unwrap(),
        reference.prim(start, None)
    );
    assert_eq!(
        graph
            .directed_triangle_count_detailed(start, Some(2), backend, sam)
            .unwrap(),
        reference.triangles(start, Some(2))
    );
    assert_eq!(
        graph
            .random_walk(start, 8, &mut StdRng::seed_from_u64(3), backend, sam)
            .unwrap(),
        reference.random_walk(start, 8, &mut StdRng::seed_from_u64(3))
    );
    assert_eq!(
        graph.contact_discovery(start, other, backend, sam).unwrap(),
        reference.contact_discovery(start, other)
    );
    assert_eq!(
        graph
            .pagerank(8, 0.8, &mut StdRng::seed_from_u64(4), backend, sam)
            .unwrap()
            .visits,
        reference.pagerank(8, 0.8, &mut StdRng::seed_from_u64(4))
    );
}
