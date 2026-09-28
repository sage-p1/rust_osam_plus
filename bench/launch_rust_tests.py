#!/usr/bin/env python3
"""Run the local OSAM experiment matrix against the native Rust benchmark.

This is the Rust counterpart of ``osam/launch_local_tests.py``. It uses the
same Erdős–Rényi matrix (block sizes, n = 2**POWERS, degrees, pointer backends
and cache settings) and the same memory-aware scheduler (vendored in
``scheduler.py``; nothing is imported from ``osam``), but each job is a single
``oblivious_graph_bench`` process that builds the graph once and runs all
eight algorithms in a row on it.

Differences from the Python launcher:

* Only **full-length runs** are measured: the binary redraws a trial's start
  vertex until the run reaches the algorithm's full length (a traversal
  visiting ``MAX_STEPS`` vertices, a walk of ``WALK_LENGTH`` moves without
  hitting a sink, a dtc run whose every neighbor retrieval returned
  ``MAX_NEIGHBORS`` entries); rejected runs are excluded from every statistic.
  Costs are reported per run and, amortized, **per step**: the run's cost
  divided by its nominal length (``MAX_STEPS`` visited vertices,
  ``WALK_LENGTH`` moves for rw and pr, two neighbor-list retrievals for cd,
  and one for dtc, which has no natural step). So the fixed costs of a run
  (the start lookup, final deletions) are amortized over the same number of
  steps in every trial. Each algorithm gets at most 1000 runs: ``status`` is
  ``ok`` when all trials were found, ``short`` when fewer were, and
  ``failed`` when no run was full length (no costs are reported then). Round
  trips are reads + writes, as in ``parse_er_tests.py``.
* Recursive-pointer jobs also report the Path ORAM baseline the Python
  parser plots: the round trips of the ORAM's structures (RecursivePointer
  and SmartQueue, as parse_er_tests.py) scaled by the position-map recursion
  depth ``ceil(max(log_{bs/8}(their allocations) - L, 1))``, with L = 1 at
  bs = 4096 and 4 otherwise (see ``oram_recursion_levels``). This needs no
  crypto mode: the recursive pointer runs on the dry-run SAM.
* Cache on runs the move pattern (the paper's OSAM w/ Move and OSAM+); cache
  off runs Python's no-move pattern (``--no-move``: OSAM and ORAM), where
  every access copies the object's nested pointers. Their logs carry
  ``_move-false``.
* The r-ary (multiwriterary) pointer fanout starts at the binary's default,
  floor((bs - 16) / 8) capped at 64, and is lowered automatically until its
  cells fit the block. With the compact r-ary cell (8 bytes per slot) the
  default of 6 already fits 64-byte blocks; the probe matters if the cell
  encoding or the default changes.
* ``original`` records cannot fit 64-byte blocks (81 bytes of fixed metadata).
  In dry-run mode they are modeled anyway with Python's edge fanout
  (bs - 16) / 8 = 6, flagged ``layout=python-sized`` in logs, log names
  (``_layout-python``) and the report; ``--exact-original-only`` skips them
  instead. Crypto jobs that cannot fit are always skipped.
* ``--no-crypto`` turns cryptography off for every job, whatever ``--mode``.
* ER graphs are generated once per (n, d, seed, generator) into
  ``bench/graphs/`` by a background process pool and reused by every job and
  every rerun. ``--generator fast`` (default) uses networkx's O(n + m)
  ``fast_gnp_random_graph`` (seconds at n = 2^20); ``--generator exact`` uses
  ``erdos_renyi_graph``, the Python benchmarks' O(n^2) generator (about 12
  hours at n = 2^20), which reproduces the Python runs' graphs. Both sample
  G(n, d/n). Exact-generator jobs carry ``_gen-exact`` in their log names.

Results: one record log per job in ``bench/results/rust-logs/``, plus
``summary.csv`` (per job and algorithm) and ``structures.csv`` (round trips by structure) there. Regenerate
them any time with ``--report-only``.

Datasets (``--datasets NAME ...``): the ``scheduler.DATASETS`` edge lists
under ``--dataset-root`` (default ``osam/real-dataset-tests``, next to this
repository; env ``DATASET_ROOT``) at ``<name>/<file>``, parsed as
``benchmark_graph_from_csv.py`` parses them (``graphs.dataset_graph``) and
exported once into ``bench/graphs/datasets/``. Logs and reports go to
``bench/results/rust-dataset-logs/``.

Graph options (passed to the binary; non-default values appear in log names):
``--static/--no-static`` builds the graph in bulk or by inserting vertices and
edges one at a time (both give the same graph); ``--prime`` runs n/10 random
walks of length 50 after the build; ``--dynamic-ops N`` applies N random
vertex/edge insertions and deletions before the algorithms. ``--move/--no-move``
forces pointer-layer caching on/off for every pointer (default:
``scheduler.cache_settings_for``).
"""

from __future__ import annotations

import argparse
import csv
import json
import math
import os
import signal
import statistics
import subprocess
import sys
from concurrent.futures import Future, ProcessPoolExecutor
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

ROOT = Path(__file__).resolve().parent  # rust_osam_plus/bench
sys.path.insert(0, str(ROOT))

import scheduler as local  # noqa: E402  (vendored scheduler, matrix and datasets)
from graphs import GENERATORS, generate_graph  # noqa: E402

