import tempfile
import unittest
from pathlib import Path

import launch_rust_tests as rust
import scheduler


def log_lines(pointer: str = "multiwriterary", bs: int = 64) -> list[str]:
    return [
        f"config mode=dry-run pointer={pointer} cache=true block_size={bs} "
        "pointer_branching_factor=6 graph_branching_factor=2 vertices=10 edges=20 layout=exact",
        "build allocations=1 reads=1 writes=1 nanos=5 install_nanos=none installation_maximum_stash=none",
        f"structure phase=build name=RecursivePointer allocations=5000 reads=0 writes=5000 roundtrips=5000",
        "trial alg=bfs index=0 start=3 attempts=1 allocations=1 reads=10 writes=10 roundtrips=20 "
        "nanos=1000000 size=4 cost=none stash_peak=none",
        "trial alg=bfs index=1 start=7 attempts=2 allocations=1 reads=12 writes=12 roundtrips=24 "
        "nanos=3000000 size=4 cost=none stash_peak=none",
        "algorithm alg=bfs status=ok trials=2 requested=2 attempts=3 length=4 allocations=2 reads=22 writes=22 nanos=4000000 "
        "mean_allocations=1.0000 var_allocations=0.0000 mean_reads=11.0000 var_reads=2.0000 "
        "mean_writes=11.0000 var_writes=2.0000 mean_roundtrips=22.0000 var_roundtrips=8.0000 "
        "rejected_allocations=1 rejected_roundtrips=6 maximum_stash=none maximum_cached_values=none "
        "cache_cleared=0",
        "structure phase=bfs name=RecursivePointer allocations=0 reads=22 writes=22 roundtrips=44",
        "done",
    ]


