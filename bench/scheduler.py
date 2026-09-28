"""Memory-aware parallel job scheduler for the Rust OSAM benchmarks.

Vendored from ``osam/launch_local_tests.py`` (the Python launcher), without
its Python-benchmark job definitions, so the Rust benchmarks do not import
anything from ``osam``. It keeps the same experiment matrix constants, the
dataset list and the scheduler: bounded parallelism per workload bucket,
admission against measured peak RSS, memory-pressure handling and serial
diagnostic retries. Jobs are duck-typed (see ``Job``).
"""

from __future__ import annotations

import os
import re
import shutil
import shlex
import signal
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Protocol


BENCH_DIR = Path(__file__).resolve().parent
# Where the dataset edge lists live (``<name>/<file>``): the Python
# repository's ``osam/real-dataset-tests`` by default (data only).
DATASET_ROOT = Path(
    os.environ.get("DATASET_ROOT", BENCH_DIR.parent.parent.parent / "osam" / "real-dataset-tests")
)


class Job(Protocol):
    """What the scheduler needs from a job (see ``launch_rust_tests.RustJob``)."""

    dataset: object | None

    @property
    def workload(self) -> int: ...

    @property
    def workload_label(self) -> str: ...

    @property
    def description(self) -> str: ...

    @property
    def log_directory(self) -> Path: ...

    def log_prefix(self, directory: Path | None = None) -> Path: ...

    def is_complete(self, directory: Path | None = None) -> bool: ...

    def command(self, log_prefix: Path, fault_handler: bool = False) -> list[str]: ...


BLOCK_SIZES = (64, 4096)
DEGREES = (20, 100)
POWERS = range(5, 21)
ALGORITHMS = ("rw", "cd", "pr", "dfs", "bfs", "dijkstra", "prim", "dtc")
MAX_STEPS = 100
# Directed triangle count is cubic in degree: at d=100 an uncapped run pulls
# ~10^6 neighbors per trial. Cap the neighbors retrieved at each of its three
# levels so the experiment is tractable (5^3 = 125 instead of ~10^6).
MAX_NEIGHBORS = 5
POINTERS = ("original", "multiwrite", "multiwriterary", "recursive")
WALK_LENGTH = 50
DAMPING_FACTOR = 0.9

SMALL_JOB_THRESHOLD = 2**19
MEDIUM_JOB_THRESHOLD = 2**22
SINGLE_JOB_THRESHOLD = 2**24
MAX_ER_WORKLOAD = 2**25


@dataclass(frozen=True)
class Dataset:
    """A CSV edge-list dataset supported by the former CSV launcher."""

    name: str
    filename: str
    directed: bool
    edges: int

    @property
    def path(self) -> Path:
        return DATASET_ROOT / self.name / self.filename

    @property
    def workload(self) -> int:
        # The synthetic scheduler uses n*d, which approximates the number of
        # adjacency entries.  An undirected edge appears in two adjacency
        # lists, so use the comparable quantity for datasets.
        return self.edges if self.directed else 2 * self.edges


DATASETS = (
    Dataset("emailEucore", "emailEucore.txt", True, 25_571),
    Dataset("p2pGnutella04", "p2pGnutella04.txt", True, 39_994),
    Dataset("gpluscombined", "gpluscombined.txt", True, 13_673_453),
    Dataset("roadNetPA", "roadNetPA.txt", False, 1_541_898),
    Dataset("comyoutube", "comyoutube.txt", False, 2_987_624),
    Dataset("higgsTwitter", "higgsTwitter.txt", True, 14_855_842),
    Dataset("twitchgamers", "twitchgamers.csv", False, 6_797_557),
)
DATASETS_BY_NAME = {dataset.name: dataset for dataset in DATASETS}


def env_non_negative_int(name: str, default: int) -> int:
    value = os.environ.get(name, str(default))
    try:
        parsed = int(value)
    except ValueError as exc:
        raise ValueError(f"{name} must be a non-negative integer") from exc
    if parsed < 0:
        raise ValueError(f"{name} must be a non-negative integer")
    return parsed


