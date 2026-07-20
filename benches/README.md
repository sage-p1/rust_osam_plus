# Running benchmarks

Use `cargo bench` to run benchmarks.

# Example benchmark output

```
% cargo bench
Running benches/benchmark.rs (target/release/deps/benchmark-a6aabfbecaf5b98f)
Gnuplot not found, using plotters backend
Benchmarking PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 270.6ms.
PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 4096)
                        time:   [26.299 ms 26.544 ms 26.839 ms]
                        change: [-9.9794% -7.7665% -5.5071%] (p = 0.00 < 0.05)
                        Performance has improved.
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 1.8s.
PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 4096)
                        time:   [114.15 ms 122.82 ms 133.25 ms]
                        change: [-5.7442% +3.7349% +14.262%] (p = 0.52 > 0.05)
                        No change in performance detected.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high mild
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 24.0s.
PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 4096)
                        time:   [2.4398 s 2.5262 s 2.6446 s]
                        change: [+2.1176% +5.7467% +11.286%] (p = 0.01 < 0.05)
                        Performance has regressed.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) high mild
  1 (10.00%) high severe

PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 4096)
                        time:   [34.075 ns 34.208 ns 34.409 ns]
                        change: [+3.0410% +4.0689% +5.6138%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 4096)
                        time:   [31.963 ns 32.110 ns 32.241 ns]
                        change: [+4.6534% +5.1840% +5.9172%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 4096)
                        time:   [34.500 ns 34.655 ns 34.815 ns]
                        change: [+3.5797% +4.9078% +6.3929%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 3 outliers among 10 measurements (30.00%)
  1 (10.00%) low mild
  2 (20.00%) high severe

PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096)
                        time:   [1.1827 ms 1.1867 ms 1.1933 ms]
                        change: [+50.584% +51.703% +53.155%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096)
                        time:   [1.4309 ms 1.4496 ms 1.4718 ms]
                        change: [+56.149% +59.086% +63.001%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high severe
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 123.3ms or enable flat sampling.
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096)
                        time:   [2.0008 ms 2.0326 ms 2.0953 ms]
                        change: [+51.429% +58.325% +66.834%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe

PathOsamPlus::read/(Capacity: 16384 Blocksize: 4096) #2
                        time:   [1.2134 ms 1.2276 ms 1.2423 ms]
                        change: [+56.108% +57.952% +59.910%] (p = 0.00 < 0.05)
                        Performance has regressed.
PathOsamPlus::read/(Capacity: 65536 Blocksize: 4096) #2
                        time:   [1.4080 ms 1.4140 ms 1.4262 ms]
                        change: [+58.528% +60.178% +62.543%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096) #2: Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 111.3ms or enable flat sampling.
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 4096) #2
                        time:   [1.9192 ms 1.9641 ms 2.0448 ms]
                        change: [+43.698% +52.378% +59.716%] (p = 0.00 < 0.05)
                        Performance has regressed.

PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096)
                        time:   [1.1569 ms 1.1661 ms 1.1759 ms]
                        change: [+54.233% +55.309% +56.419%] (p = 0.00 < 0.05)
                        Performance has regressed.
PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096)
                        time:   [1.5068 ms 1.5357 ms 1.5882 ms]
                        change: [+49.110% +58.476% +68.329%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 114.5ms or enable flat sampling.
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096)
                        time:   [1.9482 ms 2.0127 ms 2.0806 ms]
                        change: [+47.725% +56.580% +65.380%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

PathOsamPlus::write/(Capacity: 16384 Blocksize: 4096) #2
                        time:   [1.1263 ms 1.1358 ms 1.1548 ms]
                        change: [+49.165% +50.955% +53.081%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
PathOsamPlus::write/(Capacity: 65536 Blocksize: 4096) #2
                        time:   [1.3771 ms 1.4106 ms 1.4423 ms]
                        change: [+44.756% +47.371% +50.247%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
Benchmarking PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096) #2: Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 112.8ms or enable flat sampling.
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 4096) #2
                        time:   [1.9085 ms 1.9436 ms 1.9761 ms]
                        change: [+35.312% +43.763% +51.868%] (p = 0.00 < 0.05)
                        Performance has regressed.

PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096)
                        time:   [1.2006 ms 1.2343 ms 1.2662 ms]
                        change: [+3502.3% +3631.8% +3775.1%] (p = 0.00 < 0.05)
                        Performance has regressed.
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096)
                        time:   [1.1983 ms 1.2998 ms 1.3523 ms]
                        change: [+3384.8% +3616.5% +3846.5%] (p = 0.00 < 0.05)
                        Performance has regressed.
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096)
                        time:   [1.1344 ms 1.1808 ms 1.2068 ms]
                        change: [+3423.7% +3694.9% +4021.6%] (p = 0.00 < 0.05)
                        Performance has regressed.

PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 4096) #2
                        time:   [45.818 µs 47.719 µs 50.291 µs]
                        change: [+199.31% +210.64% +222.24%] (p = 0.00 < 0.05)
                        Performance has regressed.
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 4096) #2
                        time:   [30.356 µs 33.386 µs 36.011 µs]
                        change: [+111.96% +124.57% +133.60%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) low mild
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 4096) #2
                        time:   [36.384 µs 37.156 µs 37.714 µs]
                        change: [+131.90% +138.64% +144.10%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) low mild

Benchmarking PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 570.2ms.
PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 4096, Ops: 64)
                        time:   [55.587 ms 55.852 ms 56.231 ms]
                        change: [+39.607% +40.399% +41.356%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) low mild
  1 (10.00%) high severe
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 726.8ms.
PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 4096, Ops: 64)
                        time:   [71.105 ms 77.043 ms 88.007 ms]
                        change: [+50.341% +63.503% +87.678%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high severe
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 2.2s.
PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 4096, Ops: 64)
                        time:   [96.466 ms 99.606 ms 103.26 ms]
                        change: [+23.130% +33.970% +44.009%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

PathOsamPlus::initialization/(Capacity: 16384 Blocksize: 64)
                        time:   [309.53 µs 317.87 µs 327.86 µs]
                        change: [-5.3132% +0.2553% +6.6613%] (p = 0.95 > 0.05)
                        No change in performance detected.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) low mild
  1 (10.00%) high severe
Benchmarking PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 132.8ms or enable flat sampling.
PathOsamPlus::initialization/(Capacity: 65536 Blocksize: 64)
                        time:   [2.1495 ms 2.2160 ms 2.2921 ms]
                        change: [-4.9214% -2.2990% +0.4510%] (p = 0.13 > 0.05)
                        No change in performance detected.
Benchmarking PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 330.0ms.
PathOsamPlus::initialization/(Capacity: 1048576 Blocksize: 64)
                        time:   [31.971 ms 32.427 ms 32.885 ms]
                        change: [-0.4297% +1.0741% +2.7666%] (p = 0.23 > 0.05)
                        No change in performance detected.

PathOsamPlus::alloc/(Capacity: 16384 Blocksize: 64)
                        time:   [35.185 ns 35.430 ns 35.870 ns]
                        change: [+4.5664% +5.7743% +7.0120%] (p = 0.00 < 0.05)
                        Performance has regressed.
PathOsamPlus::alloc/(Capacity: 65536 Blocksize: 64)
                        time:   [32.060 ns 32.128 ns 32.239 ns]
                        change: [-0.7129% +0.8797% +2.4209%] (p = 0.30 > 0.05)
                        No change in performance detected.
PathOsamPlus::alloc/(Capacity: 1048576 Blocksize: 64)
                        time:   [35.214 ns 35.460 ns 36.004 ns]
                        change: [+4.6301% +5.8693% +7.1470%] (p = 0.00 < 0.05)
                        Performance has regressed.

PathOsamPlus::read/(Capacity: 16384 Blocksize: 64)
                        time:   [195.75 µs 196.30 µs 197.29 µs]
                        change: [+289.53% +292.03% +294.63%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high severe
PathOsamPlus::read/(Capacity: 65536 Blocksize: 64)
                        time:   [271.61 µs 273.57 µs 275.54 µs]
                        change: [+352.66% +358.72% +364.81%] (p = 0.00 < 0.05)
                        Performance has regressed.
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64)
                        time:   [438.38 µs 439.91 µs 442.91 µs]
                        change: [+381.74% +387.32% +391.97%] (p = 0.00 < 0.05)
                        Performance has regressed.

PathOsamPlus::read/(Capacity: 16384 Blocksize: 64) #2
                        time:   [196.61 µs 197.64 µs 198.95 µs]
                        change: [+292.41% +294.32% +296.15%] (p = 0.00 < 0.05)
                        Performance has regressed.
PathOsamPlus::read/(Capacity: 65536 Blocksize: 64) #2
                        time:   [262.85 µs 263.59 µs 265.16 µs]
                        change: [+344.85% +348.48% +351.49%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) low mild
PathOsamPlus::read/(Capacity: 1048576 Blocksize: 64) #2
                        time:   [413.96 µs 417.91 µs 420.47 µs]
                        change: [+355.83% +362.45% +368.65%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) low severe

PathOsamPlus::write/(Capacity: 16384 Blocksize: 64)
                        time:   [292.05 µs 332.60 µs 352.08 µs]
                        change: [+438.74% +489.25% +547.05%] (p = 0.00 < 0.05)
                        Performance has regressed.
PathOsamPlus::write/(Capacity: 65536 Blocksize: 64)
                        time:   [370.98 µs 380.01 µs 384.14 µs]
                        change: [+451.45% +468.05% +484.09%] (p = 0.00 < 0.05)
                        Performance has regressed.
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64)
                        time:   [493.22 µs 511.55 µs 521.78 µs]
                        change: [+413.33% +433.85% +453.76%] (p = 0.00 < 0.05)
                        Performance has regressed.

PathOsamPlus::write/(Capacity: 16384 Blocksize: 64) #2
                        time:   [199.72 µs 200.42 µs 201.01 µs]
                        change: [+297.71% +299.41% +300.87%] (p = 0.00 < 0.05)
                        Performance has regressed.
PathOsamPlus::write/(Capacity: 65536 Blocksize: 64) #2
                        time:   [263.64 µs 264.30 µs 265.63 µs]
                        change: [+310.83% +314.31% +318.67%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 2 outliers among 10 measurements (20.00%)
  2 (20.00%) high mild
PathOsamPlus::write/(Capacity: 1048576 Blocksize: 64) #2
                        time:   [418.66 µs 419.22 µs 420.04 µs]
                        change: [+363.83% +365.04% +366.32%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64)
                        time:   [194.22 µs 204.56 µs 214.23 µs]
                        change: [+10340% +10845% +11422%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 2 outliers among 10 measurements (20.00%)
  1 (10.00%) low mild
  1 (10.00%) high mild
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64)
                        time:   [187.66 µs 201.44 µs 213.01 µs]
                        change: [+10200% +10689% +11306%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64)
                        time:   [187.02 µs 203.36 µs 214.10 µs]
                        change: [+10067% +10568% +11149%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 10 measurements (10.00%)
  1 (10.00%) high mild

PathOsamPlus::local_write/(Capacity: 16384 Blocksize: 64) #2
                        time:   [1.7746 µs 1.8102 µs 1.8308 µs]
                        change: [+111.09% +114.67% +118.41%] (p = 0.00 < 0.05)
                        Performance has regressed.
PathOsamPlus::local_write/(Capacity: 65536 Blocksize: 64) #2
                        time:   [1.7933 µs 1.8261 µs 1.8438 µs]
                        change: [+110.56% +114.84% +118.93%] (p = 0.00 < 0.05)
                        Performance has regressed.
PathOsamPlus::local_write/(Capacity: 1048576 Blocksize: 64) #2
                        time:   [1.7873 µs 1.8375 µs 1.8761 µs]
                        change: [+113.46% +117.49% +122.63%] (p = 0.00 < 0.05)
                        Performance has regressed.

PathOsamPlus::random_operations/(Capacity: 16384 Blocksize: 64, Ops: 64)
                        time:   [9.2614 ms 9.2798 ms 9.2988 ms]
                        change: [+263.76% +264.69% +265.61%] (p = 0.00 < 0.05)
                        Performance has regressed.
Benchmarking PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 126.3ms.
PathOsamPlus::random_operations/(Capacity: 65536 Blocksize: 64, Ops: 64)
                        time:   [12.296 ms 12.355 ms 12.420 ms]
                        change: [+349.57% +351.70% +354.27%] (p = 0.00 < 0.05)
                        Performance has regressed.
Benchmarking PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, Ops: 64): Warming up for 100.00 ms
Warning: Unable to complete 10 samples in 100.0ms. You may wish to increase target time to 205.8ms.
PathOsamPlus::random_operations/(Capacity: 1048576 Blocksize: 64, Ops: 64)
                        time:   [20.683 ms 20.953 ms 21.269 ms]
                        change: [+352.85% +358.41% +366.25%] (p = 0.00 < 0.05)
                        Performance has regressed.

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
