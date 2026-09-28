//! The oblivious graph against a plaintext reference, for every pointer
//! backend, cached and uncached, on the policy-enforcing dry-run SAM.

mod common;

use common::{
    assert_algorithms_match, assert_neighbors_match, random_dynamic_ops, random_input, Reference,
};
use rand::{rngs::StdRng, SeedableRng};
use sam_model::{
    pointer::{
        CachedPointers, MultiWritePointers, OriginalPointers, PointerKind, RaryPointers,
        RecursivePointers, SmartPointerBackend,
    },
    DryRunSam, GraphBackend, GraphInput, GraphLayout, NoMovePointers, ObliviousGraph,
    SingleAccessMachine, WeightedEdge,
};

/// Outgoing-edge fanout of the test graphs: hub vertices get 3-level trees.
const FANOUT: usize = 3;

fn layout(kind: PointerKind) -> GraphLayout {
    GraphLayout::for_pointer_kind(1024, kind)
        .unwrap()
        .with_fanout(FANOUT)
        .unwrap()
}

fn sam_for<P: Clone, B: GraphBackend<P>>(kind: PointerKind) -> DryRunSam<B::Cell> {
    DryRunSam::new(kind.access_policy())
}

/// Static build, then every algorithm from several starts.
fn algorithms_match_reference<P: Clone, B: GraphBackend<P>>(kind: PointerKind, mut backend: B) {
    for seed in 0..3 {
        let input = random_input(seed, 24, FANOUT, FANOUT * FANOUT + 5);
        let reference = Reference::from_input(&input);
        let mut sam = sam_for::<P, B>(kind);
        let mut graph =
            ObliviousGraph::build_static(&input, layout(kind), &mut backend, &mut sam).unwrap();
        assert_neighbors_match(
            &mut graph,
            &reference,
            &mut backend,
            &mut sam,
            "after build",
        );
        assert_algorithms_match(&mut graph, &reference, &mut backend, &mut sam, seed);
        backend.clear_cache().unwrap();
    }
}

/// Random add/delete operations checked op by op, then the algorithms,
/// then a full tombstone purge.
fn dynamic_ops_match_reference<P: Clone, B: GraphBackend<P>>(kind: PointerKind, mut backend: B) {
    for seed in 0..2 {
        let input = random_input(seed + 10, 16, FANOUT, FANOUT * FANOUT + 2);
        let mut reference = Reference::from_input(&input);
        let mut sam = sam_for::<P, B>(kind);
        let mut graph =
            ObliviousGraph::build_static(&input, layout(kind), &mut backend, &mut sam).unwrap();
        random_dynamic_ops(
            &mut graph,
            &mut reference,
            &mut backend,
            &mut sam,
            seed,
            120,
        )
        .unwrap();
        assert_algorithms_match(&mut graph, &reference, &mut backend, &mut sam, seed);
        graph.purge_tombstones(&mut backend, &mut sam).unwrap();
        assert_eq!(graph.tombstones(), 0);
        assert_neighbors_match(
            &mut graph,
            &reference,
            &mut backend,
            &mut sam,
            "after purge",
        );
        assert_algorithms_match(&mut graph, &reference, &mut backend, &mut sam, seed + 1);
        backend.clear_cache().unwrap();
    }
}

