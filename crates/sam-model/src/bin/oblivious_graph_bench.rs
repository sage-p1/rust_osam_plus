//! Native oblivious-graph benchmark driver.
//!
//! One invocation parses the graph, builds it once, then runs each requested
//! algorithm in order on that same graph (as the Python
//! `benchmark_single_alg.py` does). Output is line-oriented `key=value`
//! records so launchers can parse it without regexes:
//!
//! ```text
//! config impl=native mode=dry-run pointer=multiwriterary cache=false move=true block_size=4096 ... build=static prime=false dynamic_ops=0
//! build allocations=.. reads=.. writes=.. flushes=.. nanos=.. install_nanos=.. installation_maximum_stash=..
//! prime walks=.. allocations=.. reads=.. writes=.. nanos=..            (with --prime)
//! dynamic ops=.. add_vertex=.. add_edge=.. delete_edge=.. delete_vertex=.. allocations=.. reads=.. writes=.. nanos=.. tombstones=..   (with --dynamic-ops N)
//! trial alg=bfs index=0 start=.. attempts=.. allocations=.. reads=.. writes=.. roundtrips=.. nanos=.. size=.. cost=none stash_peak=none
//! algorithm alg=bfs status=ok trials=50 requested=50 attempts=.. length=100 allocations=.. reads=.. writes=.. nanos=.. mean_allocations=.. var_allocations=.. ... mean_roundtrips=.. var_roundtrips=.. rejected_allocations=.. rejected_roundtrips=.. maximum_stash=.. maximum_cached_values=.. cache_cleared=..
//! structure phase=bfs name=RaryMultiWritePointer allocations=.. reads=.. writes=.. roundtrips=..
//!           (phases: build, prime, dynamic, then one per algorithm)
//! done
//! ```
//!
//! Only full-length runs are measured (see `run_one` for what full length
//! means per algorithm): start vertices are drawn until `requested` runs are
//! full length, at most 1000 runs per algorithm (rejection sampling;
//! `attempts` counts the draws). `status` is `ok` when every requested trial
//! was found, `short` when fewer were, and `failed` when none was (then the
//! record has no statistics and no `structure` records follow). So every trial
//! does the same amount of work and the run's fixed costs (the start
//! lookup, final deletions) are amortized over `length` steps. Rejected runs
//! still happen on the SAM but are excluded from every statistic, including
//! the per-algorithm `structure` records; their total is reported as
//! `rejected_*`. `mean_*` / `var_*` are per-run moments; per-step costs are
//! them divided by `length`.
//! Round trips are reads + writes, as in the Python parser (the launcher's
//! report charges the r-ary pointer reads only; reads and writes are always
//! reported separately).

use rand::{rngs::StdRng, Rng, SeedableRng};
use sam_model::{
    pointer::{
        CachedPointer, CachedPointers, FixedSizeCodec, MultiWriteCell, MultiWriteCellValueCodec,
        MultiWritePointer, MultiWritePointers, OriginalCell, OriginalCellValueCodec,
        OriginalPointer, OriginalPointers, PointerKind, RaryCell, RaryCellValueCodec, RaryPointer,
        RaryPointers, RecursivePointer, RecursivePointers, BalancedCell, BalancedCellValueCodec,
        BalancedPointer, BalancedPointers,
    },
    BalancedGraphPointerCodec,
    BlockCodec, CachedGraphPointerCodec, DryRunSam, GraphBackend, GraphInput, GraphLayout,
    GraphObject, GraphValueCodec, MultiWriteGraphPointerCodec, NoMovePointers, ObliviousGraph,
    OperationCounts, OriginalGraphPointerCodec, PathOsamSam, RaryGraphPointerCodec,
    RecursiveGraphPointerCodec, SamError, SingleAccessMachine, StructureStats, Tagged,
    TaggedGraphPointerCodec, WeightedEdge,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    error::Error,
    fmt::Display,
    fs,
    io::{self, BufWriter, Write},
    process,
    time::Instant,
};

type BenchResult<T> = Result<T, Box<dyn Error>>;

/// Algorithms accepted by `--alg`; `build` only constructs the graph.
/// Runs allowed per algorithm while looking for full-length runs. An
/// algorithm with no full-length run among them is reported as failed.
const MAX_ATTEMPTS: usize = 1000;

const ALGORITHMS: [&str; 9] = [
    "build", "rw", "bfs", "dfs", "dijkstra", "prim", "cd", "dtc", "pr",
];

#[derive(Debug)]
struct Config {
    graph: String,
    block_size: usize,
    pointer: String,
    pointer_branching_factor: Option<usize>,
    /// Algorithms in execution order, each with its own trial count.
    algorithms: Vec<(String, usize)>,
    walk_length: usize,
    max_steps: usize,
    max_neighbors: usize,
    damping_factor: f64,
    seed: u64,
    start: Option<u64>,
    cache: bool,
    /// Python's no-move (uncached) access pattern: see `NoMovePointers`.
    no_move: bool,
    crypto: bool,
    stash_size: u64,
    /// Keep per-address read/write-limit checks in the encrypted SAM. They are
    /// client state proportional to the number of addresses, so off by default.
    check_sam_policy: bool,
    /// Dry-run only: model `original` records that do not fit the block using
    /// Python's fanout instead of failing.
    pretend_original_fits: bool,
    /// Build with one bulk pass (`static`) or add_vertex/add_edge calls.
    dynamic_build: bool,
    /// Run the priming walks after the build.
    prime: bool,
    /// Random add/delete operations between the build and the algorithms.
    dynamic_ops: usize,
    output: Option<String>,
}

/// Graph-construction measurements reported before any trial.
#[derive(Debug)]
struct BuildInfo {
    operations: OperationCounts,
    /// Public write-burst flushes during construction (r-ary only; each is
    /// also counted as one read).
    flushes: u64,
    structures: BTreeMap<&'static str, StructureStats>,
    nanos: u128,
    install_nanos: Option<u128>,
    installation_maximum_stash: Option<u64>,
    /// Priming measurements (`--prime`), taken on the build SAM.
    prime: Option<Phase>,
}

/// SAM cost of one non-algorithm phase (priming, dynamic operations).
#[derive(Debug)]
struct Phase {
    operations: OperationCounts,
    structures: BTreeMap<&'static str, StructureStats>,
    before: BTreeMap<&'static str, StructureStats>,
    nanos: u128,
    count: u64,
}

/// What one trial did, as needed for the early-termination analysis.
#[derive(Debug)]
struct TrialOutcome {
    /// Vertices reached (traversals, walks), triangles (dtc), common
    /// neighbors (cd), or walk steps (pr).
    size: usize,
    /// Whether the trial ran to the algorithm's full length.
    full: bool,
    /// Total path/tree weight for dijkstra and prim.
    cost: Option<i64>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        process::exit(2);
    }
}

fn run() -> BenchResult<()> {
    let config = parse_args()?;
    let input = read_graph(&config.graph)?;
    if input.vertices.is_empty() {
        return Err("graph contains no vertices".into());
    }
    match &config.output {
        None => {
            let stdout = io::stdout();
            let mut out = BufWriter::new(stdout.lock());
            let result = run_with_output(&config, &input, &mut out);
            out.flush()?;
            result
        }
        Some(path) => {
            // Write to FILE.partial and rename only on success, so the
            // presence of FILE means a complete run.
            let partial = format!("{path}.partial");
            let mut out = BufWriter::new(fs::File::create(&partial)?);
            let result = run_with_output(&config, &input, &mut out);
            if let Err(error) = &result {
                writeln!(out, "error message={:?}", error.to_string())?;
            }
            out.flush()?;
            drop(out);
            result?;
            fs::rename(&partial, path)?;
            Ok(())
        }
    }
}

