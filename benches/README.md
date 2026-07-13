# Running benchmarks

Use `cargo bench` to run benchmarks.

# Example benchmark output

```
% cargo bench
Running benches/benchmark.rs (target/release/deps/benchmark-a6aabfbecaf5b98f)
Gnuplot not found, using plotters backend
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 4096)
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 4096): Warming up for 100.00 ms

Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 266.1ms.
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 4096): Collecting 10 samples in estimated 266.11 ms (10 iterations)
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 4096): Analyzing
PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 4096)
                        time:   [26.410 ms 26.637 ms 26.861 ms]
                        change: [-4.6277% -3.2714% -1.9198%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 4096)
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 4096): Warming up for 100.00 ms

Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 1.5s.
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 4096): Collecting 10 samples in estimated 1.4618 s (10 iterations)
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 4096): Analyzing
PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 4096)
                        time:   [110.87 ms 116.26 ms 122.79 ms]
                        change: [-3.3473% +4.1276% +11.530%] (p = 0.33 > 0.05)
                        No change in performance detected.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096)
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096): Warming up for 100.00 ms

Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 24.0s.
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096): Collecting 10 samples in estimated 23.996 s (10 iterations)
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096): Analyzing
PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096)
                        time:   [2.3649 s 2.3889 s 2.4159 s]

Benchmarking PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 4096)
Benchmarking PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 4096): Warming up for 100.00 ms
Benchmarking PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 4096): Collecting 10 samples in estimated 100.00 ms (3.0M iterations)
Benchmarking PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 4096): Analyzing
PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 4096)
                        time:   [32.914 ns 33.048 ns 33.255 ns]
Benchmarking PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 4096)
Benchmarking PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 4096): Warming up for 100.00 ms
Benchmarking PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 4096): Collecting 10 samples in estimated 100.00 ms (3.3M iterations)
Benchmarking PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 4096): Analyzing
PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 4096)
                        time:   [30.551 ns 30.587 ns 30.611 ns]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) low mild
Benchmarking PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 4096)
Benchmarking PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 4096): Warming up for 100.00 ms
Benchmarking PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 4096): Collecting 10 samples in estimated 100.00 ms (3.0M iterations)
Benchmarking PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 4096): Analyzing
PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 4096)
                        time:   [33.237 ns 33.298 ns 33.343 ns]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) low mild

Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096)
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096): Warming up for 100.00 ms
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096): Collecting 10 samples in estimated 129.59 ms (165 iterations)
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096): Analyzing
PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096)
                        time:   [785.32 µs 786.32 µs 787.02 µs]
Found 3 outliers among 10 measurements (30.00%)
  1 (10.00%) low severe
  2 (20.00%) high mild
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096)
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096): Warming up for 100.00 ms
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096): Collecting 10 samples in estimated 103.16 ms (110 iterations)
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096): Analyzing
PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096)
                        time:   [898.40 µs 905.07 µs 915.84 µs]
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096)
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096): Warming up for 100.00 ms
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096): Collecting 10 samples in estimated 144.52 ms (110 iterations)
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096): Analyzing
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096)
                        time:   [1.2481 ms 1.2722 ms 1.3244 ms]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096) #2
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096) #2: Collecting 10 samples in estimated 129.95 ms (165 iterations)
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096) #2: Analyzing
PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096) #2
                        time:   [778.20 µs 780.51 µs 783.57 µs]
Found 3 outliers among 10 measurements (30.00%)
  2 (20.00%) low severe
  1 (10.00%) high severe
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096) #2
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096) #2: Collecting 10 samples in estimated 147.85 ms (165 iterations)
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096) #2: Analyzing
PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096) #2
                        time:   [889.66 µs 891.60 µs 894.03 µs]
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096) #2
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096) #2: Collecting 10 samples in estimated 139.55 ms (110 iterations)
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096) #2: Analyzing
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096) #2
                        time:   [1.2597 ms 1.2744 ms 1.3203 ms]
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) low severe
  1 (10.00%) high severe

Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096)
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096): Warming up for 100.00 ms
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096): Collecting 10 samples in estimated 126.69 ms (165 iterations)
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096): Analyzing
PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096)
                        time:   [747.35 µs 749.62 µs 752.18 µs]
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096)
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096): Warming up for 100.00 ms
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096): Collecting 10 samples in estimated 105.16 ms (110 iterations)
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096): Analyzing
PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096)
                        time:   [943.76 µs 986.56 µs 1.0384 ms]
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high severe
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096)
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096): Warming up for 100.00 ms
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096): Collecting 10 samples in estimated 150.71 ms (110 iterations)
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096): Analyzing
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096)
                        time:   [1.2461 ms 1.3164 ms 1.3922 ms]
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high severe

Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096) #2
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096) #2: Collecting 10 samples in estimated 125.90 ms (165 iterations)
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096) #2: Analyzing
PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096) #2
                        time:   [754.12 µs 757.00 µs 759.86 µs]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096) #2
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096) #2: Collecting 10 samples in estimated 104.40 ms (110 iterations)
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096) #2: Analyzing
PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096) #2
                        time:   [938.07 µs 952.00 µs 971.53 µs]
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high severe
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096) #2
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096) #2: Collecting 10 samples in estimated 150.98 ms (110 iterations)
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096) #2: Analyzing
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096) #2
                        time:   [1.2774 ms 1.3705 ms 1.4568 ms]

Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096)
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096): Warming up for 100.00 ms
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096): Collecting 10 samples in estimated 100.12 ms (5115 iterations)
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096): Analyzing
PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096)
                        time:   [32.874 µs 33.840 µs 34.686 µs]
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) low mild
  1 (10.00%) high mild
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096)
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096): Warming up for 100.00 ms
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096): Collecting 10 samples in estimated 100.22 ms (4620 iterations)
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096): Analyzing
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096)
                        time:   [32.629 µs 33.395 µs 34.651 µs]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096)
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096): Warming up for 100.00 ms
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096): Collecting 10 samples in estimated 100.07 ms (5005 iterations)
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096): Analyzing
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096)
                        time:   [30.038 µs 30.881 µs 31.260 µs]
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) low severe

Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096) #2
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096) #2: Collecting 10 samples in estimated 100.28 ms (6490 iterations)
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096) #2: Analyzing
PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096) #2
                        time:   [15.457 µs 15.476 µs 15.491 µs]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) low mild
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096) #2
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096) #2: Collecting 10 samples in estimated 100.49 ms (6545 iterations)
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096) #2: Analyzing
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096) #2
                        time:   [15.218 µs 15.242 µs 15.282 µs]
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096) #2
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096) #2: Collecting 10 samples in estimated 100.80 ms (6490 iterations)
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096) #2: Analyzing
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096) #2
                        time:   [15.475 µs 15.524 µs 15.572 µs]

Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, Ops: 64)
Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, Ops: 64): Warming up for 100.00 ms

Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 396.8ms.
Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, Ops: 64): Collecting 10 samples in estimated 396.79 ms (10 iterations)
Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, Ops: 64): Analyzing
PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, Ops: 64)
                        time:   [39.696 ms 39.781 ms 39.864 ms]
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, Ops: 64)
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, Ops: 64): Warming up for 100.00 ms

Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 471.1ms.
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, Ops: 64): Collecting 10 samples in estimated 471.14 ms (10 iterations)
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, Ops: 64): Analyzing
PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, Ops: 64)
                        time:   [46.726 ms 47.120 ms 47.661 ms]
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096, Ops: 64)
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096, Ops: 64): Warming up for 100.00 ms

Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 2.1s.
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096, Ops: 64): Collecting 10 samples in estimated 2.1399 s (10 iterations)
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096, Ops: 64): Analyzing
PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096, Ops: 64)
                        time:   [69.688 ms 74.349 ms 80.299 ms]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 64)
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 64): Warming up for 100.00 ms
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 64): Collecting 10 samples in estimated 105.55 ms (330 iterations)
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 64): Analyzing
PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 64)
                        time:   [314.72 µs 320.25 µs 325.95 µs]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 64)
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 64): Warming up for 100.00 ms

Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 124.5ms or enable flat sampling.
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 64): Collecting 10 samples in estimated 124.54 ms (55 iterations)
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 64): Analyzing
PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 64)
                        time:   [2.2430 ms 2.2812 ms 2.3166 ms]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) low mild
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 64)
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 64): Warming up for 100.00 ms

Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 353.7ms.
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 64): Collecting 10 samples in estimated 353.74 ms (10 iterations)
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 64): Analyzing
PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 64)
                        time:   [31.858 ms 32.083 ms 32.331 ms]

Benchmarking PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 64)
Benchmarking PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 64): Warming up for 100.00 ms
Benchmarking PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 64): Collecting 10 samples in estimated 100.00 ms (2.9M iterations)
Benchmarking PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 64): Analyzing
PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 64)
                        time:   [33.697 ns 33.986 ns 34.310 ns]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 64)
Benchmarking PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 64): Warming up for 100.00 ms
Benchmarking PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 64): Collecting 10 samples in estimated 100.00 ms (3.1M iterations)
Benchmarking PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 64): Analyzing
PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 64)
                        time:   [31.324 ns 31.489 ns 31.899 ns]
Benchmarking PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 64)
Benchmarking PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 64): Warming up for 100.00 ms
Benchmarking PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 64): Collecting 10 samples in estimated 100.00 ms (2.9M iterations)
Benchmarking PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 64): Analyzing
PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 64)
                        time:   [33.947 ns 33.976 ns 34.003 ns]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) low mild

Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 64)
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 64): Warming up for 100.00 ms
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 64): Collecting 10 samples in estimated 100.49 ms (1980 iterations)
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 64): Analyzing
PathOsamPlus::read/(Capacity: 16384 Blocksize: 64)
                        time:   [50.158 µs 50.308 µs 50.521 µs]
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high mild
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 64)
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 64): Warming up for 100.00 ms
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 64): Collecting 10 samples in estimated 100.20 ms (1650 iterations)
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 64): Analyzing
PathOsamPlus::read/(Capacity: 65536 Blocksize: 64)
                        time:   [58.917 µs 59.002 µs 59.151 µs]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64)
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64): Warming up for 100.00 ms
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64): Collecting 10 samples in estimated 100.14 ms (1100 iterations)
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64): Analyzing
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64)
                        time:   [89.904 µs 90.221 µs 91.030 µs]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 64) #2
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 64) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 64) #2: Collecting 10 samples in estimated 101.77 ms (2035 iterations)
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 64) #2: Analyzing
PathOsamPlus::read/(Capacity: 16384 Blocksize: 64) #2
                        time:   [50.139 µs 50.241 µs 50.294 µs]
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 64) #2
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 64) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 64) #2: Collecting 10 samples in estimated 100.40 ms (1705 iterations)
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 64) #2: Analyzing
PathOsamPlus::read/(Capacity: 65536 Blocksize: 64) #2
                        time:   [58.747 µs 58.833 µs 58.961 µs]
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64) #2
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64) #2: Collecting 10 samples in estimated 101.57 ms (1100 iterations)
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64) #2: Analyzing
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64) #2
                        time:   [90.562 µs 91.527 µs 92.177 µs]

Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 64)
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 64): Warming up for 100.00 ms
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 64): Collecting 10 samples in estimated 100.47 ms (1980 iterations)
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 64): Analyzing
PathOsamPlus::write/(Capacity: 16384 Blocksize: 64)
                        time:   [50.147 µs 50.213 µs 50.311 µs]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 64)
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 64): Warming up for 100.00 ms
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 64): Collecting 10 samples in estimated 101.20 ms (1540 iterations)
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 64): Analyzing
PathOsamPlus::write/(Capacity: 65536 Blocksize: 64)
                        time:   [64.370 µs 64.998 µs 65.741 µs]
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64)
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64): Warming up for 100.00 ms
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64): Collecting 10 samples in estimated 100.27 ms (1100 iterations)
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64): Analyzing
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64)
                        time:   [91.310 µs 91.561 µs 91.929 µs]
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) low mild
  1 (10.00%) high mild

Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 64) #2
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 64) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 64) #2: Collecting 10 samples in estimated 102.37 ms (1980 iterations)
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 64) #2: Analyzing
PathOsamPlus::write/(Capacity: 16384 Blocksize: 64) #2
                        time:   [50.012 µs 50.138 µs 50.318 µs]
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 64) #2
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 64) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 64) #2: Collecting 10 samples in estimated 102.44 ms (1595 iterations)
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 64) #2: Analyzing
PathOsamPlus::write/(Capacity: 65536 Blocksize: 64) #2
                        time:   [64.151 µs 64.220 µs 64.296 µs]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64) #2
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64) #2: Collecting 10 samples in estimated 100.35 ms (1100 iterations)
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64) #2: Analyzing
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64) #2
                        time:   [90.104 µs 90.210 µs 90.423 µs]

Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64)
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64): Warming up for 100.00 ms
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64): Collecting 10 samples in estimated 100.06 ms (62k iterations)
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64): Analyzing
PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64)
                        time:   [1.7843 µs 1.8213 µs 1.8405 µs]
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64)
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64): Warming up for 100.00 ms
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64): Collecting 10 samples in estimated 100.07 ms (60k iterations)
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64): Analyzing
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64)
                        time:   [1.7725 µs 1.8125 µs 1.8345 µs]
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64)
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64): Warming up for 100.00 ms
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64): Collecting 10 samples in estimated 100.02 ms (61k iterations)
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64): Analyzing
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64)
                        time:   [1.7915 µs 1.8286 µs 1.8486 µs]

Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64) #2
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64) #2: Collecting 10 samples in estimated 100.02 ms (115k iterations)
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64) #2: Analyzing
PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64) #2
                        time:   [825.72 ns 826.45 ns 827.36 ns]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64) #2
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64) #2: Collecting 10 samples in estimated 100.00 ms (118k iterations)
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64) #2: Analyzing
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64) #2
                        time:   [829.29 ns 829.82 ns 830.44 ns]
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) low mild
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64) #2
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64) #2: Warming up for 100.00 ms
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64) #2: Collecting 10 samples in estimated 100.01 ms (122k iterations)
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64) #2: Analyzing
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64) #2
                        time:   [820.13 ns 821.10 ns 822.04 ns]
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe

Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 64, Ops: 64)
Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 64, Ops: 64): Warming up for 100.00 ms

Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 141.8ms or enable flat sampling.
Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 64, Ops: 64): Collecting 10 samples in estimated 141.75 ms (55 iterations)
Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 64, Ops: 64): Analyzing
PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 64, Ops: 64)
                        time:   [2.5384 ms 2.5400 ms 2.5437 ms]
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops: 64)
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops: 64): Warming up for 100.00 ms

Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 150.5ms or enable flat sampling.
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops: 64): Collecting 10 samples in estimated 150.55 ms (55 iterations)
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops: 64): Analyzing
PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops: 64)
                        time:   [2.7291 ms 2.7314 ms 2.7350 ms]
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, Ops: 64)
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, Ops: 64): Warming up for 100.00 ms
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, Ops: 64): Collecting 10 samples in estimated 137.21 ms (30 iterations)
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, Ops: 64): Analyzing
PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, Ops: 64)
                        time:   [4.5641 ms 4.5707 ms 4.5777 ms]

Each read and write read a root-to-leaf path of data. Additionally, both also deterministically download an eviction path. The eviction path may coincide with the first path read, so only `height` blocks are downloaded. At worst, the eviction path, besides the root, is completely different than the first path. This means `2*height - 1` buckets are downloaded. Writes are always the same since we evict one deterministic path.

Physical reads and writes incurred by 1 PathOsamPlus::read:
OSAM Capacity   | OSAM Blocksize  | Physical Reads  | Physical Writes
64              | 64              | 6-11            | 6              
256             | 64              | 8-15            | 8              
64              | 4096            | 6-11            | 6              
256             | 4096            | 8-15            | 8              

Physical reads and writes incurred by 1 PathOsamPlus::write:
OSAM Capacity   | OSAM Blocksize  | Physical Reads  | Physical Writes
64              | 64              | 6-11            | 6              
256             | 64              | 8-15            | 8              
64              | 4096            | 6-11            | 6              
256             | 4096            | 8-15            | 8              

Physical reads and writes incurred by 64 random PathOsamPlus operations:
OSAM Capacity   | OSAM Blocksize  | Physical Reads  | Physical Writes
64              | 64              | 384-704         | 384            
256             | 64              | 512-960         | 512            
64              | 4096            | 384-704         | 384            
256             | 4096            | 512-960         | 512                             
```
