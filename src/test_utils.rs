// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! This module contains common test utilities for crates generating tests utilizing the
//! `osam_plus` crate.

use std::collections::HashMap;
use std::ops::Div;
use std::sync::Once;
static INIT: Once = Once::new();
use crate::path_osam_plus::PathOsam;
use crate::{BucketSize, OsamPlusBlock, StashSize};
use rand::{
    distributions::{Distribution, Standard},
    rngs::StdRng,
    Rng, SeedableRng,
};
use simplelog::{Config, WriteLogger};

// For use in manual testing and inspection.
// Change log_level to "Warn" to see stash overflow events, and to "Debug" to additionally see OSAM+ initialization events.
pub(crate) fn init_logger() {
    INIT.call_once(|| {
        WriteLogger::init(
            log::LevelFilter::Error,
            Config::default(),
            std::io::stdout(),
        )
        .unwrap()
    })
}

pub(crate) fn stash_checker<V: OsamPlusBlock, const Z: BucketSize>(
    stash_size_experiment: bool,
    osam_plus: &mut PathOsam<V, Z>,
) {
    if stash_size_experiment {
        assert!(osam_plus.stash_occupancy() < 10);
    }
}

/// Tests the correctness of PathOsam on a sequence of all writes then reads
pub(crate) fn write_then_read<V: OsamPlusBlock, const Z: BucketSize>(
    osam_plus: &mut PathOsam<V, Z>,
    bucket_size: BucketSize,
    operation_factor: usize,
    overflow_size: StashSize,
    stash_size_experiment: bool,
) where
    Standard: Distribution<V>,
{
    init_logger();
    let mut rng = StdRng::seed_from_u64(0);

    let capacity = osam_plus.block_capacity();
    let mut num_operations: usize;
    if stash_size_experiment {
        num_operations = (capacity * bucket_size).div(2);
    } else {
        num_operations = capacity * bucket_size * operation_factor;
        num_operations += usize::try_from(overflow_size).unwrap().div(2);
    }

    let mut mirror_hash_map = HashMap::new();

    // Generate a sequence of allocs and write a random value
    for _ in 0..num_operations {
        let address = osam_plus.alloc(&mut rng).unwrap();
        let identifier = address.0;
        let position = address.1;
        let random_block_value = rng.gen::<V>();
        mirror_hash_map.insert(address, random_block_value);
        let _ = osam_plus.write(identifier, position, random_block_value, &mut rng);
        stash_checker(stash_size_experiment, osam_plus);
    }

    assert_eq!(
        osam_plus.write_counter(),
        StashSize::try_from(osam_plus.alloc_counter()).unwrap()
    );

    // Assert reads fetch the proper data block
    for (address, random_block_value) in mirror_hash_map.iter() {
        let identifier = address.0;
        let position = address.1;
        assert_eq!(
            osam_plus.read(identifier, position).unwrap().unwrap(),
            *random_block_value
        );
        stash_checker(stash_size_experiment, osam_plus);
    }

    assert_eq!(
        osam_plus.write_counter(),
        StashSize::try_from(osam_plus.alloc_counter()).unwrap()
    );
    assert_eq!(osam_plus.write_counter(), osam_plus.read_counter());
}

/// Tests the correctness of Path OSAM+ on a sequence of all reads then writes
pub(crate) fn read_then_write<V: OsamPlusBlock, const Z: BucketSize>(
    osam_plus: &mut PathOsam<V, Z>,
    bucket_size: BucketSize,
    operation_factor: usize,
    overflow_size: StashSize,
    stash_size_experiment: bool,
) where
    Standard: Distribution<V>,
{
    init_logger();
    let mut rng = StdRng::seed_from_u64(0);

    let capacity = osam_plus.block_capacity();
    let mut num_operations: usize;
    if stash_size_experiment {
        num_operations = (capacity * bucket_size).div(2);
    } else {
        num_operations = capacity * bucket_size * operation_factor;
        num_operations += usize::try_from(overflow_size).unwrap().div(2);
    }

    // Generate a sequence of allocs and write a random value
    for _ in 0..num_operations {
        let address = osam_plus.alloc(&mut rng).unwrap();
        let identifier = address.0;
        let position = address.1;
        assert_eq!(osam_plus.read(identifier, position).unwrap(), None);
        stash_checker(stash_size_experiment, osam_plus);
        let _ = osam_plus.write(identifier, position, V::default(), &mut rng);
        stash_checker(stash_size_experiment, osam_plus);
    }

    assert_eq!(
        osam_plus.read_counter(),
        StashSize::try_from(osam_plus.alloc_counter()).unwrap()
    );
    assert_eq!(osam_plus.read_counter(), osam_plus.write_counter());
}

