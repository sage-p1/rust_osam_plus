#!/usr/bin/env python3
"""Write the ER figures and tables of both papers from the Rust reports.

Reads ``summary.csv`` and ``structures.csv`` (written by
``launch_rust_tests.py``) and writes pgfplots files with the names the papers
already include, so no figure environment has to change:

* Concrete OSAM (``--concrete PAPER``): ``PAPER/plots/log2n_vs_<alg>_roundtrips_
  d<d>_bs<bs>_<suffix>.tex`` with four series, OSAM+ (multiwrite, move),
  OSAM (original, no-move), OSAM w/ Move (original, move) and ORAM (recursive,
  no-move), plus ``plots/legend.tex``. The file names carry ``primefalse``:
  the Rust runs do not prime, so ``macros.tex`` must set
  ``\\primetag{primefalse}``.
* BlockOSAM (``--bosam PAPER``): ``PAPER/Plots/bosam_<alg>_d<d>_bs<bs>.tex``
  with three series, BOSAM (r-ary, move), OSAM+ and ORAM, the per-configuration
  figures ``bosam_figure_d<d>_bs<bs>.tex``, the legend and
  ``bosam_summary_table.tex`` (every algorithm at the largest n).

Units. Algorithms are plotted **per step**: a run's round trips divided by its
nominal length (visited vertices for bfs/dfs/dijkstra/prim, walk moves for rw
and pr, neighbor-list retrievals for cd, the whole run for dtc), over
full-length runs only. Build is one construction and is plotted as its total.

Charging. BOSAM is charged reads only (its writes are buffered and evicted in
public batches); every other series is charged reads and writes. ORAM is the
recursive pointer's ORAM structures scaled by the position-map recursion
depth (``oram_*`` columns of the report).

Error bars are the sample standard deviation across trials, taken in linear
space and mapped through log2 (so asymmetric). Build runs once and has none.

    python3 paper_figures.py --summary ../summary.csv --structures ../structures.csv \\
        --concrete ~/Research/osrm/paper --bosam ~/Research/osrm/oblivious_hnsw
"""

from __future__ import annotations

import argparse
import math
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

import pandas as pd

ALGORITHMS = ("build", "cd", "bfs", "dfs", "dijkstra", "prim", "dtc", "pr", "rw")
CONFIGURATIONS = ((20, 64), (20, 4096), (100, 64), (100, 4096))
# The ORAM baseline's structures (as launch_rust_tests.RECURSIVE_STRUCTURES).
RECURSIVE_STRUCTURES = ("RecursivePointer", "SmartQueue")

# Concrete OSAM file-name suffixes (the Python parser's naming; `prime` is
# filled in from --prime-tag).
CONCRETE_SUFFIX = {
    "build": "copiesfalse_{prime}_staticinsertiontrue",
    "cd": "trials50_copiesfalse_{prime}_staticinsertiontrue",
    "bfs": "trials50_copiesfalse_{prime}_staticinsertiontrue_steps100",
    "dfs": "trials50_copiesfalse_{prime}_staticinsertiontrue_steps100",
    "dijkstra": "trials50_copiesfalse_{prime}_staticinsertiontrue_steps100",
    "prim": "trials50_copiesfalse_{prime}_staticinsertiontrue_steps100",
    "dtc": "trials50_copiesfalse_{prime}_staticinsertiontrue_neighbors5",
    "pr": "trials50_copiesfalse_{prime}_staticinsertiontrue_wl50_df0.9",
    "rw": "trials50_copiesfalse_{prime}_staticinsertiontrue_wl50",
}
CAPTION = {
    "build": "Build", "cd": "CD", "bfs": "BFS", "dfs": "DFS", "dijkstra": "Dijkstra",
    "prim": "Prim", "dtc": "DTC", "pr": "PR", "rw": "RW",
}


@dataclass(frozen=True)
class Series:
    label: str
    style: str
    select: Callable[[pd.DataFrame], pd.Series]
    reads_only: bool = False
    oram: bool = False


def _pointer(pointer: str, move: bool | None = None):
    def select(frame: pd.DataFrame) -> pd.Series:
        mask = frame.pointer == pointer
        if move is not None:
            mask &= frame.move == move
        return mask
    return select