/// `build_static` and `build_dynamic` give the same graph.
fn static_equals_dynamic<P: Clone, B: GraphBackend<P>>(
    kind: PointerKind,
    mut make: impl FnMut() -> B,
) {
    let input = random_input(42, 30, FANOUT, 2 * FANOUT * FANOUT + 1);
    let reference = Reference::from_input(&input);
    let (mut static_backend, mut dynamic_backend) = (make(), make());
    let mut static_sam = sam_for::<P, B>(kind);
    let mut dynamic_sam = sam_for::<P, B>(kind);
    let mut built =
        ObliviousGraph::build_static(&input, layout(kind), &mut static_backend, &mut static_sam)
            .unwrap();
    let mut grown =
        ObliviousGraph::build_dynamic(&input, layout(kind), &mut dynamic_backend, &mut dynamic_sam)
            .unwrap();
    for name in reference.names() {
        let left = built
            .neighbors(name, &mut static_backend, &mut static_sam)
            .unwrap();
        let right = grown
            .neighbors(name, &mut dynamic_backend, &mut dynamic_sam)
            .unwrap();
        assert_eq!(left, right, "neighbors of {name}");
        assert_eq!(left, reference.neighbors(name));
    }
    for start in reference.names().into_iter().step_by(4) {
        assert_eq!(
            built
                .bfs(start, None, &mut static_backend, &mut static_sam)
                .unwrap(),
            grown
                .bfs(start, None, &mut dynamic_backend, &mut dynamic_sam)
                .unwrap()
        );
        assert_eq!(
            built
                .dijkstra(start, None, &mut static_backend, &mut static_sam)
                .unwrap(),
            grown
                .dijkstra(start, None, &mut dynamic_backend, &mut dynamic_sam)
                .unwrap()
        );
        let seed = start;
        assert_eq!(
            built
                .random_walk(
                    start,
                    25,
                    &mut StdRng::seed_from_u64(seed),
                    &mut static_backend,
                    &mut static_sam
                )
                .unwrap(),
            grown
                .random_walk(
                    start,
                    25,
                    &mut StdRng::seed_from_u64(seed),
                    &mut dynamic_backend,
                    &mut dynamic_sam
                )
                .unwrap()
        );
    }
    assert_algorithms_match(
        &mut grown,
        &reference,
        &mut dynamic_backend,
        &mut dynamic_sam,
        5,
    );
}

macro_rules! backend_suite {
    ($module:ident, $kind:expr, $make:expr) => {
        mod $module {
            use super::*;

            #[test]
            fn algorithms_match_reference_uncached() {
                algorithms_match_reference($kind, $make);
            }

            #[test]
            fn algorithms_match_reference_cached() {
                algorithms_match_reference($kind, CachedPointers::new($make));
            }

            #[test]
            fn dynamic_ops_match_reference_uncached() {
                dynamic_ops_match_reference($kind, $make);
            }

            #[test]
            fn dynamic_ops_match_reference_cached() {
                dynamic_ops_match_reference($kind, CachedPointers::new($make));
            }

            #[test]
            fn static_build_equals_dynamic_build_uncached() {
                static_equals_dynamic($kind, || $make);
            }

            #[test]
            fn static_build_equals_dynamic_build_cached() {
                static_equals_dynamic($kind, || CachedPointers::new($make));
            }

            #[test]
            fn algorithms_match_reference_no_move() {
                algorithms_match_reference($kind, NoMovePointers::new($make));
            }

            #[test]
            fn dynamic_ops_match_reference_no_move() {
                dynamic_ops_match_reference($kind, NoMovePointers::new($make));
            }

            #[test]
            fn static_build_equals_dynamic_build_no_move() {
                static_equals_dynamic($kind, || NoMovePointers::new($make));
            }
        }
    };
}

backend_suite!(multiwrite, PointerKind::MultiWrite, MultiWritePointers);
backend_suite!(original, PointerKind::Original, OriginalPointers);
backend_suite!(
    recursive,
    PointerKind::Recursive,
    RecursivePointers::default()
);
backend_suite!(
    rary2,
    PointerKind::MultiWriteRary {
        branching_factor: 2
    },
    RaryPointers::new(2).unwrap()
);
backend_suite!(
    rary4,
    PointerKind::MultiWriteRary {
        branching_factor: 4
    },
    RaryPointers::new(4).unwrap()
);
backend_suite!(
    rary6,
    PointerKind::MultiWriteRary {
        branching_factor: 6
    },
    RaryPointers::new(6).unwrap()
);
backend_suite!(
    rary64,
    PointerKind::MultiWriteRary {
        branching_factor: 64
    },
    RaryPointers::new(64).unwrap()
);