/// Tests the correctness of Path OSAM+ on a sequence where
/// 1) the first half of writes are made to the OSAM+
/// 2) read half of these writes (quarter of all writes)
/// 3) the second half of writes are done
/// 4) read all remaining writes
pub(crate) fn interspersed_write_and_read<V: OsamPlusBlock, const Z: BucketSize>(
    osam_plus: &mut PathOsam<V, Z>,
    bucket_size: BucketSize,
    operation_factor: usize,
    overflow_size: StashSize,
    stash_size_experiment: bool,
) where
    Standard: Distribution<V>,
{
    init_logger();
    let mut rng = StdRng::seed_from_u64(0);

    let capacity = osam_plus.block_capacity();
    let mut num_operations: usize;
    if stash_size_experiment {
        num_operations = (capacity * bucket_size).div(2);
    } else {
        num_operations = capacity * bucket_size * operation_factor;
        num_operations += usize::try_from(overflow_size).unwrap().div(2);
    }
    let half = num_operations.div(2);
    let quarter = num_operations.div(4);

    let mut mirror_hash_map = HashMap::new();

    // Generate the first half of allocs and write a random value
    for _ in 0..half {
        let address = osam_plus.alloc(&mut rng).unwrap();
        let identifier = address.0;
        let position = address.1;
        let random_block_value = rng.gen::<V>();
        mirror_hash_map.insert(address, random_block_value);
        let _ = osam_plus.write(identifier, position, random_block_value, &mut rng);
        stash_checker(stash_size_experiment, osam_plus);
    }

    assert_eq!(
        osam_plus.write_counter(),
        StashSize::try_from(osam_plus.alloc_counter()).unwrap()
    );

    // Assert reads fetch the proper data block for half the first writes (quarter of all)
    let mut used_addresses = Vec::new();
    let mut counter = 0;
    for (address, random_block_value) in mirror_hash_map.iter() {
        if counter >= quarter {
            break;
        }
        let identifier = address.0;
        let position = address.1;
        assert_eq!(
            osam_plus.read(identifier, position).unwrap().unwrap(),
            *random_block_value
        );
        stash_checker(stash_size_experiment, osam_plus);
        used_addresses.push(address.to_owned());
        counter += 1;
    }

    // Remove used addresses to avoid double reading
    for address in used_addresses.iter() {
        mirror_hash_map.remove(address);
    }

    assert_eq!(
        osam_plus.read_counter(),
        StashSize::try_from(quarter).unwrap()
    );

    // Generate the second half of allocs and write a random value
    for _ in half..num_operations {
        let address = osam_plus.alloc(&mut rng).unwrap();
        let identifier = address.0;
        let position = address.1;
        let random_block_value = rng.gen::<V>();
        mirror_hash_map.insert(address, random_block_value);
        let _ = osam_plus.write(identifier, position, random_block_value, &mut rng);
        stash_checker(stash_size_experiment, osam_plus);
    }

    // Assert the remaining three quarters of reads are correct
    for (address, random_block_value) in mirror_hash_map.iter() {
        let identifier = address.0;
        let position = address.1;
        assert_eq!(
            osam_plus.read(identifier, position).unwrap().unwrap(),
            *random_block_value
        );
        stash_checker(stash_size_experiment, osam_plus);
    }

    assert_eq!(
        osam_plus.write_counter(),
        StashSize::try_from(osam_plus.alloc_counter()).unwrap()
    );
    assert_eq!(osam_plus.write_counter(), osam_plus.read_counter());
}

