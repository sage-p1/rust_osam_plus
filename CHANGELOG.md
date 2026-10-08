# Changelog

## Unreleased

* Added `flush_multi_paths`: a public flush that also reads a random dummy path, so flushes and multi-path reads look identical to the server.
* sam-model: `SingleAccessMachine::flush_with_dummy_read` (default: `flush`); `PathOsamSam` implements it with `flush_multi_paths`, and OSAM+'s `ReadEvictedWrites` uses it for every flush. BOSAM's bounded writer and other pointers keep the plain `flush`.
* `oblivious_graph_bench` records `flush_dummy_read=true` on the build line of encrypted OSAM+ runs; `launch_rust_tests.py --skip-existing` reruns encrypted OSAM+ logs without it.
* `bench/make_paper_results.py`: one command that runs every experiment of the Concrete OSAM paper and writes its figures and tables; `bench/skew_experiment.py` and `bench/bandwidth_table.py` for the degree-skew and bandwidth tables.

## 0.2.0-pre.1 (February 11, 2024)

* Switched to lazy initialization
* Increased MSRV to 1.81

## 0.1.0 (October 7, 2024)

* Initial release