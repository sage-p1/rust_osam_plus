//! Native oblivious-graph benchmark driver.
//!
//! One invocation parses the graph, builds it once, then runs each requested
//! algorithm in order on that same graph (as the Python
//! `benchmark_single_alg.py` does). Output is line-oriented `key=value`
//! records so launchers can parse it without regexes:
//!
//! ```text
//! config impl=native mode=dry-run pointer=multiwriterary cache=false move=true block_size=4096 ... build=static prime=false dynamic_ops=0
//! build allocations=.. reads=.. writes=.. nanos=.. install_nanos=.. installation_maximum_stash=..
//! prime walks=.. allocations=.. reads=.. writes=.. nanos=..            (with --prime)
//! dynamic ops=.. add_vertex=.. add_edge=.. delete_edge=.. delete_vertex=.. allocations=.. reads=.. writes=.. nanos=.. tombstones=..   (with --dynamic-ops N)
//! trial alg=bfs index=0 allocations=.. reads=.. writes=.. nanos=.. size=.. full=1 cost=none stash_peak=none
//! trial alg=bfs index=0 ... steps=.. tail_reads=.. tail_writes=.. cleanup_reads=.. cleanup_writes=..
//! algorithm alg=bfs trials=50 full_trials=.. allocations=.. reads=.. writes=.. nanos=.. maximum_stash=.. maximum_cached_values=..
//! steps alg=bfs count=.. mean_steps_per_trial=.. mean_reads=.. var_reads=.. ... mean_roundtrips=.. var_roundtrips=..
//! stepindex alg=bfs index=0 count=.. mean_reads=.. var_reads=.. ...
//! structure phase=bfs name=RaryMultiWritePointer allocations=.. reads=.. writes=.. roundtrips=..
//!           (phases: build, prime, dynamic, then one per algorithm)
//! done
//! ```
//!
//! `full=1` means the trial ran to the algorithm's full length rather than
//! terminating early (see `run_one`). Per-step statistics (`steps`,
//! `stepindex`, and `step` with `--step-records`) cover completed steps only
//! (see `sam_model::StepKind`): a trial's incomplete tail and its cleanup are
//! reported separately, so trials of different lengths are never mixed.
//! Round trips are reads + writes, as in the Python parser.