/// Tests the correctness of Path OSAM+ on a sequence where
/// 1) the first quarter of writes are locally to the stash
/// 2) read all of these writes
/// 3) the last three quarters writes are made to the OSAM+
/// 4) read all remaining writes
pub(crate) fn locally_interspersed_write_and_read<V: OsamPlusBlock, const Z: BucketSize>(
    osam_plus: &mut PathOsam<V, Z>,
    bucket_size: BucketSize,
    operation_factor: usize,
    overflow_size: StashSize,
    stash_size_experiment: bool,
) where
    Standard: Distribution<V>,
{
    init_logger();
    let mut rng = StdRng::seed_from_u64(0);

    let capacity = osam_plus.block_capacity();
    let mut num_operations: usize;
    if stash_size_experiment {
        num_operations = (capacity * bucket_size).div(2);
    } else {
        num_operations = capacity * bucket_size * operation_factor;
        num_operations += usize::try_from(overflow_size).unwrap().div(2);
    }
    let quarter = num_operations.div(4);

    let mut mirror_hash_map = HashMap::new();

    // Generate the first half of allocs and write a random value
    for _ in 0..quarter {
        let address = osam_plus.alloc(&mut rng).unwrap();
        let identifier = address.0;
        let position = address.1;
        let random_block_value = rng.gen::<V>();
        mirror_hash_map.insert(address, random_block_value);
        let _ = osam_plus.local_write(identifier, position, random_block_value);
        stash_checker(stash_size_experiment, osam_plus);
    }

    assert_eq!(
        osam_plus.local_write_counter(),
        StashSize::try_from(osam_plus.alloc_counter()).unwrap()
    );
    assert_eq!(osam_plus.write_counter(), 0);
    assert_eq!(osam_plus.round_trip_counter(), 0);

    // Assert reads fetch the proper data block for half the first writes (quarter of all)
    for (address, random_block_value) in mirror_hash_map.iter() {
        let identifier = address.0;
        let position = address.1;
        assert_eq!(
            osam_plus.read(identifier, position).unwrap().unwrap(),
            *random_block_value
        );
        stash_checker(stash_size_experiment, osam_plus);
    }
    mirror_hash_map.clear();

    assert_eq!(
        osam_plus.read_counter(),
        StashSize::try_from(quarter).unwrap()
    );

    // Generate the second half of allocs and write a random value
    for _ in quarter..num_operations {
        let address = osam_plus.alloc(&mut rng).unwrap();
        let identifier = address.0;
        let position = address.1;
        let random_block_value = rng.gen::<V>();
        mirror_hash_map.insert(address, random_block_value);
        let _ = osam_plus.write(identifier, position, random_block_value, &mut rng);
        stash_checker(stash_size_experiment, osam_plus);
    }

    // Assert the remaining three quarters of reads are correct
    for (address, random_block_value) in mirror_hash_map.iter() {
        let identifier = address.0;
        let position = address.1;
        assert_eq!(
            osam_plus.read(identifier, position).unwrap().unwrap(),
            *random_block_value
        );
        stash_checker(stash_size_experiment, osam_plus);
    }

    assert_eq!(
        osam_plus.local_write_counter() + osam_plus.write_counter(),
        StashSize::try_from(osam_plus.alloc_counter()).unwrap()
    );
    assert_eq!(
        osam_plus.local_write_counter() + osam_plus.write_counter(),
        osam_plus.read_counter()
    );
}

