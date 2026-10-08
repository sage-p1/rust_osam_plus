#!/usr/bin/env python3
"""Degree-skew experiment for the Concrete OSAM paper (Table ``tab:skew``).

Four graphs with n = 2^14 vertices and average degree about 20: an
Erdos-Renyi graph and three Chung-Lu graphs whose expected degrees follow
power laws with exponents 3.0, 2.5 and 2.1. Consecutive connected components
are joined by one random edge each, as in ``graphs.generate_graph``. Every
algorithm runs at bs = 64 with OSAM+ (``multiwrite``, cached) and recursive
ORAM; the original OSAM (no move) runs the walks, CD and BFS only, because its
dry-run memory grows quickly on the skewed graphs.

Usage (from ``rust_osam_plus/bench``)::

    python3 skew_experiment.py generate          # graphs/skew/*.edgelist
    python3 skew_experiment.py run [--jobs 2]    # results/skew-logs/*.log
    python3 skew_experiment.py report [--concrete <paper>]
                                                 # results/skew-summary.csv,
                                                 # <paper>/tables/skew_table.tex

Costs follow the paper's charging: OSAM+ is charged reads (which include its
public flushes), OSAM reads + writes, and ORAM the recursive structures'
reads + writes times the recursion depth (``oram_recursion_levels``). Per-step
costs divide a run by its length, as in ``launch_rust_tests``.
"""

from __future__ import annotations

import argparse
import collections
import concurrent.futures
import math
import random
import statistics
import subprocess
from pathlib import Path

from launch_rust_tests import RECURSIVE_STRUCTURES, build_binary, oram_recursion_levels

ROOT = Path(__file__).resolve().parent
GRAPH_DIR = ROOT / "graphs" / "skew"
LOG_DIR = ROOT / "results" / "skew-logs"
SUMMARY = ROOT / "results" / "skew-summary.csv"

N = 2**14
DEGREE = 20
SEED = 1
BLOCK_SIZE = 64
GRAPHS = ("er", "pl3.0", "pl2.5", "pl2.1")
LENGTH = {"rw": 50, "pr": 50, "cd": 2, "bfs": 100, "dfs": 100, "dijkstra": 100, "prim": 100, "dtc": 1}
FULL = "rw:200,pr:200,cd:200,bfs:200,dfs:200,dijkstra:200,prim:200,dtc:100"
SMALL = "rw:100,pr:100,cd:100,bfs:50"
POINTERS = {
    # tag: (--pt, algorithms, extra flags)
    "osamplus": ("multiwrite", FULL, ["--cache"]),
    "oram": ("recursive", FULL, ["--no-cache", "--no-move"]),
    "osam": ("original", SMALL, ["--no-cache", "--no-move", "--pretend-original-fits"]),
}
COMMON = ["--bs", str(BLOCK_SIZE), "--wl", "50", "--max-steps", "100", "--max-neighbors", "5",
          "--df", "0.9", "--seed", str(SEED), "--dry-run"]


def graph_path(name: str) -> Path:
    return GRAPH_DIR / f"{name}_n-{N}_d-{DEGREE}_seed-{SEED}.edgelist"


# --------------------------------------------------------------------------
# Graphs
# --------------------------------------------------------------------------


def chung_lu(gamma: float):
    """Chung-Lu graph with power-law expected degrees of mean DEGREE.

    Probabilities are capped at one, which lowers the realized mean for small
    exponents, so the weights are rescaled until the mean is within 0.4.
    """
    import networkx as nx
    import numpy as np

    base = (np.arange(N) + 1.0) ** (-1.0 / (gamma - 1.0))
    scale = 1.0
    for _ in range(6):
        weights = base * DEGREE * scale / base.mean()
        graph = nx.Graph(nx.expected_degree_graph(list(weights), seed=SEED, selfloops=False))
        mean = 2 * graph.number_of_edges() / N
        if abs(mean - DEGREE) < 0.4:
            break
        scale *= DEGREE / mean
    return graph


def write_graph(graph, path: Path) -> dict:
    import networkx as nx

    rng = random.Random(SEED)
    components = list(nx.connected_components(graph))
    for left, right in zip(components, components[1:]):
        graph.add_edge(rng.choice(tuple(left)), rng.choice(tuple(right)))
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        handle.writelines(f"v {vertex}\n" for vertex in sorted(graph.nodes))
        for source, neighbors in graph.adjacency():
            handle.writelines(f"{source} {destination} 0\n" for destination in neighbors)
    degrees = [degree for _, degree in graph.degree()]
    mean = statistics.fmean(degrees)
    return {"edges": graph.number_of_edges(), "mean": mean, "max": max(degrees),
            "cv": statistics.pstdev(degrees) / mean, "components": len(components)}


