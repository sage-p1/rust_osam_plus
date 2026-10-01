"""Representative wall-clock timings with cryptography on.

For each block size and pointer, one ``oblivious_graph_bench --crypto`` run:

1. builds the graph on the in-memory dry-run SAM (no cryptography),
2. turns cryptography on: installs the built memory into an encrypted
   Path OSAM+ tree (AES-256-GCM buckets, Z = 4), and
3. runs the 8 algorithms on the encrypted tree, timing every full-length run.

Reported per configuration:

* Build: dry-run build time, install (encryption) time, the size of the
  encrypted server tree, and the time to upload it at each network's bandwidth.
* Each algorithm, per step (a visited vertex, a walk move, a neighbor-list
  retrieval, or the whole run for DTC, as in the paper's tables):
  measured client computation, round trips, bytes transferred, and the
  end-to-end time at each network: compute + round trips * (RTT + bytes/BW).

Bandwidth model. A tree for ``capacity`` blocks (the next power of two above
the build's allocations) has ``capacity / 2`` leaves, so a path has
``H = log2(capacity)`` buckets. An encrypted bucket holds Z blocks of
``bs + 16`` bytes (identifier and leaf) plus a 16-byte GCM tag and 12-byte
nonce. Every round trip downloads and re-uploads the requested path and the
eviction paths of that access, as in the BOSAM paper's model:
``2 * (1 + e) * H`` buckets (an upper bound: the paths share their top
buckets), with ``e`` the run's ``read_evictions`` (2 for OSAM+, 1 otherwise).

Round trips are reads + writes; OSAM+ (``multiwrite``) and the r-ary pointer
(``multiwriterary``) are charged reads only: their writes stay in the stash
and reads evict.

Stash. Every row carries ``max_stash``, the most blocks the client stash held
(Build: while installing the tree; algorithms: over every attempt, rejected
ones included), and the LaTeX caption gives the maximum per configuration.

Example (from rust_osam_plus/bench):

    python3 crypto_timings.py --n 65536 --d 20 --bs 64 4096 --trials 10 \\
        --csv ../crypto-timings.csv --latex ~/Research/osrm/paper/tables/crypto_timings.tex

    python3 crypto_timings.py --datasets twitchgamers --bs 4096 --trials 5

    # OSAM+ on every dataset at both block sizes (no names = all datasets):
    python3 crypto_timings.py --datasets --bs 64 4096 --trials 10 --csv ../crypto-datasets.csv

The CSV is rewritten after every configuration, so a long run keeps its
partial results. Algorithms with no full-length run (for example DTC on
roadNet-PA) still spend up to 1000 encrypted attempts; drop them with
--algorithms if that is too slow.
"""

from __future__ import annotations

import argparse
import csv
import math
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import launch_rust_tests as lrt  # noqa: E402  (binary build, graph files)

ALGORITHMS = ("cd", "bfs", "dfs", "dijkstra", "prim", "dtc", "pr", "rw")
NAMES = {
    "cd": "CD", "bfs": "BFS", "dfs": "DFS", "dijkstra": "Dijkstra", "prim": "Prim",
    "dtc": "DTC", "pr": "PR", "rw": "RW",
}
Z = 4
BLOCK_METADATA = 16  # identifier + leaf, per block
BUCKET_OVERHEAD = 16 + 12  # AES-GCM tag + nonce, per bucket
# name -> (pointer flag, extra flags, label)
POINTERS = {
    "osam+": ("multiwrite", [], "OSAM$^+$"),
    "osam": ("original", ["--no-move"], "OSAM"),
    "osam-move": ("original", [], "OSAM w/ Move"),
    "bosam": ("multiwriterary", [], "BOSAM"),
}


@dataclass(frozen=True)
class Network:
    name: str
    rtt_ms: float
    mb_per_s: float  # megabytes (10^6 bytes) per second

    @classmethod
    def parse(cls, text: str) -> "Network":
        try:
            name, rtt, bw = text.split(":")
            return cls(name, float(rtt), float(bw))
        except ValueError:
            raise argparse.ArgumentTypeError(f"--network expects NAME:RTT_MS:MB_PER_S, not {text!r}")

    def seconds(self, roundtrips: float, nbytes: float) -> float:
        return roundtrips * self.rtt_ms / 1e3 + nbytes / (self.mb_per_s * 1e6)


def records(text: str) -> list[tuple[str, dict[str, str]]]:
    out = []
    for line in text.splitlines():
        parts = line.split()
        if not parts:
            continue
        fields = dict(part.split("=", 1) for part in parts[1:] if "=" in part)
        out.append((parts[0], fields))
    return out