use rand::{rngs::StdRng, Rng, SeedableRng};
use sam_model::{
    pointer::{
        CachedPointer, CachedPointers, FixedSizeCodec, MultiWriteCell, MultiWriteCellValueCodec,
        MultiWritePointer, MultiWritePointers, OriginalCell, OriginalCellValueCodec,
        OriginalPointer, OriginalPointers, PointerKind, RaryCell, RaryCellValueCodec, RaryPointer,
        RaryPointers, RecursivePointer, RecursivePointers,
    },
    BlockCodec, CachedGraphPointerCodec, DryRunSam, GraphBackend, GraphInput, GraphLayout,
    GraphObject, GraphValueCodec, MultiWriteGraphPointerCodec, NoMovePointers, ObliviousGraph,
    OperationCounts, OriginalGraphPointerCodec, PathOsamSam, RaryGraphPointerCodec,
    RecursiveGraphPointerCodec, SamError, SingleAccessMachine, StepKind, StructureStats, Tagged,
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
    /// Also write one `step` record per completed step.
    step_records: bool,
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

/// Per-step moments of allocations, reads, writes and round trips.
///
/// Round trips follow the Python parser's definition: reads + writes.
#[derive(Clone, Copy, Debug, Default)]
struct StepMoments {
    allocations: Moments,
    reads: Moments,
    writes: Moments,
    roundtrips: Moments,
}

impl StepMoments {
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

fn delta(after: OperationCounts, before: OperationCounts) -> OperationCounts {
    OperationCounts {
        allocations: after.allocations - before.allocations,
        reads: after.reads - before.reads,
        writes: after.writes - before.writes,
    }
}

/// Splits one trial's step marks into completed steps, the incomplete tail
/// after the last completed step, and post-loop cleanup.
fn split_steps(
    before: OperationCounts,
    after: OperationCounts,
    marks: &[sam_model::StepMark],
) -> (Vec<OperationCounts>, OperationCounts, OperationCounts) {
    let mut steps = Vec::new();
    let mut previous = before;
    let mut tail_end = None;
    let mut cleanup_end = None;
    for mark in marks {
        match mark.kind {
            StepKind::Step => {
                steps.push(delta(mark.operations, previous));
                previous = mark.operations;
            }
            StepKind::Tail => tail_end = Some(mark.operations),
            StepKind::Cleanup => cleanup_end = Some(mark.operations),
        }
    }
    let tail_end = tail_end.unwrap_or(after);
    let tail = delta(tail_end, previous);
    let cleanup = delta(cleanup_end.unwrap_or(tail_end), tail_end);
    (steps, tail, cleanup)
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
        "build allocations={} reads={} writes={} nanos={} install_nanos={} \
         installation_maximum_stash={}",
        build.operations.allocations,
        build.operations.reads,
        build.operations.writes,
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
    graph.step_marks = Some(Vec::new());

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
        let algorithm_before = sam.stats().operations;
        let structures_before = sam.stats().by_structure.clone();
        let algorithm_clock = Instant::now();
        let mut full_trials = 0;
        let mut step_moments = StepMoments::default();
        let mut step_index_moments: Vec<StepMoments> = Vec::new();
        let mut steps_per_trial = Moments::default();
        let mut tail_roundtrips = Moments::default();
        let mut cleanup_roundtrips = Moments::default();
        let mut maximum_stash: Option<u64> = None;
        for index in 0..*trials {
            let start = config
                .start
                .unwrap_or_else(|| workload.random_vertex(&mut rng));
            sam.reset_stash_maximum();
            let before = sam.stats().operations;
            if let Some(marks) = graph.step_marks.as_mut() {
                marks.clear();
            }
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
            let after = sam.stats().operations;
            let stash_peak = sam.stats().stash.map(|stash| stash.maximum);
            if let Some(peak) = stash_peak {
                maximum_stash = Some(maximum_stash.map_or(peak, |maximum| maximum.max(peak)));
            }
            full_trials += usize::from(outcome.full);
            let (steps, tail, cleanup) =
                split_steps(before, after, graph.step_marks.as_deref().unwrap_or(&[]));
            for (step_index, step) in steps.iter().enumerate() {
                step_moments.push(*step);
                if step_index_moments.len() <= step_index {
                    step_index_moments.push(StepMoments::default());
                }
                step_index_moments[step_index].push(*step);
                if config.step_records {
                    writeln!(
                        out,
                        "step alg={algorithm} trial={index} index={step_index} allocations={} \
                         reads={} writes={} roundtrips={}",
                        step.allocations,
                        step.reads,
                        step.writes,
                        step.reads + step.writes,
                    )?;
                }
            }
            steps_per_trial.push(steps.len() as f64);
            tail_roundtrips.push((tail.reads + tail.writes) as f64);
            cleanup_roundtrips.push((cleanup.reads + cleanup.writes) as f64);
            writeln!(
                out,
                "trial alg={algorithm} index={index} allocations={} reads={} writes={} \
                 nanos={nanos} size={} full={} cost={} stash_peak={} steps={} \
                 tail_reads={} tail_writes={} cleanup_reads={} cleanup_writes={}",
                after.allocations - before.allocations,
                after.reads - before.reads,
                after.writes - before.writes,
                outcome.size,
                u8::from(outcome.full),
                opt(outcome.cost),
                opt(stash_peak),
                steps.len(),
                tail.reads,
                tail.writes,
                cleanup.reads,
                cleanup.writes,
            )?;
        }
        let after = sam.stats().operations;
        writeln!(
            out,
            "algorithm alg={algorithm} trials={trials} full_trials={full_trials} allocations={} \
             reads={} writes={} nanos={} maximum_stash={} maximum_cached_values={} \
             cache_cleared={cache_cleared}",
            after.allocations - algorithm_before.allocations,
            after.reads - algorithm_before.reads,
            after.writes - algorithm_before.writes,
            algorithm_clock.elapsed().as_nanos(),
            opt(maximum_stash),
            opt(backend.max_cached_values()),
        )?;
        // Per-step statistics over completed steps only: trials that stop
        // early contribute the steps they completed, never partial work.
        writeln!(
            out,
            "steps alg={algorithm} count={} mean_steps_per_trial={} {} \
             mean_tail_roundtrips={} mean_cleanup_roundtrips={}",
            step_moments.roundtrips.count,
            opt(steps_per_trial.mean().map(|value| format!("{value:.4}"))),
            step_moments.fields(),
            opt(tail_roundtrips.mean().map(|value| format!("{value:.4}"))),
            opt(cleanup_roundtrips.mean().map(|value| format!("{value:.4}"))),
        )?;
        for (step_index, moments) in step_index_moments.iter().enumerate() {
            writeln!(
                out,
                "stepindex alg={algorithm} index={step_index} count={} {}",
                moments.roundtrips.count,
                moments.fields(),
            )?;
        }
        write_structures(
            out,
            algorithm,
            &sam.stats().by_structure,
            &structures_before,
        )?;
        out.flush()?;
    }
    Ok(())
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

  --pt original|multiwrite|multiwriterary|recursive
  --alg LIST          comma-separated ALG[:TRIALS]; may be repeated. ALG is one of
                      build rw bfs dfs dijkstra prim cd dtc pr. The graph is built
                      once and the algorithms run in order. Default: rw
  --trials N          trials for entries without an explicit :TRIALS (default 1)
  --wl N              walk length for rw and pr (default 50)
  --max-steps N       visited-vertex cap for bfs/dfs/dijkstra/prim (default 100)
  --max-neighbors N   per-level neighbor cap for dtc (default 5)
  --df X              PageRank damping factor (default 0.9)
  --seed N            RNG seed (default 1)
  --start ID          fixed start vertex instead of random starts
  --pointer-branching-factor N   r-ary pointer fanout (even, >= 2)
  --cache | --move    enable / disable the client pointer cache (default --move).
                      Accesses always move (read, work on, and write each object
                      once), so the cache changes no counts.
  --no-move           Python's no-move (uncached) access pattern: every access
                      copies the object's nested pointers, and every change is a
                      move + put (the paper's OSAM and ORAM series)
  --dry-run | --crypto [--stash-size N]   in-memory SAM (default) or Path OSAM+
  --pretend-original-fits   dry-run only: if `original` records cannot fit the block,
                      use Python's edge fanout (bs - 16) / 8 instead of failing;
                      the config record then reports layout=python-sized
  --step-records      also write one `step` record per completed algorithm step
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
    let mut step_records = false;
    let mut dynamic_build = false;
    let mut prime = false;
    let mut dynamic_ops = 0;
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        let value = |args: &mut std::iter::Skip<std::env::Args>| {
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
            "--step-records" => step_records = true,
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
        step_records,
        dynamic_build,
        prime,
        dynamic_ops,
        output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sam_model::{pointer::MultiWriteCell, AccessPolicy, StepMark};

    fn ops(reads: u64) -> OperationCounts {
        OperationCounts {
            allocations: 0,
            reads,
            writes: 0,
        }
    }

    #[test]
    fn split_steps_separates_completed_steps_tail_and_cleanup() {
        let mark = |kind, reads| StepMark {
            kind,
            operations: ops(reads),
        };
        let marks = [
            mark(StepKind::Step, 13),
            mark(StepKind::Step, 20),
            mark(StepKind::Tail, 26),
            mark(StepKind::Cleanup, 30),
        ];
        let (steps, tail, cleanup) = split_steps(ops(10), ops(30), &marks);
        assert_eq!(steps, vec![ops(3), ops(7)]);
        assert_eq!((tail, cleanup), (ops(6), ops(4)));
    }

    #[test]
    fn bfs_marks_one_step_per_visited_vertex() {
        let input = GraphInput::new(
            0..6,
            (0..6)
                .flat_map(|v| {
                    [(v + 1) % 6, (v + 2) % 6].map(|w| WeightedEdge {
                        source: v,
                        destination: w,
                        weight: 1,
                    })
                })
                .collect(),
        );
        let layout = GraphLayout::for_pointer_kind(4096, PointerKind::MultiWrite).unwrap();
        let mut backend = MultiWritePointers;
        let mut sam = DryRunSam::<MultiWriteCell<GraphObject<MultiWritePointer>>>::new(
            AccessPolicy::MULTI_WRITE,
        );
        let mut graph =
            ObliviousGraph::build_static(&input, layout, &mut backend, &mut sam).unwrap();
        graph.step_marks = Some(Vec::new());
        let before = sam.stats().operations;
        let path = graph.bfs(0, Some(4), &mut backend, &mut sam).unwrap();
        let after = sam.stats().operations;
        let (steps, _, cleanup) = split_steps(before, after, graph.step_marks.as_ref().unwrap());
        assert_eq!(steps.len(), path.len());
        assert_eq!(path.len(), 4);
        assert!(cleanup.reads > 0);
        let total: u64 = steps.iter().map(|step| step.reads).sum();
        assert!(total <= after.reads - before.reads);
    }
}