def env_positive_int(name: str, default: int) -> int:
    value = env_non_negative_int(name, default)
    if value == 0:
        raise ValueError(f"{name} must be a positive integer")
    return value


@dataclass(frozen=True)
class Settings:
    min_available_memory_gb: int
    memory_poll_seconds: int
    memory_pressure_grace_seconds: int
    job_termination_grace_seconds: int
    job_launch_settle_seconds: int
    unsampled_bucket_limit: int
    memory_headroom_factor: float
    job_limit_scale: float
    dataset_large_job_limit: int
    status_interval_seconds: int

    @property
    def memory_reserve_bytes(self) -> int:
        return self.min_available_memory_gb * 1024 * 1024 * 1024

    @classmethod
    def from_environment(cls) -> "Settings":
        return cls(
            min_available_memory_gb=env_non_negative_int("MIN_AVAILABLE_MEMORY_GB", 96),
            memory_poll_seconds=env_positive_int("MEMORY_POLL_SECONDS", 15),
            memory_pressure_grace_seconds=env_positive_int(
                "MEMORY_PRESSURE_GRACE_SECONDS", 60
            ),
            job_termination_grace_seconds=env_positive_int(
                "JOB_TERMINATION_GRACE_SECONDS", 10
            ),
            job_launch_settle_seconds=env_positive_int(
                "JOB_LAUNCH_SETTLE_SECONDS", 20
            ),
            # Until a bucket has a real RSS sample, admit only a couple of its
            # jobs. The static caps are scaled to core count, not footprint, so
            # going straight to 2*cpu_count on an unmeasured bucket is how a run
            # fills memory before the first sample is ever taken.
            unsampled_bucket_limit=env_positive_int("UNSAMPLED_BUCKET_LIMIT", 2),
            # Multiplier applied to a bucket's observed peak when projecting
            # what a not-yet-peaked job will still consume.
            memory_headroom_factor=float(
                os.environ.get("MEMORY_HEADROOM_FACTOR", "1.15")
            ),
            # Multiplier on the per-bucket fallback caps. These are derived from
            # core count, not footprint, and only bound concurrency until real
            # RSS samples exist -- the headroom gate is what actually protects
            # memory. Measured peaks are ~170 GiB for the largest ER job and
            # ~70 GiB typical, so 3 of the largest fit in ~510 GiB of the host.
            job_limit_scale=float(os.environ.get("JOB_LIMIT_SCALE", "3")),
            # Ceiling on how many of the big dataset graphs run at once. No
            # dataset reaches SINGLE_JOB_THRESHOLD, so this applies to the two
            # upper buckets (see job_limit). A single gplus build has been seen
            # at 370 GiB, so two is the most that fits with the reserve intact.
            dataset_large_job_limit=env_positive_int("DATASET_LARGE_JOB_LIMIT", 2),
            # Runs last many hours, so a per-minute progress line just buries
            # the memory-pressure and deferral messages that matter.
            status_interval_seconds=env_positive_int("STATUS_INTERVAL_SECONDS", 3600),
        )




def file_ends_with_marker(path: Path, marker: str, tail_bytes: int = 8192) -> bool:
    try:
        with path.open("rb") as handle:
            handle.seek(0, os.SEEK_END)
            size = handle.tell()
            handle.seek(max(size - tail_bytes, 0))
            tail = handle.read().decode("utf-8", errors="ignore")
    except OSError:
        return False
    return marker in tail




@dataclass
class ActiveJob:
    job: Job
    process: subprocess.Popen[object]


def cache_settings_for(pointer: str) -> tuple[bool, ...]:
    """Return the cache configurations that are meaningful for one backend."""
    if pointer in {"multiwrite", "multiwriterary"}:
        return (True,)
    if pointer == "recursive":
        # Recursive ORAM runs without moves.  For the read-only algorithms,
        # moving just adds a needless move-back.
        return (False,)
    return (True, False)




