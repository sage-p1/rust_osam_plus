# Running benchmarks

Use `cargo bench` to run benchmarks. Note that all these tests do not use encryption.

# Example benchmark output

```
% cargo bench
Running benches/benchmark.rs (target/release/deps/benchmark-7c99cbbe45212a1c)
Gnuplot not found, using plotters backend
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 366.9ms.
PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 4096)
                        time:   [28.005 ms 29.086 ms 30.291 ms]
                        change: [-51.432% -41.318% -29.186%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 2.2s.
PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 4096)
                        time:   [109.51 ms 112.14 ms 114.85 ms]
                        change: [-61.396% -60.139% -58.818%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 24.6s.
PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096)
                        time:   [2.4694 s 2.4864 s 2.5020 s]
                        change: [-41.997% -38.301% -33.979%] (p = 0.00 < 0.05)
                        Performance has improved.

PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 4096)
                        time:   [34.465 ns 34.536 ns 34.689 ns]
                        change: [-57.214% -37.894% -6.3670%] (p = 0.07 > 0.05)
                        No change in performance detected.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 4096)
                        time:   [32.495 ns 32.619 ns 32.790 ns]
                        change: [-60.349% -47.485% -31.455%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 4096)
                        time:   [35.164 ns 35.291 ns 35.536 ns]
                        change: [-3.2151% -1.9227% -0.6964%] (p = 0.01 < 0.05)
                        Change within noise threshold.

PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096)
                        time:   [1.1480 ms 1.1503 ms 1.1556 ms]
                        change: [-33.880% -23.771% -12.109%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high severe
PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096)
                        time:   [1.3499 ms 1.3546 ms 1.3668 ms]
                        change: [-45.125% -35.803% -24.145%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 106.4ms or enable flat sampling.
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096)
                        time:   [1.8805 ms 1.8901 ms 1.9083 ms]
                        change: [-62.973% -52.219% -38.067%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe

PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096) #2
                        time:   [1.1335 ms 1.1388 ms 1.1463 ms]
                        change: [-67.425% -56.625% -37.711%] (p = 0.00 < 0.05)
                        Performance has improved.
PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096) #2
                        time:   [1.3365 ms 1.3386 ms 1.3429 ms]
                        change: [-35.111% -26.057% -15.298%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096) #2: Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 102.7ms or enable flat sampling.
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096) #2
                        time:   [1.8233 ms 1.8289 ms 1.8404 ms]
                        change: [-13.286% -9.2020% -5.0356%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe

PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096)
                        time:   [1.1093 ms 1.1176 ms 1.1254 ms]
                        change: [-4.7065% -3.1057% -1.7201%] (p = 0.00 < 0.05)
                        Performance has improved.
PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096)
                        time:   [1.3082 ms 1.3416 ms 1.3771 ms]
                        change: [-10.847% -7.0893% -3.5554%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 107.0ms or enable flat sampling.
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096)
                        time:   [1.8143 ms 1.8333 ms 1.8794 ms]
                        change: [-59.855% -55.233% -50.228%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe

PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096) #2
                        time:   [1.0783 ms 1.0810 ms 1.0871 ms]
                        change: [-74.594% -64.539% -48.289%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe
PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096) #2
                        time:   [1.2811 ms 1.2838 ms 1.2888 ms]
                        change: [-17.868% -12.636% -7.4366%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096) #2: Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 105.4ms or enable flat sampling.
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096) #2
                        time:   [1.7943 ms 1.8078 ms 1.8377 ms]
                        change: [-58.279% -51.841% -44.052%] (p = 0.00 < 0.05)
                        Performance has improved.

PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096)
                        time:   [1.0216 ms 1.0960 ms 1.1376 ms]
                        change: [-32.411% -22.757% -13.054%] (p = 0.00 < 0.05)
                        Performance has improved.
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096)
                        time:   [1.0361 ms 1.1203 ms 1.2295 ms]
                        change: [-41.280% -2.1540% +72.879%] (p = 0.94 > 0.05)
                        No change in performance detected.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096)
                        time:   [1.0374 ms 1.1331 ms 1.2078 ms]
                        change: [-29.243% -24.864% -19.722%] (p = 0.00 < 0.05)
                        Performance has improved.

PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096) #2
                        time:   [15.620 µs 15.667 µs 15.780 µs]
                        change: [-25.988% -21.056% -15.754%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096) #2
                        time:   [15.470 µs 15.512 µs 15.576 µs]
                        change: [-45.657% -34.289% -19.116%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096) #2
                        time:   [15.403 µs 19.218 µs 25.119 µs]
                        change: [-67.468% -56.214% -38.986%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high severe

Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 546.6ms.
PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, Ops: 64)
                        time:   [53.482 ms 53.733 ms 53.995 ms]
                        change: [-44.165% -32.179% -17.157%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 696.9ms.
PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, Ops: 64)
                        time:   [68.359 ms 68.610 ms 68.994 ms]
                        change: [-54.278% -44.571% -30.175%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 1.8s.
PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096, Ops: 64)
                        time:   [92.185 ms 94.711 ms 97.514 ms]
                        change: [-50.555% -36.093% -16.647%] (p = 0.01 < 0.05)
                        Performance has improved.

PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 64)
                        time:   [290.61 µs 295.16 µs 305.23 µs]
                        change: [-68.189% -57.832% -42.083%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 120.0ms or enable flat sampling.
PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 64)
                        time:   [1.9215 ms 1.9791 ms 2.0710 ms]
                        change: [-47.001% -38.052% -26.328%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) low mild
  1 (10.00%) high severe
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 334.7ms.
PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 64)
                        time:   [31.589 ms 31.864 ms 32.182 ms]
                        change: [-48.188% -35.238% -16.170%] (p = 0.01 < 0.05)
                        Performance has improved.

PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 64)
                        time:   [34.437 ns 34.528 ns 34.702 ns]
                        change: [-35.444% -26.050% -15.208%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 64)
                        time:   [32.585 ns 37.481 ns 51.385 ns]
                        change: [-29.086% +4.4052% +49.479%] (p = 0.87 > 0.05)
                        No change in performance detected.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high severe
PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 64)
                        time:   [35.116 ns 35.207 ns 35.407 ns]
                        change: [-11.983% -8.5995% -5.1549%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe

PathOsamPlus::read/(Capacity: 16384 Blocksize: 64)
                        time:   [155.35 µs 155.73 µs 156.56 µs]
                        change: [-77.174% -62.417% -39.262%] (p = 0.03 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
PathOsamPlus::read/(Capacity: 65536 Blocksize: 64)
                        time:   [213.40 µs 213.85 µs 214.94 µs]
                        change: [-60.660% -48.207% -29.667%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64)
                        time:   [346.16 µs 346.86 µs 348.41 µs]
                        change: [-29.455% -24.238% -19.258%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe

PathOsamPlus::read/(Capacity: 16384 Blocksize: 64) #2
                        time:   [155.53 µs 155.89 µs 156.71 µs]
                        change: [-16.129% -10.999% -6.1368%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe
PathOsamPlus::read/(Capacity: 65536 Blocksize: 64) #2
                        time:   [213.78 µs 214.35 µs 215.63 µs]
                        change: [-52.411% -29.670% -1.0289%] (p = 0.20 > 0.05)
                        No change in performance detected.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64) #2
                        time:   [344.72 µs 345.40 µs 347.07 µs]
                        change: [-2.8793% -0.9777% +0.6680%] (p = 0.36 > 0.05)
                        No change in performance detected.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe

PathOsamPlus::write/(Capacity: 16384 Blocksize: 64)
                        time:   [330.64 µs 353.13 µs 364.68 µs]
                        change: [-11.401% -1.7278% +8.6441%] (p = 0.74 > 0.05)
                        No change in performance detected.
PathOsamPlus::write/(Capacity: 65536 Blocksize: 64)
                        time:   [299.46 µs 318.34 µs 326.58 µs]
                        change: [-14.227% -4.9590% +5.2421%] (p = 0.39 > 0.05)
                        No change in performance detected.
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64)
                        time:   [496.66 µs 498.92 µs 501.81 µs]
                        change: [+8.1850% +13.400% +17.749%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

PathOsamPlus::write/(Capacity: 16384 Blocksize: 64) #2
                        time:   [155.40 µs 158.10 µs 164.78 µs]
                        change: [-7.3307% -2.6869% +3.4569%] (p = 0.37 > 0.05)
                        No change in performance detected.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
PathOsamPlus::write/(Capacity: 65536 Blocksize: 64) #2
                        time:   [213.40 µs 213.94 µs 215.13 µs]
                        change: [-4.4484% -1.8100% +0.3295%] (p = 0.20 > 0.05)
                        No change in performance detected.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64) #2
                        time:   [345.64 µs 347.06 µs 349.22 µs]
                        change: [-6.8204% -3.9218% -1.1141%] (p = 0.02 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe

PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64)
                        time:   [192.95 µs 204.38 µs 212.15 µs]
                        change: [-4.5544% +1.3713% +7.6912%] (p = 0.70 > 0.05)
                        No change in performance detected.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) low mild
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64)
                        time:   [202.25 µs 212.92 µs 219.46 µs]
                        change: [-50.621% -27.493% -0.9676%] (p = 0.22 > 0.05)
                        No change in performance detected.
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64)
                        time:   [195.59 µs 208.59 µs 221.38 µs]
                        change: [-1.6314% +4.1518% +11.062%] (p = 0.22 > 0.05)
                        No change in performance detected.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high mild

PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64) #2
                        time:   [855.10 ns 861.66 ns 871.99 ns]
                        change: [-10.290% -7.6359% -4.6119%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64) #2
                        time:   [830.66 ns 841.22 ns 859.01 ns]
                        change: [-6.5806% -1.8165% +3.2995%] (p = 0.52 > 0.05)
                        No change in performance detected.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64) #2
                        time:   [825.47 ns 834.40 ns 842.06 ns]
                        change: [-3.1759% -0.5667% +1.6362%] (p = 0.70 > 0.05)
                        No change in performance detected.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 64, Ops: 64)
                        time:   [7.3395 ms 7.3660 ms 7.4024 ms]
                        change: [-3.9460% -2.0720% -0.2713%] (p = 0.05 > 0.05)
                        No change in performance detected.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe
PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops: 64)
                        time:   [9.8295 ms 9.8666 ms 9.9131 ms]
                        change: [-13.454% -10.707% -7.6429%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 170.1ms.
PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, Ops: 64)
                        time:   [16.880 ms 16.969 ms 17.117 ms]
                        change: [-0.7368% +0.0192% +0.9576%] (p = 0.98 > 0.05)
                        No change in performance detected.
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
