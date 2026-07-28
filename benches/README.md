# Running benchmarks

Use `cargo bench` to run benchmarks. Note that all these tests do not use encryption.

# Example benchmark output

```
% cargo bench
Running benches/benchmark.rs (target/release/deps/benchmark-7c99cbbe45212a1c)
Gnuplot not found, using plotters backend
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 291.3ms.
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 4096): Collecting
PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 4096)
                        time:   [27.671 ms 28.181 ms 28.854 ms]
                        change: [-89.378% -89.178% -88.866%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high severe
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 1.8s.
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 4096): Collecting
PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 4096)
                        time:   [110.71 ms 113.54 ms 117.93 ms]
                        change: [-94.658% -94.519% -94.281%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 24.7s.
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096): Collecti
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096): Analyzin
PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096)
                        time:   [2.4075 s 2.4333 s 2.4598 s]
                        change: [-2.5092% -0.6072% +1.1276%] (p = 0.55 > 0.05)
                        No change in performance detected.

Benchmarking PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 4096): Warming up for 100.
Benchmarking PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 4096): Collecting 10 sampl
PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 4096)
                        time:   [34.522 ns 35.063 ns 36.304 ns]
                        change: [-0.8408% +1.8231% +5.2768%] (p = 0.37 > 0.05)
                        No change in performance detected.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
Benchmarking PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 4096): Warming up for 100.
Benchmarking PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 4096): Collecting 10 sampl
PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 4096)
                        time:   [32.606 ns 33.009 ns 33.771 ns]
                        change: [+0.9258% +2.9196% +5.3707%] (p = 0.01 < 0.05)
                        Change within noise threshold.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
Benchmarking PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 4096): Warming up for 10
Benchmarking PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 4096): Collecting 10 sam
PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 4096)
                        time:   [35.225 ns 35.450 ns 35.964 ns]
                        change: [-5.2425% -3.2669% -1.0331%] (p = 0.02 < 0.05)
                        Performance has improved.
Found 4 outliers among 10 measurements (40.00%)
  2 (20.00%) low mild
  1 (10.00%) high mild
  1 (10.00%) high severe

Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096): Warming up for 100.0
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096): Collecting 10 sample
PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096)
                        time:   [1.1442 ms 1.1477 ms 1.1546 ms]
                        change: [-52.659% -42.226% -26.657%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096): Warming up for 100.0
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096): Collecting 10 sample
PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096)
                        time:   [1.3631 ms 1.3662 ms 1.3730 ms]
                        change: [-34.797% -25.258% -14.310%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 121.9ms or enable flat sampling.
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096): Collecting 10 samp
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096)
                        time:   [1.9040 ms 1.9345 ms 1.9784 ms]
                        change: [-65.213% -59.262% -52.028%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe

Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096) #2: Warming up for 10
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096) #2: Collecting 10 sam
PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096) #2
                        time:   [1.1341 ms 1.1364 ms 1.1413 ms]
                        change: [-54.421% -46.376% -38.119%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096) #2: Warming up for 10
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096) #2: Collecting 10 sam
PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096) #2
                        time:   [1.3525 ms 1.3588 ms 1.3659 ms]
                        change: [-52.358% -46.488% -39.190%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096) #2: Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 107.1ms or enable flat sampling.
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096) #2: Collecting 10 s
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096) #2
                        time:   [1.8450 ms 1.8670 ms 1.9118 ms]
                        change: [-53.254% -47.362% -39.947%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe

Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096): Warming up for 100.
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096): Collecting 10 sampl
PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096)
                        time:   [1.1379 ms 1.1712 ms 1.2020 ms]
                        change: [-59.924% -54.593% -46.642%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096): Warming up for 100.
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096): Collecting 10 sampl
PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096)
                        time:   [1.3536 ms 1.4120 ms 1.4639 ms]
                        change: [-61.892% -57.663% -52.015%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 112.4ms or enable flat sampling.
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096): Collecting 10 sam
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096)
                        time:   [1.9237 ms 1.9830 ms 2.0781 ms]
                        change: [-59.856% -54.981% -49.725%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096) #2: Warming up for 1
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096) #2: Collecting 10 sa
PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096) #2
                        time:   [1.0826 ms 1.0879 ms 1.0929 ms]
                        change: [-59.842% -52.769% -43.844%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096) #2: Warming up for 1
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096) #2: Collecting 10 sa
PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096) #2
                        time:   [1.3015 ms 1.3063 ms 1.3157 ms]
                        change: [-59.884% -56.036% -51.923%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096) #2: Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 107.6ms or enable flat sampling.
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096) #2: Collecting 10 
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096) #2
                        time:   [1.8121 ms 1.8388 ms 1.9008 ms]
                        change: [-79.215% -69.567% -52.927%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe

Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096): Warming up fo
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096): Collecting 10
PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096)
                        time:   [1.1077 ms 1.4141 ms 1.6905 ms]
                        change: [-15.663% -3.8933% +11.999%] (p = 0.64 > 0.05)
                        No change in performance detected.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096): Warming up fo
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096): Collecting 10
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096)
                        time:   [1.0543 ms 1.1266 ms 1.1796 ms]
                        change: [-35.760% -25.195% -12.986%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096): Warming up 
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096): Collecting 
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096)
                        time:   [1.1101 ms 1.1443 ms 1.1980 ms]
                        change: [-29.875% -22.253% -13.037%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096) #2: Warming up
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096) #2: Collecting
PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096) #2
                        time:   [15.777 µs 15.820 µs 15.927 µs]
                        change: [-10.492% -6.8681% -3.4691%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096) #2: Warming up
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096) #2: Collecting
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096) #2
                        time:   [15.627 µs 15.676 µs 15.749 µs]
                        change: [-6.4288% -5.1331% -3.7755%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096) #2: Warming 
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096) #2: Collecti
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096) #2: Analyzin
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096) #2
                        time:   [15.499 µs 15.591 µs 15.680 µs]
                        change: [-7.1660% -5.1687% -2.5605%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe

Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 554.5ms.
Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, Ops: 64)
Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, Ops: 64)
PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, Ops: 64)
                        time:   [54.616 ms 54.881 ms 55.096 ms]
                        change: [-41.774% -31.707% -19.106%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) low severe
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 718.8ms.
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, Ops: 64)
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, Ops: 64)
PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, Ops: 64)
                        time:   [70.045 ms 70.268 ms 70.481 ms]
                        change: [-42.833% -37.839% -33.088%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096, Ops: 6
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 1.0s.
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096, Ops: 6
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096, Ops: 6
PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096, Ops: 64)
                        time:   [95.047 ms 101.71 ms 108.23 ms]
                        change: [-98.152% -94.950% -49.495%] (p = 0.22 > 0.05)
                        No change in performance detected.

Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 64): Warming up f
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 64): Collecting 1
PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 64)
                        time:   [334.24 µs 358.34 µs 390.75 µs]
                        change: [-72.181% -57.701% -30.801%] (p = 0.01 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 142.1ms or enable flat sampling.
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 64): Collecting 1
PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 64)
                        time:   [2.2241 ms 2.2661 ms 2.3387 ms]
                        change: [-69.485% -61.352% -51.923%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 458.9ms.
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 64): Collecting
PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 64)
                        time:   [32.284 ms 33.039 ms 33.865 ms]
                        change: [-59.723% -56.079% -52.349%] (p = 0.00 < 0.05)
                        Performance has improved.

Benchmarking PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 64): Warming up for 100.00
Benchmarking PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 64): Collecting 10 samples
PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 64)
                        time:   [34.659 ns 34.789 ns 35.067 ns]
                        change: [-10.471% -7.3518% -4.6658%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 64): Warming up for 100.00
Benchmarking PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 64): Collecting 10 samples
PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 64)
                        time:   [33.095 ns 34.049 ns 35.129 ns]
                        change: [-25.640% -17.156% -8.0511%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 64): Warming up for 100.
Benchmarking PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 64): Collecting 10 sampl
PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 64)
                        time:   [35.228 ns 35.294 ns 35.358 ns]
                        change: [-65.045% -56.611% -43.738%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 64): Warming up for 100.00 
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 64): Collecting 10 samples 
PathOsamPlus::read/(Capacity: 16384 Blocksize: 64)
                        time:   [155.77 µs 156.40 µs 157.15 µs]
                        change: [-66.063% -58.896% -49.470%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 64): Warming up for 100.00 
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 64): Collecting 10 samples 
PathOsamPlus::read/(Capacity: 65536 Blocksize: 64)
                        time:   [215.53 µs 216.25 µs 217.36 µs]
                        change: [-28.733% -26.759% -25.393%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64): Warming up for 100.0
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64): Collecting 10 sample
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64)
                        time:   [347.76 µs 348.17 µs 348.55 µs]
                        change: [-45.560% -38.007% -29.011%] (p = 0.00 < 0.05)
                        Performance has improved.

Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 64) #2: Warming up for 100.
Benchmarking PathOsamPlus::read/(Capacity: 16384 Blocksize: 64) #2: Collecting 10 sampl
PathOsamPlus::read/(Capacity: 16384 Blocksize: 64) #2
                        time:   [155.24 µs 155.80 µs 156.41 µs]
                        change: [-29.505% -28.322% -27.264%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 64) #2: Warming up for 100.
Benchmarking PathOsamPlus::read/(Capacity: 65536 Blocksize: 64) #2: Collecting 10 sampl
PathOsamPlus::read/(Capacity: 65536 Blocksize: 64) #2
                        time:   [213.46 µs 214.40 µs 216.25 µs]
                        change: [-40.833% -32.639% -23.699%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high mild
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64) #2: Warming up for 10
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64) #2: Collecting 10 sam
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64) #2
                        time:   [345.96 µs 348.10 µs 351.45 µs]
                        change: [-27.838% -26.319% -24.369%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 64): Warming up for 100.00
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 64): Collecting 10 samples
PathOsamPlus::write/(Capacity: 16384 Blocksize: 64)
                        time:   [339.18 µs 360.11 µs 369.47 µs]
                        change: [-4.3328% +4.4535% +14.117%] (p = 0.38 > 0.05)
                        No change in performance detected.
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 64): Warming up for 100.00
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 64): Collecting 10 samples
PathOsamPlus::write/(Capacity: 65536 Blocksize: 64)
                        time:   [298.71 µs 317.83 µs 326.05 µs]
                        change: [-57.864% -48.998% -36.287%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64): Warming up for 100.
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64): Collecting 10 sampl
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64)
                        time:   [542.12 µs 552.25 µs 561.55 µs]
                        change: [-33.599% -26.171% -16.552%] (p = 0.00 < 0.05)
                        Performance has improved.

Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 64) #2: Warming up for 100
Benchmarking PathOsamPlus::write/(Capacity: 16384 Blocksize: 64) #2: Collecting 10 samp
PathOsamPlus::write/(Capacity: 16384 Blocksize: 64) #2
                        time:   [162.59 µs 164.36 µs 166.77 µs]
                        change: [-25.836% -24.528% -23.308%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 64) #2: Warming up for 100
Benchmarking PathOsamPlus::write/(Capacity: 65536 Blocksize: 64) #2: Collecting 10 samp
PathOsamPlus::write/(Capacity: 65536 Blocksize: 64) #2
                        time:   [227.78 µs 243.23 µs 258.90 µs]
                        change: [-27.304% -22.891% -17.403%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64) #2: Warming up for 1
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64) #2: Collecting 10 sa
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64) #2
                        time:   [345.71 µs 347.85 µs 353.03 µs]
                        change: [-72.383% -66.710% -59.045%] (p = 0.00 < 0.05)
                        Performance has improved.

Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64): Warming up for 
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64): Collecting 10 s
PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64)
                        time:   [196.55 µs 211.79 µs 219.95 µs]
                        change: [-40.600% -26.224% -5.2453%] (p = 0.04 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64): Warming up for 
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64): Collecting 10 s
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64)
                        time:   [194.71 µs 203.23 µs 208.97 µs]
                        change: [-35.758% -22.593% -6.7167%] (p = 0.02 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64): Warming up fo
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64): Collecting 10
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64)
                        time:   [194.60 µs 202.63 µs 206.61 µs]
                        change: [-44.634% -40.977% -36.751%] (p = 0.00 < 0.05)
                        Performance has improved.

Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64) #2: Warming up f
Benchmarking PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64) #2: Collecting 1
PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64) #2
                        time:   [826.21 ns 829.59 ns 833.11 ns]
                        change: [-50.779% -36.346% -16.094%] (p = 0.01 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64) #2: Warming up f
Benchmarking PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64) #2: Collecting 1
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64) #2
                        time:   [821.05 ns 845.96 ns 869.64 ns]
                        change: [-10.542% -8.4466% -6.1915%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64) #2: Warming up
Benchmarking PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64) #2: Collecting
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64) #2
                        time:   [820.68 ns 825.75 ns 831.78 ns]
                        change: [-70.082% -47.978% -16.235%] (p = 0.18 > 0.05)
                        No change in performance detected.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 64, Ops: 64): 
Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 64, Ops: 64): 
Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 64, Ops: 64): 
PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 64, Ops: 64)
                        time:   [7.3961 ms 7.4271 ms 7.4615 ms]
                        change: [-30.215% -28.340% -26.172%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 101.2ms.
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops: 64): 
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops: 64): 
PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops: 64)
                        time:   [9.9077 ms 9.9692 ms 10.033 ms]
                        change: [-30.790% -29.534% -28.143%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 170.6ms.
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, Ops: 64)
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, Ops: 64)
PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, Ops: 64)
                        time:   [16.933 ms 17.007 ms 17.124 ms]
                        change: [-37.409% -31.769% -27.449%] (p = 0.00 < 0.05)
                        Performance has improved.
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