def select_datasets(names: list[str]) -> tuple[Dataset, ...]:
    if not names:
        return DATASETS
    unknown = sorted(set(names).difference(DATASETS_BY_NAME))
    if unknown:
        available = ", ".join(DATASETS_BY_NAME)
        raise ValueError(
            f"Unknown dataset(s): {', '.join(unknown)}. Available datasets: {available}"
        )
    return tuple(DATASETS_BY_NAME[name] for name in dict.fromkeys(names))




def available_memory_bytes() -> int | None:
    """Return available memory, or ``None`` when this host cannot report it."""
    if shutil.which("vm_stat"):
        try:
            result = subprocess.run(
                ("vm_stat",),
                check=True,
                capture_output=True,
                text=True,
                timeout=5,
            )
        except (OSError, subprocess.SubprocessError):
            return None

        page_size_match = re.search(r"page size of (\d+) bytes", result.stdout)
        if page_size_match is None:
            return None
        page_size = int(page_size_match.group(1))
        page_counts = []
        for label in ("Pages free", "Pages inactive", "Pages speculative", "Pages purgeable"):
            match = re.search(rf"^{re.escape(label)}:\s+(\d+)", result.stdout, re.MULTILINE)
            if match is not None:
                page_counts.append(int(match.group(1)))
        available = sum(page_counts) * page_size
        return available if available > 0 else None

    if shutil.which("free"):
        try:
            result = subprocess.run(
                ("free", "-b"),
                check=True,
                capture_output=True,
                text=True,
                timeout=5,
            )
        except (OSError, subprocess.SubprocessError):
            return None
        for line in result.stdout.splitlines():
            columns = line.split()
            if columns and columns[0] == "Mem:" and len(columns) >= 2:
                try:
                    available = int(columns[-1])
                    return available if available > 0 else None
                except ValueError:
                    return None
    return None


def total_memory_bytes() -> int | None:
    """Return total physical memory, or ``None`` when this host cannot report it."""
    if shutil.which("vm_stat"):
        try:
            result = subprocess.run(
                ("sysctl", "-n", "hw.memsize"),
                check=True,
                capture_output=True,
                text=True,
                timeout=5,
            )
        except (OSError, subprocess.SubprocessError):
            return None
        try:
            return int(result.stdout.strip())
        except ValueError:
            return None

    if shutil.which("free"):
        try:
            result = subprocess.run(
                ("free", "-b"),
                check=True,
                capture_output=True,
                text=True,
                timeout=5,
            )
        except (OSError, subprocess.SubprocessError):
            return None
        for line in result.stdout.splitlines():
            columns = line.split()
            if columns and columns[0] == "Mem:" and len(columns) >= 2:
                try:
                    return int(columns[1])
                except ValueError:
                    return None
    return None


def process_rss_bytes(pid: int) -> int | None:
    """Return a process's resident set size, or ``None`` when unavailable.

    Only Linux's ``/proc`` exposes this cheaply; on other platforms callers
    fall back to the static, hardware-scaled job limits.
    """
    try:
        with open(f"/proc/{pid}/status") as handle:
            for line in handle:
                if line.startswith("VmRSS:"):
                    parts = line.split()
                    if len(parts) >= 2:
                        return int(parts[1]) * 1024
    except (OSError, ValueError):
        return None
    return None