/// Many aliases of one vertex (the r-ary pointer tree gets several levels),
/// then deletions of those aliases and of the vertex itself.
fn many_aliases_then_deletions<P: Clone, B: GraphBackend<P>>(kind: PointerKind, mut backend: B) {
    let hub = 0;
    let mut edges = Vec::new();
    for source in 1..=100_u64 {
        for copy in 0..2 {
            edges.push(WeightedEdge {
                source,
                destination: hub,
                weight: (source + copy) as i64 % 7 + 1,
            });
        }
        edges.push(WeightedEdge {
            source: hub,
            destination: source,
            weight: 1,
        });
    }
    let input = GraphInput::new(0..=100, edges);
    let mut reference = Reference::from_input(&input);
    let mut sam = sam_for::<P, B>(kind);
    let mut graph =
        ObliviousGraph::build_static(&input, layout(kind), &mut backend, &mut sam).unwrap();
    assert_algorithms_match(&mut graph, &reference, &mut backend, &mut sam, 3);
    for source in (1..=100_u64).step_by(3) {
        assert!(graph
            .delete_edge(source, hub, &mut backend, &mut sam)
            .unwrap());
        assert!(reference.delete_edge(source, hub));
    }
    assert_neighbors_match(
        &mut graph,
        &reference,
        &mut backend,
        &mut sam,
        "after edge deletions",
    );
    assert_algorithms_match(&mut graph, &reference, &mut backend, &mut sam, 4);
    graph.delete_vertex(hub, &mut backend, &mut sam).unwrap();
    reference.delete_vertex(hub);
    assert_eq!(graph.tombstones(), 1);
    assert_neighbors_match(
        &mut graph,
        &reference,
        &mut backend,
        &mut sam,
        "after hub deletion",
    );
    assert_algorithms_match(&mut graph, &reference, &mut backend, &mut sam, 5);
    for name in 1..=100_u64 {
        graph
            .add_edge(name, (name % 100) + 1, 2, &mut backend, &mut sam)
            .unwrap();
        reference.add_edge(name, (name % 100) + 1, 2);
    }
    assert_neighbors_match(
        &mut graph,
        &reference,
        &mut backend,
        &mut sam,
        "after re-adding",
    );
    assert_algorithms_match(&mut graph, &reference, &mut backend, &mut sam, 6);
}

#[test]
fn rary_pointers_survive_many_aliases_and_deletions() {
    for fanout in [2, 4, 6, 64] {
        let kind = PointerKind::MultiWriteRary {
            branching_factor: fanout,
        };
        many_aliases_then_deletions(kind, RaryPointers::new(fanout).unwrap());
        many_aliases_then_deletions(
            kind,
            CachedPointers::new(RaryPointers::new(fanout).unwrap()),
        );
        many_aliases_then_deletions(
            kind,
            NoMovePointers::new(RaryPointers::new(fanout).unwrap()),
        );
    }
}

#[test]
fn other_pointers_survive_many_aliases_and_deletions() {
    many_aliases_then_deletions(PointerKind::MultiWrite, MultiWritePointers);
    many_aliases_then_deletions(PointerKind::Original, OriginalPointers);
    many_aliases_then_deletions(PointerKind::Recursive, RecursivePointers::default());
    many_aliases_then_deletions(
        PointerKind::MultiWrite,
        NoMovePointers::new(MultiWritePointers),
    );
    many_aliases_then_deletions(PointerKind::Original, NoMovePointers::new(OriginalPointers));
    many_aliases_then_deletions(
        PointerKind::Recursive,
        NoMovePointers::new(RecursivePointers::default()),
    );
}