OSAM_PLUS = Series("OSAM$^+$", "blue,mark=*,mark options={fill=blue}", _pointer("multiwrite", True))
CONCRETE_SERIES = (
    OSAM_PLUS,
    Series("OSAM", "red,mark=square*,mark options={fill=red}", _pointer("original", False)),
    Series("OSAM w/ Move", "black,mark=triangle*,mark options={fill=black}", _pointer("original", True)),
    Series("ORAM", "black,mark=star,mark options={fill=black}", _pointer("recursive"), oram=True),
)
BOSAM_SERIES = (
    Series("BOSAM", "blue,mark=*,mark options={fill=blue}", _pointer("multiwriterary", True),
           reads_only=True),
    Series("OSAM$^+$", "red,mark=square*,mark options={fill=red}", _pointer("multiwrite", True)),
    Series("ORAM", "black,mark=star,mark options={fill=black}", _pointer("recursive"), oram=True),
)


@dataclass(frozen=True)
class Point:
    log2n: int
    mean: float
    sd: float | None


def load(summary_path: Path, structures_path: Path) -> tuple[pd.DataFrame, pd.DataFrame]:
    summary = pd.read_csv(summary_path)
    summary = summary[summary["mode"] == "dry-run"].copy()
    summary["d"] = summary.log.map(_degree)
    summary["log2n"] = summary.n.map(lambda n: int(n).bit_length() - 1)
    structures = pd.read_csv(structures_path)
    structures = structures[structures["mode"] == "dry-run"].copy()
    structures["d"] = structures.log.map(_degree)
    structures["log2n"] = structures.n.map(lambda n: int(n).bit_length() - 1)
    # structures.csv has no move column: no-move jobs are the cache-off ones.
    structures["move"] = structures.cache.astype(bool)
    return summary, structures


def _degree(log: str) -> int:
    match = re.search(r"_d-(\d+)_", log)
    if not match:
        raise ValueError(f"no degree in log name {log}")
    return int(match.group(1))


def algorithm_points(summary: pd.DataFrame, series: Series, alg: str, d: int, bs: int) -> list[Point]:
    rows = summary[series.select(summary) & (summary.alg == alg) & (summary.d == d)
                   & (summary.bs == bs) & (summary.status == "ok")]
    if series.oram:
        mean_col, sd_col = "oram_roundtrips_per_step", "oram_sd_roundtrips_per_step"
    elif series.reads_only:
        mean_col, sd_col = "mean_reads_per_step", "sd_reads_per_step"
    else:
        # reads + writes (the report charges every pointer but r-ary this way)
        mean_col, sd_col = "mean_roundtrips_per_step", "sd_roundtrips_per_step"
    points = []
    for _, row in rows.sort_values("log2n").iterrows():
        mean = row[mean_col]
        if pd.isna(mean) or mean <= 0:
            continue
        sd = None if pd.isna(row[sd_col]) else float(row[sd_col])
        points.append(Point(int(row.log2n), float(mean), sd))
    return points


def build_points(summary: pd.DataFrame, structures: pd.DataFrame, series: Series,
                 d: int, bs: int) -> list[Point]:
    rows = structures[series.select(structures) & (structures.phase == "build")
                      & (structures.d == d) & (structures.bs == bs)]
    points = []
    for log, group in rows.groupby("log"):
        if series.oram:
            levels = summary.loc[summary.log == log, "oram_levels"].dropna()
            if levels.empty:
                continue
            recursive = group[group.structure.isin(RECURSIVE_STRUCTURES)]
            total = float((recursive.reads + recursive.writes).sum()) * float(levels.iloc[0])
        elif series.reads_only:
            total = float(group.reads.sum())
        else:
            total = float((group.reads + group.writes).sum())
        if total > 0:
            points.append(Point(int(group.log2n.iloc[0]), total, None))
    return sorted(points, key=lambda point: point.log2n)


def log2_bar(point: Point) -> tuple[float, float]:
    """(up, down) error-bar halves in log2 units about log2(mean)."""
    if point.sd is None or point.sd <= 0:
        return 0.0, 0.0
    up = math.log2(point.mean + point.sd) - math.log2(point.mean)
    low = point.mean - point.sd
    # mean - sd <= 0 cannot be drawn on a log axis: cap the lower half at one
    # unit (a factor of two). It does not occur in the ER runs.
    down = math.log2(point.mean) - math.log2(low) if low > 0 else 1.0
    return up, down