class LocalTestScheduler:
    def __init__(self, settings: Settings, jobs: Iterable[Job]) -> None:
        self.settings = settings
        self.jobs = list(jobs)
        dataset_modes = {job.dataset is not None for job in self.jobs}
        if len(dataset_modes) > 1:
            raise ValueError("A scheduler invocation must use either dataset or ER jobs")
        self.dataset_mode = dataset_modes == {True}
        self.workload_label = "adjacency entries" if self.dataset_mode else "n*d"
        self.cpu_count = os.cpu_count() or 1
        self.total_memory = total_memory_bytes()
        # Fallback caps per workload bucket (small/medium/large/huge), scaled to
        # this host's core count rather than the fixed 16/4/1 constants that
        # were tuned for a much smaller machine. These bound concurrency until
        # real RSS samples for a bucket are available, and afterwards still
        # cap it against runaway process/scheduling overhead.
        scale = settings.job_limit_scale
        self.static_job_limits = (
            max(1, int(2 * self.cpu_count * scale)),
            max(1, int(max(4, self.cpu_count // 4) * scale)),
            max(1, int(max(2, self.cpu_count // 16) * scale)),
            max(1, int(max(1, self.cpu_count // 64) * scale)),
        )
        self.observed_rss_by_bucket: dict[int, int] = {}

        self.active: dict[int, ActiveJob] = {}
        self.failed_jobs: list[Job] = []
        self.finished_jobs = 0
        self.launched_jobs = 0
        self.largest_workload_launched = 0
        self.failed_serial_retries = 0
        self.failed_serial_retry_jobs: list[Job] = []
        self.serial_retry_process: subprocess.Popen[object] | None = None

        self.lock = threading.RLock()
        self.stop_event = threading.Event()
        self.monitor_threads: list[threading.Thread] = []

    @staticmethod
    def bucket_index(workload: int) -> int:
        if workload < SMALL_JOB_THRESHOLD:
            return 0
        if workload < MEDIUM_JOB_THRESHOLD:
            return 1
        if workload < SINGLE_JOB_THRESHOLD:
            return 2
        return 3

    def record_rss_sample(self, job: Job, rss_bytes: int) -> None:
        bucket = self.bucket_index(job.workload)
        with self.lock:
            if rss_bytes > self.observed_rss_by_bucket.get(bucket, 0):
                self.observed_rss_by_bucket[bucket] = rss_bytes

    def sample_active_rss(self) -> None:
        with self.lock:
            active_jobs = list(self.active.values())
        for active_job in active_jobs:
            if not self.process_is_running(active_job.process):
                continue
            rss = process_rss_bytes(active_job.process.pid)
            if rss is not None:
                self.record_rss_sample(active_job.job, rss)

    def job_limit(self, workload: int) -> int:
        # Dataset jobs are sized by adjacency entries rather than n*d, and the
        # per-bucket RSS samples from ER runs do not describe them well. Cap the
        # large ones explicitly at a level measured to be stable on this host,
        # and leave the small ones to the headroom gate, which sizes admission
        # from observed RSS and is a better limiter than a core-count formula.
        bucket = self.bucket_index(workload)
        if self.dataset_mode:
            # Datasets are sized by adjacency entries rather than n*d, and the
            # largest of them (higgsTwitter, 14.9M) still sits below
            # SINGLE_JOB_THRESHOLD, so the big graphs all land in bucket 2.
            # Cap those and fall through to the observed-RSS gate below, the
            # same as ER mode. Returning len(self.jobs) here is what let 25
            # dataset jobs run at once and drove the host into the OOM killer.
            static_cap = (
                max(1, self.settings.dataset_large_job_limit)
                if bucket >= 2
                else len(self.jobs) or 1
            )
        else:
            static_cap = self.static_job_limits[bucket]
        if self.total_memory is None or self.settings.memory_reserve_bytes == 0:
            return static_cap
        with self.lock:
            observed_rss = self.observed_rss_by_bucket.get(bucket)
        if not observed_rss:
            # No measurement for this bucket yet. Ramp up instead of trusting a
            # core-count-derived cap that knows nothing about footprint.
            return max(1, min(static_cap, self.settings.unsampled_bucket_limit))
        usable = self.total_memory - self.settings.memory_reserve_bytes
        dynamic_cap = max(1, usable // observed_rss)
        return max(1, min(static_cap, dynamic_cap))

    def active_count(self) -> int:
        with self.lock:
            return len(self.active)

    def start_process(
        self,
        job: Job,
        *,
        fault_handler: bool = False,
        stdout: object | None = None,
        stderr: object | None = None,
    ) -> subprocess.Popen[object]:
        environment = os.environ.copy()
        # numpy's BLAS backend spawns one worker thread per core on import, so
        # on a 96 core box every job carries a ~95 thread pool that never does
        # any work: the benchmarks touch numpy only to read the edge list, and
        # the oblivious structures are pure Python. Pin them to one thread so
        # concurrent jobs do not oversubscribe the machine. Respect the values
        # already in the environment if the caller set them deliberately.
        for variable in (
            "OMP_NUM_THREADS",
            "OPENBLAS_NUM_THREADS",
            "MKL_NUM_THREADS",
            "NUMEXPR_NUM_THREADS",
            "VECLIB_MAXIMUM_THREADS",
        ):
            environment.setdefault(variable, "1")
        if fault_handler:
            environment["PYTHONFAULTHANDLER"] = "1"
        job.log_directory.mkdir(parents=True, exist_ok=True)
        return subprocess.Popen(
            job.command(job.log_prefix(), fault_handler=fault_handler),
            cwd=BENCH_DIR,
            env=environment,
            start_new_session=True,
            stdout=stdout,
            stderr=stderr,
        )

    def start_initial_job(self, job: Job) -> None:
        process = self.start_process(job)
        with self.lock:
            self.active[process.pid] = ActiveJob(job, process)
            self.launched_jobs += 1
            self.largest_workload_launched = max(
                self.largest_workload_launched, job.workload
            )

    def projected_peak_bytes(self, job: Job) -> int | None:
        """What one job of this bucket is expected to peak at, if known."""
        bucket = self.bucket_index(job.workload)
        with self.lock:
            observed = self.observed_rss_by_bucket.get(bucket)
        if not observed:
            return None
        return int(observed * self.settings.memory_headroom_factor)

    def pending_growth_bytes(self) -> int:
        """Memory already-running jobs are still expected to claim.

        Each active job is projected to reach its bucket's observed peak; the
        difference between that and its current RSS is memory that is spoken
        for but not yet allocated. Admitting against free memory alone ignores
        this and is what lets a batch of young jobs overcommit the machine.
        """
        with self.lock:
            active_jobs = list(self.active.values())
        pending = 0
        for active_job in active_jobs:
            if not self.process_is_running(active_job.process):
                continue
            projected = self.projected_peak_bytes(active_job.job)
            if projected is None:
                continue
            current = process_rss_bytes(active_job.process.pid) or 0
            pending += max(0, projected - current)
        return pending

    @staticmethod
    def return_status(return_code: int) -> int:
        return 128 + -return_code if return_code < 0 else return_code

    def reap_finished_jobs(self) -> bool:
        finished: list[tuple[ActiveJob, int]] = []
        with self.lock:
            for pid, active_job in list(self.active.items()):
                return_code = active_job.process.poll()
                if return_code is not None:
                    del self.active[pid]
                    finished.append((active_job, return_code))

        for active_job, return_code in finished:
            status = self.return_status(return_code)
            if status == 0:
                print(f"Finished job: {active_job.job.description}", flush=True)
            else:
                print(
                    f"Job failed with status {status}: {active_job.job.description}",
                    file=sys.stderr,
                    flush=True,
                )
                with self.lock:
                    self.failed_jobs.append(active_job.job)
            with self.lock:
                self.finished_jobs += 1
        return bool(finished)

    def wait_for_one_job(self) -> None:
        while self.active_count() > 0:
            if self.reap_finished_jobs():
                return
            time.sleep(1)

    def wait_for_headroom(self, job: Job) -> None:
        """Hold a launch until this job's own peak also fits.

        wait_for_memory only checks that free memory clears the reserve right
        now. That is satisfied moments after a launch, while the jobs just
        admitted are still growing, so the loop keeps admitting. Here we require
        free memory to cover the reserve, everything active jobs have yet to
        claim, and the projected peak of the job about to start.
        """
        if self.settings.memory_reserve_bytes == 0:
            return

        reported_wait = False
        while True:
            self.reap_finished_jobs()
            self.sample_active_rss()

            available = available_memory_bytes()
            if available is None:
                return

            projected = self.projected_peak_bytes(job) or 0
            pending = self.pending_growth_bytes()
            required = self.settings.memory_reserve_bytes + pending + projected

            if available >= required or self.active_count() == 0:
                if reported_wait:
                    print(
                        "Headroom available again "
                        f"({available // (1024 ** 3)} GiB free, "
                        f"{required // (1024 ** 3)} GiB needed); launching",
                        flush=True,
                    )
                return

            if not reported_wait:
                print(
                    f"Deferring launch: {available // (1024 ** 3)} GiB free but "
                    f"{required // (1024 ** 3)} GiB needed "
                    f"(reserve {self.settings.min_available_memory_gb} GiB + "
                    f"{pending // (1024 ** 3)} GiB still to be claimed by "
                    f"{self.active_count()} active job(s) + "
                    f"{projected // (1024 ** 3)} GiB for this one)",
                    flush=True,
                )
                reported_wait = True

            if self.stop_event.wait(self.settings.memory_poll_seconds):
                raise KeyboardInterrupt

    def wait_for_memory(self) -> None:
        if self.settings.memory_reserve_bytes == 0:
            return

        reported_wait = False
        while True:
            available = available_memory_bytes()
            if available is None:
                if reported_wait:
                    print("Memory query unavailable; continuing with concurrency limits")
                return
            if available >= self.settings.memory_reserve_bytes:
                if reported_wait:
                    print(
                        "Available memory is above the "
                        f"{self.settings.min_available_memory_gb} GiB reserve"
                    )
                return
            if not reported_wait:
                print(
                    "Waiting for available memory to reach "
                    f"{self.settings.min_available_memory_gb} GiB before launching another job"
                )
                reported_wait = True
            self.stop_event.wait(self.settings.memory_poll_seconds)
            if self.stop_event.is_set():
                raise KeyboardInterrupt

    def wait_for_job_capacity(self, job: Job) -> None:
        self.reap_finished_jobs()
        # Decide against fresh numbers. The watchdog samples only every
        # memory_poll_seconds, so without this the admission decision can be
        # made from RSS readings that are a whole poll interval stale -- an
        # eternity while several interpreters are building graphs.
        self.sample_active_rss()
        job_limit = self.job_limit(job.workload)

        if job_limit == 1:
            print(
                f"{job.workload_label}={job.workload} requires exclusive memory; "
                "waiting for all active jobs"
            )
            while self.active_count() > 0:
                self.wait_for_one_job()
        else:
            while self.active_count() >= job_limit:
                self.wait_for_one_job()
        self.wait_for_memory()
        self.wait_for_headroom(job)

    def status_monitor(self) -> None:
        while not self.stop_event.wait(self.settings.status_interval_seconds):
            with self.lock:
                finished = self.finished_jobs
                launched = self.launched_jobs
                largest = self.largest_workload_launched
            largest_display = str(largest) if largest else "none"
            running = self.active_count()
            print(
                f"Progress: {finished}/{len(self.jobs)} jobs finished; "
                f"{launched}/{len(self.jobs)} launched ({running} running); "
                f"largest {self.workload_label} launched: {largest_display}",
                flush=True,
            )

    @staticmethod
    def process_is_running(process: subprocess.Popen[object]) -> bool:
        return process.poll() is None

    @staticmethod
    def signal_process_group(process: subprocess.Popen[object], signum: int) -> None:
        if process.poll() is not None:
            return
        try:
            os.killpg(process.pid, signum)
        except ProcessLookupError:
            return
        except OSError:
            try:
                process.send_signal(signum)
            except ProcessLookupError:
                pass

    def terminate_for_memory_pressure(self, active_job: ActiveJob) -> None:
        process = active_job.process
        if not self.process_is_running(process):
            return
        rss = process_rss_bytes(process.pid)
        resident = f", {rss / 2**30:.0f} GiB resident" if rss else ""
        print(
            "Memory pressure persisted for "
            f"{self.settings.memory_pressure_grace_seconds}s; terminating job: "
            f"{active_job.job.workload_label}={active_job.job.workload} "
            f"(pid {process.pid}{resident})",
            file=sys.stderr,
            flush=True,
        )
        self.signal_process_group(process, signal.SIGTERM)
        deadline = time.monotonic() + self.settings.job_termination_grace_seconds
        while time.monotonic() < deadline:
            if not self.process_is_running(process):
                return
            if self.stop_event.wait(1):
                return
        self.signal_process_group(process, signal.SIGKILL)

    def memory_watchdog(self) -> None:
        pressure_started_at: float | None = None
        while not self.stop_event.wait(self.settings.memory_poll_seconds):
            if self.settings.memory_reserve_bytes == 0:
                pressure_started_at = None
                continue
            self.sample_active_rss()
            available = available_memory_bytes()
            if available is None or available >= self.settings.memory_reserve_bytes:
                pressure_started_at = None
                continue
            with self.lock:
                running_jobs = [
                    active_job
                    for active_job in self.active.values()
                    if self.process_is_running(active_job.process)
                ]
            if not running_jobs:
                pressure_started_at = None
                continue
            if pressure_started_at is None:
                pressure_started_at = time.monotonic()
                print(
                    "Available memory is below the "
                    f"{self.settings.min_available_memory_gb} GiB reserve; will terminate "
                    "the largest active job if it lasts "
                    f"{self.settings.memory_pressure_grace_seconds}s",
                    file=sys.stderr,
                    flush=True,
                )
                continue
            if (
                time.monotonic() - pressure_started_at
                >= self.settings.memory_pressure_grace_seconds
            ):
                # Pick by measured RSS, not by workload: adjacency entries
                # are a poor proxy for footprint across datasets, and choosing
                # by them killed 6M-entry comyoutube jobs while a much larger
                # resident process was the one exhausting the host.
                largest_job = max(
                    running_jobs,
                    key=lambda active_job: (
                        process_rss_bytes(active_job.process.pid) or 0,
                        active_job.job.workload,
                    ),
                )
                self.terminate_for_memory_pressure(largest_job)
                pressure_started_at = None

    def start_monitors(self) -> None:
        self.stop_event.clear()
        self.monitor_threads = [
            threading.Thread(target=self.status_monitor, name="status-monitor", daemon=True),
            threading.Thread(target=self.memory_watchdog, name="memory-watchdog", daemon=True),
        ]
        for thread in self.monitor_threads:
            thread.start()

    def stop_monitors(self) -> None:
        self.stop_event.set()
        for thread in self.monitor_threads:
            thread.join(timeout=self.settings.job_termination_grace_seconds + 1)
        self.monitor_threads = []

    def terminate_all(self) -> None:
        with self.lock:
            processes = [active_job.process for active_job in self.active.values()]
            if self.serial_retry_process is not None:
                processes.append(self.serial_retry_process)
        for process in processes:
            self.signal_process_group(process, signal.SIGTERM)
        deadline = time.monotonic() + self.settings.job_termination_grace_seconds
        while time.monotonic() < deadline:
            if not any(self.process_is_running(process) for process in processes):
                return
            time.sleep(0.2)
        for process in processes:
            self.signal_process_group(process, signal.SIGKILL)

    def retry_failed_jobs_serially(self) -> None:
        with self.lock:
            jobs_to_retry = list(self.failed_jobs)
        if not jobs_to_retry:
            return

        # The regular monitors have stopped after the initial batch.  Reuse
        # the memory-reserve wait for the serial runs without treating that
        # normal shutdown as an interruption.
        self.stop_event.clear()
        print(
            f"Retrying {len(jobs_to_retry)} failed job(s) serially with Python fault handling",
            file=sys.stderr,
            flush=True,
        )
        for retry_index, job in enumerate(jobs_to_retry, start=1):
            retry_dir = job.log_directory / "retries" / f"retry-{retry_index}"
            retry_dir.mkdir(parents=True, exist_ok=True)
            console_log = retry_dir / "launcher.log"
            print(
                f"Serial retry {retry_index}/{len(jobs_to_retry)}: {job.description}",
                file=sys.stderr,
                flush=True,
            )
            self.wait_for_memory()
            environment = os.environ.copy()
            environment["PYTHONFAULTHANDLER"] = "1"
            with console_log.open("w") as output:
                process = subprocess.Popen(
                    job.command(job.log_prefix(retry_dir), fault_handler=True),
                    cwd=BENCH_DIR,
                    env=environment,
                    start_new_session=True,
                    stdout=output,
                    stderr=subprocess.STDOUT,
                )
                with self.lock:
                    self.serial_retry_process = process
                return_code = process.wait()
                with self.lock:
                    self.serial_retry_process = None
            status = self.return_status(return_code)
            if status == 0:
                print(f"Serial retry passed: {job.description}", file=sys.stderr, flush=True)
            else:
                self.failed_serial_retries += 1
                self.failed_serial_retry_jobs.append(job)
                print(
                    f"Serial retry failed with status {status}: {job.description} "
                    f"(diagnostics: {console_log})",
                    file=sys.stderr,
                    flush=True,
                )

    def print_remaining_manual_reruns(self) -> None:
        if not self.failed_serial_retry_jobs:
            return
        print(
            "The following jobs failed both attempts and still need a manual, "
            "one-at-a-time rerun:",
            file=sys.stderr,
        )
        for rerun_index, job in enumerate(self.failed_serial_retry_jobs, start=1):
            rerun_directory = job.log_directory / "manual-reruns" / f"job-{rerun_index}"
            command = job.command(job.log_prefix(rerun_directory), fault_handler=True)
            print(f"  # {job.description}", file=sys.stderr)
            print(f"  {shlex.join(command)}", file=sys.stderr)

    def run(self) -> int:
        missing_datasets = sorted(
            {
                job.dataset.path
                for job in self.jobs
                if job.dataset is not None and not job.dataset.path.is_file()
            }
        )
        if missing_datasets:
            print("Dataset mode requires these edge-list files:", file=sys.stderr)
            for path in missing_datasets:
                print(f"  {path}", file=sys.stderr)
            return 1
        self.start_monitors()
        try:
            for job in self.jobs:
                self.wait_for_job_capacity(job)
                self.start_initial_job(job)
                # Let the new interpreter build its graph before the next
                # capacity decision sees its memory footprint.
                if self.stop_event.wait(self.settings.job_launch_settle_seconds):
                    raise KeyboardInterrupt
            while self.active_count() > 0:
                self.wait_for_one_job()
        finally:
            self.stop_monitors()

        self.retry_failed_jobs_serially()
        with self.lock:
            failed_count = len(self.failed_jobs)
            finished = self.finished_jobs
            largest = self.largest_workload_launched
        if failed_count:
            print(
                f"{failed_count} test job(s) failed; "
                f"{self.failed_serial_retries} serial retry job(s) also failed",
                file=sys.stderr,
            )
            self.print_remaining_manual_reruns()
            return 1
        print(
            f"Progress: {finished}/{len(self.jobs)} jobs finished; "
            f"largest {self.workload_label} launched: {largest}."
        )
        print("All local test jobs completed successfully.")
        return 0