/// Every temporary pointer copy an algorithm makes is deleted: recursive
/// reference counts and the multi-write SAM's live block count return.
#[test]
fn algorithms_leave_no_pointer_copies_behind() {
    let input = random_input(7, 24, FANOUT, FANOUT * FANOUT + 5);
    let reference = Reference::from_input(&input);

    let kind = PointerKind::Recursive;
    let mut backend = RecursivePointers::default();
    let mut sam = sam_for::<_, RecursivePointers>(kind);
    let mut graph =
        ObliviousGraph::build_static(&input, layout(kind), &mut backend, &mut sam).unwrap();
    let references = backend.live_references();
    let blocks = sam.live_blocks();
    assert_algorithms_match(&mut graph, &reference, &mut backend, &mut sam, 1);
    assert_eq!(backend.live_references(), references);
    assert_eq!(sam.live_blocks(), blocks);

    let mut backend = CachedPointers::new(RecursivePointers::default());
    let mut sam = sam_for::<_, CachedPointers<_, RecursivePointers>>(kind);
    let mut graph =
        ObliviousGraph::build_static(&input, layout(kind), &mut backend, &mut sam).unwrap();
    let references = backend.backend().live_references();
    assert_algorithms_match(&mut graph, &reference, &mut backend, &mut sam, 1);
    assert_eq!(backend.backend().live_references(), references);
    assert_eq!(backend.cached_values(), 0);
    assert_eq!(backend.deferred_deletes(), 0);

    let kind = PointerKind::MultiWrite;
    let mut backend = MultiWritePointers;
    let mut sam = sam_for::<_, MultiWritePointers>(kind);
    let mut graph =
        ObliviousGraph::build_static(&input, layout(kind), &mut backend, &mut sam).unwrap();
    let blocks = sam.live_blocks();
    assert_algorithms_match(&mut graph, &reference, &mut backend, &mut sam, 1);
    assert_eq!(sam.live_blocks(), blocks);

    // No-move copies every nested pointer on each access and must delete
    // every one of those copies again.
    let kind = PointerKind::Recursive;
    let mut backend = NoMovePointers::new(RecursivePointers::default());
    let mut sam = sam_for::<_, NoMovePointers<RecursivePointers>>(kind);
    let mut graph =
        ObliviousGraph::build_static(&input, layout(kind), &mut backend, &mut sam).unwrap();
    let references = backend.inner().live_references();
    let blocks = sam.live_blocks();
    assert_algorithms_match(&mut graph, &reference, &mut backend, &mut sam, 1);
    assert_eq!(backend.inner().live_references(), references);
    assert_eq!(sam.live_blocks(), blocks);

    let kind = PointerKind::MultiWrite;
    let mut backend = NoMovePointers::new(MultiWritePointers);
    let mut sam = sam_for::<_, NoMovePointers<MultiWritePointers>>(kind);
    let mut graph =
        ObliviousGraph::build_static(&input, layout(kind), &mut backend, &mut sam).unwrap();
    let blocks = sam.live_blocks();
    assert_algorithms_match(&mut graph, &reference, &mut backend, &mut sam, 1);
    assert_eq!(sam.live_blocks(), blocks);
}