def axis(series_points: list[tuple[Series, list[Point]]], ylabel: str, width: str,
         height: str, extra: str = "") -> str:
    lines = [
        "\\begin{tikzpicture}",
        "\\begin{axis}[",
        f"xlabel={{$\\log_2 n$}}, ylabel={{{ylabel}}},",
        f"width={width}, height={height},",
        "grid=major,",
    ]
    if extra:
        lines.append(extra)
    lines.append("]")
    for series, points in series_points:
        lines.append(
            f"\\addplot+[only marks,{series.style},error bars/.cd,y dir=both,y explicit] coordinates {{"
        )
        for point in points:
            up, down = log2_bar(point)
            lines.append(
                f"({point.log2n:.1f}, {math.log2(point.mean):.2f}) += (0, {up:.2f}) -= (0, {down:.2f})"
            )
        lines.append("};")
    lines += ["\\end{axis}", "\\end{tikzpicture}", ""]
    return "\n".join(lines)


def ylabel(alg: str) -> str:
    if alg == "build":
        return "$\\log_2 (\\text{round trips})$"
    return "$\\log_2 (\\text{round trips per step})$"


def points_for(summary, structures, series, alg, d, bs):
    if alg == "build":
        return build_points(summary, structures, series, d, bs)
    return algorithm_points(summary, series, alg, d, bs)


HEADER = "% Generated by rust_osam_plus/bench/paper_figures.py from the Rust benchmark -- do not edit by hand.\n"


def legend(series: tuple[Series, ...], name: str, columns: int) -> str:
    lines = [
        HEADER.rstrip("\n"),
        f"% Defines the shared legend referenced as \\ref{{{name}}}.",
        "\\begin{tikzpicture}",
        "\\begin{axis}[",
        "hide axis,",
        "xmin=0, xmax=1, ymin=0, ymax=1,",
        f"legend columns={columns},",
        "legend cell align={left},",
        "legend style={draw=none, column sep=0.6cm},",
        f"legend to name={name},",
        "]",
    ]
    for entry in series:
        lines.append(f"\\addlegendimage{{only marks,{entry.style}}}")
        lines.append(f"\\addlegendentry{{{entry.label}}}")
    lines += ["\\end{axis}", "\\end{tikzpicture}", ""]
    return "\n".join(lines)


def write_concrete(root: Path, summary, structures, prime_tag: str) -> list[Path]:
    plots = root / "plots"
    plots.mkdir(parents=True, exist_ok=True)
    written = []
    for alg in ALGORITHMS:
        for d, bs in CONFIGURATIONS:
            data = [(series, points_for(summary, structures, series, alg, d, bs))
                    for series in CONCRETE_SERIES]
            name = f"log2n_vs_{alg}_roundtrips_d{d}_bs{bs}_" + CONCRETE_SUFFIX[alg].format(prime=prime_tag)
            path = plots / f"{name}.tex"
            path.write_text(HEADER + axis(data, ylabel(alg), "10cm", "8cm"))
            written.append(path)
    path = plots / "legend.tex"
    path.write_text(legend(CONCRETE_SERIES, "myLegend", 4))
    written.append(path)
    return written


FIGURE_CAPTION = (
    "Round trips at $d={d}$ and $bs={bs}$: total for graph construction (Build), per step for the "
    "eight graph algorithms (a visited vertex for BFS, DFS, Dijkstra and Prim; a move for RW and PR; "
    "a neighbor-list retrieval for CD; the whole run for DTC), over full-length runs only. "
    "\\sysname is charged reads only, since its writes are buffered and evicted in public batches; "
    "OSAM$^+$ and ORAM are charged reads and writes, both of which cost a round trip."
)


