# Rust OSAM benchmarks

Everything needed to run the Rust graph benchmarks and parse their output
lives here. Nothing is imported from the Python repository (`osam/`); only the
dataset edge lists are read from `osam/real-dataset-tests` (override with
`--dataset-root` or `DATASET_ROOT`).

| File | Purpose |
|---|---|
| `make_paper_results.py` | **One command for every result of the Concrete OSAM paper**: runs (or resumes) the ER matrix, the datasets, the crypto runs, and the degree-skew runs, then writes every generated figure and table into the paper. See below. |
| `launch_rust_tests.py` | Runs the experiment matrix (block sizes × n × degree × pointer × cache) with `oblivious_graph_bench`, then writes the reports. `--report-only` re-parses existing logs. |
| `scheduler.py` | Memory-aware parallel scheduler, experiment matrix and dataset list, vendored from `osam/launch_local_tests.py`. |
| `graphs.py` | ER graph generators and SNAP/CSV dataset export. |
| `prepare_rust_graph_benchmark.py` | Generates one ER graph and prints (or `--run`s) one benchmark command. |
| `launch_rust_tests_test.py` | Unit tests: `python3 -m unittest launch_rust_tests_test` (from this directory). |
| `skew_experiment.py` | Degree-skew experiment of the Concrete OSAM paper (`tab:skew`): `generate` writes an ER graph and three Chung–Lu power-law graphs (n = 2^14, average degree ≈ 20, γ = 3.0 / 2.5 / 2.1), `run` runs OSAM+, recursive ORAM and OSAM on them at bs = 64, `report` writes `results/skew-summary.csv` and, with `--concrete <paper>`, `tables/skew_table.tex`. |
| `bandwidth_table.py` | Memory, bytes per round trip, upload time and modeled WAN time of OSAM+ vs recursive ORAM on the datasets (`tab:bandwidth`), from the dataset reports. |

Outputs (created on first run):

- `graphs/`: generated graphs, reused across runs (`graphs/datasets/` for datasets)
- `results/rust-logs/`: one record log per ER job, plus `summary.csv` and `structures.csv`
- `results/rust-dataset-logs/`: the same for `--datasets`
- `graphs/skew/` and `results/skew-logs/`: the degree-skew graphs and logs (`skew_experiment.py`), with `results/skew-summary.csv`

Examples:

```bash
cd rust_osam_plus/bench
python3 launch_rust_tests.py --no-crypto --block-sizes 64 --degrees 20 --min-power 5 --max-power 8 --trials 10
python3 launch_rust_tests.py --no-crypto --no-static --prime --dynamic-ops 1000 --pointers multiwriterary
python3 launch_rust_tests.py --datasets emailEucore --no-crypto
python3 launch_rust_tests.py --report-only
```

## Reproducing the Concrete OSAM paper

```bash
cd rust_osam_plus/bench
python3 make_paper_results.py --paper ../../../paper            # everything
python3 make_paper_results.py --paper ../../../paper --report-only   # tables/figures from existing results
python3 make_paper_results.py --stages crypto report             # a subset
python3 make_paper_results.py --print-only                       # show the commands
```

Stages: `er` (ER matrix, dry run), `datasets` (SNAP datasets, dry run),
`crypto` (OSAM+ with cryptography, 10 runs: all datasets at bs = 64;
email-EU-core, p2p-Gnutella04 and roadNet-PA at bs = 4096), `skew`, and
`report`. Experiment stages resume (`--skip-existing`); crypto logs written
before OSAM+ flushes read a dummy path are not counted as complete and are
rerun. `report` rebuilds the summaries from the logs when the logs are present
(otherwise it uses the CSVs at the repository root), copies them to the
repository root, and writes `plots/`, `tables/dataset_table*.tex`,
`tables/dataset_acceptance.tex`, `tables/memory_size_timings.tex`,
`tables/bandwidth_table.tex` and `tables/skew_table.tex`. The crypto stage
needs about 700 GB of memory. `--bosam <paper>` also runs BOSAM and writes the
BlockOSAM paper's figures.

The individual commands, for reference. Concrete OSAM paper tables that do not
come from `paper_figures.py`:

```bash
python3 skew_experiment.py generate && python3 skew_experiment.py run --jobs 2
python3 skew_experiment.py report --concrete ../../../paper
python3 bandwidth_table.py --dataset-summary ../dataset-summary.csv \
    --dataset-structures ../dataset-structures.csv --concrete ../../../paper
```

The skew runs take about 30 minutes on two cores. The original OSAM keeps every
queue cell in the dry-run model, so on the γ = 2.1 graph it needs more than 8 GB
and is killed during BFS; `report` then uses the walks and CD from its
`.partial` log.

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
