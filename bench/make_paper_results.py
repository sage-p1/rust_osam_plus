#!/usr/bin/env python3
"""Generate every experimental result of the Concrete OSAM paper.

One command runs (or resumes) all experiments and regenerates every generated
figure and table the paper inputs::

    cd rust_osam_plus/bench
    python3 make_paper_results.py --paper ../../../paper

Stages, in order (``--stages`` selects a subset; ``--report-only`` runs only
``report``):

``er``        Erdos-Renyi matrix, dry run: bs 64/4096, d 20/100, n = 2^5..2^20
              (the launcher's workload cap stops d = 100 at 2^18), pointers
              OSAM (with and without moves), OSAM+, and recursive ORAM,
              50 full-length runs per algorithm.
``datasets``  The seven SNAP datasets, dry run, same pointers and runs.
``crypto``    OSAM+ with cryptography (encrypted Path OSAM+, AES-GCM),
              10 runs per algorithm: every dataset at bs = 64, and
              email-EU-core, p2p-Gnutella04 and roadNet-PA at bs = 4096 (the
              others need 0.5-1.1 TB of memory). Logs written before flushes
              read a dummy path are rerun automatically.
``skew``      Degree-skew graphs and runs (``skew_experiment.py``).
``report``    Rebuild the summary CSVs from the logs and write the paper's
              generated files:

              ==========================================  =====================
              ``plots/*.tex``, ``plots/legend.tex``       ``paper_figures.py``
              ``tables/dataset_table.tex``,               ``paper_figures.py``
              ``tables/dataset_table_bs4096.tex``,
              ``tables/dataset_acceptance.tex``
              ``tables/memory_size_timings.tex``          ``crypto_timings.py``
              ``tables/bandwidth_table.tex``              ``bandwidth_table.py``
              ``tables/skew_table.tex``                   ``skew_experiment.py``
              ==========================================  =====================

Every experiment stage resumes: completed logs are kept (``--skip-existing``),
so rerunning the script after an interruption, or after adding datasets, only
runs what is missing. Logs live under ``bench/results/``; the summaries are
also copied to the repository root (``summary.csv``, ``structures.csv``,
``dataset-summary.csv``, ``dataset-structures.csv``), where the earlier
per-script commands expected them.

Datasets are read from ``--dataset-root`` (default: the launcher's,
``osam/real-dataset-tests``). The crypto stage needs a machine with about
700 GB of memory for the largest trees (roadNet-PA at bs = 4096 is 276 GB).
``--print-only`` shows the commands without running anything. With
``--bosam``, the r-ary pointer (BOSAM) is also run and the BlockOSAM paper's
figures are written too (into ``--bosam <paper>`` if given).

On a machine without the papers (e.g. the server), leave out ``--paper``: the
experiment stages need no paper, and ``report`` writes the generated files
into ``--out`` (default ``bench/results/paper-out``), as ``concrete/`` (the
Concrete OSAM paper's ``plots/`` and ``tables/``) and ``bosam/Plots/``, ready
to copy into the two papers::

    python3 make_paper_results.py --bosam              # server: run + report
    rsync -a server:.../bench/results/paper-out/concrete/ ~/Research/osrm/paper/
    rsync -a server:.../bench/results/paper-out/bosam/ ~/Research/osrm/oblivious_hnsw/
"""

from __future__ import annotations

import argparse
import shlex
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent          # rust_osam_plus/bench
REPO = ROOT.parent                              # rust_osam_plus
DEFAULT_PAPER = (REPO / ".." / ".." / "paper").resolve()  # osrm/paper
DEFAULT_OUT = ROOT / "results" / "paper-out"     # report target without --paper
PYTHON = sys.executable

ER_LOGS = ROOT / "results" / "rust-logs"
DATASET_LOGS = ROOT / "results" / "rust-dataset-logs"

CONCRETE_POINTERS = ("original", "multiwrite", "recursive")
BOSAM_POINTER = "multiwriterary"
DATASETS = ("emailEucore", "p2pGnutella04", "higgsTwitter", "gpluscombined",
            "twitchgamers", "roadNetPA", "comyoutube")
# Datasets whose encrypted trees fit in memory at bs = 4096.
CRYPTO_4096_DATASETS = ("emailEucore", "p2pGnutella04", "roadNetPA")
CRYPTO_TRIALS = 10
STAGES = ("er", "datasets", "crypto", "skew", "report")

