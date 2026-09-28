# sam-model

Native Rust model of the SAM interface and smart-pointer data structures used
by the oblivious-graph benchmarks.

The crate begins with two deliberately separate layers:

1. `DryRunSam` reproduces the fast Python simulation: access limits, consumed
   addresses, address reuse, operation counters, per-structure attribution,
   and write-batch accounting.
2. Pointer algorithms depend only on `SingleAccessMachine<V>`. They do not know
   whether values are in memory or backed by Path OSAM+.

`DryRunSam::snapshot` is the handoff after graph construction. Logical
oblivious identifiers start at one, matching `PathOsamPlus::alloc`, and pointer
cells contain only those logical identifiers. The cryptographic adapter
(`PathOsamSam`) therefore:

1. allocates identifiers through `PathOsamPlus` in order;
2. derives each identifier's tree leaf as `PRF_k(identifier)` (AES-128) on
   every access, so the client keeps only the key `k` and no position map:
   every oblivious address is read at most once and identifiers are never
   reused, so derived leaves look as uniform and independent to the server
   as sampled ones;
3. encodes and batch-writes the live snapshot;
4. evicts until the initial tree has been sent to the server; and
5. continues the benchmark with encryption enabled, excluding server time from
   the reported client/model metrics.

Per-address read/write-limit checks are also client state, so the benchmark
runs the adapter with them off (`--check-sam-policy` re-enables them); the
dry-run SAM always enforces the policy. The adapter takes an `AccessStrategy`. In particular,
`AccessStrategy::MULTI_WRITE_RARY` maps logical reads to
`PathOsamPlus::read_multi_paths`, maps writes to `local_write` (or
`local_write_batch` during installation), and records `stash_occupancy` after
each operation. `StashStats.maximum` is the benchmark-visible high-water mark.
The existing `PathOsamPlus::max_occupancy` remains the lower-level cross-check.

Currently migrated:

- backend-neutral SAM interface;
- in-memory dry-run backend and snapshots;
- all four Python pointer backends: original single-write queues,
  recursive/reference-counted pointers, balanced binary multi-write pointers,
  and arbitrary-even-fanout r-ary multi-write pointers;
- the shared `SmartPointerBackend`/`SmartPointer` interface, including scalar
  and bulk copy, ownership-aware nested-pointer access, get/get-attribute,
  put/put-attribute, modify/move, delete/temp-delete, and single-reference
  checks;
- balanced bulk copy for binary and r-ary pointers, with scalar `copy`
  retaining its scalar return type;
- ports of the Python pointer lifecycle, nested-pointer, r-ary shape,
  deletion/contraction, arbitrary fanout, and stress tests;
- r-ary delete repair that borrows from a sibling group only when it has a
  spare member and otherwise merges (always fits: at most 2(b/2 - 1) < b
  members), so deletes stay correct when one object has far more than `b`
  aliases (a vertex with in-degree > b). The Python backend raised
  "underfull non-leaf parent group" there; `tests/rary_many_aliases.rs`
  checks values, path length and leaks with up to 3,000 aliases at b = 64;
- the paper's client-side cache (`CachedPointers`) for all four backends. It
  maps write-back root addresses to live values with reference counts: a
  dereference caches the value under its freshly staged root, and another
  alias's walk that reaches that root stops there without reading it (its own
  path below the root is still rebuilt). The final release writes the value
  back and evicts it, so the cache holds only values the program still
  references (peak 6 in the graph benchmarks, independent of n). Pointers
  stay real SAM⁺ pointers (`CachedPointer<P>` wraps the raw pointer inline),
  so the client keeps no per-pointer table; deletions defer until the cache
  is empty, pointer-field access is typed, and phase clearing rejects leaked
  handles;
- composable fixed-size codecs for original, binary multi-write, and r-ary
  pointer cells. Payloads use a `ValueCodec`, pointer metadata is encoded with
  stable logical addresses, and `FixedSizeCodec` adds a checked length prefix
  plus zero padding for Path OSAM+ blocks;
- Path OSAM+ snapshot installation and runtime adapter;
- r-ary access profile using multi-path reads, local writes, explicit
  multi-path eviction, and maximum-stash tracking.

## The oblivious graph

`ObliviousGraph` (in `src/graph/`) is the one graph implementation. All of
its structure lives in the SAM:

- each vertex is a `Vertex` record behind a smart pointer; its outgoing
  edges form a balanced fan-out tree of `FanOut` records with fanout `bf`
  (`GraphLayout::graph_branching_factor`): with out-degree `d` the tree has
  the smallest height `h` with `bf^h >= d`, and leaf `i` sits at the base-`bf`
  digits of `i`. A leaf holds `edge` (an alias of the destination vertex's
  pointer), the weight, and the destination id (data, for reporting);
