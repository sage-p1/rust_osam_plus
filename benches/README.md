# Running benchmarks

Use `cargo bench` to run benchmarks.

# Example benchmark output

```
% cargo bench
Running benches/benchmark.rs (target/release/deps/benchmark-a6aabfbecaf5b98f)
Gnuplot not found, using plotters backend
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 274.0ms.
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 4096): Col
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 4096): Ana
PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 4096)
                        time:   [26.716 ms 26.858 ms 27.013 ms]
                        change: [-27.874% -14.082% -3.0917%] (p = 0.10 > 0.05)
                        No change in performance detected.
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 1.8s.
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 4096): Col
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 4096): Ana
PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 4096)
                        time:   [112.34 ms 122.43 ms 132.93 ms]
                        change: [-7.7700% +3.0736% +14.475%] (p = 0.61 > 0.05)
                        No change in performance detected.
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 24.4s.
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096): C
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096): A
PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096)
                        time:   [2.4154 s 2.4481 s 2.4869 s]
                        change: [-7.5410% -3.0914% +0.7743%] (p = 0.22 > 0.05)
                        No change in performance detected.

Benchmarking PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 4096): Warming up f
Benchmarking PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 4096): Collecting 1
PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 4096)
                        time:   [34.342 ns 34.580 ns 34.825 ns]
                        change: [-1.0333% +0.3938% +1.5014%] (p = 0.61 > 0.05)
                        No change in performance detected.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 4096): Warming up f
Benchmarking PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 4096): Collecting 1
PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 4096)
                        time:   [34.003 ns 34.309 ns 34.732 ns]
                        change: [+5.5605% +6.5462% +7.6140%] (p = 0.00 < 0.05)
                        Performance has regressed.
Benchmarking PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 4096): Warming up
Benchmarking PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 4096): Collecting
PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 4096)
                        time:   [36.231 ns 37.092 ns 37.799 ns]
                        change: [+3.3774% +5.3122% +7.4219%] (p = 0.00 < 0.05)
                        Performance has regressed.

Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 111.1ms or enable flat sampling.
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096): Collecting 10
PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096)
                        time:   [1.3442 ms 1.9120 ms 2.4956 ms]
                        change: [+33.337% +67.320% +105.67%] (p = 0.00 < 0.05)
                        Performance has regressed.
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 125.7ms or enable flat sampling.
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096): Collecting 10
PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096)
                        time:   [1.5826 ms 2.1241 ms 2.3821 ms]
                        change: [+11.734% +26.576% +42.864%] (p = 0.00 < 0.05)
                        Performance has regressed.
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096): Warming up 
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096): Collecting 
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096)
                        time:   [4.2421 ms 4.8985 ms 5.6736 ms]
                        change: [+103.74% +137.12% +173.77%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096) #2: Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 163.5ms or enable flat sampling.
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096) #2: Collecting
PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096) #2
                        time:   [1.9942 ms 2.0661 ms 2.1830 ms]
                        change: [+51.958% +72.152% +103.68%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 3 outliers among 10 measurements (30.00%)
  1 (10.00%) low mild
  1 (10.00%) high mild
  1 (10.00%) high severe
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096) #2: Warming up
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096) #2: Collecting
PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096) #2
                        time:   [2.2482 ms 2.5551 ms 2.8671 ms]
                        change: [+57.809% +79.165% +100.94%] (p = 0.00 < 0.05)
                        Performance has regressed.
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096) #2: Warming 
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096) #2: Collecti
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096) #2: Analyzin
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096) #2
                        time:   [3.1079 ms 3.5467 ms 3.9850 ms]
                        change: [+57.331% +80.064% +104.20%] (p = 0.00 < 0.05)
                        Performance has regressed.

Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 118.3ms or enable flat sampling.
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096): Collecting 1
PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096)
                        time:   [2.2886 ms 2.5085 ms 2.7846 ms]
                        change: [+90.469% +122.02% +150.37%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) low mild
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096): Warming up f
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096): Collecting 1
PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096)
                        time:   [2.8911 ms 3.2700 ms 3.6249 ms]
                        change: [+83.292% +111.89% +135.24%] (p = 0.00 < 0.05)
                        Performance has regressed.
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096): Warming up
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096): Collecting
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096)
                        time:   [4.0021 ms 4.4447 ms 4.9502 ms]
                        change: [+96.172% +120.08% +145.86%] (p = 0.00 < 0.05)
                        Performance has regressed.

Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096) #2: Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 126.0ms or enable flat sampling.
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096) #2: Collectin
PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096) #2
                        time:   [2.1381 ms 2.3660 ms 2.6568 ms]
                        change: [+67.101% +102.37% +140.12%] (p = 0.00 < 0.05)
                        Performance has regressed.
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096) #2: Warming u
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096) #2: Collectin
PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096) #2
                        time:   [2.7315 ms 2.9861 ms 3.2736 ms]
                        change: [+95.511% +113.66% +132.71%] (p = 0.00 < 0.05)
                        Performance has regressed.
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096) #2: Warming
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096) #2: Collect
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096) #2: Analyzi
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096) #2
                        time:   [4.0392 ms 6.2257 ms 9.0485 ms]
                        change: [+112.73% +220.19% +337.78%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096): Warmin
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096): Collec
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096): Analyz
PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096)
                        time:   [1.2169 ms 1.2682 ms 1.3120 ms]
                        change: [-2.4146% +1.6994% +5.7982%] (p = 0.44 > 0.05)
                        No change in performance detected.
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096): Warmin
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096): Collec
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096): Analyz
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096)
                        time:   [1.3562 ms 1.6303 ms 1.9783 ms]
                        change: [+3.3810% +19.552% +39.461%] (p = 0.05 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096): Warm
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096): Coll
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096): Anal
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096)
                        time:   [1.3799 ms 1.5056 ms 1.5812 ms]
                        change: [+15.130% +27.508% +40.444%] (p = 0.00 < 0.05)
                        Performance has regressed.

Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096) #2: War
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096) #2: Col
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096) #2: Ana
PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096) #2
                        time:   [16.294 µs 16.751 µs 17.531 µs]
                        change: [-66.188% -64.434% -62.507%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high severe
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096) #2: War
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096) #2: Col
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096) #2: Ana
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096) #2
                        time:   [16.393 µs 16.464 µs 16.641 µs]
                        change: [-53.787% -51.716% -49.041%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high mild
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096) #2: W
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096) #2: C
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096) #2: A
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096) #2
                        time:   [16.405 µs 16.486 µs 16.664 µs]
                        change: [-56.259% -55.136% -53.839%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high severe

Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, O
Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 1.2s.
Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, O
Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, O
PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, Ops: 64)
                        time:   [67.881 ms 80.362 ms 94.186 ms]
                        change: [+23.375% +43.884% +67.496%] (p = 0.00 < 0.05)
                        Performance has regressed.
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, O
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 794.6ms.
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, O
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, O
PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, Ops: 64)
                        time:   [105.10 ms 113.04 ms 122.90 ms]
                        change: [+24.919% +46.726% +67.691%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096,
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 5.7s.
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096,
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096,
PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096, Ops: 64)
                        time:   [203.01 ms 2.0139 s 5.4307 s]
                        change: [+105.85% +1921.9% +5446.6%] (p = 0.22 > 0.05)
                        No change in performance detected.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe

Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 103.3ms or enable flat sampling.
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 64): Colle
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 64): Analy
PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 64)
                        time:   [533.37 µs 1.2216 ms 1.8179 ms]
                        change: [+76.978% +190.00% +321.84%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 64): Warmi
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 64): Colle
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 64): Analy
PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 64)
                        time:   [4.8937 ms 6.0646 ms 7.6478 ms]
                        change: [+116.29% +170.57% +239.66%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 682.3ms.
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 64): Col
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 64): Ana
PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 64)
                        time:   [69.687 ms 75.224 ms 81.739 ms]
                        change: [+113.43% +131.98% +150.50%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

Benchmarking PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 64): Warming up for
Benchmarking PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 64): Collecting 10 
PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 64)
                        time:   [36.427 ns 36.805 ns 37.427 ns]
                        change: [+2.5995% +5.5479% +9.3837%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe
Benchmarking PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 64): Warming up for
Benchmarking PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 64): Collecting 10 
PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 64)
                        time:   [34.862 ns 36.969 ns 41.878 ns]
                        change: [+14.150% +26.822% +40.243%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high mild
Benchmarking PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 64): Warming up f
Benchmarking PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 64): Collecting 1
PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 64)
                        time:   [63.940 ns 76.990 ns 94.472 ns]
                        change: [+78.188% +126.39% +183.25%] (p = 0.00 < 0.05)
                        Performance has regressed.

Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 64): Warming up for 
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 64): Collecting 10 s
PathOsamPlus::read/(Capacity: 16384 Blocksize: 64)
                        time:   [288.32 µs 391.87 µs 520.73 µs]
                        change: [+55.830% +93.755% +131.00%] (p = 0.00 < 0.05)
                        Performance has regressed.
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 64): Warming up for 
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 64): Collecting 10 s
PathOsamPlus::read/(Capacity: 65536 Blocksize: 64)
                        time:   [289.81 µs 292.70 µs 296.34 µs]
                        change: [+6.5246% +8.9507% +11.918%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64): Warming up fo
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64): Collecting 10
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64)
                        time:   [533.71 µs 654.28 µs 714.16 µs]
                        change: [+13.066% +26.968% +43.302%] (p = 0.00 < 0.05)
                        Performance has regressed.

Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 64) #2: Warming up f
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 64) #2: Collecting 1
PathOsamPlus::read/(Capacity: 16384 Blocksize: 64) #2
                        time:   [214.79 µs 219.87 µs 224.54 µs]
                        change: [+8.5754% +10.102% +11.967%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 64) #2: Warming up f
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 64) #2: Collecting 1
PathOsamPlus::read/(Capacity: 65536 Blocksize: 64) #2
                        time:   [284.82 µs 368.79 µs 422.91 µs]
                        change: [+6.1083% +20.853% +38.680%] (p = 0.01 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64) #2: Warming up
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64) #2: Collecting
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64) #2
                        time:   [471.89 µs 478.80 µs 484.87 µs]
                        change: [+10.583% +13.373% +15.999%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) low mild

Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 64): Warming up for
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 64): Collecting 10 
PathOsamPlus::write/(Capacity: 16384 Blocksize: 64)
                        time:   [299.16 µs 341.24 µs 364.61 µs]
                        change: [-3.6883% +7.9148% +20.882%] (p = 0.23 > 0.05)
                        No change in performance detected.
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 64): Warming up for
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 64): Collecting 10 
PathOsamPlus::write/(Capacity: 65536 Blocksize: 64)
                        time:   [541.03 µs 700.17 µs 795.24 µs]
                        change: [+31.360% +59.110% +90.040%] (p = 0.00 < 0.05)
                        Performance has regressed.
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64): Warming up f
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64): Collecting 1
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64)
                        time:   [736.08 µs 766.73 µs 817.29 µs]
                        change: [+32.130% +50.712% +69.027%] (p = 0.00 < 0.05)
                        Performance has regressed.

Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 64) #2: Warming up 
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 64) #2: Collecting 
PathOsamPlus::write/(Capacity: 16384 Blocksize: 64) #2
                        time:   [213.96 µs 215.69 µs 218.67 µs]
                        change: [+6.8669% +8.2526% +10.012%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high mild
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 64) #2: Warming up 
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 64) #2: Collecting 
PathOsamPlus::write/(Capacity: 65536 Blocksize: 64) #2
                        time:   [305.01 µs 315.16 µs 321.62 µs]
                        change: [+10.728% +14.715% +18.115%] (p = 0.00 < 0.05)
                        Performance has regressed.
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64) #2: Warming u
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64) #2: Collectin
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64) #2
                        time:   [771.36 µs 923.49 µs 1.1508 ms]
                        change: [+105.92% +152.94% +207.21%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64): Warming 
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64): Collecti
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64): Analyzin
PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64)
                        time:   [257.42 µs 342.58 µs 387.68 µs]
                        change: [+6.3509% +37.863% +66.954%] (p = 0.03 < 0.05)
                        Performance has regressed.
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64): Warming 
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64): Collecti
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64): Analyzin
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64)
                        time:   [226.65 µs 308.99 µs 361.92 µs]
                        change: [+8.1816% +32.145% +61.049%] (p = 0.02 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64): Warmin
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64): Collec
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64): Analyz
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64)
                        time:   [318.81 µs 333.07 µs 347.75 µs]
                        change: [+59.145% +72.375% +85.424%] (p = 0.00 < 0.05)
                        Performance has regressed.

Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64) #2: Warmi
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64) #2: Colle
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64) #2: Analy
PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64) #2
                        time:   [952.37 ns 1.2980 µs 1.9083 µs]
                        change: [-42.106% -26.369% -3.4552%] (p = 0.03 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64) #2: Warmi
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64) #2: Colle
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64) #2: Analy
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64) #2
                        time:   [893.33 ns 904.54 ns 917.87 ns]
                        change: [-50.234% -48.899% -47.465%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64) #2: War
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64) #2: Col
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64) #2: Ana
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64) #2
                        time:   [960.30 ns 2.4920 µs 4.7444 µs]
                        change: [-45.332% -11.508% +53.131%] (p = 0.79 > 0.05)
                        No change in performance detected.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe

Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 64, Ops
Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 64, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 108.2ms.
Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 64, Ops
Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 64, Ops
PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 64, Ops: 64)
                        time:   [10.063 ms 10.364 ms 10.642 ms]
                        change: [+8.3974% +11.687% +14.253%] (p = 0.00 < 0.05)
                        Performance has regressed.
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 148.8ms.
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops
PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops: 64)
                        time:   [13.891 ms 14.148 ms 14.388 ms]
                        change: [+12.483% +14.506% +16.637%] (p = 0.00 < 0.05)
                        Performance has regressed.
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, O
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 376.3ms.
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, O
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, O
PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, Ops: 64)
                        time:   [23.441 ms 24.926 ms 27.151 ms]
                        change: [+11.552% +18.960% +28.452%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe

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