def write_bosam(root: Path, summary, structures) -> list[Path]:
    plots = root / "Plots"
    plots.mkdir(parents=True, exist_ok=True)
    written = []
    for alg in ALGORITHMS:
        for d, bs in CONFIGURATIONS:
            data = [(series, points_for(summary, structures, series, alg, d, bs))
                    for series in BOSAM_SERIES]
            path = plots / f"bosam_{alg}_d{d}_bs{bs}.tex"
            path.write_text(HEADER + axis(data, ylabel(alg), "10cm", "7cm", "enlarge y limits=0.08,"))
            written.append(path)
    for d, bs in CONFIGURATIONS:
        lines = [HEADER.rstrip("\n"), "\\begin{figure*}[t]", "    \\centering"]
        for index, alg in enumerate(ALGORITHMS):
            lines += [
                "    \\begin{subfigure}{0.327\\textwidth}",
                "        \\centering",
                f"        \\resizebox{{\\linewidth}}{{!}}{{\\input{{Plots/bosam_{alg}_d{d}_bs{bs}}}}}",
                f"        \\caption{{{CAPTION[alg]}}}",
                "    \\end{subfigure}" + ("\\\\" if index % 3 == 2 and index < len(ALGORITHMS) - 1 else ""),
            ]
        if (d, bs) == TABLE_CONFIGURATIONS[0]:
            # The first figure in document order defines the shared legend.
            lines.append("    \\raisebox{0pt}[0pt][0pt]{\\makebox[0pt][l]{\\input{Plots/bosam_legend}}}")
        lines += [
            "    \\ref{bosamlegend}",
            f"    \\caption{{{FIGURE_CAPTION.format(d=d, bs=bs)}}}",
            f"    \\label{{fig:osam-graph-workloads-d{d}-bs{bs}}}",
            "\\end{figure*}",
            "",
        ]
        path = plots / f"bosam_figure_d{d}_bs{bs}.tex"
        path.write_text("\n".join(lines))
        written.append(path)
    path = plots / "bosam_legend.tex"
    path.write_text(legend(BOSAM_SERIES, "bosamlegend", 3))
    written.append(path)
    path = plots / "bosam_summary_table.tex"
    path.write_text(summary_table(summary, structures))
    written.append(path)
    return written


# Configurations of the summary table, in order (the headline one first).
TABLE_CONFIGURATIONS = ((100, 4096), (20, 64), (100, 64), (20, 4096))


def cell(point: Point | None) -> str:
    if point is None:
        return "--"
    value = math.log2(point.mean)
    if point.sd is None:
        return f"${value:.1f}$"
    up, down = log2_bar(point)
    return f"${value:.1f}^{{+{up:.2f}}}_{{-{down:.2f}}}$"


def summary_table(summary, structures) -> str:
    header = " & ".join(CAPTION[alg] for alg in ALGORITHMS)
    lines = [
        HEADER.rstrip("\n"),
        "\\begin{table*}[t]",
        "\\centering\\footnotesize",
        "\\renewcommand{\\arraystretch}{1.35}",
        "\\resizebox{\\textwidth}{!}{%",
        "\\begin{tabular}{l | l | r r r r r r r r r}",
        "\\hline",
        f"Configuration & Impl. & {header}\\\\",
        "\\hline",
    ]
    for d, bs in TABLE_CONFIGURATIONS:
        rows = summary[(summary.d == d) & (summary.bs == bs)]
        n = int(rows.log2n.max())
        lines.append(f"\\multirow{{{len(BOSAM_SERIES)}}}{{*}}{{\\shortstack[l]{{$d={d}$\\\\$bs={bs}$\\\\$n=2^{{{n}}}$}}}}")
        for index, series in enumerate(BOSAM_SERIES):
            cells = []
            for alg in ALGORITHMS:
                points = {point.log2n: point for point in points_for(summary, structures, series, alg, d, bs)}
                cells.append(cell(points.get(n)))
            end = "\\\\ \\hline" if index == len(BOSAM_SERIES) - 1 else "\\\\"
            lines.append(f"& {series.label} & " + " & ".join(cells) + end)
    lines += [
        "\\end{tabular}}",
        "\\caption{Base-two logarithm of round trips at the largest graph size of each configuration, "
        "for \\sysname, OSAM$^+$ and recursive Path ORAM: the total for Build and the cost per step for "
        "the algorithms (a visited vertex for BFS, DFS, Dijkstra and Prim; a move for RW and PR; a "
        "neighbor-list retrieval for CD; the whole run for DTC), averaged over full-length runs. "
        "\\sysname is charged reads only, since its writes are buffered and evicted in public batches; "
        "the other two are charged reads and writes, both of which cost a round trip. Bars are the "
        "sample standard deviation across trials, taken in linear space and mapped through the "
        "logarithm, so they are asymmetric; Build runs once and carries none. Per-size behaviour is in "
        "Appendix~\\ref{app:osam-graph-workloads}.}",
        "\\label{tab:osam-graph-workloads}",
        "\\end{table*}",
        "",
    ]
    return "\n".join(lines)



