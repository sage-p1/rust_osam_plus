//! The graph on the encrypted Path OSAM+ SAM, with per-address policy
//! checks, for every backend it supports (the recursive pointer needs
//! multi-read addresses, which the Path OSAM+ adapter does not provide).

mod common;

use common::{assert_algorithms_match_once, random_dynamic_ops, random_input, Reference};
use sam_model::{
    pointer::{
        CachedPointers, FixedSizeCodec, MultiWriteCellValueCodec, MultiWritePointers,
        OriginalCellValueCodec, OriginalPointers, PointerKind, RaryCellValueCodec, RaryPointers,
    },
    AccessPolicy, AccessStrategy, BlockCodec, CachedGraphPointerCodec, DryRunSam, GraphBackend,
    GraphLayout, GraphValueCodec, MemoryClass, MultiWriteGraphPointerCodec, ObliviousGraph,
    OriginalGraphPointerCodec, PathOsamSam, RaryGraphPointerCodec, SingleAccessMachine, U64Codec,
};

const STRUCTURE: &str = "encrypted-smoke-test";
const FANOUT: usize = 2;

#[test]
fn dry_run_build_can_move_to_encrypted_osam() {
    // Build quickly without cryptography.
    let mut dry_run = DryRunSam::new(AccessPolicy::MULTI_WRITE);
    let address = dry_run.alloc(MemoryClass::Oblivious, STRUCTURE);
    dry_run.write(address, 42_u64, STRUCTURE).unwrap();

    // At the build boundary, install all live blocks into an encrypted tree.
    let mut encrypted = PathOsamSam::<u64, _, 64, 4, 1>::from_snapshot(
        dry_run.snapshot(),
        64, // OSAM block capacity; must be a power of two.
        40, // Stash overflow capacity.
        true,
        AccessPolicy::MULTI_WRITE,
        AccessStrategy::MULTI_WRITE_RARY,
        true, // Deterministic reverse-lexicographic eviction.
        7,    // Reproducible RNG seed.
        U64Codec,
    )
    .unwrap();

    // MULTI_WRITE_RARY reads with read_multi_paths and tracks the stash.
    assert_eq!(encrypted.read(address, STRUCTURE).unwrap(), Some(42));
    let stash = encrypted.stats().stash.unwrap();
    assert_eq!(stash.samples, 1);
    assert!(encrypted.backend_max_stash_occupancy() >= stash.maximum);
}

/// Builds a small graph on the dry-run SAM, installs it into Path OSAM+,
/// then runs dynamic operations and every algorithm encrypted.
fn encrypted_graph<P, B, C, const BLOCK: usize>(kind: PointerKind, mut backend: B, codec: C)
where
    P: Clone,
    B: GraphBackend<P>,
    C: BlockCodec<B::Cell, BLOCK>,
{
    let layout = GraphLayout::for_pointer_kind(BLOCK, kind)
        .unwrap()
        .with_fanout(FANOUT)
        .unwrap();
    let input = random_input(3, 8, FANOUT, 5);
    let mut reference = Reference::from_input(&input);
    let mut dry = DryRunSam::<B::Cell>::new(kind.access_policy());
    let mut graph = ObliviousGraph::build_static(&input, layout, &mut backend, &mut dry).unwrap();
    let snapshot = dry.snapshot();
    let capacity = snapshot.next_identifier.next_power_of_two();
    let mut sam = PathOsamSam::<B::Cell, _, BLOCK, 4, 1>::from_snapshot(
        snapshot,
        capacity,
        40,
        true,
        kind.access_policy(),
        kind.access_strategy(),
        true,
        9,
        codec,
    )
    .unwrap()
    .with_policy_checks(true);
    random_dynamic_ops(&mut graph, &mut reference, &mut backend, &mut sam, 1, 6).unwrap();
    assert_algorithms_match_once(&mut graph, &reference, &mut backend, &mut sam);
    assert!(sam.stats().operations.reads > 0);
}

fn graph_codec<C>(pointers: C) -> GraphValueCodec<C> {
    GraphValueCodec::new(pointers, FANOUT).unwrap()
}

#[test]
fn multiwrite_graph_runs_encrypted() {
    let kind = PointerKind::MultiWrite;
    encrypted_graph::<_, _, _, 64>(
        kind,
        MultiWritePointers,
        FixedSizeCodec::new(MultiWriteCellValueCodec::new(graph_codec(
            MultiWriteGraphPointerCodec,
        ))),
    );
    encrypted_graph::<_, _, _, 64>(
        kind,
        CachedPointers::new(MultiWritePointers),
        FixedSizeCodec::new(MultiWriteCellValueCodec::new(graph_codec(
            CachedGraphPointerCodec::new(MultiWriteGraphPointerCodec),
        ))),
    );
}

#[test]
fn rary_graph_runs_encrypted() {
    for fanout in [2, 4] {
        let kind = PointerKind::MultiWriteRary {
            branching_factor: fanout,
        };
        encrypted_graph::<_, _, _, 64>(
            kind,
            RaryPointers::new(fanout).unwrap(),
            FixedSizeCodec::new(RaryCellValueCodec::new(graph_codec(
                RaryGraphPointerCodec::new(fanout),
            ))),
        );
        encrypted_graph::<_, _, _, 64>(
            kind,
            CachedPointers::new(RaryPointers::new(fanout).unwrap()),
            FixedSizeCodec::new(RaryCellValueCodec::new(graph_codec(
                CachedGraphPointerCodec::new(RaryGraphPointerCodec::new(fanout)),
            ))),
        );
    }
}

#[test]
fn original_graph_runs_encrypted() {
    // An original-pointer record needs 89 bytes for one outgoing slot.
    let kind = PointerKind::Original;
    encrypted_graph::<_, _, _, 128>(
        kind,
        OriginalPointers,
        FixedSizeCodec::new(OriginalCellValueCodec::new(graph_codec(
            OriginalGraphPointerCodec,
        ))),
    );
    encrypted_graph::<_, _, _, 128>(
        kind,
        CachedPointers::new(OriginalPointers),
        FixedSizeCodec::new(OriginalCellValueCodec::new(graph_codec(
            CachedGraphPointerCodec::new(OriginalGraphPointerCodec),
        ))),
    );
}