def generate() -> None:
    import networkx as nx

    for name in GRAPHS:
        graph = (nx.fast_gnp_random_graph(N, DEGREE / N, seed=SEED) if name == "er"
                 else chung_lu(float(name[2:])))
        stats = write_graph(graph, graph_path(name))
        print(f"{name}: {stats}")


# --------------------------------------------------------------------------
# Runs
# --------------------------------------------------------------------------


def log_path(graph: str, tag: str) -> Path:
    return LOG_DIR / f"skew_{graph}_bs-{BLOCK_SIZE}_pt-{tag}.log"


def run_job(binary: Path, graph: str, tag: str) -> str:
    pointer, algorithms, extra = POINTERS[tag]
    output = log_path(graph, tag)
    if output.is_file() and "done" in output.read_text().split():
        return f"{output.name}: up to date"
    command = [str(binary), "--graph", str(graph_path(graph)), "--pt", pointer, "--alg", algorithms,
               *COMMON, *extra, "--output", str(output)]
    result = subprocess.run(command, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    status = "ok" if result.returncode == 0 else f"exit {result.returncode}"
    return f"{output.name}: {status} {result.stderr.strip()[-200:]}"


def run(jobs: int) -> None:
    binary = build_binary()
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    fast = [(g, t) for g in GRAPHS for t in ("osamplus", "oram")]
    # The original OSAM is memory-hungry; run it one job at a time at the end.
    with concurrent.futures.ThreadPoolExecutor(jobs) as pool:
        for message in pool.map(lambda job: run_job(binary, *job), fast):
            print(message, flush=True)
    for graph in GRAPHS:
        print(run_job(binary, graph, "osam"), flush=True)


# --------------------------------------------------------------------------
# Report
# --------------------------------------------------------------------------


def records(path: Path):
    for line in path.read_text().splitlines():
        parts = line.split()
        if not parts:
            continue
        yield parts[0], dict(part.split("=", 1) for part in parts[1:] if "=" in part)


def summarize(graph: str, tag: str) -> list[dict]:
    path = log_path(graph, tag)
    if not path.is_file():
        # A run killed part-way (OSAM can exhaust memory on the most skewed
        # graph) leaves the benchmark's .partial file; its completed
        # algorithms are still usable.
        path = path.with_name(path.name + ".partial")
        if not path.is_file():
            return []
    trials, structures = collections.defaultdict(list), []
    for kind, fields in records(path):
        if kind == "trial":
            trials[fields["alg"]].append(fields)
        elif kind == "structure":
            structures.append(fields)
    levels = None
    if tag == "oram":
        allocations = sum(int(s["allocations"]) for s in structures if s["name"] in RECURSIVE_STRUCTURES)
        levels = oram_recursion_levels(allocations, BLOCK_SIZE)
    rows = []
    for algorithm, runs in trials.items():
        reads = [int(run["reads"]) for run in runs]
        writes = [int(run["writes"]) for run in runs]
        if tag == "osamplus":
            costs = [float(r) for r in reads]
        elif tag == "osam":
            costs = [float(r + w) for r, w in zip(reads, writes)]
        else:
            phase = [s for s in structures if s["phase"] == algorithm]
            total = sum(int(s["roundtrips"]) for s in phase)
            recursive = sum(int(s["roundtrips"]) for s in phase if s["name"] in RECURSIVE_STRUCTURES)
            costs = [(r + w) * recursive / total * levels for r, w in zip(reads, writes)]
        mean = statistics.fmean(costs)
        rows.append({
            "graph": graph, "pointer": tag, "alg": algorithm, "runs": len(costs),
            "mean_per_run": mean, "sd_per_run": statistics.stdev(costs) if len(costs) > 1 else 0.0,
            "max_per_run": max(costs), "mean_per_step": mean / LENGTH[algorithm],
            "cv": (statistics.stdev(costs) / mean) if len(costs) > 1 else 0.0,
            "oram_levels": levels if levels is not None else "",
        })
    return rows


def degree_stats(graph: str) -> tuple[int, float]:
    degrees = collections.Counter()
    for line in graph_path(graph).read_text().splitlines():
        parts = line.split()
        if parts and parts[0] != "v":
            degrees[parts[0]] += 1
    values = list(degrees.values()) + [0] * (N - len(degrees))
    mean = statistics.fmean(values)
    return max(values), statistics.pstdev(values) / mean


def latex_table(rows: list[dict]) -> str:
    names = {"er": r"Erd\H{o}s--R\'enyi", "pl3.0": r"$\gamma=3.0$", "pl2.5": r"$\gamma=2.5$",
             "pl2.1": r"$\gamma=2.1$"}
    algorithms = [("rw", "RW"), ("pr", "PR"), ("cd", "CD"), ("bfs", "BFS"), ("dfs", "DFS"),
                  ("dijkstra", "Dij."), ("dtc", "DTC")]
    pointers = [("osamplus", "OSAM$^+$"), ("oram", "ORAM"), ("osam", "OSAM")]
    by_key = {(r["graph"], r["pointer"], r["alg"]): r for r in rows}
    lines = [r"\begin{table*}[t]", r"\centering\footnotesize", r"\setlength{\tabcolsep}{4pt}",
             r"\begin{tabular}{l l | " + " ".join("r" * len(algorithms)) + "}", r"\hline",
             "Graph & Impl. & " + " & ".join(label for _, label in algorithms) + r"\\", r"\hline"]
    for graph in GRAPHS:
        if not graph_path(graph).is_file():
            continue
        maximum, cv = degree_stats(graph)
        for index, (tag, label) in enumerate(pointers):
            first = (rf"\multirow{{3}}{{*}}{{\shortstack[l]{{{names[graph]}\\max deg.\ {maximum}"
                     rf"\\deg.\ CV {cv:.2f}}}}}" if index == 0 else "")
            cells = []
            for algorithm, _ in algorithms:
                row = by_key.get((graph, tag, algorithm))
                if row is None:
                    cells.append("--")
                    continue
                value = row["mean_per_step"]
                text = f"{value:.0f}" if value >= 100 else f"{value:.1f}"
                cells.append(f"${text}$ {{\\scriptsize({row['cv']:.2f})}}")
            end = r" \hline" if index == len(pointers) - 1 else r" \cline{2-" + str(2 + len(algorithms)) + "}"
            lines.append(f"{first} & {label} & " + " & ".join(cells) + r"\\" + end)
    lines += [r"\end{tabular}",
              r"\caption{Effect of degree skew ($n=2^{14}$, average degree $\approx 20$, $\mathit{bs}=64$): "
              r"round trips per step (per run for DTC), averaged over $200$ full-length runs ($100$ for DTC; "
              r"$100$ walks and $50$ BFS runs for OSAM), with the coefficient of variation of the cost of a run "
              r"across entry points in parentheses. Rows are an Erd\H{o}s--R\'enyi graph and Chung--Lu graphs "
              r"with power-law expected degrees of exponent $\gamma$; ``max deg.'' and ``deg.\ CV'' give the "
              r"maximum degree and the coefficient of variation of the degree. Prim behaves like Dijkstra and "
              r"is omitted. -- marks runs we did not perform (OSAM on the largest workloads exceeded the memory "
              r"of our test machine).}",
              r"\label{tab:skew}", r"\end{table*}"]
    return ("% Generated by rust_osam_plus/bench/skew_experiment.py -- do not edit by hand.\n"
            + "\n".join(lines) + "\n")


def report(concrete: Path | None) -> None:
    import csv

    rows = [row for graph in GRAPHS for tag in POINTERS for row in summarize(graph, tag)]
    SUMMARY.parent.mkdir(parents=True, exist_ok=True)
    with SUMMARY.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    print(f"wrote {SUMMARY} ({len(rows)} rows)")
    if concrete is not None:
        output = Path(concrete) / "tables" / "skew_table.tex"
        output.write_text(latex_table(rows))
        print(f"wrote {output}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("generate")
    runner = sub.add_parser("run")
    runner.add_argument("--jobs", type=int, default=2)
    reporter = sub.add_parser("report")
    reporter.add_argument("--concrete", type=Path, help="Concrete OSAM paper directory")
    args = parser.parse_args()
    if args.command == "generate":
        generate()
    elif args.command == "run":
        run(args.jobs)
    else:
        report(args.concrete)


if __name__ == "__main__":
    main()
