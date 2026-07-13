// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! This module contains benchmarks for the `osam_plus` crate.

extern crate criterion;
use core::fmt;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use osam_plus::path_osam_plus::DEFAULT_STASH_OVERFLOW_SIZE;
use osam_plus::{BlockSize, BlockValue, BucketSize, Identifier, OsamPlus, PathOsamPlus, TreeIndex};
use std::mem;
use std::time::Duration;

use rand::{rngs::StdRng, Rng, SeedableRng};

const CAPACITIES_TO_BENCHMARK: [Identifier; 3] = [1 << 14, 1 << 16, 1 << 20];

// Here, all benchmarks are run for OSAM+ and block sizes of 64 and 4096.
criterion_group!(
    name = benches;
    config = Criterion::default().warm_up_time(Duration::new(0, 1_000_000_00)).measurement_time(Duration::new(0, 1_000_000_00)).sample_size(10);
    targets =
    benchmark_initialization::<4096, 4>,
    benchmark_alloc::<4096, 4>,
    benchmark_alloc_and_read::<4096, 4>,
    benchmark_read::<4096, 4>,
    benchmark_alloc_and_write::<4096, 4>,
    benchmark_write::<4096, 4>,
    benchmark_alloc_and_local_write::<4096, 4>,
    benchmark_local_write::<4096, 4>,
    benchmark_random_operations::<4096, 4>,
    benchmark_initialization::<64, 4>,
    benchmark_alloc::<64, 4>,
    benchmark_alloc_and_read::<64, 4>,
    benchmark_read::<64, 4>,
    benchmark_alloc_and_write::<64, 4>,
    benchmark_write::<64, 4>,
    benchmark_alloc_and_local_write::<64, 4>,
    benchmark_local_write::<64, 4>,
    benchmark_random_operations::<64, 4>,
);

criterion_main!(benches);

fn benchmark_initialization<const B: BlockSize, const Z: BucketSize>(c: &mut Criterion) {
    let mut group = c.benchmark_group(String::from("PathOsamPlus") + "::initialization");
    for capacity in CAPACITIES_TO_BENCHMARK.iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(ReadWriteParameters {
                capacity: *capacity,
                block_size: mem::size_of::<BlockValue<B>>(),
            }),
            capacity,
            |b, capacity| {
                b.iter(|| {
                    PathOsamPlus::<BlockValue<B>, Z>::new_with_parameters(
                        *capacity,
                        DEFAULT_STASH_OVERFLOW_SIZE,
                    )
                })
            },
        );
    }
}

fn benchmark_alloc<const B: BlockSize, const Z: BucketSize>(c: &mut Criterion) {
    let mut group = c.benchmark_group(String::from("PathOsamPlus") + "::alloc");
    let mut rng = StdRng::seed_from_u64(0);
    for capacity in CAPACITIES_TO_BENCHMARK.iter() {
        let mut osam_plus = PathOsamPlus::<BlockValue<B>, Z>::new_with_parameters(
            *capacity,
            DEFAULT_STASH_OVERFLOW_SIZE,
        )
        .unwrap();
        group.bench_function(
            BenchmarkId::from_parameter(ReadWriteParameters {
                capacity: *capacity,
                block_size: mem::size_of::<BlockValue<B>>(),
            }),
            |b| b.iter(|| osam_plus.alloc(&mut rng)),
        );
    }
}

fn benchmark_alloc_and_read<const B: BlockSize, const Z: BucketSize>(c: &mut Criterion) {
    let mut group = c.benchmark_group(String::from("PathOsamPlus") + "::read");
    let mut rng = StdRng::seed_from_u64(0);
    for capacity in CAPACITIES_TO_BENCHMARK.iter() {
        let mut osam_plus = PathOsamPlus::<BlockValue<B>, Z>::new_with_parameters(
            *capacity,
            DEFAULT_STASH_OVERFLOW_SIZE,
        )
        .unwrap();
        group.bench_function(
            BenchmarkId::from_parameter(ReadWriteParameters {
                capacity: *capacity,
                block_size: mem::size_of::<BlockValue<B>>(),
            }),
            |b| {
                let address = osam_plus.alloc(&mut rng).unwrap();
                b.iter(|| osam_plus.read(address.0, address.1));
            },
        );
    }
}