fn run_with_output(config: &Config, input: &GraphInput, out: &mut dyn Write) -> BenchResult<()> {
    let pointer_bf = config
        .pointer_branching_factor
        .unwrap_or_else(|| config.block_size.saturating_sub(16).div_euclid(8).min(64));
    match config.pointer.as_str() {
        "original" => {
            let kind = PointerKind::Original;
            let layout = match GraphLayout::for_pointer_kind(config.block_size, kind) {
                Ok(layout) => layout,
                // Dry-run only (parse_args rejects --crypto with this flag):
                // the in-memory SAM never encodes blocks, so the oversized
                // record can be modeled with Python's fanout.
                Err(_) if config.pretend_original_fits => {
                    GraphLayout::python_sized(config.block_size, kind)?
                }
                Err(error) => return Err(error.into()),
            };
            if config.no_move {
                run_selected::<Tagged<OriginalPointer>, _, _>(
                    config,
                    input,
                    out,
                    layout,
                    NoMovePointers::new(OriginalPointers),
                    DryRunSam::<OriginalCell<GraphObject<Tagged<OriginalPointer>>>>::new(
                        kind.access_policy(),
                    ),
                    None,
                    kind,
                    FixedSizeCodec::new(OriginalCellValueCodec::new(GraphValueCodec::new(
                        TaggedGraphPointerCodec::new(OriginalGraphPointerCodec),
                        layout.graph_branching_factor(),
                    )?)),
                )?
            } else if config.cache {
                run_selected::<CachedPointer<OriginalPointer>, _, _>(
                    config,
                    input,
                    out,
                    layout,
                    CachedPointers::new(OriginalPointers),
                    DryRunSam::<OriginalCell<GraphObject<CachedPointer<OriginalPointer>>>>::new(
                        kind.access_policy(),
                    ),
                    None,
                    kind,
                    FixedSizeCodec::new(OriginalCellValueCodec::new(GraphValueCodec::new(
                        CachedGraphPointerCodec::new(OriginalGraphPointerCodec),
                        layout.graph_branching_factor(),
                    )?)),
                )?
            } else {
                run_selected::<OriginalPointer, _, _>(
                    config,
                    input,
                    out,
                    layout,
                    OriginalPointers,
                    DryRunSam::<OriginalCell<GraphObject<OriginalPointer>>>::new(
                        kind.access_policy(),
                    ),
                    None,
                    kind,
                    FixedSizeCodec::new(OriginalCellValueCodec::new(GraphValueCodec::new(
                        OriginalGraphPointerCodec,
                        layout.graph_branching_factor(),
                    )?)),
                )?
            }
        }
        "multiwrite" => {
            let kind = PointerKind::MultiWrite;
            let layout = GraphLayout::for_pointer_kind(config.block_size, kind)?;
            if config.no_move {
                run_selected::<Tagged<MultiWritePointer>, _, _>(
                    config,
                    input,
                    out,
                    layout,
                    NoMovePointers::new(MultiWritePointers),
                    DryRunSam::<MultiWriteCell<GraphObject<Tagged<MultiWritePointer>>>>::new(
                        kind.access_policy(),
                    ),
                    None,
                    kind,
                    FixedSizeCodec::new(MultiWriteCellValueCodec::new(GraphValueCodec::new(
                        TaggedGraphPointerCodec::new(MultiWriteGraphPointerCodec),
                        layout.graph_branching_factor(),
                    )?)),
                )?
            } else if config.cache {
                run_selected::<CachedPointer<MultiWritePointer>, _, _>(
                    config,
                    input,
                    out,
                    layout,
                    CachedPointers::new(MultiWritePointers),
                    DryRunSam::<MultiWriteCell<GraphObject<CachedPointer<MultiWritePointer>>>>::new(
                        kind.access_policy(),
                    ),
                    None,
                    kind,
                    FixedSizeCodec::new(MultiWriteCellValueCodec::new(GraphValueCodec::new(
                        CachedGraphPointerCodec::new(MultiWriteGraphPointerCodec),
                        layout.graph_branching_factor(),
                    )?)),
                )?
            } else {
                run_selected::<MultiWritePointer, _, _>(
                    config,
                    input,
                    out,
                    layout,
                    MultiWritePointers,
                    DryRunSam::<MultiWriteCell<GraphObject<MultiWritePointer>>>::new(
                        kind.access_policy(),
                    ),
                    None,
                    kind,
                    FixedSizeCodec::new(MultiWriteCellValueCodec::new(GraphValueCodec::new(
                        MultiWriteGraphPointerCodec,
                        layout.graph_branching_factor(),
                    )?)),
                )?
            }
        }
        "multiwriterary" | "multiwrite_rary" => {
            let kind = PointerKind::MultiWriteRary {
                branching_factor: pointer_bf,
            };
            let layout = GraphLayout::for_pointer_kind(config.block_size, kind)?;
            if config.no_move {
                run_selected::<Tagged<RaryPointer>, _, _>(
                    config,
                    input,
                    out,
                    layout,
                    NoMovePointers::new(RaryPointers::new(pointer_bf)?),
                    DryRunSam::<RaryCell<GraphObject<Tagged<RaryPointer>>>>::new(
                        kind.access_policy(),
                    ),
                    Some(pointer_bf),
                    kind,
                    FixedSizeCodec::new(RaryCellValueCodec::new(GraphValueCodec::new(
                        TaggedGraphPointerCodec::new(RaryGraphPointerCodec::new(pointer_bf)),
                        layout.graph_branching_factor(),
                    )?)),
                )?
            } else if config.cache {
                run_selected::<CachedPointer<RaryPointer>, _, _>(
                    config,
                    input,
                    out,
                    layout,
                    CachedPointers::new(RaryPointers::new(pointer_bf)?),
                    DryRunSam::<RaryCell<GraphObject<CachedPointer<RaryPointer>>>>::new(
                        kind.access_policy(),
                    ),
                    Some(pointer_bf),
                    kind,
                    FixedSizeCodec::new(RaryCellValueCodec::new(GraphValueCodec::new(
                        CachedGraphPointerCodec::new(RaryGraphPointerCodec::new(pointer_bf)),
                        layout.graph_branching_factor(),
                    )?)),
                )?
            } else {
                run_selected::<RaryPointer, _, _>(
                    config,
                    input,
                    out,
                    layout,
                    RaryPointers::new(pointer_bf)?,
                    DryRunSam::<RaryCell<GraphObject<RaryPointer>>>::new(kind.access_policy()),
                    Some(pointer_bf),
                    kind,
                    FixedSizeCodec::new(RaryCellValueCodec::new(GraphValueCodec::new(
                        RaryGraphPointerCodec::new(pointer_bf),
                        layout.graph_branching_factor(),
                    )?)),
                )?
            }
        }
        "balanced" | "balancedrary" => {
            // `balanced`: binary by default and unbuffered, every write a round
            // trip (compare with multiwrite). `balancedrary`: the r-ary default
            // fanout with buffered writes, charged reads only (compare with
            // multiwriterary).
            let buffered = config.pointer == "balancedrary";
            let b = if buffered {
                pointer_bf
            } else {
                config.pointer_branching_factor.unwrap_or(2)
            };
            let kind = PointerKind::Balanced {
                branching_factor: b,
                buffered_writes: buffered,
            };
            // The root's alias count, height and child-list address (9 bytes
            // more than multiwrite's) can leave fewer than two outgoing-edge
            // slots at small blocks. Dry-run only: the in-memory SAM never
            // encodes blocks, so lay the records out as for multiwrite (the
            // same fanout as OSAM+).
            let layout = match GraphLayout::for_pointer_kind(config.block_size, kind) {
                Ok(layout) if layout.graph_branching_factor() >= 2 => layout,
                Ok(_) | Err(_) if config.pretend_original_fits => {
                    GraphLayout::for_pointer_kind(config.block_size, PointerKind::MultiWrite)?
                        .marked_inexact()
                }
                Ok(_) => {
                    return Err(SamError::Backend(format!(
                        "block size {} leaves fewer than two outgoing-edge slots after the \
                         balanced pointer's root metadata",
                        config.block_size
                    ))
                    .into())
                }
                Err(error) => return Err(error.into()),
            };
            let backend = if buffered {
                BalancedPointers::new(b)?
            } else {
                BalancedPointers::new(b)?.unbuffered()
            };
            if config.no_move {
                run_selected::<Tagged<BalancedPointer>, _, _>(
                    config,
                    input,
                    out,
                    layout,
                    NoMovePointers::new(backend),
                    DryRunSam::<BalancedCell<GraphObject<Tagged<BalancedPointer>>>>::new(
                        kind.access_policy(),
                    ),
                    Some(b),
                    kind,
                    FixedSizeCodec::new(BalancedCellValueCodec::new(GraphValueCodec::new(
                        TaggedGraphPointerCodec::new(BalancedGraphPointerCodec),
                        layout.graph_branching_factor(),
                    )?)),
                )?
            } else if config.cache {
                run_selected::<CachedPointer<BalancedPointer>, _, _>(
                    config,
                    input,
                    out,
                    layout,
                    CachedPointers::new(backend),
                    DryRunSam::<BalancedCell<GraphObject<CachedPointer<BalancedPointer>>>>::new(
                        kind.access_policy(),
                    ),
                    Some(b),
                    kind,
                    FixedSizeCodec::new(BalancedCellValueCodec::new(GraphValueCodec::new(
                        CachedGraphPointerCodec::new(BalancedGraphPointerCodec),
                        layout.graph_branching_factor(),
                    )?)),
                )?
            } else {
                run_selected::<BalancedPointer, _, _>(
                    config,
                    input,
                    out,
                    layout,
                    backend,
                    DryRunSam::<BalancedCell<GraphObject<BalancedPointer>>>::new(
                        kind.access_policy(),
                    ),
                    Some(b),
                    kind,
                    FixedSizeCodec::new(BalancedCellValueCodec::new(GraphValueCodec::new(
                        BalancedGraphPointerCodec,
                        layout.graph_branching_factor(),
                    )?)),
                )?
            }
        }
        "recursive" => {
            let kind = PointerKind::Recursive;
            let layout = GraphLayout::for_pointer_kind(config.block_size, kind)?;
            if config.no_move {
                run_selected::<Tagged<RecursivePointer>, _, _>(
                    config,
                    input,
                    out,
                    layout,
                    NoMovePointers::new(RecursivePointers::default()),
                    DryRunSam::<GraphObject<Tagged<RecursivePointer>>>::new(kind.access_policy()),
                    None,
                    kind,
                    FixedSizeCodec::new(GraphValueCodec::new(
                        TaggedGraphPointerCodec::new(RecursiveGraphPointerCodec),
                        layout.graph_branching_factor(),
                    )?),
                )?
            } else if config.cache {
                run_selected::<CachedPointer<RecursivePointer>, _, _>(
                    config,
                    input,
                    out,
                    layout,
                    CachedPointers::new(RecursivePointers::default()),
                    DryRunSam::<GraphObject<CachedPointer<RecursivePointer>>>::new(
                        kind.access_policy(),
                    ),
                    None,
                    kind,
                    FixedSizeCodec::new(GraphValueCodec::new(
                        CachedGraphPointerCodec::new(RecursiveGraphPointerCodec),
                        layout.graph_branching_factor(),
                    )?),
                )?
            } else {
                run_selected::<RecursivePointer, _, _>(
                    config,
                    input,
                    out,
                    layout,
                    RecursivePointers::default(),
                    DryRunSam::<GraphObject<RecursivePointer>>::new(kind.access_policy()),
                    None,
                    kind,
                    FixedSizeCodec::new(GraphValueCodec::new(
                        RecursiveGraphPointerCodec,
                        layout.graph_branching_factor(),
                    )?),
                )?
            }
        }
        unknown => return Err(format!("unknown pointer backend {unknown:?}").into()),
    };
    writeln!(out, "done")?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_selected<P, B, C>(
    config: &Config,
    input: &GraphInput,
    out: &mut dyn Write,
    layout: GraphLayout,
    backend: B,
    dry: DryRunSam<B::Cell>,
    pointer_branching_factor: Option<usize>,
    kind: PointerKind,
    codec: C,
) -> BenchResult<()>
where
    P: Clone,
    B: GraphBackend<P>,
    C: BlockCodec<B::Cell, 64> + BlockCodec<B::Cell, 4096>,
{
    if !config.crypto {
        return run_backend(
            config,
            input,
            out,
            layout,
            backend,
            dry,
            pointer_branching_factor,
        );
    }
    match config.block_size {
        64 => run_encrypted_backend::<P, _, _, 64>(
            config,
            input,
            out,
            layout,
            backend,
            dry,
            pointer_branching_factor,
            kind,
            codec,
        ),
        4096 => run_encrypted_backend::<P, _, _, 4096>(
            config,
            input,
            out,
            layout,
            backend,
            dry,
            pointer_branching_factor,
            kind,
            codec,
        ),
        _ => Err(SamError::InvalidParameter(
            "encrypted graph runs currently support block sizes 64 and 4096",
        )
        .into()),
    }
}

fn run_backend<P, B>(
    config: &Config,
    input: &GraphInput,
    out: &mut dyn Write,
    layout: GraphLayout,
    mut backend: B,
    mut sam: DryRunSam<B::Cell>,
    pointer_branching_factor: Option<usize>,
) -> BenchResult<()>
where
    P: Clone,
    B: GraphBackend<P>,
{
    let (mut graph, build) = build_graph(config, input, layout, &mut backend, &mut sam)?;
    run_trials(
        config,
        input,
        out,
        layout,
        &mut backend,
        &mut sam,
        &mut graph,
        pointer_branching_factor,
        build,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_encrypted_backend<P, B, C, const BLOCK: usize>(
    config: &Config,
    input: &GraphInput,
    out: &mut dyn Write,
    layout: GraphLayout,
    mut backend: B,
    mut dry: DryRunSam<B::Cell>,
    pointer_branching_factor: Option<usize>,
    kind: PointerKind,
    codec: C,
) -> BenchResult<()>
where
    P: Clone,
    B: GraphBackend<P>,
    C: BlockCodec<B::Cell, BLOCK>,
{
    let (mut graph, mut build) = build_graph(config, input, layout, &mut backend, &mut dry)?;
    let install_clock = Instant::now();
    let snapshot = dry.snapshot();
    let allocated = snapshot.next_identifier.saturating_sub(1).max(2);
    let block_capacity = allocated
        .checked_next_power_of_two()
        .ok_or_else(|| SamError::Backend("OSAM block capacity overflow".into()))?;
    let mut encrypted = PathOsamSam::<B::Cell, _, BLOCK, 4, 1>::from_snapshot(
        snapshot,
        block_capacity,
        config.stash_size,
        true,
        kind.access_policy(),
        kind.access_strategy(),
        true,
        config.seed,
        codec,
    )?
    .with_policy_checks(config.check_sam_policy);
    build.install_nanos = Some(install_clock.elapsed().as_nanos());
    build.installation_maximum_stash = Some(encrypted.build_max_stash_occupancy());
    run_trials(
        config,
        input,
        out,
        layout,
        &mut backend,
        &mut encrypted,
        &mut graph,
        pointer_branching_factor,
        build,
    )
}

/// Builds the graph (statically or by dynamic insertions) and optionally
/// primes it, measuring both on the build SAM.
fn build_graph<P, B, S>(
    config: &Config,
    input: &GraphInput,
    layout: GraphLayout,
    backend: &mut B,
    sam: &mut S,
) -> BenchResult<(ObliviousGraph<P>, BuildInfo)>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    let clock = Instant::now();
    let mut graph = if config.dynamic_build {
        ObliviousGraph::build_dynamic(input, layout, backend, sam)?
    } else {
        ObliviousGraph::build_static(input, layout, backend, sam)?
    };
    let mut build = BuildInfo {
        operations: sam.stats().operations,
        flushes: sam.stats().flushes,
        structures: sam.stats().by_structure.clone(),
        nanos: clock.elapsed().as_nanos(),
        install_nanos: None,
        installation_maximum_stash: None,
        prime: None,
    };
    if config.prime {
        let mut rng = StdRng::seed_from_u64(config.seed ^ PRIME_SEED);
        let clock = Instant::now();
        let walks = graph.prime(&mut rng, backend, sam)?;
        build.prime = Some(Phase {
            operations: delta(sam.stats().operations, build.operations),
            structures: sam.stats().by_structure.clone(),
            before: build.structures.clone(),
            nanos: clock.elapsed().as_nanos(),
            count: walks,
        });
    }
    Ok((graph, build))
}

const PRIME_SEED: u64 = 0x5052_494d_4531;
const DYNAMIC_SEED: u64 = 0x4459_4e41_4d31;

/// The benchmark driver's view of the graph, used only to generate valid
/// random operations and start vertices; the oblivious graph never reads it.
struct Workload {
    /// Live vertex names, in a stable order for seeded choices.
    live: Vec<u64>,
    /// Outgoing destination names of each live vertex, in edge order.
    edges: HashMap<u64, Vec<u64>>,
    next_name: u64,
}

impl Workload {
    fn new(input: &GraphInput) -> Self {
        let mut edges: HashMap<u64, Vec<u64>> = input
            .vertices
            .iter()
            .map(|name| (*name, Vec::new()))
            .collect();
        for edge in &input.edges {
            edges.entry(edge.source).or_default().push(edge.destination);
        }
        Self {
            live: input.vertices.clone(),
            edges,
            next_name: input.vertices.iter().max().map_or(0, |name| name + 1),
        }
    }

    fn random_vertex<R: Rng>(&self, rng: &mut R) -> u64 {
        self.live[rng.gen_range(0..self.live.len())]
    }
}

/// Counts of each dynamic operation kind.
#[derive(Debug, Default)]
struct DynamicCounts {
    add_vertex: u64,
    add_edge: u64,
    delete_edge: u64,
    delete_vertex: u64,
}

/// Runs `count` seeded random operations: 40% add_edge, 30% delete_edge,
/// 20% add_vertex, 10% delete_vertex (at least two vertices stay live; an
/// impossible delete_edge becomes an add_edge).
fn run_dynamic_ops<P, B, S>(
    config: &Config,
    workload: &mut Workload,
    graph: &mut ObliviousGraph<P>,
    backend: &mut B,
    sam: &mut S,
) -> BenchResult<DynamicCounts>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    let mut rng = StdRng::seed_from_u64(config.seed ^ DYNAMIC_SEED);
    let mut counts = DynamicCounts::default();
    for _ in 0..config.dynamic_ops {
        let roll = rng.gen_range(0..10);
        let with_edges = workload
            .live
            .iter()
            .copied()
            .filter(|name| !workload.edges[name].is_empty())
            .collect::<Vec<_>>();
        if (4..7).contains(&roll) && !with_edges.is_empty() {
            let source = with_edges[rng.gen_range(0..with_edges.len())];
            let targets = workload.edges.get_mut(&source).unwrap();
            let index = rng.gen_range(0..targets.len());
            let destination = targets[index];
            if !graph.delete_edge(source, destination, backend, sam)? {
                return Err(format!("edge {source}->{destination} was not found").into());
            }
            // The graph removes the first such edge, moving its last edge there.
            let first = targets
                .iter()
                .position(|name| *name == destination)
                .unwrap();
            targets.swap_remove(first);
            counts.delete_edge += 1;
        } else if (7..9).contains(&roll) {
            let name = workload.next_name;
            workload.next_name += 1;
            graph.add_vertex(name, backend, sam)?;
            workload.live.push(name);
            workload.edges.insert(name, Vec::new());
            counts.add_vertex += 1;
        } else if roll == 9 && workload.live.len() > 2 {
            let index = rng.gen_range(0..workload.live.len());
            let name = workload.live.remove(index);
            graph.delete_vertex(name, backend, sam)?;
            workload.edges.remove(&name);
            for targets in workload.edges.values_mut() {
                targets.retain(|target| *target != name);
            }
            counts.delete_vertex += 1;
        } else {
            let source = workload.random_vertex(&mut rng);
            let destination = workload.random_vertex(&mut rng);
            let weight = rng.gen_range(1..=10);
            graph.add_edge(source, destination, weight, backend, sam)?;
            workload.edges.get_mut(&source).unwrap().push(destination);
            counts.add_edge += 1;
        }
    }
    Ok(counts)
}

/// Running mean and sample variance (Welford).
#[derive(Clone, Copy, Debug, Default)]
struct Moments {
    count: u64,
    mean: f64,
    m2: f64,
}

impl Moments {
    fn push(&mut self, value: f64) {
        self.count += 1;
        let delta = value - self.mean;
        self.mean += delta / self.count as f64;
        self.m2 += delta * (value - self.mean);
    }

    fn mean(&self) -> Option<f64> {
        (self.count > 0).then_some(self.mean)
    }

    fn variance(&self) -> Option<f64> {
        (self.count > 1).then(|| self.m2 / (self.count - 1) as f64)
    }
}

/// Per-run moments of allocations, reads, writes and round trips.
///
/// Round trips follow the Python parser's definition: reads + writes.
#[derive(Clone, Copy, Debug, Default)]
struct RunMoments {
    allocations: Moments,
    reads: Moments,
    writes: Moments,
    roundtrips: Moments,
}

impl RunMoments {
    fn push(&mut self, step: OperationCounts) {
        self.allocations.push(step.allocations as f64);
        self.reads.push(step.reads as f64);
        self.writes.push(step.writes as f64);
        self.roundtrips.push((step.reads + step.writes) as f64);
    }

    fn fields(&self) -> String {
        let number = |value: Option<f64>| opt(value.map(|value| format!("{value:.4}")));
        let field = |name: &str, moments: &Moments| {
            format!(
                "mean_{name}={} var_{name}={}",
                number(moments.mean()),
                number(moments.variance())
            )
        };
        [
            field("allocations", &self.allocations),
            field("reads", &self.reads),
            field("writes", &self.writes),
            field("roundtrips", &self.roundtrips),
        ]
        .join(" ")
    }
}

fn add(total: OperationCounts, run: OperationCounts) -> OperationCounts {
    OperationCounts {
        allocations: total.allocations + run.allocations,
        reads: total.reads + run.reads,
        writes: total.writes + run.writes,
    }
}

fn delta(after: OperationCounts, before: OperationCounts) -> OperationCounts {
    OperationCounts {
        allocations: after.allocations - before.allocations,
        reads: after.reads - before.reads,
        writes: after.writes - before.writes,
    }
}

fn write_structures(
    out: &mut dyn Write,
    phase: &str,
    after: &BTreeMap<&'static str, StructureStats>,
    before: &BTreeMap<&'static str, StructureStats>,
) -> io::Result<()> {
    for (name, stats) in after {
        let base = before.get(name).copied().unwrap_or_default();
        let (allocations, reads, writes) = (
            stats.allocations - base.allocations,
            stats.reads - base.reads,
            stats.writes - base.writes,
        );
        if allocations + reads + writes == 0 {
            continue;
        }
        writeln!(
            out,
            "structure phase={phase} name={name} allocations={allocations} reads={reads} \
             writes={writes} roundtrips={}",
            reads + writes
        )?;
    }
    Ok(())
}

/// Renders an optional value as a record field, using `none` when absent.
fn opt<T: Display>(value: Option<T>) -> String {
    value.map_or_else(|| "none".to_string(), |value| value.to_string())
}

#[allow(clippy::too_many_arguments)]
fn run_trials<P, B, S>(
    config: &Config,
    input: &GraphInput,
    out: &mut dyn Write,
    layout: GraphLayout,
    backend: &mut B,
    sam: &mut S,
    graph: &mut ObliviousGraph<P>,
    pointer_branching_factor: Option<usize>,
    build: BuildInfo,
) -> BenchResult<()>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    writeln!(
        out,
        "config impl=native mode={} pointer={} cache={} move={} block_size={} pointer_branching_factor={} \
         graph_branching_factor={} vertices={} edges={} seed={} walk_length={} max_steps={} \
         max_neighbors={} damping_factor={} stash_size={} sam_policy_checks={} layout={} \
         build={} prime={} dynamic_ops={}",
        if config.crypto { "crypto" } else { "dry-run" },
        config.pointer,
        config.cache,
        !config.no_move,
        config.block_size,
        opt(pointer_branching_factor),
        layout.graph_branching_factor(),
        input.vertices.len(),
        input.edges.len(),
        config.seed,
        config.walk_length,
        config.max_steps,
        config.max_neighbors,
        config.damping_factor,
        opt(config.crypto.then_some(config.stash_size)),
        // The dry-run SAM always enforces the policy; it is the reference model.
        !config.crypto || config.check_sam_policy,
        if layout.is_exact() {
            "exact"
        } else {
            "python-sized"
        },
        if config.dynamic_build {
            "dynamic"
        } else {
            "static"
        },
        config.prime,
        config.dynamic_ops,
    )?;
    writeln!(
        out,
        "build allocations={} reads={} writes={} flushes={} nanos={} install_nanos={} \
         installation_maximum_stash={}",
        build.operations.allocations,
        build.operations.reads,
        build.operations.writes,
        build.flushes,
        build.nanos,
        opt(build.install_nanos),
        opt(build.installation_maximum_stash),
    )?;
    write_structures(out, "build", &build.structures, &BTreeMap::new())?;
    if let Some(prime) = &build.prime {
        writeln!(
            out,
            "prime walks={} allocations={} reads={} writes={} nanos={}",
            prime.count,
            prime.operations.allocations,
            prime.operations.reads,
            prime.operations.writes,
            prime.nanos,
        )?;
        write_structures(out, "prime", &prime.structures, &prime.before)?;
    }
    out.flush()?;

    let mut workload = Workload::new(input);
    if config.dynamic_ops > 0 {
        backend.clear_cache()?;
        let before = sam.stats().operations;
        let structures_before = sam.stats().by_structure.clone();
        let clock = Instant::now();
        let counts = run_dynamic_ops(config, &mut workload, graph, backend, sam)?;
        let nanos = clock.elapsed().as_nanos();
        let operations = delta(sam.stats().operations, before);
        writeln!(
            out,
            "dynamic ops={} add_vertex={} add_edge={} delete_edge={} delete_vertex={} \
             allocations={} reads={} writes={} nanos={nanos} tombstones={}",
            config.dynamic_ops,
            counts.add_vertex,
            counts.add_edge,
            counts.delete_edge,
            counts.delete_vertex,
            operations.allocations,
            operations.reads,
            operations.writes,
            graph.tombstones(),
        )?;
        write_structures(
            out,
            "dynamic",
            &sam.stats().by_structure,
            &structures_before,
        )?;
        out.flush()?;
    }
    if let Some(start) = config.start {
        if !workload.live.contains(&start) {
            return Err(format!("start vertex {start} is not in the graph").into());
        }
    }
    for (algorithm, trials) in &config.algorithms {
        if algorithm == "build" {
            continue;
        }
        // Start every algorithm with an empty client cache, so no algorithm
        // benefits from values cached by graph construction or its predecessor.
        let cache_cleared = backend.clear_cache()?;
        // Each algorithm gets the same seeded start-vertex sequence, so the
        // first N trials of a longer run use the starts of an N-trial run.
        let mut rng = rand::rngs::StdRng::seed_from_u64(config.seed);
        let length = run_length(config, algorithm);
        let mut attempts = 0;
        let mut accepted_trials = 0;
        let mut moments = RunMoments::default();
        let mut accepted = OperationCounts::default();
        let mut rejected = OperationCounts::default();
        let mut structures: BTreeMap<&'static str, StructureStats> = BTreeMap::new();
        let mut nanos_total = 0;
        let mut maximum_stash: Option<u64> = None;
        // Rejection sampling: only full-length runs are measured, so a run's
        // fixed costs (the start lookup, final deletions) are amortized over
        // the same number of steps in every trial. At most MAX_ATTEMPTS runs
        // per algorithm; a fixed start gives the same run every time except
        // for the random walk, so a short run from it ends the search.
        let mut trial_attempts = 0;
        while accepted_trials < *trials && attempts < MAX_ATTEMPTS {
            attempts += 1;
            trial_attempts += 1;
            let start = config
                .start
                .unwrap_or_else(|| workload.random_vertex(&mut rng));
            sam.reset_stash_maximum();
            let before = sam.stats().operations;
            let structures_before = sam.stats().by_structure.clone();
            let clock = Instant::now();
            let outcome = run_one(
                config,
                algorithm,
                start,
                &workload.live,
                graph,
                &mut rng,
                backend,
                sam,
            )?;
            let nanos = clock.elapsed().as_nanos();
            let run = delta(sam.stats().operations, before);
            if !outcome.full {
                rejected = add(rejected, run);
                if config.start.is_some() && algorithm != "rw" {
                    break;
                }
                continue;
            }
            accepted = add(accepted, run);
            moments.push(run);
            nanos_total += nanos;
            for (name, stats) in &sam.stats().by_structure {
                let base = structures_before.get(name).copied().unwrap_or_default();
                let total = structures.entry(name).or_default();
                total.allocations += stats.allocations - base.allocations;
                total.reads += stats.reads - base.reads;
                total.writes += stats.writes - base.writes;
            }
            let stash_peak = sam.stats().stash.map(|stash| stash.maximum);
            if let Some(peak) = stash_peak {
                maximum_stash = Some(maximum_stash.map_or(peak, |maximum| maximum.max(peak)));
            }
            writeln!(
                out,
                "trial alg={algorithm} index={accepted_trials} start={start} \
                 attempts={trial_attempts} allocations={} reads={} writes={} roundtrips={} \
                 nanos={nanos} size={} cost={} stash_peak={}",
                run.allocations,
                run.reads,
                run.writes,
                run.reads + run.writes,
                outcome.size,
                opt(outcome.cost),
                opt(stash_peak),
            )?;
            accepted_trials += 1;
            trial_attempts = 0;
        }
        // `ok`: every requested trial is a full-length run; `short`: some
        // were found, but fewer than requested; `failed`: none.
        let status = if accepted_trials == *trials {
            "ok"
        } else if accepted_trials > 0 {
            "short"
        } else {
            "failed"
        };
        if status != "ok" {
            eprintln!(
                "warning: {algorithm} {status}: {accepted_trials} of {trials} runs were full \
                 length after {attempts} attempts"
            );
        }
        // Whole-run statistics over the accepted (full-length) runs; per-step
        // costs are these divided by `length`.
        writeln!(
            out,
            "algorithm alg={algorithm} status={status} trials={accepted_trials} \
             requested={trials} attempts={attempts} length={length} \
             allocations={} reads={} writes={} nanos={nanos_total} {} \
             rejected_allocations={} rejected_roundtrips={} maximum_stash={} \
             maximum_cached_values={} cache_cleared={cache_cleared}",
            accepted.allocations,
            accepted.reads,
            accepted.writes,
            moments.fields(),
            rejected.allocations,
            rejected.reads + rejected.writes,
            opt(maximum_stash),
            opt(backend.max_cached_values()),
        )?;
        write_structures(out, algorithm, &structures, &BTreeMap::new())?;
        out.flush()?;
    }
    Ok(())
}