RUST_CRATE = Path(os.environ.get("RUST_CRATE", ROOT.parent / "crates" / "sam-model"))
RESULTS_DIR = ROOT / "results"
LOG_DIR = RESULTS_DIR / "rust-logs"
GRAPH_DIR = ROOT / "graphs"
SUMMARY_CSV = LOG_DIR / "summary.csv"
DATASET_LOG_DIR = RESULTS_DIR / "rust-dataset-logs"
DATASET_ROOT = local.DATASET_ROOT
NATIVE_BINARY = "oblivious_graph_bench"

# Matrix: shared with the Python launcher so the two stay comparable.
BLOCK_SIZES = local.BLOCK_SIZES
DEGREES = local.DEGREES
POWERS = local.POWERS
POINTERS = local.POINTERS
MAX_ER_WORKLOAD = local.MAX_ER_WORKLOAD
ALGORITHMS = local.ALGORITHMS  # rw cd pr dfs bfs dijkstra prim dtc, in this order
MAX_STEPS = local.MAX_STEPS
MAX_NEIGHBORS = local.MAX_NEIGHBORS
WALK_LENGTH = local.WALK_LENGTH
DAMPING_FACTOR = local.DAMPING_FACTOR
SEED = 1

TRIALS = 50
# Algorithms whose runs can stop before their full length. The binary
# rejection-samples full-length runs, so by default they run TRIALS like the
# others; --early-trials can still give them a different count.
EARLY_TERMINATING = ("rw", "bfs", "dfs", "dijkstra", "prim", "dtc")
EARLY_TRIALS = TRIALS
SUMMARY_FILES = ("summary.csv", "structures.csv")
# The ORAM baseline's structures, as in osam's parse_er_tests.py: the
# recursive pointer store and the SAM queues it backs (dtc's neighbor queues;
# traversal frontiers are client-side, and AVL lookups are not ORAM work,
# since an ORAM addresses a vertex by its id directly).
RECURSIVE_STRUCTURES = ("RecursivePointer", "SmartQueue")