/// No-move pays Python's copying on every access: more round trips than the
/// move pattern for the single-read pointers, and for the recursive ORAM a
/// read is a single read with no write-back.
#[test]
fn no_move_costs_more_than_move_and_reads_without_write_back() {
    fn round_trips<P: Clone, B: GraphBackend<P>>(
        kind: PointerKind,
        mut backend: B,
    ) -> (u64, u64, u64) {
        let input = random_input(3, 40, FANOUT, FANOUT * FANOUT + 5);
        let mut sam = sam_for::<P, B>(kind);
        let mut graph =
            ObliviousGraph::build_static(&input, layout(kind), &mut backend, &mut sam).unwrap();
        let before = sam.stats().operations;
        for start in input.vertices.iter().copied().step_by(5) {
            graph
                .random_walk(
                    start,
                    20,
                    &mut StdRng::seed_from_u64(start),
                    &mut backend,
                    &mut sam,
                )
                .unwrap();
            graph.bfs(start, Some(10), &mut backend, &mut sam).unwrap();
        }
        let after = sam.stats().operations;
        let (reads, writes) = (after.reads - before.reads, after.writes - before.writes);
        (reads + writes, reads, writes)
    }
    for kind in [PointerKind::Original, PointerKind::MultiWrite] {
        let moved = match kind {
            PointerKind::Original => round_trips(kind, OriginalPointers).0,
            _ => round_trips(kind, MultiWritePointers).0,
        };
        let copied = match kind {
            PointerKind::Original => round_trips(kind, NoMovePointers::new(OriginalPointers)).0,
            _ => round_trips(kind, NoMovePointers::new(MultiWritePointers)).0,
        };
        assert!(copied > moved, "{kind:?}: no-move {copied} <= move {moved}");
    }
    let kind = PointerKind::Recursive;
    let (_, reads, writes) = round_trips(kind, RecursivePointers::default());
    assert_eq!(reads, writes, "the move pattern writes every read back");
    let (_, reads, writes) = round_trips(kind, NoMovePointers::new(RecursivePointers::default()));
    assert!(
        writes < reads,
        "no-move reads skip the write-back ({reads} reads, {writes} writes)"
    );
}

/// Deleting a vertex frees its own edges and fan-out nodes; once the
/// tombstone is purged no reference to it remains.
#[test]
fn deleting_vertices_and_purging_releases_their_pointers() {
    let input = random_input(8, 20, FANOUT, FANOUT * FANOUT + 1);
    let kind = PointerKind::Recursive;
    let mut backend = RecursivePointers::default();
    let mut sam = sam_for::<_, RecursivePointers>(kind);
    let mut graph =
        ObliviousGraph::build_static(&input, layout(kind), &mut backend, &mut sam).unwrap();
    let mut reference = Reference::from_input(&input);
    for name in reference.names().into_iter().take(5) {
        graph.delete_vertex(name, &mut backend, &mut sam).unwrap();
        reference.delete_vertex(name);
    }
    graph.purge_tombstones(&mut backend, &mut sam).unwrap();
    // One alias per vertex (its entry point) plus one per edge, plus one
    // pointer per fan-out node.
    let edges = reference.adjacency.values().map(Vec::len).sum::<usize>();
    let mut fan_out_nodes = 0;
    for list in reference.adjacency.values() {
        let mut level = list.len();
        while level > 0 {
            fan_out_nodes += level;
            level = if level <= FANOUT {
                0
            } else {
                level.div_ceil(FANOUT)
            };
        }
    }
    assert_eq!(
        backend.live_references(),
        reference.ids.len() + edges + fan_out_nodes
    );
    assert_neighbors_match(
        &mut graph,
        &reference,
        &mut backend,
        &mut sam,
        "after purge",
    );
}

#[test]
fn lookups_and_errors() {
    let input = random_input(9, 10, FANOUT, 4);
    let kind = PointerKind::MultiWrite;
    let mut backend = MultiWritePointers;
    let mut sam = sam_for::<_, MultiWritePointers>(kind);
    let mut graph =
        ObliviousGraph::build_static(&input, layout(kind), &mut backend, &mut sam).unwrap();
    let names = input.vertices.clone();
    assert_eq!(
        graph.vertex_id(names[3], &mut backend, &mut sam).unwrap(),
        Some(3)
    );
    assert_eq!(graph.vertex_id(7, &mut backend, &mut sam).unwrap(), None);
    let (mut pointer, id) = graph
        .get_pointer(names[2], &mut backend, &mut sam)
        .unwrap()
        .unwrap();
    assert_eq!(id, 2);
    backend.delete(&mut sam, &mut pointer).unwrap();
    assert!(graph.add_vertex(names[0], &mut backend, &mut sam).is_err());
    assert!(graph
        .add_edge(names[0], 7, 1, &mut backend, &mut sam)
        .is_err());
    assert!(graph
        .add_edge(7, names[0], 1, &mut backend, &mut sam)
        .is_err());
    assert!(graph.bfs(7, None, &mut backend, &mut sam).is_err());
    assert!(graph
        .bfs(names[0], Some(0), &mut backend, &mut sam)
        .is_err());
    assert!(!graph
        .delete_edge(names[3], names[4], &mut backend, &mut sam)
        .unwrap());
    graph
        .delete_vertex(names[4], &mut backend, &mut sam)
        .unwrap();
    assert!(graph
        .delete_vertex(names[4], &mut backend, &mut sam)
        .is_err());
    assert!(graph
        .add_edge(names[0], names[4], 1, &mut backend, &mut sam)
        .is_err());
    assert_eq!(graph.vertex_count(), 9);
    assert_eq!(graph.next_vertex_id(), 10);
    assert_eq!(
        graph.add_vertex(names[4], &mut backend, &mut sam).unwrap(),
        10
    );
    // The dry-run SAM enforced single reads throughout.
    assert!(sam.stats().operations.reads > 0);
}