/// Nominal length of a full run, the unit of per-step costs: walk moves (rw,
/// pr), visited vertices (bfs, dfs, dijkstra, prim), neighbor-list
/// retrievals (cd: two), or the whole run (dtc, which has no natural step).
fn run_length(config: &Config, algorithm: &str) -> usize {
    match algorithm {
        "rw" | "pr" => config.walk_length,
        "bfs" | "dfs" | "dijkstra" | "prim" => config.max_steps,
        "cd" => 2,
        _ => 1,
    }
}

/// Runs one trial and decides whether it ran to full length:
///
/// * `rw`: the walk took all `walk_length` steps (no sink was hit);
/// * `bfs`, `dfs`, `dijkstra`, `prim`: `max_steps` vertices were visited
///   (the frontier did not empty first);
/// * `dtc`: every neighbor retrieval returned `max_neighbors` entries;
/// * `cd`, `pr`: fixed work, always full.
#[allow(clippy::too_many_arguments)]
fn run_one<P, B, S, R>(
    config: &Config,
    algorithm: &str,
    start: u64,
    live: &[u64],
    graph: &mut ObliviousGraph<P>,
    rng: &mut R,
    backend: &mut B,
    sam: &mut S,
) -> Result<TrialOutcome, SamError>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
    R: Rng,
{
    let steps = config.max_steps;
    let traversal = |size: usize| TrialOutcome {
        size,
        full: size == steps,
        cost: None,
    };
    Ok(match algorithm {
        "rw" => {
            let size = graph
                .random_walk(start, config.walk_length, rng, backend, sam)?
                .len();
            TrialOutcome {
                size,
                full: size == config.walk_length + 1,
                cost: None,
            }
        }
        "bfs" => traversal(graph.bfs(start, Some(steps), backend, sam)?.len()),
        "dfs" => traversal(graph.dfs(start, Some(steps), backend, sam)?.len()),
        "dijkstra" => {
            let result = graph.dijkstra(start, Some(steps), backend, sam)?;
            TrialOutcome {
                cost: Some(result.total_cost),
                ..traversal(result.costs.len())
            }
        }
        "prim" => {
            let result = graph.prim(start, Some(steps), backend, sam)?;
            TrialOutcome {
                cost: Some(result.total_cost),
                ..traversal(result.edges.len() + 1)
            }
        }
        "cd" => {
            let mut second = start;
            while live.len() > 1 && second == start {
                second = live[rng.gen_range(0..live.len())];
            }
            TrialOutcome {
                size: graph.contact_discovery(start, second, backend, sam)?.len(),
                full: true,
                cost: None,
            }
        }
        "dtc" => {
            let result = graph.directed_triangle_count_detailed(
                start,
                Some(config.max_neighbors),
                backend,
                sam,
            )?;
            TrialOutcome {
                size: result.triangles.len(),
                full: result.complete,
                cost: None,
            }
        }
        "pr" => {
            let result =
                graph.pagerank(config.walk_length, config.damping_factor, rng, backend, sam)?;
            TrialOutcome {
                size: result.visits.values().sum(),
                full: true,
                cost: None,
            }
        }
        _ => unreachable!("algorithm names are validated in parse_args"),
    })
}

