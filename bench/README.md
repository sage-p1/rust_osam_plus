# Rust OSAM benchmarks

Everything needed to run the Rust graph benchmarks and parse their output
lives here. Nothing is imported from the Python repository (`osam/`); only the
dataset edge lists are read from `osam/real-dataset-tests` (override with
`--dataset-root` or `DATASET_ROOT`).

| File | Purpose |
|---|---|
| `launch_rust_tests.py` | Runs the experiment matrix (block sizes × n × degree × pointer × cache) with `oblivious_graph_bench`, then writes the reports. `--report-only` re-parses existing logs. |
| `scheduler.py` | Memory-aware parallel scheduler, experiment matrix and dataset list, vendored from `osam/launch_local_tests.py`. |
| `graphs.py` | ER graph generators and SNAP/CSV dataset export. |
| `prepare_rust_graph_benchmark.py` | Generates one ER graph and prints (or `--run`s) one benchmark command. |
| `launch_rust_tests_test.py` | Unit tests: `python3 -m unittest launch_rust_tests_test` (from this directory). |

Outputs (created on first run):

- `graphs/`: generated graphs, reused across runs (`graphs/datasets/` for datasets)
- `results/rust-logs/`: one record log per ER job, plus `summary.csv`, `steps_by_index.csv` and `structures.csv`
- `results/rust-dataset-logs/`: the same for `--datasets`

Examples:

```bash
cd rust_osam_plus/bench
python3 launch_rust_tests.py --no-crypto --block-sizes 64 --degrees 20 --min-power 5 --max-power 8 --trials 10
python3 launch_rust_tests.py --no-crypto --no-static --prime --dynamic-ops 1000 --pointers multiwriterary
python3 launch_rust_tests.py --datasets emailEucore --no-crypto
python3 launch_rust_tests.py --report-only
```

ER graphs: `--generator fast` (default) uses networkx's O(n + m)
`fast_gnp_random_graph` (about a minute at n = 2^20, d = 20);
`--generator exact` uses `erdos_renyi_graph`, the Python benchmarks' O(n^2)
generator (about 12 hours at n = 2^20), which gives the same graphs as the
Python runs. Both sample G(n, d/n). Graph files and exact-generator log names
say which generator made them.

Setup (Python >= 3.11, which networkx 3.6 and pandas 3.0 require):

```bash
pip install -r requirements.txt
```

`networkx` generates the ER graphs and is pinned to the Python benchmarks'
version, so the same seed gives the same graph. `pandas` parses the datasets.
