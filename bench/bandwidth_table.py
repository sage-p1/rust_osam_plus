#!/usr/bin/env python3
"""Memory, bandwidth, and modeled end-to-end time of OSAM+ vs recursive ORAM
on the real datasets (Concrete OSAM paper, Table ``tab:bandwidth``).

Reads the dry-run rows of the launcher's dataset reports and writes
``<paper>/tables/bandwidth_table.tex``::

    python3 bandwidth_table.py --dataset-summary ../dataset-summary.csv \\
        --dataset-structures ../dataset-structures.csv --concrete <paper> [--csv out.csv]

Model (Z = 4, AES-GCM buckets as in ``crypto_timings.py``):

* Cells: memory cells allocated by Build.
* OSAM+ bytes per round trip: 2 (down and up) x 3 paths (requested or dummy
  path plus two eviction paths) x H buckets, with H = log2 of the next power
  of two above Build's cells.
* ORAM bytes per round trip: one path, read and written back, per level of
  recursion (data tree, then position-map trees that are bs/8 times smaller),
  averaged over the ``oram_recursion_levels`` levels.
* Upload: the encrypted trees sized to Build's cells (for ORAM the data tree
  and the position-map trees not held by the client) at the network bandwidth.
* Time per step: round trips x (RTT + bytes per round trip / bandwidth);
  client computation (< 1.5 ms per round trip) is not included.
"""

from __future__ import annotations

import argparse
import csv
import math
from pathlib import Path

import pandas as pd

from crypto_timings import bucket_bytes, tree_geometry
from launch_rust_tests import RECURSIVE_STRUCTURES

DATASETS = {
    "emailEucore": "email-EU-core", "p2pGnutella04": "p2p-Gnutella04", "higgsTwitter": "Higgs-Twitter",
    "gpluscombined": "ego-Gplus", "twitchgamers": "twitch-gamers", "roadNetPA": "roadNet-PA",
    "comyoutube": "com-Youtube",
}
ALGORITHMS = (("bfs", "BFS"), ("cd", "CD"), ("dtc", "DTC"), ("pr", "PR"), ("rw", "RW"))
NETWORKS = {"WAN": (0.1, 100e6), "LAN": (0.001, 1e9)}
OSAM_PLUS_PATHS = 3  # requested (or dummy) path + two eviction paths


def tree_bytes(cells: int, bs: int) -> int:
    return tree_geometry(cells, bs)[3]


def height(cells: int) -> int:
    return tree_geometry(cells, bs=64)[1]


def configurations(summary_path: Path, structures_path: Path) -> list[dict]:
    summary = pd.read_csv(summary_path)
    summary = summary[summary["mode"] == "dry-run"]
    structures = pd.read_csv(structures_path)
    structures = structures[structures["mode"] == "dry-run"]
    out = []
    for log, group in structures.groupby("log"):
        rows = summary[summary.log == log]
        if rows.empty:
            continue
        pointer, bs = rows.pointer.iloc[0], int(rows.bs.iloc[0])
        if pointer not in ("multiwrite", "recursive"):
            continue
        dataset = log.split("_bs")[0]
        build = group[group.phase == "build"]
        cells = int(build.allocations.sum())
        config = {"dataset": dataset, "bs": bs, "pointer": pointer, "cells": cells}
        if pointer == "multiwrite":
            config["bytes_per_roundtrip"] = 2 * OSAM_PLUS_PATHS * height(cells) * bucket_bytes(bs)
            config["upload_bytes"] = tree_bytes(cells, bs)
            steps = {row.alg: row.mean_reads_per_step for row in rows.itertuples()}
        else:
            levels = int(rows.oram_levels.dropna().iloc[0])
            recursive_cells = int(group[group.structure.isin(RECURSIVE_STRUCTURES)].allocations.sum())
            fanout = bs / 8
            level_cells = [max(int(recursive_cells / fanout**i), 2) for i in range(levels)]
            config["bytes_per_roundtrip"] = sum(2 * height(c) * bucket_bytes(bs) for c in level_cells) / levels
            config["upload_bytes"] = sum(tree_bytes(max(int(cells / fanout**i), 2), bs) for i in range(levels))
            steps = {row.alg: row.oram_roundtrips_per_step for row in rows.itertuples()}
        status = {row.alg: row.status for row in rows.itertuples()}
        config["steps"] = {alg: value for alg, value in steps.items()
                           if status.get(alg) != "failed" and pd.notna(value)}
        out.append(config)
    return out