GENERATED = (
    "plots/legend.tex",
    "tables/dataset_table.tex",
    "tables/dataset_table_bs4096.tex",
    "tables/dataset_acceptance.tex",
    "tables/memory_size_timings.tex",
    "tables/bandwidth_table.tex",
    "tables/skew_table.tex",
)


class Runner:
    def __init__(self, print_only: bool) -> None:
        self.print_only = print_only

    def __call__(self, *command: object) -> None:
        argv = [str(part) for part in command]
        print("\n$ " + " ".join(shlex.quote(part) for part in argv), flush=True)
        if self.print_only:
            return
        start = time.monotonic()
        subprocess.run(argv, cwd=ROOT, check=True)
        print(f"  ({time.monotonic() - start:.0f} s)", flush=True)


def launcher(*arguments: object) -> list[object]:
    return [PYTHON, ROOT / "launch_rust_tests.py", *arguments]


def dataset_root_args(args: argparse.Namespace) -> list[object]:
    return ["--dataset-root", args.dataset_root] if args.dataset_root else []


def pointers(args: argparse.Namespace) -> list[str]:
    return list(CONCRETE_POINTERS) + ([BOSAM_POINTER] if args.bosam else [])


def stage_er(run: Runner, args: argparse.Namespace) -> None:
    run(*launcher("--skip-existing", "--pointers", *pointers(args)))


def stage_datasets(run: Runner, args: argparse.Namespace) -> None:
    run(*launcher("--skip-existing", "--datasets", *DATASETS, *dataset_root_args(args),
                  "--pointers", *pointers(args)))


def stage_crypto(run: Runner, args: argparse.Namespace) -> None:
    crypto_pointers = ["multiwrite"] + ([BOSAM_POINTER] if args.bosam else [])
    common = ["--skip-existing", "--mode", "crypto", "--trials", CRYPTO_TRIALS,
              "--pointers", *crypto_pointers, *dataset_root_args(args)]
    run(*launcher(*common, "--block-sizes", 64, "--datasets", *DATASETS))
    run(*launcher(*common, "--block-sizes", 4096, "--datasets", *CRYPTO_4096_DATASETS))


def stage_skew(run: Runner, args: argparse.Namespace) -> None:
    run(PYTHON, ROOT / "skew_experiment.py", "generate")
    run(PYTHON, ROOT / "skew_experiment.py", "run", "--jobs", args.skew_jobs)


def rows(path: Path) -> int:
    """Data rows of a CSV (0 if missing)."""
    if not path.is_file():
        return 0
    with path.open(errors="replace") as handle:
        return max(sum(1 for _ in handle) - 1, 0)


def publish(run: Runner, directory: Path, names: tuple[str, str]) -> None:
    """Copy a launcher summary pair to the repository root.

    A rebuilt summary with fewer rows than the published one (e.g. a machine
    with only a few stray logs) does not replace it; otherwise the published
    file is kept as ``<name>.prev`` before it is overwritten."""
    copies = ((directory / "summary.csv", REPO / names[0]),
              (directory / "structures.csv", REPO / names[1]))
    if not run.print_only and rows(copies[0][0]) < rows(copies[0][1]):
        print(f"\nkeep {names[0]} and {names[1]}: the summary rebuilt from {directory.relative_to(REPO)} "
              f"has {rows(copies[0][0])} rows, the published one {rows(copies[0][1])}")
        return
    for source, target in copies:
        print(f"\ncopy {source.relative_to(REPO)} -> {target.relative_to(REPO)}")
        if not run.print_only:
            if target.is_file():
                shutil.copyfile(target, target.with_name(target.name + ".prev"))
            shutil.copyfile(source, target)


def has_logs(directory: Path) -> bool:
    return any(directory.glob("*.log"))