# --------------------------------------------------------------------------
# Real datasets (Concrete OSAM Table "real results")
# --------------------------------------------------------------------------

# name in the logs -> (display name, directed), in table order.
DATASETS = {
    "emailEucore": ("email-EU-core", True),
    "p2pGnutella04": ("p2p-Gnutella04", True),
    "higgsTwitter": ("Higgs-Twitter", True),
    "gpluscombined": ("ego-Gplus", True),
    "twitchgamers": ("twitch-gamers", False),
    "roadNetPA": ("roadNet-PA", False),
    "comyoutube": ("com-Youtube", False),
}
DATASET_COLUMNS = ("build", "bfs", "cd", "dfs", "dijkstra", "dtc", "pr", "prim", "rw")
DATASET_HEADER = {"dijkstra": "Dij."}
# Concrete OSAM series in the table's row order.
DATASET_SERIES = (CONCRETE_SERIES[0], CONCRETE_SERIES[3], CONCRETE_SERIES[2], CONCRETE_SERIES[1])
DATASET_LABEL = {"OSAM w/ Move": "w/ Move"}


def load_datasets(summary_path: Path, structures_path: Path) -> tuple[pd.DataFrame, pd.DataFrame]:
    summary = pd.read_csv(summary_path)
    summary = summary[summary["mode"] == "dry-run"].copy()
    structures = pd.read_csv(structures_path)
    structures = structures[structures["mode"] == "dry-run"].copy()
    for frame in (summary, structures):
        frame["dataset"] = frame.log.str.split("_").str[0]
    structures["move"] = structures.cache.astype(bool)
    return summary, structures


def dataset_point(summary, structures, series: Series, dataset: str, alg: str, bs: int):
    """(Point or None, status, trials) for one table cell."""
    if alg == "build":
        rows = structures[series.select(structures) & (structures.phase == "build")
                          & (structures.dataset == dataset) & (structures.bs == bs)]
        if rows.empty:
            return None, "missing", 0
        if series.oram:
            levels = summary.loc[summary.log == rows.log.iloc[0], "oram_levels"].dropna()
            if levels.empty:
                return None, "missing", 0
            recursive = rows[rows.structure.isin(RECURSIVE_STRUCTURES)]
            total = float((recursive.reads + recursive.writes).sum()) * float(levels.iloc[0])
        elif series.reads_only:
            total = float(rows.reads.sum())
        else:
            total = float((rows.reads + rows.writes).sum())
        return Point(0, total, None), "ok", 1
    rows = summary[series.select(summary) & (summary.alg == alg) & (summary.dataset == dataset)
                   & (summary.bs == bs)]
    if rows.empty:
        return None, "missing", 0
    row = rows.iloc[0]
    if row.status == "failed":
        return None, "failed", 0
    if series.oram:
        mean, sd = row.oram_roundtrips_per_step, row.oram_sd_roundtrips_per_step
    elif series.reads_only:
        mean, sd = row.mean_reads_per_step, row.sd_reads_per_step
    else:
        mean, sd = row.mean_roundtrips_per_step, row.sd_roundtrips_per_step
    if pd.isna(mean) or mean <= 0:
        return None, "failed", 0
    return Point(0, float(mean), None if pd.isna(sd) else float(sd)), row.status, int(row.trials)


def dataset_cell(point: Point | None, status: str) -> str:
    """log2 of the mean, then the coefficient of variation (sd / mean) across
    runs. On skewed graphs sd often exceeds the mean, so a log-scale bar would
    have no lower end."""
    if point is None:
        return "--"
    text = f"${math.log2(point.mean):.1f}$"
    if point.sd is not None:
        text += f" {{\\scriptsize({point.sd / point.mean:.2f})}}"
    return text + "$^{\\dagger}$" if status == "short" else text