fn benchmark_read<const B: BlockSize, const Z: BucketSize>(c: &mut Criterion) {
    let mut group = c.benchmark_group(String::from("PathOsamPlus") + "::read");
    for capacity in CAPACITIES_TO_BENCHMARK.iter() {
        let mut osam_plus = PathOsamPlus::<BlockValue<B>, Z>::new_with_parameters(
            *capacity,
            DEFAULT_STASH_OVERFLOW_SIZE,
        )
        .unwrap();
        group.bench_function(
            BenchmarkId::from_parameter(ReadWriteParameters {
                capacity: *capacity,
                block_size: mem::size_of::<BlockValue<B>>(),
            }),
            |b| b.iter(|| osam_plus.read(1, *capacity - 1)),
        );
    }
}

fn benchmark_alloc_and_write<const B: BlockSize, const Z: BucketSize>(c: &mut Criterion) {
    let mut group = c.benchmark_group(String::from("PathOsamPlus") + "::write");
    let mut rng = StdRng::seed_from_u64(0);
    for capacity in CAPACITIES_TO_BENCHMARK.iter() {
        let mut osam_plus = PathOsamPlus::<BlockValue<B>, Z>::new_with_parameters(
            *capacity,
            DEFAULT_STASH_OVERFLOW_SIZE,
        )
        .unwrap();
        group.bench_function(
            BenchmarkId::from_parameter(ReadWriteParameters {
                capacity: *capacity,
                block_size: mem::size_of::<BlockValue<B>>(),
            }),
            |b| {
                let address = osam_plus.alloc(&mut rng).unwrap();
                b.iter(|| {
                    osam_plus.write(address.0, address.1, BlockValue::<B>::default(), &mut rng)
                });
            },
        );
    }
}

fn benchmark_write<const B: BlockSize, const Z: BucketSize>(c: &mut Criterion) {
    let mut group = c.benchmark_group(String::from("PathOsamPlus") + "::write");
    let mut rng = StdRng::seed_from_u64(0);
    for capacity in CAPACITIES_TO_BENCHMARK.iter() {
        let mut osam_plus = PathOsamPlus::<BlockValue<B>, Z>::new_with_parameters(
            *capacity,
            DEFAULT_STASH_OVERFLOW_SIZE,
        )
        .unwrap();
        group.bench_function(
            BenchmarkId::from_parameter(ReadWriteParameters {
                capacity: *capacity,
                block_size: mem::size_of::<BlockValue<B>>(),
            }),
            |b| b.iter(|| osam_plus.write(1, *capacity - 1, BlockValue::<B>::default(), &mut rng)),
        );
    }
}

fn benchmark_alloc_and_local_write<const B: BlockSize, const Z: BucketSize>(c: &mut Criterion) {
    let mut group = c.benchmark_group(String::from("PathOsamPlus") + "::local_write");
    let mut rng = StdRng::seed_from_u64(0);
    for capacity in CAPACITIES_TO_BENCHMARK.iter() {
        let mut osam_plus = PathOsamPlus::<BlockValue<B>, Z>::new_with_parameters(
            *capacity,
            DEFAULT_STASH_OVERFLOW_SIZE,
        )
        .unwrap();
        group.bench_function(
            BenchmarkId::from_parameter(ReadWriteParameters {
                capacity: *capacity,
                block_size: mem::size_of::<BlockValue<B>>(),
            }),
            |b| {
                let address = osam_plus.alloc(&mut rng).unwrap();
                b.iter(|| osam_plus.local_write(address.0, address.1, BlockValue::<B>::default()));
            },
        );
    }
}

fn benchmark_local_write<const B: BlockSize, const Z: BucketSize>(c: &mut Criterion) {
    let mut group = c.benchmark_group(String::from("PathOsamPlus") + "::local_write");
    for capacity in CAPACITIES_TO_BENCHMARK.iter() {
        let mut osam_plus = PathOsamPlus::<BlockValue<B>, Z>::new_with_parameters(
            *capacity,
            DEFAULT_STASH_OVERFLOW_SIZE,
        )
        .unwrap();
        group.bench_function(
            BenchmarkId::from_parameter(ReadWriteParameters {
                capacity: *capacity,
                block_size: mem::size_of::<BlockValue<B>>(),
            }),
            |b| b.iter(|| osam_plus.local_write(1, *capacity - 1, BlockValue::<B>::default())),
        );
    }
}

