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

Charging. BOSAM and OSAM+ are charged reads only (their writes stay in the
stash and are evicted by reads; BOSAM also flushes in public batches); every
other series is charged reads and writes. ORAM is the
recursive pointer's ORAM structures scaled by the position-map recursion
depth (``oram_*`` columns of the report).

Datasets (``--dataset-summary``): ``tables/dataset_table.tex`` (bs = 64, in the
body) and ``tables/dataset_table_bs4096.tex`` (bs = 4096, in the appendix).
Counts come from dry-run rows, or from crypto rows for jobs run only with
cryptography (both make the same accesses); when crypto rows exist, each
caption gives the maximum client stash occupancy over them.

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


# OSAM+ keeps its writes in the stash and evicts two paths per read, so like
# BOSAM it is charged reads only.
OSAM_PLUS = Series("OSAM$^+$", "blue,mark=*,mark options={fill=blue}", _pointer("multiwrite", True),
                   reads_only=True)
CONCRETE_SERIES = (
    OSAM_PLUS,
    Series("OSAM", "red,mark=square*,mark options={fill=red}", _pointer("original", False)),
    Series("OSAM w/ Move", "black,mark=triangle*,mark options={fill=black}", _pointer("original", True)),
    Series("ORAM", "black,mark=star,mark options={fill=black}", _pointer("recursive"), oram=True),
)
BOSAM_SERIES = (
    Series("BOSAM", "blue,mark=*,mark options={fill=blue}", _pointer("multiwriterary", True),
           reads_only=True),
    Series("OSAM$^+$", "red,mark=square*,mark options={fill=red}", _pointer("multiwrite", True),
           reads_only=True),
    Series("ORAM", "black,mark=star,mark options={fill=black}", _pointer("recursive"), oram=True),
)


def fit_open() -> str:
    """Start boxing a tabular so fit_close() can scale it to a width and a height."""
    return ("\\ExplSyntaxOn\\cs_gset:Npn \\osamfpeval #1 {\\fp_eval:n {#1}}\\ExplSyntaxOff\n"
            "\\ifdefined\\osamfitbox\\else\\newsavebox\\osamfitbox\\fi\n\\sbox\\osamfitbox{%")


def fit_close(width: str, height: str = "0.78\\textheight") -> str:
    """Close fit_open(): scale to `width`, shrinking further if taller than `height`."""
    return ("}%\n\\scalebox{\\osamfpeval{min((\\the\\dimexpr " + width + "\\relax)/(\\the\\wd\\osamfitbox),"
            " (\\the\\dimexpr " + height + "\\relax)/(\\the\\ht\\osamfitbox + \\the\\dp\\osamfitbox))}}"
            "{\\usebox\\osamfitbox}")


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
        # reads + writes (the report charges OSAM and ORAM this way)
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