macro_rules! create_path_osam_plus_correctness_tests_all_parameters {
    ($osam_plus_type: ident, $prefix: literal, $block_capacity: expr, $block_size: expr, $bucket_size: expr, $overflow_size: expr, $operation_factor: expr, $stash_size_experiment: expr) => {
        paste::paste! {
            #[test]
            fn [<"write_then_read" $prefix $block_capacity _ $block_size _ $bucket_size _ $overflow_size _ $operation_factor _ $stash_size_experiment>]() {
                let mut osam_plus = $osam_plus_type::<BlockValue<$block_size>, $bucket_size>::new_with_parameters($block_capacity, $overflow_size).unwrap();
                write_then_read(&mut osam_plus, $bucket_size, $operation_factor, $overflow_size, $stash_size_experiment);
            }

            #[test]
            fn [<"read_then_write" $prefix $block_capacity _ $block_size _ $bucket_size _ $overflow_size _ $operation_factor _ $stash_size_experiment>]() {
                let mut osam_plus = $osam_plus_type::<BlockValue<$block_size>, $bucket_size>::new_with_parameters($block_capacity, $overflow_size).unwrap();
                read_then_write(&mut osam_plus, $bucket_size, $operation_factor, $overflow_size, $stash_size_experiment);
            }

            #[test]
            fn [<"interspersed_write_and_read" $prefix $block_capacity _ $block_size _ $bucket_size _ $overflow_size _ $operation_factor _ $stash_size_experiment>]() {
                let mut osam_plus = $osam_plus_type::<BlockValue<$block_size>, $bucket_size>::new_with_parameters($block_capacity, $overflow_size).unwrap();
                interspersed_write_and_read(&mut osam_plus, $bucket_size, $operation_factor, $overflow_size, $stash_size_experiment);
            }

            #[test]
            fn [<"locally_interspersed_write_and_read" $prefix $block_capacity _ $block_size _ $bucket_size _ $overflow_size _ $operation_factor _ $stash_size_experiment>]() {
                let mut osam_plus = $osam_plus_type::<BlockValue<$block_size>, $bucket_size>::new_with_parameters($block_capacity, $overflow_size).unwrap();
                locally_interspersed_write_and_read(&mut osam_plus, $bucket_size, $operation_factor, $overflow_size, $stash_size_experiment);
            }
        }
    };
}

macro_rules! create_path_osam_plus_correctness_tests_helper {
    ($osam_plus_type: ident, $prefix: literal, $bucket_size: expr, $overflow_size: expr, $stash_size_experiment: expr) => {
        create_path_osam_plus_correctness_tests_all_parameters!(
            $osam_plus_type,
            $prefix,
            8,
            1,
            $bucket_size,
            $overflow_size,
            1,
            $stash_size_experiment
        );
        create_path_osam_plus_correctness_tests_all_parameters!(
            $osam_plus_type,
            $prefix,
            4,
            1,
            $bucket_size,
            $overflow_size,
            1,
            $stash_size_experiment
        );
        // Block size 4 blocks, block size 2 bytes, testing with 100 operations
        create_path_osam_plus_correctness_tests_all_parameters!(
            $osam_plus_type,
            $prefix,
            4,
            2,
            $bucket_size,
            $overflow_size,
            2,
            $stash_size_experiment
        );
        create_path_osam_plus_correctness_tests_all_parameters!(
            $osam_plus_type,
            $prefix,
            16,
            1,
            $bucket_size,
            $overflow_size,
            3,
            $stash_size_experiment
        );
        create_path_osam_plus_correctness_tests_all_parameters!(
            $osam_plus_type,
            $prefix,
            2,
            1,
            $bucket_size,
            $overflow_size,
            1,
            $stash_size_experiment
        );
    };
}

macro_rules! create_path_osam_plus_correctness_tests {
    ($bucket_size: expr, $overflow_size: expr) => {
        create_path_osam_plus_correctness_tests_helper!(
            PathOsam,
            "_",
            $bucket_size,
            $overflow_size,
            false
        );
    };
}

macro_rules! create_path_osam_plus_stash_size_tests {
    ($bucket_size: expr, $overflow_size: expr) => {
        create_path_osam_plus_correctness_tests_helper!(
            PathOsam,
            "_stash_size_",
            $bucket_size,
            $overflow_size,
            true
        );
    };
}

pub(crate) use create_path_osam_plus_correctness_tests;
pub(crate) use create_path_osam_plus_correctness_tests_all_parameters;
pub(crate) use create_path_osam_plus_correctness_tests_helper;
pub(crate) use create_path_osam_plus_stash_size_tests;