# The Python launcher's defaults are tuned for a ~500 GiB server. Rust jobs
# are much lighter; keep the same knobs but with laptop-friendly defaults.
# Anything already set in the environment wins.
os.environ.setdefault("MIN_AVAILABLE_MEMORY_GB", "4")
os.environ.setdefault("JOB_LAUNCH_SETTLE_SECONDS", "1")
os.environ.setdefault("STATUS_INTERVAL_SECONDS", "600")
GRAPH_WORKERS = int(os.environ.get("GRAPH_WORKERS", str(max(1, min(4, (os.cpu_count() or 1) // 2)))))

MODES = ("dry-run", "crypto")

# In dry-run mode, run `original` at block sizes it cannot fit (64 bytes) with
# Python's edge fanout instead of skipping it; see --exact-original-only.
PRETEND_ORIGINAL_FITS = True


def trials_for(algorithm: str) -> int:
    return EARLY_TRIALS if algorithm in EARLY_TERMINATING else TRIALS


def algorithm_spec(algorithms: Iterable[str] = ALGORITHMS) -> str:
    """The binary's ``--alg`` value, e.g. ``rw:200,cd:50,...``."""
    return ",".join(f"{algorithm}:{trials_for(algorithm)}" for algorithm in algorithms)


def graph_path(n: int, d: int, seed: int = SEED, generator: str = "fast") -> Path:
    # Same names prepare_rust_graph_benchmark.py uses, so graphs are shared.
    # The exact generator keeps the original name (graphs made before the
    # fast generator existed are exact ones).
    suffix = "" if generator == "exact" else f"_gen-{generator}"
    return GRAPH_DIR / f"ER_n-{n}_d-{d}_seed-{seed}{suffix}.edgelist"


def ensure_graph_file(n: int, d: int, seed: int = SEED, generator: str = "fast") -> str:
    """Generate one ER edge list if missing; atomic so concurrent use is safe."""
    path = graph_path(n, d, seed, generator)
    if path.is_file():
        return str(path)
    partial = path.with_name(f"{path.name}.{os.getpid()}.partial")
    generate_graph(n, d, seed, partial, generator)
    os.replace(partial, path)
    return str(path)


@dataclass(frozen=True)
class DatasetRef:
    """A ``scheduler.Dataset`` located under a chosen root."""

    name: str
    filename: str
    directed: bool
    edges: int
    root: Path = DATASET_ROOT

    @classmethod
    def of(cls, dataset: Any, root: Path | None = None) -> "DatasetRef":
        return cls(dataset.name, dataset.filename, dataset.directed, dataset.edges,
                   DATASET_ROOT if root is None else Path(root))

    @property
    def path(self) -> Path:
        return self.root / self.name / self.filename

    @property
    def workload(self) -> int:
        # As scheduler.Dataset: adjacency entries.
        return self.edges if self.directed else 2 * self.edges


def select_datasets(names: Iterable[str], root: Path | None = None) -> tuple[DatasetRef, ...]:
    """``--datasets a,b c`` -> DatasetRefs (no names: every dataset)."""
    split = [part for name in names for part in name.split(",") if part]
    return tuple(DatasetRef.of(dataset, root) for dataset in local.select_datasets(split))


@dataclass(frozen=True)
class GraphSpec:
    """One graph input file (native edge list): an ER graph or a dataset."""

    kind: str  # "er" | "dataset"
    n: int = 0
    d: int = 0
    seed: int = SEED
    dataset: DatasetRef | None = None
    generator: str = "fast"

    @property
    def path(self) -> Path:
        if self.kind == "er":
            return graph_path(self.n, self.d, self.seed, self.generator)
        assert self.dataset is not None
        return GRAPH_DIR / "datasets" / f"{self.dataset.name}.edgelist"

    def ensure(self) -> str:
        """Write the file if missing (atomically)."""
        if self.kind == "er":
            return ensure_graph_file(self.n, self.d, self.seed, self.generator)
        path = self.path
        if path.is_file():
            return str(path)
        import graphs  # networkx + pandas

        assert self.dataset is not None
        graph = graphs.dataset_graph(self.dataset.path, self.dataset.directed)
        graphs.write_native_graph(graph, path)
        return str(path)


def ensure_graph_spec(spec: GraphSpec) -> str:
    """Process-pool entry point for GraphSpec.ensure."""
    return spec.ensure()


# --------------------------------------------------------------------------
# Rust binary and block-fit probing
# --------------------------------------------------------------------------


def build_binary(name: str = NATIVE_BINARY) -> Path:
    """``cargo build --release`` once and return the benchmark executable."""
    command = [
        "cargo",
        "build",
        "--release",
        "--manifest-path",
        str(RUST_CRATE / "Cargo.toml"),
        "--bin",
        name,
        "--message-format=json-render-diagnostics",
    ]
    print("Building:", " ".join(command), flush=True)
    result = subprocess.run(command, check=True, stdout=subprocess.PIPE, text=True)
    executable = None
    for line in result.stdout.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("reason") == "compiler-artifact" and message.get("executable"):
            if message["target"]["name"] == name:
                executable = message["executable"]
    if executable is None:
        raise RuntimeError(f"cargo did not report the {name} executable")
    return Path(executable)


def default_pointer_branching_factor(bs: int) -> int:
    # Mirrors the binary's default: floor((bs - 16) / 8), capped at 64.
    return min(max(bs - 16, 0) // 8, 64)


@dataclass(frozen=True)
class Feasibility:
    feasible: bool
    pointer_bf: int | None = None
    reason: str = ""
    # Dry-run original records that do not fit the block, modeled with
    # Python's fanout via --pretend-original-fits.
    python_layout: bool = False


class BlockFitProbe:
    """Finds, per configuration, whether and how a pointer backend fits a block.

    Runs the binary's ``build`` algorithm on a tiny complete graph. For
    multiwriterary the fanout is lowered (even values only, as the r-ary
    pointer requires) until the build succeeds.
    """

    def __init__(self, binary: Path) -> None:
        self.binary = binary
        self.cache: dict[tuple[int, str, bool, str], Feasibility] = {}
        self.graph = LOG_DIR / ".probe-K8.edgelist"

    def _write_graph(self) -> None:
        if self.graph.is_file():
            return
        self.graph.parent.mkdir(parents=True, exist_ok=True)
        lines = [f"{s} {t} {s + t}" for s in range(8) for t in range(8) if s != t]
        self.graph.write_text("\n".join(lines) + "\n")

    def _try(
        self, bs: int, pointer: str, cache: bool, mode: str, bf: int | None, pretend: bool = False
    ) -> str | None:
        command = [
            str(self.binary), "--graph", str(self.graph), "--bs", str(bs),
            "--pt", pointer, "--alg", "build", "--cache" if cache else "--move",
            "--crypto" if mode == "crypto" else "--dry-run",
        ]
        if pretend:
            command.append("--pretend-original-fits")
        if bf is not None:
            command += ["--pointer-branching-factor", str(bf)]
        result = subprocess.run(command, capture_output=True, text=True)
        if result.returncode == 0:
            return None
        return (result.stderr.strip() or result.stdout.strip()).removeprefix("error: ")

    def check(self, bs: int, pointer: str, cache: bool, mode: str) -> Feasibility:
        key = (bs, pointer, cache, mode)
        if key in self.cache:
            return self.cache[key]
        self._write_graph()
        if pointer == "multiwriterary":
            default = default_pointer_branching_factor(bs)
            first = default - (default % 2)
            error = "no even branching factor >= 2 is available"
            result = None
            for bf in range(first, 1, -2):
                error = self._try(bs, pointer, cache, mode, bf) or ""
                if not error:
                    result = Feasibility(True, bf)
                    if bf != default:
                        print(
                            f"bs={bs} cache={str(cache).lower()} mode={mode}: multiwriterary "
                            f"branching factor lowered {default} -> {bf} to fit the block",
                            flush=True,
                        )
                    break
            if result is None:
                result = Feasibility(False, None, error)
        else:
            error = self._try(bs, pointer, cache, mode, None)
            result = Feasibility(error is None, None, error or "")
            if error and pointer == "original" and mode == "dry-run" and PRETEND_ORIGINAL_FITS:
                # Encrypted blocks must fit; the in-memory dry run does not
                # encode, so model the oversized records with Python's fanout.
                if self._try(bs, pointer, cache, mode, None, pretend=True) is None:
                    result = Feasibility(True, None, error, python_layout=True)
                    print(
                        f"bs={bs} cache={str(cache).lower()} mode=dry-run: original does not fit "
                        f"({error}); using Python's fanout {max(bs - 16, 0) // 8} (layout=python-sized)",
                        flush=True,
                    )
        self.cache[key] = result
        return result


@dataclass(frozen=True)
class GraphOptions:
    """How each job builds and prepares its graph before the algorithms."""

    static: bool = True
    prime: bool = False
    dynamic_ops: int = 0

    def flags(self) -> list[str]:
        flags = ["--build", "static" if self.static else "dynamic"]
        if self.prime:
            flags.append("--prime")
        if self.dynamic_ops:
            flags += ["--dynamic-ops", str(self.dynamic_ops)]
        return flags

    @property
    def tag(self) -> str:
        """Log-name suffix; empty for the defaults, so existing names are unchanged."""
        return (
            ("" if self.static else "_build-dynamic")
            + ("_prime" if self.prime else "")
            + (f"_dynops-{self.dynamic_ops}" if self.dynamic_ops else "")
        )


# --------------------------------------------------------------------------
# Jobs (duck-typed to scheduler.Job)
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class RustJob:
    n: int
    d: int
    bs: int
    pointer: str
    cache_enabled: bool
    mode: str
    pointer_bf: int | None
    binary: Path
    seed: int = SEED
    python_layout: bool = False
    # None for ER jobs; the scheduler distinguishes ER and dataset jobs.
    dataset: DatasetRef | None = None
    options: GraphOptions = GraphOptions()
    generator: str = "fast"

    @property
    def workload(self) -> int:
        return self.dataset.workload if self.dataset is not None else self.n * self.d

    @property
    def workload_label(self) -> str:
        return "adjacency entries" if self.dataset is not None else "n*d"

    @property
    def log_directory(self) -> Path:
        return DATASET_LOG_DIR if self.dataset is not None else LOG_DIR

    @property
    def graph_spec(self) -> GraphSpec:
        if self.dataset is not None:
            return GraphSpec("dataset", seed=self.seed, dataset=self.dataset)
        return GraphSpec("er", self.n, self.d, self.seed, generator=self.generator)

    @property
    def graph(self) -> Path:
        if self.dataset is not None:
            return self.graph_spec.path
        return graph_path(self.n, self.d, self.seed, self.generator)

    @property
    def name(self) -> str:
        graph = f"ER_n-{self.n}_d-{self.d}" if self.dataset is None else self.dataset.name
        name = (
            f"{graph}_bs-{self.bs}_trials-{TRIALS}-{EARLY_TRIALS}"
            f"_pt-{self.pointer}_cache-{str(self.cache_enabled).lower()}_mode-{self.mode}"
        )
        if self.pointer_bf is not None:
            name += f"_b-{self.pointer_bf}"
        if self.python_layout:
            name += "_layout-python"
        if not self.cache_enabled:
            # Cache-off jobs use Python's no-move access pattern; the suffix
            # keeps them apart from logs of the old move-semantics runs.
            name += "_move-false"
        if self.dataset is None and self.generator != "fast":
            name += f"_gen-{self.generator}"
        return name + self.options.tag

    @property
    def description(self) -> str:
        bf = f" b={self.pointer_bf}" if self.pointer_bf is not None else ""
        graph = (
            f"n={self.n} d={self.d} n*d={self.workload}"
            if self.dataset is None
            else f"dataset={self.dataset.name} {self.workload_label}={self.workload}"
        )
        return (
            f"{graph} bs={self.bs} pointer={self.pointer}"
            f"{bf} cache={str(self.cache_enabled).lower()} mode={self.mode}"
            + (" layout=python-sized" if self.python_layout else "")
            + (f" {' '.join(self.options.flags())}" if self.options != GraphOptions() else "")
        )

    def log_prefix(self, directory: Path | None = None) -> Path:
        return (self.log_directory if directory is None else directory) / self.name

    def log_path(self, directory: Path | None = None) -> Path:
        return self.log_prefix(directory).with_suffix(".log")

    def is_complete(self, directory: Path | None = None) -> bool:
        # The binary writes FILE.partial and renames it only on success.
        return local.file_ends_with_marker(self.log_path(directory), "done")

    def command(self, log_prefix: Path, fault_handler: bool = False) -> list[str]:
        del fault_handler  # Python-only option; kept for the scheduler's interface
        command = [
            str(self.binary),
            "--graph", str(self.graph),
            "--bs", str(self.bs),
            "--pt", self.pointer,
            "--alg", algorithm_spec(),
            "--wl", str(WALK_LENGTH),
            "--max-steps", str(MAX_STEPS),
            "--max-neighbors", str(MAX_NEIGHBORS),
            "--df", str(DAMPING_FACTOR),
            "--seed", str(self.seed),
            # Cache on: the move pattern (OSAM w/ Move, OSAM+). Cache off:
            # Python's no-move pattern (OSAM, ORAM), as in the Python runs.
            *(["--cache"] if self.cache_enabled else ["--no-cache", "--no-move"]),
            "--crypto" if self.mode == "crypto" else "--dry-run",
            "--output", str(log_prefix.with_suffix(".log")),
            *self.options.flags(),
        ]
        if self.pointer_bf is not None:
            command += ["--pointer-branching-factor", str(self.pointer_bf)]
        if self.python_layout:
            command.append("--pretend-original-fits")
        return command


@dataclass(frozen=True)
class SkippedConfig:
    bs: int
    pointer: str
    cache_enabled: bool
    mode: str
    reason: str
    jobs: int


def graphs_for(
    powers: Iterable[int], degrees: Iterable[int], datasets: Iterable[DatasetRef] | None
) -> list[tuple[int, int, DatasetRef | None]]:
    """(n, d, None) for the ER matrix, or (0, 0, dataset) per dataset."""
    if datasets is not None:
        return [(0, 0, dataset) for dataset in datasets]
    graphs: list[tuple[int, int, DatasetRef | None]] = []
    for power in powers:
        n = 2**power
        for d in degrees:
            if d > n or n * d > MAX_ER_WORKLOAD:
                continue
            graphs.append((n, d, None))
    return graphs


def cache_settings(pointer: str, move: bool | None) -> tuple[bool, ...]:
    """The matrix's cache settings, or the one forced by --move/--no-move."""
    return local.cache_settings_for(pointer) if move is None else (move,)


def build_jobs(
    binary: Path,
    probe: BlockFitProbe,
    modes: Iterable[str],
    block_sizes: Iterable[int] = BLOCK_SIZES,
    pointers: Iterable[str] = POINTERS,
    degrees: Iterable[int] = DEGREES,
    powers: Iterable[int] = POWERS,
    datasets: Iterable[DatasetRef] | None = None,
    move: bool | None = None,
    options: GraphOptions = GraphOptions(),
    generator: str = "fast",
) -> tuple[list[RustJob], list[SkippedConfig]]:
    jobs: list[RustJob] = []
    skipped: dict[tuple[int, str, bool, str], list[Any]] = {}
    graphs = graphs_for(powers, degrees, datasets)
    for mode in modes:
        for bs in block_sizes:
            for n, d, dataset in graphs:
                for pointer in pointers:
                    for cache_enabled in cache_settings(pointer, move):
                        fit = probe.check(bs, pointer, cache_enabled, mode)
                        if not fit.feasible:
                            entry = skipped.setdefault(
                                (bs, pointer, cache_enabled, mode), [fit.reason, 0]
                            )
                            entry[1] += 1
                            continue
                        jobs.append(
                            RustJob(
                                n, d, bs, pointer, cache_enabled, mode, fit.pointer_bf, binary,
                                python_layout=fit.python_layout, dataset=dataset,
                                options=options, generator=generator,
                            )
                        )
    return jobs, [
        SkippedConfig(bs, pointer, cache, mode, reason, count)
        for (bs, pointer, cache, mode), (reason, count) in skipped.items()
    ]


class RustTestScheduler(local.LocalTestScheduler):
    """The Python launcher's scheduler, launching Rust jobs.

    Graphs are generated by a process pool ahead of the scheduler; a job's
    launch waits only for its own graph.
    """

    def __init__(self, settings: local.Settings, jobs: Iterable[RustJob]) -> None:
        super().__init__(settings, jobs)
        self.graph_pool: ProcessPoolExecutor | None = None
        self.graph_futures: dict[GraphSpec, Future[str]] = {}

    def start_graph_generation(self) -> None:
        pending: list[GraphSpec] = []
        for job in self.jobs:
            key = job.graph_spec
            if key not in self.graph_futures and key not in pending and not job.graph.is_file():
                pending.append(key)
        if not pending:
            return
        kind = "ER " if all(spec.kind == "er" for spec in pending) else ""
        print(f"Generating {len(pending)} {kind}graph(s) with {GRAPH_WORKERS} worker(s)", flush=True)
        self.graph_pool = ProcessPoolExecutor(max_workers=GRAPH_WORKERS)
        for key in pending:  # job order: the first jobs' graphs come first
            self.graph_futures[key] = self.graph_pool.submit(ensure_graph_spec, key)

    def shutdown_graph_generation(self) -> None:
        if self.graph_pool is not None:
            self.graph_pool.shutdown(wait=False, cancel_futures=True)
            self.graph_pool = None

    def start_process(self, job: RustJob, *, fault_handler: bool = False, stdout: Any = None, stderr: Any = None):  # type: ignore[override]
        future = self.graph_futures.get(job.graph_spec)
        if future is not None:
            if not future.done():
                print(f"Waiting for graph {job.graph.name}", flush=True)
            future.result()
        job.log_directory.mkdir(parents=True, exist_ok=True)
        return subprocess.Popen(
            job.command(job.log_prefix()),
            cwd=ROOT,
            start_new_session=True,
            stdout=stdout,
            stderr=stderr,
        )

    def run(self) -> int:
        self.start_graph_generation()
        try:
            return super().run()
        finally:
            self.shutdown_graph_generation()


# --------------------------------------------------------------------------
# Log parsing and the report
# --------------------------------------------------------------------------


def _value(text: str) -> Any:
    if text == "none":
        return None
    for kind in (int, float):
        try:
            return kind(text)
        except ValueError:
            pass
    if text in ("true", "false"):
        return text == "true"
    return text


def parse_log(path: Path) -> dict[str, Any]:
    parsed: dict[str, Any] = {
        "config": {}, "build": {}, "trials": {}, "algorithms": {},
        "structures": [], "done": False,
    }
    for line in path.read_text().splitlines():
        kind, _, rest = line.partition(" ")
        fields = dict(token.split("=", 1) for token in rest.split() if "=" in token)
        record = {key: _value(value) for key, value in fields.items()}
        if kind in ("config", "build"):
            parsed[kind] = record
        elif kind == "trial":
            parsed["trials"].setdefault(record["alg"], []).append(record)
        elif kind == "algorithm":
            parsed["algorithms"][record["alg"]] = record
        elif kind in ("steps", "stepindex"):
            raise ValueError(
                f"{path}: per-step log from an older binary (before full-length runs); rerun it"
            )
        elif kind == "structure":
            parsed["structures"].append(record)
        elif kind == "done":
            parsed["done"] = True
    return parsed


def _mean(values: list[float]) -> float | None:
    return statistics.fmean(values) if values else None


def _stdev(values: list[float]) -> float | None:
    return statistics.stdev(values) if len(values) > 1 else None


def oram_recursion_levels(total_allocations: int, bs: int) -> int | None:
    """Path ORAM position-map recursion depth, as in ``parse_er_tests.py``.

    levels = ceil(max(log_{bs/8}(total_allocations) - L, 1)), L = 1 for
    bs = 4096 and L = 4 otherwise, where total_allocations counts
    RecursivePointer allocations over the build and every algorithm.
    """
    base = bs / 8
    if total_allocations < 1 or base <= 1:
        return None
    levels = math.log(total_allocations, base)
    offset = 1 if bs == 4096 else 4
    return math.ceil(max(levels - offset, 1))


def _sd(variance: float | None) -> float | None:
    return math.sqrt(variance) if variance is not None else None


# Graph-option columns, present when a log was built with non-default options.
OPTION_FIELDS = ["build", "prime", "dynamic_ops"]


def option_columns(config: dict[str, Any]) -> dict[str, Any]:
    columns = {key: config[key] for key in OPTION_FIELDS if key in config}
    if columns == {"build": "static", "prime": False, "dynamic_ops": 0}:
        return {}
    return columns


def report_fields(fields: list[str], extra: list[str], rows: list[dict[str, Any]]) -> list[str]:
    """``fields`` plus the ``extra`` columns some row has."""
    return fields + [name for name in extra if any(name in row for row in rows)]


def report_rows(parsed: dict[str, Any], log: Path) -> list[dict[str, Any]]:
    config, build = parsed["config"], parsed["build"]
    base = {
        "n": config.get("vertices"),
        "edges": config.get("edges"),
        "bs": config.get("block_size"),
        "pointer": config.get("pointer"),
        "cache": config.get("cache"),
        "move": config.get("move", True),
        "mode": config.get("mode"),
        "pointer_bf": config.get("pointer_branching_factor"),
        "graph_bf": config.get("graph_branching_factor"),
        "layout": config.get("layout", "exact"),
        "build_ms": build["nanos"] / 1e6 if build.get("nanos") is not None else None,
        "install_ms": build["install_nanos"] / 1e6 if build.get("install_nanos") is not None else None,
        "installation_max_stash": build.get("installation_maximum_stash"),
        "complete_log": parsed["done"],
        **option_columns(config),
    }
    levels = None
    if config.get("pointer") == "recursive":
        recursive_allocations = sum(
            record["allocations"]
            for record in parsed["structures"]
            if record["name"] in RECURSIVE_STRUCTURES
        )
        levels = oram_recursion_levels(recursive_allocations, int(config["block_size"]))
    rows = []
    for algorithm, summary in parsed["algorithms"].items():
        trials = parsed["trials"].get(algorithm, [])
        nanos = [trial["nanos"] for trial in trials if trial.get("nanos") is not None]
        stash = [trial["stash_peak"] for trial in trials if trial.get("stash_peak") is not None]
        length = summary["length"]
        row = {
            **base,
            "alg": algorithm,
            "status": summary.get("status", "ok"),
            "trials": len(trials),
            "requested_trials": summary.get("requested"),
            "attempts": summary["attempts"],
            "length": length,
        }
        for metric in ("allocations", "reads", "writes", "roundtrips"):
            mean, sd = summary.get(f"mean_{metric}"), _sd(summary.get(f"var_{metric}"))
            row[f"mean_{metric}_per_trial"] = mean
            row[f"sd_{metric}_per_trial"] = sd
            row[f"mean_{metric}_per_step"] = mean / length if mean is not None else None
            row[f"sd_{metric}_per_step"] = sd / length if sd is not None else None
        row["rejected_roundtrips"] = summary.get("rejected_roundtrips")
        # ORAM baseline: the recursive structures' share of this algorithm's
        # round trips, scaled by the recursion depth (as parse_er_tests.py).
        phase = [record for record in parsed["structures"] if record["phase"] == algorithm]
        total = sum(record["roundtrips"] for record in phase)
        recursive = sum(record["roundtrips"] for record in phase if record["name"] in RECURSIVE_STRUCTURES)
        share = recursive / total if total else None
        row["oram_levels"] = levels
        for unit in ("trial", "step"):
            mean, sd = row[f"mean_roundtrips_per_{unit}"], row[f"sd_roundtrips_per_{unit}"]
            if levels is not None and share is not None and mean is not None:
                row[f"oram_roundtrips_per_{unit}"] = mean * share * levels
                row[f"oram_sd_roundtrips_per_{unit}"] = sd * share * levels if sd is not None else None
            else:
                row[f"oram_roundtrips_per_{unit}"] = row[f"oram_sd_roundtrips_per_{unit}"] = None
        row["mean_ms_per_trial"] = statistics.fmean(nanos) / 1e6 if nanos else None
        row["max_stash_peak"] = max(stash) if stash else None
        row["log"] = log.name
        rows.append(row)
    return rows


def structure_rows(parsed: dict[str, Any], log: Path) -> list[dict[str, Any]]:
    config = parsed["config"]
    trials = {algorithm: len(records) for algorithm, records in parsed["trials"].items()}
    rows = []
    for record in parsed["structures"]:
        count = trials.get(record["phase"])
        rows.append(
            {
                "n": config.get("vertices"),
                "bs": config.get("block_size"),
                "pointer": config.get("pointer"),
                "cache": config.get("cache"),
                "mode": config.get("mode"),
                "layout": config.get("layout", "exact"),
                "phase": record["phase"],
                "structure": record["name"],
                "allocations": record["allocations"],
                "reads": record["reads"],
                "writes": record["writes"],
                "roundtrips": record["roundtrips"],
                "trials": count,
                "roundtrips_per_trial": record["roundtrips"] / count if count else None,
                "log": log.name,
            }
        )
    return rows


FIELDS = [
    "n", "edges", "bs", "pointer", "cache", "move", "mode", "pointer_bf", "graph_bf", "layout", "alg",
    "status", "trials", "requested_trials", "attempts", "length",
    "mean_allocations_per_trial", "sd_allocations_per_trial",
    "mean_reads_per_trial", "sd_reads_per_trial",
    "mean_writes_per_trial", "sd_writes_per_trial",
    "mean_roundtrips_per_trial", "sd_roundtrips_per_trial",
    "mean_allocations_per_step", "sd_allocations_per_step",
    "mean_reads_per_step", "sd_reads_per_step",
    "mean_writes_per_step", "sd_writes_per_step",
    "mean_roundtrips_per_step", "sd_roundtrips_per_step",
    "rejected_roundtrips",
    "oram_levels", "oram_roundtrips_per_trial", "oram_sd_roundtrips_per_trial",
    "oram_roundtrips_per_step", "oram_sd_roundtrips_per_step",
    "mean_ms_per_trial", "max_stash_peak",
    "build_ms", "install_ms", "installation_max_stash", "complete_log", "log",
]
STRUCTURE_FIELDS = [
    "n", "bs", "pointer", "cache", "mode", "layout", "phase", "structure",
    "allocations", "reads", "writes", "roundtrips", "trials", "roundtrips_per_trial", "log",
]


def find_logs(directory: Path = LOG_DIR) -> list[Path]:
    """Completed logs, preferring the main run over serial retries."""
    logs = {path.name: path for path in sorted(directory.glob("retries/*/*.log"))}
    logs.update({path.name: path for path in directory.glob("*.log")})
    return [logs[name] for name in sorted(logs)]


def _write_csv(path: Path, fields: list[str], rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fields)
        writer.writeheader()
        writer.writerows(rows)


def write_report(directory: Path = LOG_DIR, output: Path = SUMMARY_CSV) -> list[dict[str, Any]]:
    rows, structures = [], []
    for log in find_logs(directory):
        parsed = parse_log(log)
        rows.extend(report_rows(parsed, log))
        structures.extend(structure_rows(parsed, log))
    _write_csv(output, report_fields(FIELDS, OPTION_FIELDS, rows), rows)
    _write_csv(
        output.with_name("structures.csv"),
        STRUCTURE_FIELDS,
        structures,
    )
    return rows


def _fmt(value: Any, digits: int = 1) -> str:
    if value is None:
        return "-"
    if isinstance(value, float):
        return f"{value:.{digits}f}"
    return str(value)


def print_report(rows: list[dict[str, Any]], limit: int | None = None) -> None:
    header = (
        f"{'n':>8} {'bs':>5} {'pointer':<15} {'cache':<5} {'mode':<7} {'b':>3} {'alg':<9}"
        f"{'tries':>6} {'rt/trial':>10} {'rt/step':>9} {'±sd':>9} {'reads':>8} {'writes':>8} "
        f"{'ORAM rt/step':>13} {'stash':>6}"
    )
    print(header)
    print("-" * len(header))
    for row in rows[:limit]:
        print(
            f"{row['n']:>8} {row['bs']:>5} "
            f"{row['pointer'] + ('*' if row.get('layout') == 'python-sized' else ''):<15} "
            f"{str(row['cache']).lower():<5} "
            f"{row['mode']:<7} {_fmt(row['pointer_bf']):>3} {row['alg']:<9}"
            f"{_fmt(row['attempts']):>6} "
            f"{('FAILED' if row['status'] == 'failed' else _fmt(row['mean_roundtrips_per_trial']) + ('!' if row['status'] == 'short' else '')):>10} "
            f"{_fmt(row['mean_roundtrips_per_step']):>9} "
            f"{_fmt(row['sd_roundtrips_per_step']):>9} {_fmt(row['mean_reads_per_step']):>8} "
            f"{_fmt(row['mean_writes_per_step']):>8} "
            f"{_fmt(row['oram_roundtrips_per_step']):>13} {_fmt(row['max_stash_peak']):>6}"
        )
    print(
        "\nFull-length runs only ('tries': start draws for all trials, at most 1000; FAILED: "
        "no full-length run, !: fewer full-length runs than requested); per-step costs are per-run costs / "
        "run length. Round trips = reads + writes."
    )
    if any(row.get("layout") == "python-sized" for row in rows):
        print(
            "* layout=python-sized: dry-run only; these records do not fit the block and use "
            "Python's edge fanout (bs - 16) / 8."
        )


# --------------------------------------------------------------------------
# Entry point
# --------------------------------------------------------------------------


def parse_arguments(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument(
        "--mode", choices=("dry-run", "crypto", "both"), default="dry-run",
        help="SAM backend: in-memory dry run (default), encrypted Path OSAM+, or both.",
    )
    parser.add_argument(
        "--no-crypto", action="store_true",
        help="Turn off cryptography for every job: all runs use the in-memory dry-run "
        "SAM, overriding --mode crypto/both.",
    )
    parser.add_argument(
        "--exact-original-only", action="store_true",
        help="Skip dry-run original jobs that do not fit the block instead of modeling them "
        "with Python's fanout (layout=python-sized).",
    )
    parser.add_argument("--dry-run", action="store_true", help="Plan only: probe block fits and print the job count.")
    parser.add_argument("--skip-existing", action="store_true", help="Skip jobs whose log is already complete.")
    parser.add_argument("--report-only", action="store_true", help="Rebuild summary.csv from existing logs and exit.")
    parser.add_argument("--block-sizes", type=int, nargs="+", default=list(BLOCK_SIZES))
    parser.add_argument("--pointers", nargs="+", choices=POINTERS, default=list(POINTERS))
    parser.add_argument("--degrees", type=int, nargs="+", default=list(DEGREES))
    parser.add_argument("--trials", type=int, default=TRIALS, help=f"Trials for cd and pr (default {TRIALS}).")
    parser.add_argument(
        "--early-trials", type=int, default=None,
        help=f"Trials for {', '.join(EARLY_TERMINATING)} (default {EARLY_TRIALS}, as --trials).",
    )
    parser.add_argument("--min-power", type=int, default=min(POWERS))
    parser.add_argument("--max-power", type=int, default=max(POWERS))
    parser.add_argument(
        "--datasets", nargs="*", metavar="NAME",
        help="Run dataset jobs (names from scheduler.DATASETS, space or comma "
        "separated; none = all) instead of the ER matrix.",
    )
    parser.add_argument(
        "--dataset-root", type=Path, default=None,
        help=f"Directory holding <name>/<file> dataset edge lists (default {DATASET_ROOT}).",
    )
    flag = argparse.BooleanOptionalAction
    parser.add_argument("--static", action=flag, default=True,
                        help="Build in bulk (default) or, with --no-static, by inserting "
                        "vertices and edges one at a time (same graph).")
    parser.add_argument("--prime", action=flag, default=False,
                        help="Run n/10 random walks of length 50 after the build (default off).")
    parser.add_argument("--dynamic-ops", type=int, default=0, metavar="N",
                        help="Apply N random vertex/edge insertions and deletions before the "
                        "algorithms (default 0).")
    parser.add_argument("--generator", choices=GENERATORS, default="fast",
                        help="ER graph generator: fast = networkx fast_gnp_random_graph, "
                        "O(n + m) (default); exact = erdos_renyi_graph, O(n^2), the Python "
                        "benchmarks' graphs (about 12 h at n = 2^20).")
    parser.add_argument("--move", action=flag, default=None,
                        help="Force pointer-layer caching on/off for every pointer (Python's "
                        "--move); default: scheduler.cache_settings_for.")
    return parser.parse_args(argv)


def log_directory_for(datasets: bool) -> Path:
    return DATASET_LOG_DIR if datasets else LOG_DIR


def main() -> int:
    global TRIALS, EARLY_TRIALS, PRETEND_ORIGINAL_FITS
    args = parse_arguments()
    PRETEND_ORIGINAL_FITS = not args.exact_original_only
    early_trials = args.early_trials if args.early_trials is not None else args.trials
    if min(args.trials, early_trials) < 1:
        print("trial counts must be positive", file=sys.stderr)
        return 1
    TRIALS, EARLY_TRIALS = args.trials, early_trials
    if args.dynamic_ops < 0:
        print("--dynamic-ops must be non-negative", file=sys.stderr)
        return 1
    try:
        datasets = None if args.datasets is None else select_datasets(args.datasets, args.dataset_root)
    except ValueError as exc:
        print(str(exc), file=sys.stderr)
        return 1
    log_dir = log_directory_for(datasets is not None)
    summary_csv = log_dir / SUMMARY_CSV.name
    if args.report_only:
        rows = write_report(log_dir, summary_csv)
        print_report(rows)
        print(f"\nWrote {len(rows)} row(s) to {summary_csv} (and structures.csv)")
        return 0

    binary = build_binary(NATIVE_BINARY)
    modes = MODES if args.mode == "both" else (args.mode,)
    if args.no_crypto:
        if args.mode != "dry-run":
            print(f"--no-crypto: ignoring --mode {args.mode}; running every job without cryptography")
        modes = ("dry-run",)
    powers = range(args.min_power, args.max_power + 1)
    options = GraphOptions(static=args.static, prime=args.prime, dynamic_ops=args.dynamic_ops)
    probe = BlockFitProbe(binary)
    jobs, skipped = build_jobs(
        binary, probe, modes, args.block_sizes, args.pointers, args.degrees, powers,
        datasets=datasets, move=args.move, options=options, generator=args.generator,
    )
    for config in skipped:
        print(
            f"Skipping {config.jobs} job(s) bs={config.bs} pointer={config.pointer} "
            f"cache={str(config.cache_enabled).lower()} mode={config.mode}: {config.reason}",
            file=sys.stderr,
        )
    if args.skip_existing:
        planned = len(jobs)
        jobs = [job for job in jobs if not job.is_complete()]
        if planned - len(jobs):
            print(f"Skipping {planned - len(jobs)} already-completed job(s)", flush=True)

    try:
        settings = local.Settings.from_environment()
    except ValueError as exc:
        print(str(exc), file=sys.stderr)
        return 1
    scheduler = RustTestScheduler(settings, jobs)
    if args.dry_run:
        print(f"Planned {len(jobs)} Rust job(s); algorithms per job: {algorithm_spec()}")
        return 0

    # As in the Python launcher: children run in their own sessions, so
    # route terminating signals through KeyboardInterrupt to clean them up.
    previous_handlers: list[tuple[int, Any]] = []
    for signal_name in ("SIGTERM", "SIGHUP", "SIGQUIT"):
        signum = getattr(signal, signal_name, None)
        if signum is not None:
            try:
                previous_handlers.append((signum, signal.signal(signum, signal.default_int_handler)))
            except (OSError, ValueError):
                pass
    try:
        status = scheduler.run()
    except KeyboardInterrupt:
        print("Interrupted; terminating active Rust test jobs", file=sys.stderr)
        status = 130
    finally:
        scheduler.terminate_all()
        scheduler.stop_monitors()
        scheduler.shutdown_graph_generation()
        for signum, handler in previous_handlers:
            try:
                signal.signal(signum, handler)
            except (OSError, ValueError):
                pass

    rows = write_report(log_dir, summary_csv)
    print()
    print_report(rows)
    print(f"\nWrote {len(rows)} row(s) to {summary_csv} (and structures.csv)")
    return status


if __name__ == "__main__":
    raise SystemExit(main())
