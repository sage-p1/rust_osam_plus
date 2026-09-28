#!/usr/bin/env python3
"""Graph inputs for ``oblivious_graph_bench``: ER graphs and SNAP / CSV datasets.

``generate_graph`` writes the Erdős–Rényi graph of the Python benchmarks;
``dataset_graph`` + ``write_native_graph`` load a SNAP / CSV dataset edge list.

The parsing matches ``benchmark_graph_from_csv.py`` (comments, header rows,
relabeling of ids too wide for int64, ``nx.from_pandas_edgelist`` into a
``DiGraph`` or ``Graph``). Undirected graphs become one arc per direction.

Output: ``v ID`` for every node, then ``SRC DST W`` for every arc.

    graphs.py --file DATA.txt --directed --output DATA.edgelist
"""

from __future__ import annotations

import argparse
import os
import random
import sys
from pathlib import Path
from typing import Any

import networkx as nx


def generate_graph(n: int, d: int, seed: int, output: Path) -> None:
    """The ER graph of the Python benchmarks, as a native edge list.

    ``erdos_renyi_graph(n, d / n, seed)``, consecutive connected components
    joined by one random edge each, then every undirected edge written in
    both directions (as OGraph converts an undirected graph).
    """
    from networkx import connected_components, erdos_renyi_graph

    if n < 1 or d < 0 or d >= n:
        raise ValueError("require 1 <= n and 0 <= d < n")
    rng = random.Random(seed)
    graph = erdos_renyi_graph(n, d / n, seed=seed)
    components = list(connected_components(graph))
    for left, right in zip(components, components[1:]):
        graph.add_edge(rng.choice(tuple(left)), rng.choice(tuple(right)))
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("w", encoding="utf-8") as edge_file:
        for vertex in sorted(graph.nodes):
            edge_file.write(f"v {vertex}\n")
        for source, destination in graph.to_directed().edges:
            edge_file.write(f"{source} {destination} 0\n")


def load_edge_list(file: str | os.PathLike[str]) -> Any:
    """benchmark_graph_from_csv.py's parsing: an int64 edge frame."""
    import pandas as pd

    raw = pd.read_csv(
        file, sep=r"[,\s]+", header=None, comment="#", engine="python",
        skip_blank_lines=True, dtype=str,
    )
    csv = raw.iloc[:, :2].copy()
    csv.columns = ["start_node", "end_node"]
    csv = csv.dropna()
    is_pair = csv["start_node"].str.fullmatch(r"[+-]?\d+") & csv["end_node"].str.fullmatch(r"[+-]?\d+")
    csv = csv[is_pair]
    if csv.empty:
        raise ValueError(f"{file} contained no integer edge pairs")
    widest = max(csv["start_node"].str.len().max(), csv["end_node"].str.len().max())
    if widest > 18:
        labels = pd.unique(pd.concat([csv["start_node"], csv["end_node"]], ignore_index=True))
        mapping = pd.Series(range(len(labels)), index=labels, dtype="int64")
        csv = pd.DataFrame({
            "start_node": mapping.reindex(csv["start_node"]).to_numpy(),
            "end_node": mapping.reindex(csv["end_node"]).to_numpy(),
        })
        print(f"Relabeled {len(labels)} node identifiers too wide for int64", file=sys.stderr)
    else:
        csv = csv.astype("int64")
    return csv


def dataset_graph(file: str | os.PathLike[str], directed: bool) -> nx.Graph:
    csv = load_edge_list(file)
    graph = nx.from_pandas_edgelist(
        csv, source="start_node", target="end_node",
        create_using=nx.DiGraph() if directed else nx.Graph(),
    )
    if graph.number_of_nodes() < 2 or graph.number_of_edges() == 0:
        raise ValueError(f"{file} parsed to {graph.number_of_nodes()} nodes and {graph.number_of_edges()} edges")
    return graph


def write_native_graph(graph: nx.Graph, output: Path) -> None:
    """``v ID`` per node, ``SRC DST W`` per arc (written atomically)."""
    dg = graph if isinstance(graph, nx.DiGraph) else nx.DiGraph(graph)
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    partial = output.with_name(f"{output.name}.{os.getpid()}.partial")
    with partial.open("w", encoding="utf-8") as handle:
        for name in dg.nodes:
            if isinstance(name, bool) or not isinstance(name, int) or name < 0:
                raise ValueError(f"vertex ids must be non-negative ints, got {name!r}")
            handle.write(f"v {name}\n")
        for u, v, data in dg.edges(data=True):
            weight = data.get("weight", 0)
            if not isinstance(weight, int):
                raise ValueError(f"non-integer edge weight {weight!r}")
            handle.write(f"{u} {v} {weight}\n")
    os.replace(partial, output)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--file", required=True)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--directed", action="store_true")
    group.add_argument("--undirected", action="store_true")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    write_native_graph(dataset_graph(args.file, args.directed), args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