class RustLauncherTest(unittest.TestCase):
    def test_algorithm_spec_runs_every_algorithm_the_same_number_of_trials(self) -> None:
        spec = dict(entry.split(":") for entry in rust.algorithm_spec().split(","))
        self.assertEqual(list(spec), list(rust.ALGORITHMS))
        self.assertEqual(set(spec.values()), {str(rust.TRIALS)})

    def test_default_pointer_branching_factor_matches_binary(self) -> None:
        self.assertEqual(rust.default_pointer_branching_factor(4096), 64)
        self.assertEqual(rust.default_pointer_branching_factor(64), 6)

    def test_oram_recursion_levels_match_the_python_parser(self) -> None:
        # ceil(max(log_{bs/8}(allocs) - L, 1)), L = 1 at 4096 and 4 otherwise.
        self.assertEqual(rust.oram_recursion_levels(512**3, 4096), 2)
        self.assertEqual(rust.oram_recursion_levels(10, 4096), 1)
        self.assertEqual(rust.oram_recursion_levels(8**10, 64), 6)
        self.assertIsNone(rust.oram_recursion_levels(0, 64))

    def test_report_is_per_full_length_run_and_amortized_per_step(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "job.log"
            log.write_text("\n".join(log_lines()) + "\n")
            rows = rust.report_rows(rust.parse_log(log), log)
        (row,) = rows
        self.assertEqual((row["alg"], row["trials"], row["attempts"], row["length"]), ("bfs", 2, 3, 4))
        self.assertEqual(row["mean_roundtrips_per_trial"], 22.0)
        self.assertAlmostEqual(row["sd_roundtrips_per_trial"], 8.0 ** 0.5)
        self.assertEqual(row["mean_roundtrips_per_step"], 5.5)
        self.assertAlmostEqual(row["sd_roundtrips_per_step"], 8.0 ** 0.5 / 4)
        self.assertEqual(row["rejected_roundtrips"], 6)
        self.assertEqual(row["mean_ms_per_trial"], 2.0)
        self.assertIsNone(row["oram_levels"])  # not a recursive job

    def test_failed_algorithms_get_a_row_without_costs(self) -> None:
        lines = log_lines()[:3] + [
            "algorithm alg=bfs status=failed trials=0 requested=2 attempts=1000 length=4 "
            "allocations=0 reads=0 writes=0 nanos=0 mean_allocations=none var_allocations=none "
            "mean_reads=none var_reads=none mean_writes=none var_writes=none "
            "mean_roundtrips=none var_roundtrips=none rejected_allocations=5 rejected_roundtrips=90 "
            "maximum_stash=none maximum_cached_values=none cache_cleared=0",
            "done",
        ]
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "job.log"
            log.write_text("\n".join(lines) + "\n")
            (row,) = rust.report_rows(rust.parse_log(log), log)
            rows = rust.write_report(Path(directory), Path(directory) / "summary.csv")
        self.assertEqual((row["status"], row["trials"], row["requested_trials"]), ("failed", 0, 2))
        self.assertIsNone(row["mean_roundtrips_per_step"])
        self.assertIsNone(row["oram_roundtrips_per_step"])
        rust.print_report(rows)

    def test_old_logs_are_rerun_by_skip_existing_and_left_out_of_the_report(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "old.log").write_text(
                "algorithm alg=bfs trials=2 full_trials=2 allocations=1 reads=1 writes=1\n"
                "steps alg=bfs count=7 mean_roundtrips=4.0000\ndone\n"
            )
            (root / "new.log").write_text("\n".join(log_lines()) + "\n")
            self.assertFalse(rust.is_current_log(root / "old.log"))
            self.assertTrue(rust.is_current_log(root / "new.log"))
            rows = rust.write_report(root, root / "summary.csv")
        self.assertEqual({row["log"] for row in rows}, {"new.log"})

    def test_old_per_step_logs_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "old.log"
            log.write_text("steps alg=bfs count=7 mean_roundtrips=4.0000\n")
            with self.assertRaisesRegex(ValueError, "rerun"):
                rust.parse_log(log)

    def test_recursive_jobs_report_the_oram_baseline(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "job.log"
            log.write_text("\n".join(log_lines(pointer="recursive", bs=64)) + "\n")
            (row,) = rust.report_rows(rust.parse_log(log), log)
            structures = rust.structure_rows(rust.parse_log(log), log)
        # 5000 recursive allocations at bs=64: log_8(5000) - 4 < 1, so 1 level.
        self.assertEqual(row["oram_levels"], 1)
        self.assertEqual(row["oram_roundtrips_per_trial"], 22.0)
        self.assertEqual(row["oram_roundtrips_per_step"], 5.5)
        bfs = [record for record in structures if record["phase"] == "bfs"]
        self.assertEqual(bfs[0]["roundtrips_per_trial"], 22.0)

    def test_find_logs_prefers_main_run_over_retry(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "retries" / "retry-1").mkdir(parents=True)
            (root / "retries" / "retry-1" / "a.log").write_text("done\n")
            (root / "retries" / "retry-1" / "b.log").write_text("done\n")
            (root / "a.log").write_text("done\n")
            self.assertEqual(
                rust.find_logs(root),
                [root / "a.log", root / "retries" / "retry-1" / "b.log"],
            )


class Fit:
    """A probe that accepts every configuration."""

    def check(self, bs, pointer, cache, mode):
        bf = rust.default_pointer_branching_factor(bs) if pointer == "multiwriterary" else None
        return rust.Feasibility(True, bf)


class GraphOptionsTest(unittest.TestCase):
    def test_default_options_keep_existing_job_names_and_commands(self) -> None:
        job = rust.RustJob(64, 20, 64, "multiwriterary", True, "dry-run", 6, Path("b"))
        self.assertNotIn("build-", job.name)
        self.assertNotIn("prime", job.name)
        command = job.command(job.log_prefix())
        self.assertEqual(command[command.index("--build") + 1], "static")
        self.assertNotIn("--prime", command)
        self.assertNotIn("--dynamic-ops", command)

    def test_graph_options_reach_the_binary_and_the_log_name(self) -> None:
        options = rust.GraphOptions(static=False, prime=True, dynamic_ops=500)
        jobs, _ = rust.build_jobs(Path("b"), Fit(), ("dry-run",), (64,), ("multiwrite",),
                                  degrees=(4,), powers=(5,), move=False, options=options)
        (job,) = jobs
        self.assertTrue(job.name.endswith("_build-dynamic_prime_dynops-500"))
        command = job.command(job.log_prefix())
        self.assertEqual(command[command.index("--build") + 1], "dynamic")
        self.assertIn("--prime", command)
        self.assertEqual(command[command.index("--dynamic-ops") + 1], "500")

    def test_arguments(self) -> None:
        args = rust.parse_arguments([])
        self.assertEqual((args.static, args.prime, args.dynamic_ops, args.move), (True, False, 0, None))
        args = rust.parse_arguments(["--no-static", "--prime", "--dynamic-ops", "7", "--no-move"])
        self.assertEqual((args.static, args.prime, args.dynamic_ops, args.move), (False, True, 7, False))

    def test_dataset_jobs(self) -> None:
        (email,) = rust.select_datasets(["emailEucore"], Path("/data"))
        self.assertEqual(email.path, Path("/data/emailEucore/emailEucore.txt"))
        self.assertEqual(
            [d.name for d in rust.select_datasets(["emailEucore,roadNetPA"])],
            ["emailEucore", "roadNetPA"],
        )
        with self.assertRaises(ValueError):
            rust.select_datasets(["nope"])
        jobs, _ = rust.build_jobs(Path("b"), Fit(), ("dry-run",), (64,), ("recursive",), datasets=[email])
        (job,) = jobs
        self.assertIs(job.dataset, email)
        self.assertEqual(job.workload, email.edges)
        self.assertTrue(job.name.startswith("emailEucore_bs-64_"))
        self.assertEqual(job.graph, rust.GRAPH_DIR / "datasets" / "emailEucore.edgelist")
        self.assertEqual(job.log_directory, rust.DATASET_LOG_DIR)
        self.assertEqual(job.graph_spec.kind, "dataset")

    def test_option_columns_appear_only_for_non_default_builds(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            lines = log_lines()
            (root / "plain.log").write_text("\n".join(lines) + "\n")
            rows = rust.write_report(root, root / "summary.csv")
            header = (root / "summary.csv").read_text().splitlines()[0].split(",")
            self.assertEqual(header, rust.FIELDS)
            lines[0] += " impl=native build=dynamic prime=true dynamic_ops=100"
            (root / "dynamic.log").write_text("\n".join(lines) + "\n")
            rows = rust.write_report(root, root / "summary.csv")
            header = (root / "summary.csv").read_text().splitlines()[0].split(",")
        self.assertEqual(header[len(rust.FIELDS):], rust.OPTION_FIELDS)
        dynamic = [row for row in rows if row["log"] == "dynamic.log"]
        self.assertEqual((dynamic[0]["build"], dynamic[0]["prime"], dynamic[0]["dynamic_ops"]),
                         ("dynamic", True, 100))

    def test_dataset_export_writes_native_edge_lists(self) -> None:
        import graphs

        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "tiny.txt"
            source.write_text("# comment\nFromNodeId ToNodeId\n0 1\n1 2\n2 0\n")
            output = Path(directory) / "tiny.edgelist"
            graphs.write_native_graph(graphs.dataset_graph(source, directed=False), output)
            lines = output.read_text().splitlines()
        self.assertEqual(sorted(line for line in lines if line.startswith("v ")), ["v 0", "v 1", "v 2"])
        self.assertEqual(len([line for line in lines if not line.startswith("v ")]), 6)  # both directions


class SelfContainedTest(unittest.TestCase):
    def test_nothing_is_imported_from_osam(self) -> None:
        import sys

        bench = Path(rust.__file__).resolve().parent
        for module in (rust, scheduler):
            self.assertEqual(Path(module.__file__).resolve().parent, bench)
        self.assertNotIn("launch_local_tests", sys.modules)
        self.assertNotIn("prepare_rust_graph_benchmark", sys.modules)
        # Outputs stay inside rust_osam_plus/bench.
        for path in (rust.LOG_DIR, rust.DATASET_LOG_DIR, rust.GRAPH_DIR):
            self.assertTrue(path.resolve().is_relative_to(bench), path)
        self.assertEqual(rust.RUST_CRATE.resolve(), (bench.parent / "crates" / "sam-model").resolve())

    def test_er_generator_joins_components_and_writes_both_directions(self) -> None:
        import graphs

        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "er.edgelist"
            graphs.generate_graph(40, 2, 1, output)
            lines = output.read_text().splitlines()
        vertices = [line for line in lines if line.startswith("v ")]
        arcs = [tuple(map(int, line.split())) for line in lines if not line.startswith("v ")]
        self.assertEqual(len(vertices), 40)
        self.assertEqual({(u, v) for u, v, _ in arcs}, {(v, u) for u, v, _ in arcs})
        # connected after joining components
        adjacency: dict[int, set[int]] = {}
        for u, v, _ in arcs:
            adjacency.setdefault(u, set()).add(v)
        seen, stack = {0}, [0]
        while stack:
            for w in adjacency.get(stack.pop(), ()):
                if w not in seen:
                    seen.add(w)
                    stack.append(w)
        self.assertEqual(len(seen), 40)


class GeneratorTest(unittest.TestCase):
    def test_fast_is_the_default_and_keeps_log_names(self) -> None:
        self.assertEqual(rust.parse_arguments([]).generator, "fast")
        job = rust.RustJob(64, 20, 64, "multiwrite", True, "dry-run", None, Path("b"))
        self.assertNotIn("gen-", job.name)
        self.assertEqual(job.graph.name, "ER_n-64_d-20_seed-1_gen-fast.edgelist")

    def test_exact_generator_is_named_apart(self) -> None:
        jobs, _ = rust.build_jobs(Path("b"), Fit(), ("dry-run",), (64,), ("multiwrite",),
                                  degrees=(4,), powers=(5,), move=True, generator="exact")
        (job,) = jobs
        self.assertTrue(job.name.endswith("_gen-exact"))
        # exact graphs keep the original file name, so cached ones are reused
        self.assertEqual(job.graph.name, "ER_n-32_d-4_seed-1.edgelist")
        self.assertEqual(job.graph_spec.generator, "exact")

    def test_generators_sample_valid_graphs(self) -> None:
        import graphs

        with tempfile.TemporaryDirectory() as directory:
            texts = {}
            for generator in graphs.GENERATORS:
                output = Path(directory) / f"{generator}.edgelist"
                graphs.generate_graph(300, 10, 1, output, generator)
                texts[generator] = output.read_text()
        for generator, text in texts.items():
            arcs = [line for line in text.splitlines() if not line.startswith("v ")]
            # mean out-degree near d (connecting components adds a few arcs)
            self.assertLess(abs(len(arcs) / 300 - 10), 1.5, generator)
        self.assertNotEqual(texts["fast"], texts["exact"])
        with self.assertRaises(ValueError):
            graphs.generate_graph(10, 2, 1, Path(directory) / "x", "slow")


class NoMoveTest(unittest.TestCase):
    def test_cache_off_jobs_run_python_no_move_and_are_named_apart(self) -> None:
        off = rust.RustJob(64, 20, 4096, "original", False, "dry-run", None, Path("b"))
        on = rust.RustJob(64, 20, 4096, "original", True, "dry-run", None, Path("b"))
        off_command, on_command = off.command(off.log_prefix()), on.command(on.log_prefix())
        self.assertIn("--no-move", off_command)
        self.assertNotIn("--cache", off_command)
        self.assertIn("--cache", on_command)
        self.assertNotIn("--no-move", on_command)
        self.assertIn("_move-false", off.name)
        self.assertNotIn("_move-false", on.name)

    def test_oram_counts_recursive_pointer_and_queue(self) -> None:
        self.assertEqual(rust.RECURSIVE_STRUCTURES, ("RecursivePointer", "SmartQueue"))

if __name__ == "__main__":
    unittest.main()