# Per-step units and charging are defined once, in the paper's experimental
# setting (Section sec:eval-osam-workloads); the captions point there.
FIGURE_CAPTION = (
    "Round trips at $d={d}$ and $bs={bs}$: total for Build, per step for the algorithms "
    "(Section~\\ref{{sec:eval-osam-workloads}})."
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
        fit_open(),
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
        "\\end{tabular}" + fit_close("\\textwidth"),
        "\\caption{Base-two logarithm of round trips at the largest graph size of each configuration: "
        "total for Build, per step for the algorithms (Section~\\ref{sec:eval-osam-workloads}). "
        "Superscripts and subscripts give one standard deviation across trials, mapped through the "
        "logarithm. Every graph size is in Appendix~\\ref{app:osam-graph-workloads}.}",
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


# A job's identity without its mode: dry-run and crypto runs of the same job
# make the same accesses, so either gives the round trips.
_JOB_KEY = ("dataset", "bs", "pointer", "cache", "move", "layout")


def _prefer_dry_run(frame: pd.DataFrame) -> pd.DataFrame:
    """Dry-run rows, plus the crypto rows of jobs that only ran in crypto mode."""
    key = [column for column in _JOB_KEY if column in frame.columns]
    dry = frame[frame["mode"] == "dry-run"]
    crypto = frame[frame["mode"] == "crypto"]
    have = set(map(tuple, dry[key].astype(str).values))
    only = crypto[[tuple(row) not in have for row in crypto[key].astype(str).values]]
    return pd.concat([dry, only]).copy()


def load_datasets(summary_path: Path, structures_path: Path) -> tuple[pd.DataFrame, pd.DataFrame]:
    """Summary and structure rows for the dataset tables. ``summary.attrs
    ["stash"]`` keeps every crypto summary row, for the captions' stash maxima."""
    summary = pd.read_csv(summary_path)
    structures = pd.read_csv(structures_path)
    for frame in (summary, structures):
        frame["dataset"] = frame.log.str.split("_").str[0]
    structures["move"] = structures.cache.astype(bool)
    stash = summary[summary["mode"] == "crypto"].copy()
    # OSAM+ crypto logs from before the write-burst flushes (no OsamPlusFlush in
    # their build) ran a different eviction schedule; keep them out of the
    # stash maxima.
    flushed = set(structures.log[(structures.phase == "build")
                                  & (structures.structure == "OsamPlusFlush")])
    stash = stash[(stash.pointer != "multiwrite") | stash.log.isin(flushed)]
    # Likewise BOSAM and OSAM+ crypto runs from before two evictions per read.
    two_paths = stash.pointer.isin(["multiwrite", "multiwriterary"])
    stash = stash[~two_paths | (pd.to_numeric(stash.read_evictions, errors="coerce") == 2)]
    summary, structures = _prefer_dry_run(summary), _prefer_dry_run(structures)
    summary.attrs["stash"] = stash
    return summary, structures


STASH_COLUMNS = ("max_stash_all", "max_stash_peak", "installation_max_stash", "dynamic_max_stash")


def stash_caption(summary, series_list, bs: int, datasets=None) -> str:
    """'Maximum stash ...' over the crypto runs behind one table, or ''."""
    stash = summary.attrs.get("stash")
    if stash is None or stash.empty:
        return ""
    stash = stash[stash.bs == bs]
    if datasets is not None:
        stash = stash[stash.dataset.isin(datasets)]
    parts = []
    for series in series_list:
        rows = stash[series.select(stash)]
        values = [rows[column].max() for column in STASH_COLUMNS if column in rows.columns]
        values = [value for value in values if pd.notna(value)]
        if values:
            parts.append(f"{series.label} ${int(max(values))}$")
    if not parts:
        return ""
    return (f" Maximum client stash occupancy, in blocks, over installation and every run with "
            f"cryptography at $bs={bs}$: " + ", ".join(parts) + ".")


# The launcher's algorithm order (bench/launch_rust_tests.py): every run of an
# algorithm starts with whatever the previous runs left in the stash.
RUN_ORDER = ("rw", "cd", "pr", "dfs", "bfs", "dijkstra", "prim", "dtc")


def stash_by_algorithm_caption(summary, series: Series, others, bs: int) -> str:
    """'Maximum <series> stash per algorithm ...' at one block size, or ''.

    Per algorithm, the maximum stash over its full-length runs with
    cryptography (``max_stash_peak``) on every dataset that has them."""
    stash = summary.attrs.get("stash")
    if stash is None or stash.empty:
        return ""
    stash = stash[stash.bs == bs]
    rows = stash[series.select(stash)]
    if rows.empty:
        return ""
    peaks = pd.to_numeric(rows.max_stash_peak, errors="coerce")
    parts = []
    for alg in RUN_ORDER:
        value = peaks[rows.alg == alg].max()
        if pd.notna(value):
            parts.append(f"{CAPTION[alg]} ${int(value)}$")
    names = [DATASETS[d][0] for d in DATASETS if d in set(rows.dataset)]
    where = ("the seven graphs" if len(names) == len(DATASETS)
             else ", ".join(names[:-1]) + (" and " if len(names) > 1 else "") + names[-1])
    text = (f" Maximum {series.label} client stash, in blocks, over the full-length runs with "
            f"cryptography at $bs={bs}$ ({where}): " + ", ".join(parts))
    other_parts = []
    for other in others:
        o = stash[other.select(stash)]
        values = [pd.to_numeric(o[c], errors="coerce").max() for c in STASH_COLUMNS if c in o.columns]
        values = [v for v in values if pd.notna(v)]
        if values:
            other_parts.append(f"{other.label} at most ${int(max(values))}$")
    if other_parts:
        text += "; " + ", ".join(other_parts)
    return text + "."


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


def dataset_table(summary, structures, bs: int = 64, table_label: str = "tab:real results",
                  other: str = "") -> str:
    header = " & ".join(DATASET_HEADER.get(alg, CAPTION[alg]) for alg in DATASET_COLUMNS)
    width = 3 + len(DATASET_COLUMNS)
    lines = [
        HEADER.rstrip("\n"),
        "\\begin{table*}[tp]",
        "\\centering\\footnotesize",
        "\\renewcommand{\\arraystretch}{1.35}",
        fit_open(),
        "\\begin{tabular}{l | r | l | " + " ".join("r" for _ in DATASET_COLUMNS) + "}",
        "\\hline",
        f"Dataset & $bs$ & Impl. & {header}\\\\",
        "\\hline",
    ]
    short = set()
    shown = []
    for dataset, (name, directed) in DATASETS.items():
        rows = summary[summary.dataset == dataset]
        if rows.empty:
            continue
        n, edges = int(rows.n.iloc[0]), int(rows.edges.iloc[0])
        shown.append(dataset)
        kind = "Directed" if directed else "Undirected"
        stack = f"{name}\\\\{kind}\\\\$\\log n = {math.log2(n):.1f}$\\\\$d={edges / n:.1f}$"
        span = len(DATASET_SERIES)
        lines.append(f"\\multirow{{{span}}}{{*}}{{\\shortstack[r]{{{stack}}}}}")
        for block in (1,):
            lines.append(f"& \\multirow{{{len(DATASET_SERIES)}}}{{*}}{{${bs}$}}")
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
        "\\end{tabular}" + fit_close("\\textwidth"),
        "\\caption{Base-two logarithm of round trips on each real dataset (reads and public flushes "
        "for OSAM$^+$, whose writes stay in the stash and are evicted by reads; reads plus writes "
        "otherwise): the "
        "total for Build and the cost per step for the algorithms (a visited vertex for BFS, DFS, "
        "Dijkstra, and Prim; a move for RW and PR; a neighbor-list retrieval for CD; the whole run for "
        "DTC), averaged over $50$ full-length runs. In parentheses: the coefficient of variation "
        "(sample standard deviation divided by the mean) of that cost across runs. Entries marked -- had no full-length run in $1000$ draws of the entry point."
        + other + short_text + stash_caption(summary, DATASET_SERIES, bs, shown) + "}",
        f"\\label{{{table_label}}}",
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
    # bs = 64 in the body, bs = 4096 in the appendix.
    path = tables / "dataset_table.tex"
    path.write_text(dataset_table(
        summary, structures, 64, "tab:real results",
        f" Block size $bs=64$; $bs=4096$ is in Table~\\ref{{tab:real results 4096}}."))
    written.append(path)
    path = tables / "dataset_table_bs4096.tex"
    path.write_text(dataset_table(
        summary, structures, 4096, "tab:real results 4096",
        f" Block size $bs=4096$; $bs=64$ is in Table~\\ref{{tab:real results}}."))
    written.append(path)
    path = tables / "dataset_acceptance.tex"
    path.write_text(acceptance_table(summary))
    written.append(path)
    return written


# BlockOSAM real-dataset tables: datasets with average out-degree >= 10.
BOSAM_MIN_DEGREE = 10
BOSAM_MAIN_COLUMNS = ("build", "cd", "bfs", "rw")
BOSAM_FULL_COLUMNS = ("build", "cd", "bfs", "dfs", "dijkstra", "prim", "dtc", "pr", "rw")


def bosam_dataset_table(summary, structures, columns, full: bool) -> str:
    width = 3 + len(columns)
    header = " & ".join(f"\\textsf{{{CAPTION[alg] if alg != 'dijkstra' else 'Dijkstra'}}}" for alg in columns)
    if full:
        lines = [HEADER.rstrip("\n"), "\\begin{table*}[tp]", "\\centering"]
    else:
        lines = [HEADER.rstrip("\n"), "\\begin{table}[tp]", "\\centering"]
    short = set()
    body = []
    for dataset, (name, directed) in DATASETS.items():
        rows = summary[summary.dataset == dataset]
        if rows.empty:
            continue
        n, edges = int(rows.n.iloc[0]), int(rows.edges.iloc[0])
        if edges / n < BOSAM_MIN_DEGREE:
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
        short_text = (" $^{\\dagger}$Fewer than $50$ full-length runs ("
                      + "; ".join(notes) + ").")
    what = ("all eight algorithms" if full else "Build, CD, BFS and RW (all eight algorithms are in "
            "Table~\\ref{tab:real-graph-results-full})")
    caption = (
        f"\\caption{{Base-two logarithm of round trips on the SNAP graphs with $d\\ge {BOSAM_MIN_DEGREE}$, "
        f"for {what}: total for Build, per step for the algorithms "
        "(Section~\\ref{sec:eval-osam-workloads}). -- marks no full-length run in $1000$ draws of "
        "the entry point." + short_text + "}"
    )
    lines.append(caption)
    if not full:
        lines += ["\\ifnum\\conference=0", "\\def\\realgraphtablewidth{0.70\\columnwidth}", "\\else",
                  "\\def\\realgraphtablewidth{\\columnwidth}", "\\fi",
                  fit_open()]
    else:
        lines.append(fit_open())
    lines += [
        "\\begin{tabular}{l | r | l | " + " | ".join("r" for _ in columns) + "}",
        "\\toprule",
        f"\\textsf{{Dataset}} & $\\mathit{{bs}}$ & \\textsf{{Impl.}} & {header}\\\\",
        "\\midrule",
    ] + body + [
        "\\end{tabular}" + fit_close("\\realgraphtablewidth" if not full else "\\textwidth"),
        "\\label{tab:real-graph-results-full}" if full else "\\label{tab:real-graph-results}",
        "\\end{table*}" if full else "\\end{table}",
        "",
    ]
    return "\n".join(lines)


def bosam_stash_table(summary) -> str:
    """Maximum BOSAM stash per algorithm (full-length crypto runs), one row per block size."""
    stash = summary.attrs.get("stash")
    bosam, osam_plus = BOSAM_SERIES[0], BOSAM_SERIES[1]
    rows, where, other = [], {}, 0
    for bs, b in ((64, 6), (4096, 64)):
        part = stash[(stash.bs == bs) & bosam.select(stash)]
        peaks = pd.to_numeric(part.max_stash_peak, errors="coerce")
        cells = []
        for alg in RUN_ORDER:
            value = peaks[part.alg == alg].max()
            cells.append("--" if pd.isna(value) else f"{int(value)}")
        rows.append(f"${bs}$ & ${b}$ & " + " & ".join(cells) + " \\\\")
        where[bs] = [DATASETS[d][0] for d in DATASETS if d in set(part.dataset)]
        o = stash[(stash.bs == bs) & osam_plus.select(stash)]
        for column in STASH_COLUMNS:
            if column in o.columns:
                value = pd.to_numeric(o[column], errors="coerce").max()
                if pd.notna(value):
                    other = max(other, int(value))
    big = where[4096]
    big_text = ", ".join(big[:-1]) + (" and " if len(big) > 1 else "") + big[-1]
    return "\n".join([
        HEADER.rstrip("\n"),
        "\\begin{table}[tp]",
        "\\centering\\small",
        "\\caption{Maximum \\sysname client stash, in blocks, per algorithm over its full-length runs "
        f"with cryptography: all seven graphs at $bs=64$; {big_text} at $bs=4096$. Columns follow the "
        f"order the algorithms run on one installed graph. OSAM$^+$ never exceeds ${other}$.}}",
        "\\label{tab:dataset-stash}",
        "\\setlength{\\tabcolsep}{3.5pt}",
        "\\resizebox{\\columnwidth}{!}{%",
        "\\begin{tabular}{r r | " + " ".join("r" for _ in RUN_ORDER) + "}",
        "\\toprule",
        "$\\mathit{bs}$ & $b$ & " + " & ".join(
            "Dij." if a == "dijkstra" else CAPTION[a] for a in RUN_ORDER) + " \\\\",
        "\\midrule",
    ] + rows + ["\\bottomrule", "\\end{tabular}}", "\\end{table}", ""])


def write_bosam_datasets(root: Path, summary, structures) -> list[Path]:
    plots = root / "Plots"
    plots.mkdir(parents=True, exist_ok=True)
    written = []
    for name, columns, full in (("bosam_dataset_table.tex", BOSAM_MAIN_COLUMNS, False),
                                ("bosam_dataset_table_full.tex", BOSAM_FULL_COLUMNS, True)):
        path = plots / name
        path.write_text(bosam_dataset_table(summary, structures, columns, full))
        written.append(path)
    if summary.attrs.get("stash") is not None:
        path = plots / "bosam_dataset_stash.tex"
        path.write_text(bosam_stash_table(summary))
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
            written += write_bosam_datasets(args.bosam.expanduser(), summary, structures)
    print(f"wrote {len(written)} file(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