fn benchmark_random_operations<const B: BlockSize, const Z: BucketSize>(c: &mut Criterion) {
    let mut group = c.benchmark_group(String::from("PathOsamPlus") + "::random_operations");
    let mut rng = StdRng::seed_from_u64(0);

    for capacity in CAPACITIES_TO_BENCHMARK {
        let mut osam_plus = PathOsamPlus::<BlockValue<B>, Z>::new_with_parameters(
            capacity,
            DEFAULT_STASH_OVERFLOW_SIZE,
        )
        .unwrap();

        let number_of_operations_to_run = 64 as usize;

        let block_size = B;
        let parameters = &RandomOperationsParameters {
            capacity,
            block_size,
            number_of_operations_to_run,
        };

        let mut addresses: Vec<(Identifier, TreeIndex)> =
            vec![(Identifier::MAX, 0); number_of_operations_to_run];
        let mut read_versus_write_randomness = vec![false; number_of_operations_to_run];
        let capacity_usize: usize = capacity.try_into().unwrap();
        let mut value_randomness = vec![0u8; block_size * capacity_usize];
        for i in 0..number_of_operations_to_run {
            addresses[i] = osam_plus.alloc(&mut rng).unwrap();
        }

        rng.fill(&mut read_versus_write_randomness[..]);
        rng.fill(&mut value_randomness[..]);

        group.bench_with_input(
            BenchmarkId::from_parameter(parameters),
            parameters,
            |b, &parameters| {
                b.iter(|| {
                    run_many_random_accesses::<B, Z>(
                        &mut osam_plus,
                        parameters.number_of_operations_to_run,
                        black_box(&addresses),
                        black_box(&read_versus_write_randomness),
                        black_box(&value_randomness),
                    )
                })
            },
        );
    }
    group.finish();
}

fn run_many_random_accesses<const B: BlockSize, const Z: BucketSize>(
    osam_plus: &mut PathOsamPlus<BlockValue<B>, Z>,
    number_of_operations_to_run: usize,
    addresses: &[(Identifier, TreeIndex)],
    read_versus_write_randomness: &[bool],
    value_randomness: &[u8],
) {
    let mut rng = StdRng::seed_from_u64(0);
    for operation_number in 0..number_of_operations_to_run {
        let address = addresses[operation_number];
        let identifier = address.0;
        let position = address.1;
        let random_read_versus_write: bool = read_versus_write_randomness[operation_number];

        if random_read_versus_write {
            osam_plus.read(identifier, position).unwrap();
        } else {
            let block_size = B;
            let start_index = block_size * operation_number;
            let end_index = block_size + start_index;
            let random_bytes: [u8; B] =
                value_randomness[start_index..end_index].try_into().unwrap();
            let random_eviction = rng.gen_bool(0.5);
            if random_eviction {
                osam_plus
                    .write(
                        identifier,
                        position,
                        BlockValue::new(random_bytes),
                        &mut rng,
                    )
                    .unwrap();
            } else {
                osam_plus
                    .local_write(identifier, position, BlockValue::new(random_bytes))
                    .unwrap();
            }
        }
    }
}

#[derive(Clone, Copy)]
struct ReadWriteParameters {
    capacity: Identifier,
    block_size: usize,
}

impl fmt::Display for ReadWriteParameters {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "(Capacity: {} Blocksize: {})",
            self.capacity, self.block_size,
        )
    }
}

#[derive(Clone, Copy)]
struct RandomOperationsParameters {
    capacity: Identifier,
    block_size: usize,
    number_of_operations_to_run: usize,
}

impl fmt::Display for RandomOperationsParameters {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "(Capacity: {} Blocksize: {}, Ops: {})",
            self.capacity, self.block_size, self.number_of_operations_to_run,
        )
    }
}