def bucket_bytes(bs: int) -> int:
    return Z * (bs + BLOCK_METADATA) + BUCKET_OVERHEAD


def tree_geometry(allocations: int, bs: int, evictions: int = 1) -> tuple[int, int, int, int]:
    """(capacity, path buckets H, bytes per round trip, server tree bytes)."""
    capacity = 1 << max(1, math.ceil(math.log2(max(allocations, 2))))
    height = int(math.log2(capacity))
    per_roundtrip = 2 * (1 + evictions) * height * bucket_bytes(bs)
    tree = (capacity - 1) * bucket_bytes(bs)
    return capacity, height, per_roundtrip, tree


def run(command: list[str]) -> list[tuple[str, dict[str, str]]]:
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode != 0:
        message = (result.stderr.strip() or result.stdout.strip()).removeprefix("error: ")
        if result.returncode < 0:
            message = f"killed by signal {-result.returncode} (out of memory?) {message}".strip()
        raise RuntimeError(message or f"exit status {result.returncode}")
    return records(result.stdout)


def base_command(binary: Path, graph: str, bs: int, pointer: str, args: argparse.Namespace) -> list[str]:
    flag, extra, _ = POINTERS[pointer]
    command = [
        str(binary), "--graph", graph, "--bs", str(bs), "--pt", flag, *extra,
        "--seed", str(args.seed), "--wl", str(args.walk_length),
        "--max-steps", str(args.max_steps), "--max-neighbors", str(args.max_neighbors),
    ]
    if args.stash_size is not None:
        command += ["--stash-size", str(args.stash_size)]
    return command


def measure(binary: Path, graph: str, bs: int, pointer: str, args: argparse.Namespace,
            networks: list[Network]) -> list[dict]:
    base = base_command(binary, graph, bs, pointer, args)
    label = POINTERS[pointer][2]
    reads_only = POINTERS[pointer][0] in ("multiwrite", "multiwriterary")

    # A dry-run build first: it sizes the encrypted tree before committing memory.
    dry = dict(run(base + ["--dry-run", "--alg", "build"]))
    allocations = int(dry["build"]["allocations"])
    capacity, height, per_roundtrip, tree = tree_geometry(allocations, bs)
    if tree / 2**30 > args.max_tree_gb:
        raise RuntimeError(
            f"encrypted tree would be {tree / 2**30:.1f} GiB (> --max-tree-gb {args.max_tree_gb})")
    print(f"  bs={bs} {label}: {allocations} blocks, capacity 2^{height}, "
          f"tree {tree / 1e6:.0f} MB, {per_roundtrip / 1e3:.1f} KB per round trip", flush=True)

    command = base + ["--crypto", "--alg", ",".join(f"{alg}:{args.trials}" for alg in args.algorithms)]
    out = run(command)
    config = next(fields for kind, fields in out if kind == "config")
    build = next(fields for kind, fields in out if kind == "build")
    evictions = _int(build.get("read_evictions")) or 1
    capacity, height, per_roundtrip, tree = tree_geometry(allocations, bs, evictions)

    rows = []
    common = {
        "graph": Path(graph).stem, "vertices": config.get("vertices"), "edges": config.get("edges"),
        "bs": bs, "pointer": label, "capacity_log2": height,
        "bytes_per_roundtrip": per_roundtrip, "tree_bytes": tree, "read_evictions": evictions,
    }
    build_s = int(build["nanos"]) / 1e9
    install_s = int(build.get("install_nanos") or 0) / 1e9
    row = {**common, "algorithm": "Build", "status": "ok", "trials": 1, "length": 1,
           "compute_ms": (build_s + install_s) * 1e3, "dry_build_ms": build_s * 1e3,
           "install_ms": install_s * 1e3, "roundtrips": 0, "bytes": tree,
           "max_stash": _int(build.get("installation_maximum_stash"))}
    for net in networks:
        # The encrypted tree is uploaded once; building needs no round trips.
        row[f"{net.name}_ms"] = row["compute_ms"] + net.seconds(0, tree) * 1e3
    rows.append(row)

    for kind, fields in out:
        if kind != "algorithm":
            continue
        alg = fields["alg"]
        trials = int(fields["trials"])
        if fields["status"] == "failed" or trials == 0:
            rows.append({**common, "algorithm": NAMES.get(alg, alg), "status": "failed", "trials": 0})
            continue
        steps = trials * int(fields["length"])
        reads, writes = int(fields["reads"]), int(fields["writes"])
        roundtrips = (reads if reads_only else reads + writes) / steps
        compute_ms = int(fields["nanos"]) / 1e6 / steps
        row = {**common, "algorithm": NAMES.get(alg, alg), "status": fields["status"],
               "trials": trials, "length": int(fields["length"]), "compute_ms": compute_ms,
               "roundtrips": roundtrips, "bytes": roundtrips * per_roundtrip,
               "max_stash": _int(fields.get("all_maximum_stash") or fields.get("maximum_stash"))}
        for net in networks:
            row[f"{net.name}_ms"] = compute_ms + net.seconds(roundtrips, roundtrips * per_roundtrip) * 1e3
        rows.append(row)
    return rows