def dataset_table(summary, structures) -> str:
    header = " & ".join(DATASET_HEADER.get(alg, CAPTION[alg]) for alg in DATASET_COLUMNS)
    width = 3 + len(DATASET_COLUMNS)
    lines = [
        HEADER.rstrip("\n"),
        "\\begin{table*}[t]",
        "\\centering\\footnotesize",
        "\\renewcommand{\\arraystretch}{1.35}",
        "\\resizebox{\\textwidth}{!}{%",
        "\\begin{tabular}{l | r | l | " + " ".join("r" for _ in DATASET_COLUMNS) + "}",
        "\\hline",
        f"Dataset & $bs$ & Impl. & {header}\\\\",
        "\\hline",
    ]
    short = set()
    for dataset, (name, directed) in DATASETS.items():
        rows = summary[summary.dataset == dataset]
        if rows.empty:
            continue
        n, edges = int(rows.n.iloc[0]), int(rows.edges.iloc[0])
        kind = "Directed" if directed else "Undirected"
        stack = f"{name}\\\\{kind}\\\\$\\log n = {math.log2(n):.1f}$\\\\$d={edges / n:.1f}$"
        span = 2 * len(DATASET_SERIES)
        lines.append(f"\\multirow{{{span}}}{{*}}{{\\shortstack[r]{{{stack}}}}}")
        for block, bs in enumerate((64, 4096)):
            prefix = "& " if block == 0 else "& "
            lines.append(f"{prefix}\\multirow{{{len(DATASET_SERIES)}}}{{*}}{{${bs}$}}")
            for index, series in enumerate(DATASET_SERIES):
                cells = []
                for alg in DATASET_COLUMNS:
                    point, status, trials = dataset_point(summary, structures, series, dataset, alg, bs)
                    if status == "short":
                        short.add((name, alg, trials))
                    cells.append(dataset_cell(point, status))
                label = DATASET_LABEL.get(series.label, series.label)
                lead = "& " if index == 0 else "& & "
                if block == 1 and index == len(DATASET_SERIES) - 1:
                    end = "\\\\ \\hline"
                elif index == len(DATASET_SERIES) - 1:
                    end = f"\\\\ \\cline{{2-{width}}}"
                else:
                    end = f"\\\\ \\cline{{3-{width}}}"
                lines.append(f"{lead}{label} & " + " & ".join(cells) + end)
    notes = sorted({f"{name} {CAPTION[alg]}: {trials}" for name, alg, trials in short})
    short_text = ""
    if notes:
        short_text = (" $^{\\dagger}$Fewer than $50$ full-length runs were found in $1000$ draws of the "
                      "entry point (" + "; ".join(notes) + " runs).")
    lines += [
        "\\end{tabular}}",
        "\\caption{Base-two logarithm of round trips (reads plus writes) on each real dataset: the "
        "total for Build and the cost per step for the algorithms (a visited vertex for BFS, DFS, "
        "Dijkstra, and Prim; a move for RW and PR; a neighbor-list retrieval for CD; the whole run for "
        "DTC), averaged over $50$ full-length runs. In parentheses: the coefficient of variation "
        "(sample standard deviation divided by the mean) of that cost across runs. Entries marked -- had no full-length run in $1000$ draws of the entry point."
        + short_text + "}",
        "\\label{tab:real results}",
        "\\end{table*}",
        "",
    ]
    return "\n".join(lines)


# Full-length conditions and the algorithms they apply to (acceptance is the
# same for every pointer: each algorithm draws the same seeded entry points).
ACCEPTANCE_COLUMNS = (("rw", "RW"), ("bfs", "BFS/DFS/Dij./Prim"), ("dtc", "DTC"))


def acceptance_table(summary) -> str:
    lines = [
        HEADER.rstrip("\n"),
        "\\begin{table}[t]",
        "\\centering\\small",
        "\\begin{tabular}{l | " + " ".join("r" for _ in ACCEPTANCE_COLUMNS) + "}",
        "\\hline",
        "Dataset & " + " & ".join(label for _, label in ACCEPTANCE_COLUMNS) + "\\\\",
        "\\hline",
    ]
    for dataset, (name, _) in DATASETS.items():
        rows = summary[(summary.dataset == dataset) & (summary.pointer == "multiwrite") & (summary.bs == 64)]
        if rows.empty:
            continue
        cells = []
        for alg, _ in ACCEPTANCE_COLUMNS:
            row = rows[rows.alg == alg]
            if row.empty:
                cells.append("--")
                continue
            row = row.iloc[0]
            cells.append(f"${100 * row.trials / row.attempts:.1f}\\%$" if row.trials else f"$0/{int(row.attempts)}$")
        lines.append(f"{name} & " + " & ".join(cells) + "\\\\")
    lines += [
        "\\hline",
        "\\end{tabular}",
        "\\caption{Share of random entry points that give a full-length run: a random walk of $50$ "
        "moves that reaches no vertex without outgoing edges, a traversal that visits $100$ vertices, "
        "and a triangle count whose every neighbor list has at least $5$ entries. Entries are drawn "
        "until $50$ runs are full length, at most $1000$ times; $0/1000$ means none was found.}",
        "\\label{tab:dataset-acceptance}",
        "\\end{table}",
        "",
    ]
    return "\n".join(lines)