#[test]
fn priming_keeps_the_graph_intact() {
    fn primed<P: Clone, B: GraphBackend<P>>(kind: PointerKind, mut backend: B) {
        let input = random_input(11, 40, FANOUT, 12);
        let reference = Reference::from_input(&input);
        let mut sam = sam_for::<P, B>(kind);
        let mut graph =
            ObliviousGraph::build_static(&input, layout(kind), &mut backend, &mut sam).unwrap();
        let before = sam.stats().operations;
        let walks = graph
            .prime(&mut StdRng::seed_from_u64(1), &mut backend, &mut sam)
            .unwrap();
        assert_eq!(walks, 4);
        assert!(sam.stats().operations.reads > before.reads);
        assert_algorithms_match(&mut graph, &reference, &mut backend, &mut sam, 2);
    }
    let kind = PointerKind::MultiWriteRary {
        branching_factor: 4,
    };
    primed(kind, RaryPointers::new(4).unwrap());
    primed(kind, CachedPointers::new(RaryPointers::new(4).unwrap()));
    primed(PointerKind::Original, OriginalPointers);
}

/// Traversals look up only their start vertex in the lookup trees and reach
/// every other vertex by following edge pointers: a traversal costs the AVL
/// trees exactly what one `get_pointer` costs, however many vertices it
/// visits.
#[test]
fn traversals_look_up_only_the_start_vertex() {
    let input = random_input(5, 60, FANOUT, FANOUT * FANOUT + 5);
    let kind = PointerKind::MultiWrite;
    let mut backend = MultiWritePointers;
    let mut sam = sam_for::<_, MultiWritePointers>(kind);
    let mut graph =
        ObliviousGraph::build_static(&input, layout(kind), &mut backend, &mut sam).unwrap();
    let avl = |sam: &DryRunSam<_>| {
        sam.stats()
            .by_structure
            .get("SmartAVLTree")
            .map_or(0, |stats| stats.reads + stats.writes)
    };
    let start = input.vertices[0];
    let before = avl(&sam);
    let (mut pointer, _) = graph
        .get_pointer(start, &mut backend, &mut sam)
        .unwrap()
        .unwrap();
    let lookup = avl(&sam) - before;
    backend.delete(&mut sam, &mut pointer).unwrap();
    assert!(lookup > 0);
    for order in 0..4 {
        let before = avl(&sam);
        let visited = match order {
            0 => graph
                .bfs(start, Some(20), &mut backend, &mut sam)
                .unwrap()
                .len(),
            1 => graph
                .dfs(start, Some(20), &mut backend, &mut sam)
                .unwrap()
                .len(),
            2 => graph
                .dijkstra(start, Some(20), &mut backend, &mut sam)
                .unwrap()
                .costs
                .len(),
            _ => {
                graph
                    .prim(start, Some(20), &mut backend, &mut sam)
                    .unwrap()
                    .edges
                    .len()
                    + 1
            }
        };
        assert_eq!(visited, 20, "order {order}");
        assert_eq!(avl(&sam) - before, lookup, "order {order}");
    }
}