- vertex lookup uses two oblivious AVL trees stored as raw SAM cells
  (`structures::avl`): `v_ids` (name -> id) and `entry_points` (id -> entry
  pointer). `get_pointer(name)` returns a copy of the stored entry pointer;
  the stored pointer's refreshed state is written back with its AVL node
  (`SmartAvlTree::update_with`);
- walks (rw, pr) and dtc follow pointers: a move copies the chosen leaf's
  edge pointer and dereferences the copy (dtc queues its level-one and
  level-two neighbors in SAM queues); every copy is deleted before an
  algorithm returns;
- traversals (bfs, dfs, dijkstra, prim) keep their frontier (FIFO queue,
  LIFO stack, or priority queue) and visited set on the client and follow
  pointers lazily. Visiting a vertex dereferences its pointer and scans its
  fan-out tree for destination ids; each new destination becomes a frontier
  entry naming the visited vertex and the edge index, with no copy. The
  client holds the pointer of each visited vertex while it has entries in
  the frontier. Popping an entry for an unvisited vertex reopens its source,
  copies that one edge pointer and follows it; entries for vertices already
  visited, and entries left in the frontier at the end, cost nothing in SAM.
  Only the start vertex is looked up in the trees (once per run), no visited
  flags are written, and every held pointer is deleted before returning.
  (Vertex records keep the Python layout's `visited` and `label` fields,
  unused, so the graph fanout at each block size is unchanged.)

Between operations the client keeps O(1) state: both AVL roots, the next
vertex id, the vertex count, the number of possibly referenced tombstones,
and the layout. A running traversal also holds its frontier, visited set and
the pointers of visited vertices that still have frontier entries, which
grow with the frontier.