def write_concrete_datasets(root: Path, summary, structures) -> list[Path]:
    tables = root / "tables"
    tables.mkdir(parents=True, exist_ok=True)
    written = []
    path = tables / "dataset_table.tex"
    path.write_text(dataset_table(summary, structures))
    written.append(path)
    path = tables / "dataset_acceptance.tex"
    path.write_text(acceptance_table(summary))
    written.append(path)
    return written


# BlockOSAM real-dataset tables: datasets with average out-degree >= 10.
BOSAM_MIN_DEGREE = 10
BOSAM_MAIN_COLUMNS = ("build", "cd", "bfs", "rw")
BOSAM_FULL_COLUMNS = ("build", "cd", "bfs", "dfs", "dijkstra", "prim", "dtc", "pr", "rw")


def bosam_dataset_table(summary, structures, columns, full: bool,
                        min_degree: float = BOSAM_MIN_DEGREE) -> str:
    width = 3 + len(columns)
    header = " & ".join(f"\\textsf{{{CAPTION[alg] if alg != 'dijkstra' else 'Dijkstra'}}}" for alg in columns)
    if full:
        # Six or more datasets overflow a page at the default row height.
        lines = [HEADER.rstrip("\n"), "\\begin{table*}[t]", "\\centering",
                 "\\renewcommand{\\arraystretch}{0.9}"]
    else:
        lines = [HEADER.rstrip("\n"), "\\begin{table}[t]", "\\centering"]
    short = set()
    body = []
    for dataset, (name, directed) in DATASETS.items():
        rows = summary[summary.dataset == dataset]
        if rows.empty:
            continue
        n, edges = int(rows.n.iloc[0]), int(rows.edges.iloc[0])
        if edges / n < min_degree:
            continue
        kind = "Directed" if directed else "Undirected"
        stack = f"{name}\\\\{kind}\\\\$\\log_2 n={math.log2(n):.1f}$\\\\$d={edges / n:.0f}$"
        body.append(f"\\multirow{{{2 * len(BOSAM_SERIES)}}}{{*}}{{\\shortstack[l]{{{stack}}}}}")
        for block, bs in enumerate((64, 4096)):
            body.append(f"& \\multirow{{{len(BOSAM_SERIES)}}}{{*}}{{${bs}$}}")
            for index, series in enumerate(BOSAM_SERIES):
                cells = []
                for alg in columns:
                    point, status, trials = dataset_point(summary, structures, series, dataset, alg, bs)
                    if status == "short":
                        short.add((name, alg, trials))
                    if point is None:
                        cells.append("--")
                    else:
                        cells.append(f"{math.log2(point.mean):.1f}" + ("$^{\\dagger}$" if status == "short" else ""))
                lead = "& " if index == 0 else "& & "
                end = " \\\\"
                if index == len(BOSAM_SERIES) - 1:
                    end = f" \\\\ \\cmidrule{{2-{width}}}" if block == 0 else " \\\\"
                body.append(f"{lead}{series.label} & " + " & ".join(cells) + end)
        body.append("\\midrule")
    if body and body[-1] == "\\midrule":
        body[-1] = "\\bottomrule"
    notes = sorted({f"{name} {CAPTION[alg]}: {trials}" for name, alg, trials in short if alg in columns})
    short_text = ""
    if notes:
        short_text = (" $^{\\dagger}$Fewer than $50$ full-length runs in $1000$ draws of the entry point ("
                      + "; ".join(notes) + " runs).")
    what = ("all eight algorithms" if full else "Build, Contact Discovery, BFS (the frontier "
            "traversals behave alike; all eight algorithms are in "
            "Table~\\ref{tab:real-graph-results-full}), and Random Walk")
    caption = (
        f"\\caption{{Base-two logarithm of round trips on the SNAP graphs"
        + (f" with average out-degree $d\\ge {min_degree:g}$" if min_degree > 0 else "")
        + f", for {what}: the total for Build and the cost per step for the "
        "algorithms (a visited vertex, a walk move, or a neighbor-list retrieval; the whole run for "
        "DTC), averaged over $50$ full-length runs. \\sysname is charged reads only, since its writes "
        "are buffered and evicted in public batches; OSAM$^+$ and recursive Path ORAM are charged reads "
        "and writes. Entries marked -- had no full-length run in $1000$ draws of the entry point."
        + short_text + "}"
    )
    lines.append(caption)
    if not full:
        lines += ["\\ifnum\\conference=0", "\\def\\realgraphtablewidth{0.70\\columnwidth}", "\\else",
                  "\\def\\realgraphtablewidth{\\columnwidth}", "\\fi",
                  "\\resizebox{\\realgraphtablewidth}{!}{%"]
    else:
        lines.append("\\resizebox{\\textwidth}{!}{%")
    lines += [
        "\\begin{tabular}{l | r | l | " + " | ".join("r" for _ in columns) + "}",
        "\\toprule",
        f"\\textsf{{Dataset}} & $\\mathit{{bs}}$ & \\textsf{{Impl.}} & {header}\\\\",
        "\\midrule",
    ] + body + [
        "\\end{tabular}}",
        "\\label{tab:real-graph-results-full}" if full else "\\label{tab:real-graph-results}",
        "\\end{table*}" if full else "\\end{table}",
        "",
    ]
    return "\n".join(lines)


