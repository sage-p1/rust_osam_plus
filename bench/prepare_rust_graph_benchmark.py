"""Generate a NetworkX graph, then print or run one native Rust benchmark.

Python is intentionally outside the timed/trial loop. The generated edge list
is reusable, and every requested trial executes inside one Rust process.
"""

from __future__ import annotations

import argparse
import shlex
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from graphs import GENERATORS, generate_graph  # noqa: E402  (the shared ER generator)

RUST_CRATE = Path(__file__).resolve().parent.parent / "crates" / "sam-model"
GRAPH_DIR = Path(__file__).resolve().parent / "graphs"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--n", type=int, required=True)
    parser.add_argument("--d", type=int, required=True)
    parser.add_argument("--bs", type=int, required=True)
    parser.add_argument("--trials", type=int, default=50)
    parser.add_argument("--pt", default="multiwriterary")
    parser.add_argument(
        "--alg",
        choices=("build", "rw", "bfs", "dfs", "dijkstra", "prim", "cd", "dtc", "pr"),
        default="rw",
    )
    parser.add_argument("--wl", type=int, default=50)
    parser.add_argument("--max-steps", type=int, default=100)
    parser.add_argument("--max-neighbors", type=int, default=5)
    parser.add_argument("--df", type=float, default=0.9)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument(
        "--generator",
        choices=GENERATORS,
        default="fast",
        help="fast = fast_gnp_random_graph, O(n + m); exact = erdos_renyi_graph, O(n^2), "
        "the Python benchmarks' graphs",
    )
    parser.add_argument("--pointer-branching-factor", type=int)
    parser.add_argument("--cache", action="store_true")
    parser.add_argument("--run", action="store_true")
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    # Same names as launch_rust_tests.graph_path, so graphs are shared.
    suffix = "" if args.generator == "exact" else f"_gen-{args.generator}"
    output = args.output or GRAPH_DIR / (
        f"ER_n-{args.n}_d-{args.d}_seed-{args.seed}{suffix}.edgelist"
    )
    if not output.is_file():
        generate_graph(args.n, args.d, args.seed, output, args.generator)

    command = [
        "cargo",
        "run",
        "--manifest-path",
        str(RUST_CRATE / "Cargo.toml"),
    ]
    if args.release:
        command.append("--release")
    command += [
        "--bin",
        "oblivious_graph_bench",
        "--",
        "--graph",
        str(output.resolve()),
        "--bs",
        str(args.bs),
        "--pt",
        args.pt,
        "--alg",
        args.alg,
        "--trials",
        str(args.trials),
        "--wl",
        str(args.wl),
        "--max-steps",
        str(args.max_steps),
        "--max-neighbors",
        str(args.max_neighbors),
        "--df",
        str(args.df),
        "--seed",
        str(args.seed),
        "--dry-run",
    ]
    if args.pointer_branching_factor is not None:
        command += [
            "--pointer-branching-factor",
            str(args.pointer_branching_factor),
        ]
    command.append("--cache" if args.cache else "--move")

    print("Generated", output.resolve())
    print("Run:")
    print(shlex.join(command))
    if args.run:
        subprocess.run(command, check=True)


if __name__ == "__main__":
    main()