Operations: `build_static` (bulk: one `copy_many` per destination, trees
built bottom-up, AVL trees built balanced) produces exactly the graph that
`build_dynamic` (`add_vertex` then `add_edge` in input order) produces;
`add_vertex` (duplicate names are errors), `add_edge` (appends leaf
`out_degree`, growing the tree by a level when full), `delete_edge` (moves
the last leaf into the removed slot and shrinks the tree when the rest fits
one level lower), `delete_vertex` (deletes its edges and fan-out nodes,
removes it from both trees by name and id, and `put`s a `DeletedObject`
tombstone, since other vertices' edges still alias it), `neighbors`,
`prime` (`vertex_count / 10` random walks of 50 steps) and
`purge_tombstones`. While tombstones may be referenced, scans dereference
each edge, skip edges to tombstones and remove them from the scanned tree
(keeping the order of the others); `purge_tombstones` scans every vertex
once so that later scans stop checking.

Algorithms: `bfs`, `dfs`, `dijkstra`, `prim`, `random_walk`,
`contact_discovery`, `directed_triangle_count(_detailed)`, `pagerank`, all
reporting vertex ids. Random walks pick `rng.gen_range(0..out_degree)` and
access only the chosen leaf's path; PageRank jumps to a uniformly random
live vertex by drawing ids until one is live. `tests/graph_semantics.rs`
checks every algorithm against a plaintext reference (same RNG sequence for
the walks) for every backend, cached and uncached, after static and dynamic
builds and after random dynamic operations; `tests/encrypted_smoke.rs` runs
them on Path OSAM+ with policy checks.

`GraphLayout` computes the outgoing-edge-tree fanout from the exact bytes
left after the fixed envelope, pointer-cell metadata, graph metadata, and
one eight-byte slot per child. It errors if even one child pointer, or the
largest AVL/queue/stack cell, cannot fit. This graph fanout is independent
of the multiwriterary pointer fanout. `GraphLayout::python_sized` (dry run
only, `--pretend-original-fits`) uses the Python sizing instead.

### `oblivious_graph_bench`

The input accepts `SRC DST [WEIGHT]` records and `v ID` records for isolated
vertices. For example:

```bash
cargo +1.89.0 run --release --bin oblivious_graph_bench -- \
  --graph tests/data/tiny.edgelist --bs 4096 --pt multiwriterary \
  --alg rw:200,cd:50,pr:50,dfs:200,bfs:200,dijkstra:200,prim:200,dtc:200 \
  --wl 50 --seed 1 --dry-run --output run.log
```

The Rust process parses the graph, builds it (`--build static`, the default,
or `--build dynamic`), optionally primes it (`--prime`) and applies
`--dynamic-ops N` seeded random `add_vertex`/`add_edge`/`delete_edge`/
`delete_vertex` operations, and then runs each `ALG[:TRIALS]` entry of
`--alg` in order on that one graph (entries without `:TRIALS` use
`--trials`). `build` alone reports construction only. `--crypto` installs
the built (and primed) graph into Path OSAM+ before the dynamic operations
and trials. Start vertices are drawn from the live vertex names; the driver
tracks names and edges only to generate valid operations.

Output is line-oriented `key=value` records: one `config` (with
`impl=native build=static|dynamic prime=BOOL dynamic_ops=N`), one `build`,
a `prime` record with `--prime` (`walks allocations reads writes nanos`), a
`dynamic` record with `--dynamic-ops` (`ops add_vertex add_edge delete_edge
delete_vertex allocations reads writes nanos tombstones`), a `trial` record
per accepted trial (start vertex, draws, SAM operation deltas, wall time,
stash peak), an `algorithm` summary per algorithm, and a final `done`.
With `--output FILE` the records go to `FILE.partial`, renamed to `FILE` only
on success.

Only full-length runs are measured. A run is full length when a traversal
visits `--max-steps` vertices, a walk makes `--wl` moves without reaching a
sink, and every dtc neighbor retrieval returns `--max-neighbors` entries (cd
and pr always are). Start vertices are redrawn until the requested number
of runs are full length (rejection sampling), with at most 1000 runs per
algorithm. The `algorithm` record's `status` is `ok` when every requested
trial was found, `short` when fewer were, and `failed` when no run was full
length (then it carries no statistics); the benchmark then moves on to the
next algorithm. A short run from a fixed `--start` fails the algorithm at
once (except rw, whose moves are random). Rejected runs still execute on the SAM
but are excluded from every statistic; their total is reported as
`rejected_*`. The `algorithm` record gives the per-run means and variances of
allocations, reads, writes and round trips (reads + writes, as in the Python
parser) and the run `length`: visited vertices (bfs, dfs, dijkstra, prim),
walk moves (rw, pr), 2 neighbor-list retrievals (cd), or 1 (dtc, which has
no natural step). Per-step costs are per-run costs divided by `length`, so a
run's fixed costs (the start lookup, final deletions) are amortized over the
same number of steps in every trial. `structure` records give
per-structure counts for the build, `prime`, `dynamic` and each algorithm
(accepted runs only), from which the launcher computes round trips by
structure and, for the recursive pointer, the Python parser's Path ORAM
baseline (round trips times the position-map recursion depth).

Everything that runs these benchmarks and parses their output is in
`rust_osam_plus/bench/` (see its README); nothing is imported from the Python
repository. `bench/launch_rust_tests.py` runs the whole ER experiment matrix
this way (see its docstring), lowering the r-ary pointer fanout where a block
is too small; `--datasets NAME ...` runs the SNAP/CSV datasets instead.
`bench/prepare_rust_graph_benchmark.py` generates one ER graph with the Python
benchmarks' generator (`bench/graphs.py`), writes the reusable edge list, and
prints this command; `--run` launches one Rust process for the full build and
trial set.

## Move and no-move access

Graph code reads and writes each object once per operation and works on it
in place (`with_value`): the move pattern of the Python benchmarks
(`move-true`, the paper's OSAM w/ Move and OSAM⁺). Because of that the client
cache below changes no counts.

`NoMovePointers<B>` (`--no-move`) wraps any backend with Python's uncached
`SmartPointer` cost model (`move-false`, the paper's OSAM and ORAM):

- **read:** a full-ownership `get_attr`. One access copies every nested
  pointer, the operation runs on the detached copy, then the copies are
  deleted.
- **change:** a `put_attr` (move + put). The stored object's nested pointers
  are deleted and a smart copy of the new value is written.

No object is held while another is accessed, and on the recursive ORAM a
read has no write-back. The algorithms are unchanged; the wrapper tells
reads from changes by tagging every alias it hands out (`Tagged`).


The backends' cache hooks (`CacheablePointerBackend::deref_cached` and
`copy_cached`) port Python's `deref_cached`/`splay_cached`/`copy_cached`.
`CachedPointers<B>` is a backend wrapper, so the graph runs on it
unchanged; the graph's AVL, queue and stack cells bypass the cache
(`RawValueCells` for `CachedPointers`). The benchmark clears the cache
between algorithms.

Unlike Python, the Rust cache does not depend on garbage-collector finalizers.
`CacheObject` handles are explicitly released through their `CachedPointers`
manager. This makes commit timing deterministic and lets tests reject leaked
handles at a phase boundary. Rust field closures replace Python's reflective
string/index access while preserving ownership of nested smart pointers.