def _int(text: str | None) -> int | None:
    return None if text in (None, "none") else int(text)


def stash_caption(rows: list[dict]) -> str:
    """'Maximum stash: ...' over each (bs, pointer) configuration, or ''."""
    parts = []
    for key in dict.fromkeys((row["bs"], row["pointer"]) for row in rows):
        values = [row["max_stash"] for row in rows
                  if (row["bs"], row["pointer"]) == key and row.get("max_stash") is not None]
        if values:
            parts.append(f"{key[1]}, $\\mathit{{bs}}={key[0]}$: ${max(values)}$")
    if not parts:
        return ""
    return (" Maximum stash occupancy (blocks) over installation and every run of each configuration: "
            + "; ".join(parts) + ".")


def fmt(value: float) -> str:
    if value >= 100:
        return f"{value:,.0f}"
    if value >= 10:
        return f"{value:.1f}"
    return f"{value:.2f}"


def latex_table(rows: list[dict], networks: list[Network], label: str = "tab:crypto_timings") -> str:
    """Rows: Build and the 8 algorithms; columns per (bs, pointer)."""
    keys = []
    for row in rows:
        key = (row["bs"], row["pointer"])
        if key not in keys:
            keys.append(key)
    net = networks[0]
    per_col = ["Comp", "KB", net.name]
    header1 = " & ".join(
        f"\\multicolumn{{3}}{{c{'|' if i < len(keys) - 1 else ''}}}{{{p}, $\\mathit{{bs}}={bs}$}}"
        for i, (bs, p) in enumerate(keys))
    header2 = " & ".join(" & ".join(per_col) for _ in keys)
    lines = [
        "% Generated by rust_osam_plus/bench/crypto_timings.py -- do not edit by hand.",
        "\\begin{table}[t]", "\\centering", "\\small",
        "\\begin{tabular}{l|" + "|".join("rrr" for _ in keys) + "}", "\\hline",
        f" & {header1} \\\\", f"Algorithm & {header2} \\\\", "\\hline",
    ]
    for name in ["Build"] + [NAMES[a] for a in ALGORITHMS]:
        cells = []
        for key in keys:
            row = next((r for r in rows if (r["bs"], r["pointer"]) == key and r["algorithm"] == name), None)
            if row is None or row["status"] == "failed":
                cells += ["--"] * 3
            elif name == "Build":
                cells += [fmt(row["compute_ms"] / 1e3), fmt(row["bytes"] / 1e6), fmt(row[f"{net.name}_ms"] / 1e3)]
            else:
                cells += [fmt(row["compute_ms"]), fmt(row["bytes"] / 1e3), fmt(row[f"{net.name}_ms"])]
        lines.append(f"{name} & " + " & ".join(cells) + " \\\\")
        if name == "Build":
            lines.append("\\hline")
    graph = rows[0]
    lines += [
        "\\hline", "\\end{tabular}",
        f"\\caption{{Wall-clock cost with cryptography on, on {graph['graph'].replace('_', '-')} "
        f"($|V|={graph['vertices']}$, {graph['edges']} edges). "
        "The graph is built on the in-memory SAM and then installed into an encrypted Path OSAM$^+$ tree. "
        "Build: total build and installation time (Comp, s), size of the encrypted tree (MB), and the "
        f"time to upload it on the {net.name} network (s). Algorithms, per step: client computation (Comp, ms), "
        f"data transferred (KB), and end-to-end time on the {net.name} network (ms), with "
        f"$\\mathrm{{RTT}}={net.rtt_ms:g}$\\,ms and $\\mathrm{{BW}}={net.mb_per_s:g}$\\,MB/s."
        + stash_caption(rows) + "}",
        f"\\label{{{label}}}", "\\end{table}", "",
    ]
    return "\n".join(lines)