def ratios(configs: list[dict], network: str) -> list[dict]:
    rtt, bandwidth = NETWORKS[network]
    index = {(c["dataset"], c["bs"], c["pointer"]): c for c in configs}
    rows = []
    for bs in (64, 4096):
        for dataset in DATASETS:
            plus, oram = index.get((dataset, bs, "multiwrite")), index.get((dataset, bs, "recursive"))
            if plus is None or oram is None:
                continue
            row = {"dataset": dataset, "bs": bs, "network": network,
                   "cells_osamplus": plus["cells"], "cells_oram": oram["cells"],
                   "kb_per_rt_osamplus": plus["bytes_per_roundtrip"] / 1000,
                   "kb_per_rt_oram": oram["bytes_per_roundtrip"] / 1000,
                   "upload_s_osamplus": plus["upload_bytes"] / bandwidth,
                   "upload_s_oram": oram["upload_bytes"] / bandwidth}
            for alg, _ in ALGORITHMS:
                if alg in plus["steps"] and alg in oram["steps"]:
                    t_plus = plus["steps"][alg] * (rtt + plus["bytes_per_roundtrip"] / bandwidth)
                    t_oram = oram["steps"][alg] * (rtt + oram["bytes_per_roundtrip"] / bandwidth)
                    row[f"time_ratio_{alg}"] = t_oram / t_plus
                    row[f"bytes_ratio_{alg}"] = (plus["steps"][alg] * plus["bytes_per_roundtrip"]) / (
                        oram["steps"][alg] * oram["bytes_per_roundtrip"])
            rows.append(row)
    return rows


def number(value: float) -> str:
    return f"{value:.0f}" if value >= 10 else f"{value:.1f}"


def cells(value: int) -> str:
    return f"{value / 1e6:.2f}" if value < 1e7 else f"{value / 1e6:.0f}"


def latex_table(rows: list[dict]) -> str:
    lines = [r"\begin{table*}[t]", r"\centering\footnotesize", r"\setlength{\tabcolsep}{4pt}",
             r"\begin{tabular}{l r | r r | r r | r r | " + " ".join("r" * len(ALGORITHMS)) + "}", r"\hline",
             r" & & \multicolumn{2}{c|}{Cells ($10^6$)} & \multicolumn{2}{c|}{KB per round trip} & "
             r"\multicolumn{2}{c|}{Upload (s)} & \multicolumn{" + str(len(ALGORITHMS))
             + r"}{c}{WAN time, ORAM $\div$ OSAM$^+$} \\",
             r"Dataset & $\mathit{bs}$ & OSAM$^+$ & ORAM & OSAM$^+$ & ORAM & OSAM$^+$ & ORAM & "
             + " & ".join(label for _, label in ALGORITHMS) + r"\\", r"\hline"]
    for bs in (64, 4096):
        for row in (r for r in rows if r["bs"] == bs):
            cells_ = [DATASETS[row["dataset"]], str(bs), cells(row["cells_osamplus"]), cells(row["cells_oram"]),
                      f"{row['kb_per_rt_osamplus']:.0f}", f"{row['kb_per_rt_oram']:.0f}",
                      number(row["upload_s_osamplus"]), number(row["upload_s_oram"])]
            for alg, _ in ALGORITHMS:
                value = row.get(f"time_ratio_{alg}")
                if value is None or math.isnan(value):
                    cells_.append("--")
                else:
                    cells_.append(rf"$\mathbf{{{value:.2f}}}$" if value > 1 else f"${value:.2f}$")
            lines.append(" & ".join(cells_) + r"\\")
        lines.append(r"\hline")
    lines += [r"\end{tabular}",
              r"\caption{Memory, bandwidth, and modeled end-to-end time of OSAM$^+$ and recursive Path ORAM on "
              r"the real datasets. Cells: memory cells allocated by Build. KB per round trip: data moved per "
              r"round trip, down and up ($Z=4$, encrypted buckets): for OSAM$^+$ the requested (or dummy) path "
              r"and two eviction paths in a tree sized to its cells; for ORAM one path, read and written back, "
              r"per level of recursion, averaged over the levels. Upload: time to upload the encrypted trees "
              r"built on the client (for ORAM, the data tree and the position-map trees not held by the client) "
              r"at $100$\,MB/s. WAN time: ratio of ORAM's to OSAM$^+$'s time per step (per run for DTC) with a "
              r"$100$\,ms round-trip time and $100$\,MB/s; bold entries favor OSAM$^+$. Client computation, "
              r"below $1.5$\,ms per round trip (Table~\ref{tab:memory_size_timings}), is not included. -- marks "
              r"algorithms without full-length runs.}",
              r"\label{tab:bandwidth}", r"\end{table*}"]
    return ("% Generated by rust_osam_plus/bench/bandwidth_table.py -- do not edit by hand.\n"
            + "\n".join(lines) + "\n")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--dataset-summary", type=Path, required=True)
    parser.add_argument("--dataset-structures", type=Path, required=True)
    parser.add_argument("--concrete", type=Path, help="Concrete OSAM paper directory")
    parser.add_argument("--csv", type=Path, help="Also write every ratio (WAN and LAN) to this CSV")
    args = parser.parse_args()
    configs = configurations(args.dataset_summary, args.dataset_structures)
    wan = ratios(configs, "WAN")
    if args.csv:
        rows = wan + ratios(configs, "LAN")
        fields = sorted({key for row in rows for key in row}, key=lambda k: (k not in rows[0], k))
        with args.csv.open("w", newline="") as handle:
            writer = csv.DictWriter(handle, fieldnames=fields)
            writer.writeheader()
            writer.writerows(rows)
        print(f"wrote {args.csv}")
    if args.concrete:
        output = args.concrete / "tables" / "bandwidth_table.tex"
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(latex_table(wan))
        print(f"wrote {output}")


if __name__ == "__main__":
    main()