def stage_report(run: Runner, args: argparse.Namespace) -> None:
    paper = args.paper
    # Rebuild a summary from its logs when the logs are here; a machine that
    # only has the published CSVs (e.g. a laptop without the server's logs)
    # keeps using them.
    for directory, extra, names in (
        (ER_LOGS, [], ("summary.csv", "structures.csv")),
        (DATASET_LOGS, ["--datasets", *DATASETS, *dataset_root_args(args)],
         ("dataset-summary.csv", "dataset-structures.csv")),
    ):
        if has_logs(directory) or run.print_only:
            run(*launcher("--report-only", *extra))
            publish(run, directory, names)
        else:
            print(f"\nno logs in {directory.relative_to(REPO)}: using the existing "
                  + " and ".join(names))
    summary, structures = REPO / "summary.csv", REPO / "structures.csv"
    dataset_summary, dataset_structures = REPO / "dataset-summary.csv", REPO / "dataset-structures.csv"
    figures = [PYTHON, ROOT / "paper_figures.py", "--summary", summary, "--structures", structures,
               "--dataset-summary", dataset_summary, "--dataset-structures", dataset_structures,
               "--concrete", paper]
    if args.bosam:
        figures += ["--bosam", args.bosam]
    run(*figures)
    run(PYTHON, ROOT / "crypto_timings.py", "--from-summary", dataset_summary,
        "--from-structures", dataset_structures, "--trials", CRYPTO_TRIALS,
        "--latex", paper / "tables" / "memory_size_timings.tex")
    run(PYTHON, ROOT / "bandwidth_table.py", "--dataset-summary", dataset_summary,
        "--dataset-structures", dataset_structures, "--concrete", paper)
    run(PYTHON, ROOT / "skew_experiment.py", "report", "--concrete", paper)
    if not run.print_only:
        if (paper / "main.tex").is_file():
            check(paper)
        else:
            print(f"\nWrote the Concrete OSAM files under {paper} and the BlockOSAM files under "
                  f"{args.bosam}; copy them into the papers (see --help).")


def check(paper: Path) -> None:
    """Report the generated files and any figure the paper inputs but lacks."""
    print("\nGenerated files in", paper)
    for name in GENERATED:
        path = paper / name
        state = time.strftime("%Y-%m-%d %H:%M", time.localtime(path.stat().st_mtime)) if path.is_file() else "MISSING"
        print(f"  {name:40s} {state}")
    missing = []
    for source in sorted(paper.glob("*.tex")):
        for line in source.read_text(errors="replace").splitlines():
            if line.lstrip().startswith("%") or "\\plotfile{" not in line:
                continue
            for chunk in line.split("\\plotfile{")[1:]:
                name = chunk.split("}", 1)[0].replace("\\primetag", "primefalse")
                if name != "<basename>" and "#" not in name and not (paper / "plots" / f"{name}.tex").is_file():
                    missing.append(f"{source.name}: plots/{name}.tex")
    if missing:
        print("\nFigures the paper inputs but no stage produced:")
        for entry in sorted(set(missing)):
            print("  " + entry)
    else:
        print("\nEvery \\plotfile the paper inputs exists.")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--paper", type=Path, default=None,
                        help="Concrete OSAM paper directory (has main.tex). Without it, report writes "
                             "into --out/concrete")
    parser.add_argument("--bosam", nargs="?", const="", default=None, metavar="PAPER",
                        help="Also run BOSAM and write the BlockOSAM paper's figures, into PAPER/Plots "
                             "if given, else --out/bosam/Plots")
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT,
                        help=f"Where report writes without --paper / a --bosam path (default {DEFAULT_OUT})")
    parser.add_argument("--dataset-root", type=Path, help="Directory with the SNAP edge lists")
    parser.add_argument("--stages", nargs="+", choices=STAGES, default=list(STAGES))
    parser.add_argument("--report-only", action="store_true", help="Only rebuild reports, figures and tables")
    parser.add_argument("--skew-jobs", type=int, default=2, help="Parallel skew jobs (default 2)")
    parser.add_argument("--print-only", action="store_true", help="Print the commands without running them")
    args = parser.parse_args()
    out = args.out.expanduser().resolve()
    if args.paper is not None:
        args.paper = args.paper.expanduser().resolve()
        if not (args.paper / "main.tex").is_file():
            print(f"{args.paper} does not look like the paper directory (no main.tex)", file=sys.stderr)
            return 1
    else:
        args.paper = out / "concrete"
    if args.bosam is not None:
        args.bosam = Path(args.bosam).expanduser().resolve() if args.bosam else out / "bosam"
    stages = ["report"] if args.report_only else [stage for stage in STAGES if stage in args.stages]
    run = Runner(args.print_only)
    actions = {"er": stage_er, "datasets": stage_datasets, "crypto": stage_crypto,
               "skew": stage_skew, "report": stage_report}
    for stage in stages:
        print(f"\n=== {stage} ===", flush=True)
        actions[stage](run, args)
    return 0


if __name__ == "__main__":
    sys.exit(main())