fn read_graph(path: &str) -> Result<GraphInput, Box<dyn std::error::Error>> {
    let source = fs::read_to_string(path)?;
    let mut vertices = BTreeSet::new();
    let mut edges = Vec::new();
    for (line_number, raw) in source.lines().enumerate() {
        let line = raw.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let fields = line
            .split(|character: char| character == ',' || character.is_ascii_whitespace())
            .filter(|field| !field.is_empty())
            .collect::<Vec<_>>();
        if fields[0] == "v" {
            if fields.len() != 2 {
                return Err(format!("line {}: expected `v ID`", line_number + 1).into());
            }
            vertices.insert(fields[1].parse::<u64>()?);
            continue;
        }
        if !(2..=3).contains(&fields.len()) {
            return Err(format!("line {}: expected `SRC DST [WEIGHT]`", line_number + 1).into());
        }
        let source = fields[0].parse::<u64>()?;
        let destination = fields[1].parse::<u64>()?;
        let weight = fields.get(2).map_or(Ok(0), |value| value.parse::<i64>())?;
        vertices.insert(source);
        vertices.insert(destination);
        edges.push(WeightedEdge {
            source,
            destination,
            weight,
        });
    }
    Ok(GraphInput::new(vertices, edges))
}

const HELP: &str = "\
oblivious_graph_bench --graph FILE --bs BYTES --pt POINTER [options]

  --pt original|multiwrite|multiwriterary|recursive|balanced|balancedrary
                      balanced: complete binary tree with downward pointers, every
                      write a round trip (b = --pointer-branching-factor, default 2);
                      balancedrary: the same with the r-ary default fanout and
                      buffered writes (charged reads only, like multiwriterary)
  --alg LIST          comma-separated ALG[:TRIALS]; may be repeated. ALG is one of
                      build rw bfs dfs dijkstra prim cd dtc pr. The graph is built
                      once and the algorithms run in order. Default: rw
  --trials N          trials for entries without an explicit :TRIALS (default 1).
                      Only full-length runs count: start vertices are redrawn until
                      N runs are full length, at most 1000 runs per algorithm; an
                      algorithm with none is reported as status=failed
  --wl N              walk length for rw and pr (default 50)
  --max-steps N       visited-vertex cap for bfs/dfs/dijkstra/prim (default 100)
  --max-neighbors N   per-level neighbor cap for dtc (default 5)
  --df X              PageRank damping factor (default 0.9)
  --seed N            RNG seed (default 1)
  --start ID          fixed start vertex instead of random starts (a short run
                      from it fails the algorithm, except rw, which is retried)
  --pointer-branching-factor N   r-ary pointer fanout (even, >= 2)
  --cache | --move    enable / disable the client pointer cache (default --move).
                      Accesses always move (read, work on, and write each object
                      once), so the cache changes no counts.
  --no-move           Python's no-move (uncached) access pattern: every access
                      copies the object's nested pointers, and every change is a
                      move + put (the paper's OSAM and ORAM series)
  --dry-run | --crypto [--stash-size N]   in-memory SAM (default) or Path OSAM+
  --pretend-original-fits   dry-run only: if `original` records cannot fit the block,
                      use Python's edge fanout (bs - 16) / 8 instead of failing; if
                      `balanced` root cells cannot fit, use multiwrite's layout. The
                      config record then reports layout=python-sized
  --build static|dynamic   build in one bulk pass (default) or by add_vertex/add_edge
                      calls in input order; both give the same graph
  --prime             after the build, run vertices/10 random walks of 50 steps
  --dynamic-ops N     after the build (and priming), run N seeded random add_vertex /
                      add_edge / delete_edge / delete_vertex operations
  --check-sam-policy  with --crypto, enforce per-address read/write limits (keeps
                      an O(#addresses) client table; off by default)
  --output FILE       write records to FILE (via FILE.partial, renamed on success)

Graph format: `v ID` for an isolated vertex or `SRC DST [WEIGHT]` per edge.";

fn canonical_algorithm(name: &str) -> String {
    match name {
        "random walk" | "random_walk" => "rw",
        "contact discovery" | "contact_discovery" => "cd",
        "directed triangle count" | "directed_triangle_count" => "dtc",
        "pagerank" => "pr",
        other => other,
    }
    .to_string()
}

fn parse_args() -> BenchResult<Config> {
    parse_arg_list(env::args().skip(1))
}

fn parse_arg_list(args: impl IntoIterator<Item = String>) -> BenchResult<Config> {
    let mut graph = None;
    let mut block_size = None;
    let mut pointer = None;
    let mut pointer_branching_factor = None;
    let mut algorithm_specs: Vec<String> = Vec::new();
    let mut trials = 1;
    let mut walk_length = 50;
    let mut max_steps = 100;
    let mut max_neighbors = 5;
    let mut damping_factor = 0.9;
    let mut seed = 1;
    let mut start = None;
    let mut cache = false;
    let mut no_move = false;
    let mut crypto = false;
    let mut stash_size: u64 = 40; // osam_plus::DEFAULT_STASH_OVERFLOW_SIZE
    let mut output = None;
    let mut check_sam_policy = false;
    let mut pretend_original_fits = false;
    let mut dynamic_build = false;
    let mut prime = false;
    let mut dynamic_ops = 0;
    let mut args = args.into_iter().collect::<Vec<_>>().into_iter();
    while let Some(argument) = args.next() {
        let value = |args: &mut std::vec::IntoIter<String>| {
            args.next()
                .ok_or_else(|| format!("missing value after {argument}"))
        };
        match argument.as_str() {
            "--graph" => graph = Some(value(&mut args)?),
            "--block-size" | "--bs" => block_size = Some(value(&mut args)?.parse()?),
            "--pointer" | "--pt" => pointer = Some(value(&mut args)?),
            "--pointer-branching-factor" => {
                pointer_branching_factor = Some(value(&mut args)?.parse()?)
            }
            "--algorithm" | "--alg" => algorithm_specs.push(value(&mut args)?),
            "--trials" => trials = value(&mut args)?.parse()?,
            "--walk-length" | "--wl" => walk_length = value(&mut args)?.parse()?,
            "--max-steps" | "--steps" => max_steps = value(&mut args)?.parse()?,
            "--max-neighbors" | "--neighbors" => max_neighbors = value(&mut args)?.parse()?,
            "--damping-factor" | "--df" => damping_factor = value(&mut args)?.parse()?,
            "--seed" => seed = value(&mut args)?.parse()?,
            "--start" => start = Some(value(&mut args)?.parse()?),
            "--cache" | "--caching" => cache = true,
            "--move" | "--no-cache" => cache = false,
            "--no-move" | "--no_move" => no_move = true,
            "--no-copies" | "--no_copies" | "--no-prime" | "--no_prime" => {}
            "--staticinsertion" => dynamic_build = false,
            "--prime" => prime = true,
            "--build" => {
                dynamic_build = match value(&mut args)?.as_str() {
                    "static" => false,
                    "dynamic" => true,
                    other => {
                        return Err(
                            format!("--build expects static or dynamic, not {other:?}").into()
                        )
                    }
                }
            }
            "--dynamic-ops" => dynamic_ops = value(&mut args)?.parse()?,
            "--dry-run" => crypto = false,
            "--crypto" | "--encrypted" => crypto = true,
            "--stash-size" => stash_size = value(&mut args)?.parse()?,
            "--output" => output = Some(value(&mut args)?),
            "--check-sam-policy" => check_sam_policy = true,
            "--pretend-original-fits" => pretend_original_fits = true,
            "--help" | "-h" => {
                println!("{HELP}");
                process::exit(0);
            }
            other => return Err(format!("unknown argument {other:?}").into()),
        }
    }
    if trials == 0 {
        return Err("trials must be positive".into());
    }
    if no_move && cache {
        return Err("--no-move and --cache exclude each other: the cache is a move pattern".into());
    }
    if pretend_original_fits && crypto {
        return Err("--pretend-original-fits is dry-run only: encrypted blocks must fit".into());
    }
    if max_steps == 0 {
        return Err("max_steps must be positive".into());
    }
    if max_neighbors == 0 {
        return Err("max_neighbors must be positive".into());
    }
    if !(0.0..=1.0).contains(&damping_factor) {
        return Err("damping_factor must be between zero and one".into());
    }
    if algorithm_specs.is_empty() {
        algorithm_specs.push("rw".to_string());
    }
    let mut algorithms = Vec::new();
    for spec in algorithm_specs.iter().flat_map(|spec| spec.split(',')) {
        let spec = spec.trim();
        if spec.is_empty() {
            continue;
        }
        let (name, count) = match spec.split_once(':') {
            Some((name, count)) => (name, count.parse::<usize>()?),
            None => (spec, trials),
        };
        let name = canonical_algorithm(name.trim());
        if !ALGORITHMS.contains(&name.as_str()) {
            return Err(format!("algorithm {name:?} is not migrated yet").into());
        }
        if count == 0 {
            return Err(format!("trials for {name} must be positive").into());
        }
        algorithms.push((name, count));
    }
    Ok(Config {
        graph: graph.ok_or("--graph is required")?,
        block_size: block_size.ok_or("--bs is required")?,
        pointer: pointer.ok_or("--pt is required")?,
        pointer_branching_factor,
        algorithms,
        walk_length,
        max_steps,
        max_neighbors,
        damping_factor,
        seed,
        start,
        cache,
        no_move,
        crypto,
        stash_size,
        check_sam_policy,
        pretend_original_fits,
        dynamic_build,
        prime,
        dynamic_ops,
        output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the benchmark on `edges` with `args` and returns its records.
    fn bench(edges: &str, args: &[&str]) -> BenchResult<Vec<String>> {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = env::temp_dir().join(format!(
            "oblivious_graph_bench_test_{}_{}.edgelist",
            process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::write(&path, edges)?;
        let mut list = vec!["--graph".to_string(), path.display().to_string()];
        list.extend(args.iter().map(|arg| arg.to_string()));
        let config = parse_arg_list(list)?;
        let input = read_graph(&config.graph)?;
        let mut out = Vec::new();
        let result = run_with_output(&config, &input, &mut out);
        fs::remove_file(&path)?;
        result?;
        Ok(String::from_utf8(out)?
            .lines()
            .map(str::to_string)
            .collect())
    }

    fn field<'a>(record: &'a str, key: &str) -> &'a str {
        record
            .split_whitespace()
            .find_map(|token| token.strip_prefix(&format!("{key}=")))
            .unwrap_or_else(|| panic!("{record} has no {key}"))
    }

    /// A directed 6-cycle (every start reaches all six vertices) plus a
    /// separate 2-cycle (a start there reaches only two).
    const TWO_COMPONENTS: &str = "0 1\n1 2\n2 3\n3 4\n4 5\n5 0\n6 7\n7 6\n";

    #[test]
    fn only_full_length_runs_are_measured() {
        let records = bench(
            TWO_COMPONENTS,
            &[
                "--bs",
                "4096",
                "--pt",
                "multiwrite",
                "--alg",
                "bfs,dijkstra,rw",
                "--trials",
                "20",
                "--max-steps",
                "4",
                "--wl",
                "5",
            ],
        )
        .unwrap();
        for algorithm in ["bfs", "dijkstra"] {
            let trials: Vec<_> = records
                .iter()
                .filter(|record| record.starts_with(&format!("trial alg={algorithm} ")))
                .collect();
            assert_eq!(trials.len(), 20);
            for trial in &trials {
                assert_eq!(field(trial, "size"), "4");
                assert!(field(trial, "start").parse::<u64>().unwrap() < 6);
            }
            let summary = records
                .iter()
                .find(|record| record.starts_with(&format!("algorithm alg={algorithm} ")))
                .unwrap();
            assert_eq!(field(summary, "status"), "ok");
            let attempts: usize = field(summary, "attempts").parse().unwrap();
            // A quarter of the starts are in the 2-cycle: some were rejected.
            assert!(attempts > 20, "{summary}");
            assert_eq!(field(summary, "length"), "4");
            assert!(
                field(summary, "rejected_roundtrips")
                    .parse::<u64>()
                    .unwrap()
                    > 0
            );
            let reads: f64 = field(summary, "reads").parse().unwrap();
            let mean: f64 = field(summary, "mean_reads").parse().unwrap();
            assert!((reads / 20.0 - mean).abs() < 1e-6);
        }
        // The graph has no sinks: every walk is full length.
        let summary = records
            .iter()
            .find(|record| record.starts_with("algorithm alg=rw "))
            .unwrap();
        assert_eq!(field(summary, "attempts"), "20");
    }

    #[test]
    fn a_fixed_short_start_fails_the_algorithm_after_one_run() {
        let records = bench(
            TWO_COMPONENTS,
            &[
                "--bs",
                "4096",
                "--pt",
                "recursive",
                "--alg",
                "bfs,cd",
                "--max-steps",
                "4",
                "--start",
                "6",
                "--trials",
                "3",
            ],
        )
        .unwrap();
        let bfs = records
            .iter()
            .find(|r| r.starts_with("algorithm alg=bfs "))
            .unwrap();
        assert_eq!(field(bfs, "status"), "failed");
        assert_eq!(field(bfs, "attempts"), "1");
        assert_eq!(field(bfs, "trials"), "0");
        assert_eq!(field(bfs, "mean_roundtrips"), "none");
        assert!(!records
            .iter()
            .any(|r| r.starts_with("structure phase=bfs ")));
        // The next algorithm still runs.
        let cd = records
            .iter()
            .find(|r| r.starts_with("algorithm alg=cd "))
            .unwrap();
        assert_eq!(field(cd, "status"), "ok");
    }

    #[test]
    fn an_algorithm_without_full_runs_fails_after_the_attempt_cap() {
        let records = bench(
            TWO_COMPONENTS,
            &[
                "--bs",
                "4096",
                "--pt",
                "recursive",
                "--alg",
                "bfs,rw",
                "--max-steps",
                "7",
                "--trials",
                "2",
                "--wl",
                "3",
            ],
        )
        .unwrap();
        let bfs = records
            .iter()
            .find(|r| r.starts_with("algorithm alg=bfs "))
            .unwrap();
        assert_eq!(field(bfs, "status"), "failed");
        assert_eq!(field(bfs, "attempts"), MAX_ATTEMPTS.to_string());
        assert_eq!(field(bfs, "requested"), "2");
        assert!(!records.iter().any(|r| r.starts_with("trial alg=bfs ")));
        let rw = records
            .iter()
            .find(|r| r.starts_with("algorithm alg=rw "))
            .unwrap();
        assert_eq!(field(rw, "status"), "ok");
        assert_eq!(records.last().unwrap(), "done");
    }

    #[test]
    fn too_few_full_runs_are_reported_short() {
        // Only starts in the 6-cycle (3/4 of the draws) give full runs, so
        // 1000 draws find fewer than 900.
        let records = bench(
            TWO_COMPONENTS,
            &[
                "--bs",
                "4096",
                "--pt",
                "recursive",
                "--alg",
                "bfs",
                "--max-steps",
                "6",
                "--trials",
                "900",
            ],
        )
        .unwrap();
        let bfs = records
            .iter()
            .find(|r| r.starts_with("algorithm alg=bfs "))
            .unwrap();
        assert_eq!(field(bfs, "status"), "short", "{bfs}");
        assert_eq!(field(bfs, "attempts"), MAX_ATTEMPTS.to_string());
        let trials: usize = field(bfs, "trials").parse().unwrap();
        assert!(trials > 0 && trials < 900);
    }
}