def write_bosam_datasets(root: Path, summary, structures,
                         min_degree: float = BOSAM_MIN_DEGREE) -> list[Path]:
    plots = root / "Plots"
    plots.mkdir(parents=True, exist_ok=True)
    written = []
    for name, columns, full in (("bosam_dataset_table.tex", BOSAM_MAIN_COLUMNS, False),
                                ("bosam_dataset_table_full.tex", BOSAM_FULL_COLUMNS, True)):
        path = plots / name
        path.write_text(bosam_dataset_table(summary, structures, columns, full, min_degree))
        written.append(path)
    return written

def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--summary", type=Path, help="ER summary.csv")
    parser.add_argument("--structures", type=Path, help="ER structures.csv")
    parser.add_argument("--dataset-summary", type=Path, help="dataset summary.csv")
    parser.add_argument("--dataset-structures", type=Path, help="dataset structures.csv")
    parser.add_argument("--skip-datasets", nargs="*", default=[], metavar="NAME",
                        help="datasets (log names) to leave out of the dataset tables")
    parser.add_argument("--concrete", type=Path, help="Concrete OSAM paper root (has plots/)")
    parser.add_argument("--bosam", type=Path, help="BlockOSAM paper root (has Plots/)")
    parser.add_argument("--bosam-min-degree", type=float, default=0.0,
                        help="keep only datasets with average out-degree >= this in the BlockOSAM "
                             "dataset tables (0 keeps every dataset; the old default was "
                             f"{BOSAM_MIN_DEGREE})")
    parser.add_argument("--prime-tag", default="primefalse",
                        help="file-name tag matching \\primetag in the Concrete OSAM macros.tex")
    args = parser.parse_args()
    written = []
    if args.summary and args.structures:
        summary, structures = load(args.summary.expanduser(), args.structures.expanduser())
        if args.concrete:
            written += write_concrete(args.concrete.expanduser(), summary, structures, args.prime_tag)
        if args.bosam:
            written += write_bosam(args.bosam.expanduser(), summary, structures)
    if args.dataset_summary and args.dataset_structures:
        summary, structures = load_datasets(args.dataset_summary.expanduser(),
                                            args.dataset_structures.expanduser())
        summary = summary[~summary.dataset.isin(args.skip_datasets)]
        structures = structures[~structures.dataset.isin(args.skip_datasets)]
        if args.concrete:
            written += write_concrete_datasets(args.concrete.expanduser(), summary, structures)
        if args.bosam:
            written += write_bosam_datasets(args.bosam.expanduser(), summary, structures,
                                            args.bosam_min_degree)
    print(f"wrote {len(written)} file(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