def print_rows(rows: list[dict], networks: list[Network]) -> None:
    nets = "".join(f"{n.name + ' ms':>12}" for n in networks)
    print(f"\n{'graph':<16}{'bs':>5} {'pointer':<14}{'alg':<10}{'comp ms':>10}{'RT':>9}{'KB':>12}{nets}{'stash':>7}")
    for row in rows:
        if row["status"] == "failed":
            print(f"{row['graph'][:15]:<16}{row['bs']:>5} {row['pointer']:<14}{row['algorithm']:<10}  failed (no full-length run)")
            continue
        times = "".join(f"{row[f'{n.name}_ms']:>12.1f}" for n in networks)
        print(f"{row['graph'][:15]:<16}{row['bs']:>5} {row['pointer']:<14}{row['algorithm']:<10}{row['compute_ms']:>10.2f}"
              f"{row['roundtrips']:>9.1f}{row['bytes'] / 1e3:>12.1f}{times}"
              f"{'-' if row.get('max_stash') is None else row['max_stash']:>7}")
    print("(Build row: comp = dry-run build + install; KB = encrypted tree; network time = its upload;"
          " stash = most blocks in the client stash, installing for Build, over every attempt otherwise.)")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    graph = parser.add_mutually_exclusive_group(required=True)
    graph.add_argument("--graph", type=Path, help="native edge list (as the launcher writes)")
    graph.add_argument("--n", type=int, help="Erdos-Renyi graph with n vertices (with --d)")
    graph.add_argument("--datasets", nargs="*",
                       help="dataset names, as for launch_rust_tests.py --datasets (none: all)")
    parser.add_argument("--d", type=int, default=20)
    parser.add_argument("--dataset-root", type=Path)
    parser.add_argument("--bs", type=int, nargs="+", default=[64, 4096], choices=(64, 4096))
    parser.add_argument("--pointers", nargs="+", default=["osam+"], choices=sorted(POINTERS))
    parser.add_argument("--algorithms", nargs="+", default=list(ALGORITHMS), choices=ALGORITHMS)
    parser.add_argument("--trials", type=int, default=10, help="full-length runs per algorithm")
    parser.add_argument("--walk-length", type=int, default=50)
    parser.add_argument("--max-steps", type=int, default=100)
    parser.add_argument("--max-neighbors", type=int, default=5)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--stash-size", type=int)
    parser.add_argument("--network", type=Network.parse, action="append",
                        help="NAME:RTT_MS:MB_PER_S, repeatable (default WAN:100:100 and LAN:1:1000); "
                        "the first one goes in the LaTeX table")
    parser.add_argument("--max-tree-gb", type=float, default=32.0,
                        help="skip configurations whose encrypted tree would exceed this (GiB)")
    parser.add_argument("--binary", type=Path, help="prebuilt oblivious_graph_bench (default: cargo build)")
    parser.add_argument("--csv", type=Path)
    parser.add_argument("--latex", type=Path)
    args = parser.parse_args(argv)
    networks = args.network or [Network("WAN", 100, 100), Network("LAN", 1, 1000)]

    if args.graph:
        graph_files = [str(args.graph.expanduser())]
        if not Path(graph_files[0]).is_file():
            parser.error(f"--graph {graph_files[0]} does not exist")
    elif args.n:
        graph_files = [lrt.ensure_graph_file(args.n, args.d, args.seed)]
    else:
        datasets = lrt.select_datasets(args.datasets, args.dataset_root)
        missing = [str(dataset.path) for dataset in datasets if not dataset.path.is_file()]
        if missing:
            parser.error("dataset files do not exist: " + ", ".join(missing))
        graph_files = [lrt.GraphSpec("dataset", dataset=dataset).ensure() for dataset in datasets]
    binary = args.binary.expanduser() if args.binary else lrt.build_binary()

    rows = []
    for graph_file in graph_files:
        print(f"graph {graph_file}", flush=True)
        for bs in args.bs:
            for pointer in args.pointers:
                try:
                    rows += measure(binary, graph_file, bs, pointer, args, networks)
                except RuntimeError as error:
                    print(f"  bs={bs} {POINTERS[pointer][2]}: skipped: {error}", flush=True)
                    continue
                if args.csv:
                    write_csv(args.csv.expanduser(), rows)
    if not rows:
        print("no configuration ran")
        return 1
    print_rows(rows, networks)
    if args.csv:
        print(f"wrote {args.csv}")
    if args.latex:
        path = args.latex.expanduser()
        path.parent.mkdir(parents=True, exist_ok=True)
        graphs = list(dict.fromkeys(row["graph"] for row in rows))
        tables = [latex_table([row for row in rows if row["graph"] == graph], networks,
                              label="tab:crypto_timings" + ("" if len(graphs) == 1 else f"_{graph}"))
                  for graph in graphs]
        path.write_text("\n".join(tables))
        print(f"wrote {path}")
    return 0


def write_csv(path: Path, rows: list[dict]) -> None:
    fields = list(dict.fromkeys(key for row in rows for key in row))
    partial = path.with_name(path.name + ".partial")
    with partial.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)
    partial.replace(path)


if __name__ == "__main__":
    sys.exit(main())
