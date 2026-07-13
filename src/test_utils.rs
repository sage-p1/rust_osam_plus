// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! This module contains common test utilities for crates generating tests utilizing the
//! `osam_plus` crate.

use std::collections::HashMap;
use std::sync::Once;
static INIT: Once = Once::new();
use crate::path_osam_plus::PathOsamPlus;
use crate::{BucketSize, Identifier, OsamPlus, OsamPlusBlock, OsamPlusError, StashSize, TreeIndex};
use rand::{
    distributions::{Distribution, Standard},
    rngs::StdRng,
    CryptoRng, Rng, SeedableRng,
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

/// Tests the correctness of OSAM+ on a sequence of all writes then reads
pub(crate) fn write_then_read<T: OsamPlus>(osam_plus: &mut T, num_operations: usize)
where
    Standard: Distribution<T::V>,
{
    init_logger();
    let mut rng = StdRng::seed_from_u64(0);
    let mut mirror_hash_map = HashMap::new();

    // Generate a sequence of allocs and write a random value
    for _ in 0..num_operations {
        let address = osam_plus.alloc(&mut rng).unwrap();
        let identifier = address.0;
        let position = address.1;
        let random_block_value = rng.gen::<T::V>();
        mirror_hash_map.insert(address, random_block_value);
        let _ = osam_plus.write(identifier, position, random_block_value, &mut rng);
    }

    // Assert reads fetch the proper data block
    for (address, random_block_value) in mirror_hash_map.iter() {
        let identifier = address.0;
        let position = address.1;
        assert_eq!(
            osam_plus.read(identifier, position).unwrap().unwrap(),
            *random_block_value
        );
    }
}

/// Tests the correctness of Path OSAM+ on a sequence of all reads then writes
pub(crate) fn read_then_write<T: OsamPlus>(osam_plus: &mut T, num_operations: usize)
where
    Standard: Distribution<T::V>,
{
    init_logger();
    let mut rng = StdRng::seed_from_u64(0);

    // Generate a sequence of allocs and write a random value
    for _ in 0..num_operations {
        let address = osam_plus.alloc(&mut rng).unwrap();
        let identifier = address.0;
        let position = address.1;
        assert_eq!(osam_plus.read(identifier, position).unwrap(), None);
        let _ = osam_plus.write(identifier, position, T::V::default(), &mut rng);
    }
}

/// Tests the correctness of Path OSAM+ on a sequence where
/// 1) the first half of writes are made to the OSAM+
/// 2) read half of these writes (quarter of all writes)
/// 3) the second half of writes are done
/// 4) read all remaining writes
pub(crate) fn interspersed_write_and_read<T: OsamPlus>(osam_plus: &mut T, num_operations: usize)
where
    Standard: Distribution<T::V>,
{
    init_logger();
    let mut rng = StdRng::seed_from_u64(0);
    let mut mirror_hash_map = HashMap::new();

    let half = num_operations.checked_div(2).unwrap();
    let quarter = num_operations.checked_div(4).unwrap();

    // Generate the first half of allocs and write a random value
    for _ in 0..half {
        let address = osam_plus.alloc(&mut rng).unwrap();
        let identifier = address.0;
        let position = address.1;
        let random_block_value = rng.gen::<T::V>();
        mirror_hash_map.insert(address, random_block_value);
        let _ = osam_plus.write(identifier, position, random_block_value, &mut rng);
    }

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
        used_addresses.push(address.to_owned());
        counter += 1;
    }

    // Remove used addresses to avoid double reading
    for address in used_addresses.iter() {
        mirror_hash_map.remove(address);
    }

    // Generate the second half of allocs and write a random value
    for _ in half..num_operations {
        let address = osam_plus.alloc(&mut rng).unwrap();
        let identifier = address.0;
        let position = address.1;
        let random_block_value = rng.gen::<T::V>();
        mirror_hash_map.insert(address, random_block_value);
        let _ = osam_plus.write(identifier, position, random_block_value, &mut rng);
    }

    // Assert the remaining three quarters of reads are correct
    for (address, random_block_value) in mirror_hash_map.iter() {
        let identifier = address.0;
        let position = address.1;
        assert_eq!(
            osam_plus.read(identifier, position).unwrap().unwrap(),
            *random_block_value
        );
    }
}

/// Tests the correctness of Path OSAM+ where the values of an address are overwritten several times
pub(crate) fn overwrite_then_read<T: OsamPlus>(
    osam_plus: &mut T,
    num_operations: usize,
    overwrite_cycles: usize,
) where
    Standard: Distribution<T::V>,
{
    init_logger();
    let mut rng = StdRng::seed_from_u64(0);
    let mut mirror_hash_map = HashMap::new();

    // Generate a sequence of allocs and write a random value
    for _ in 0..num_operations {
        let address = osam_plus.alloc(&mut rng).unwrap();
        let identifier = address.0;
        let position = address.1;
        let random_block_value = rng.gen::<T::V>();
        mirror_hash_map.insert(address, random_block_value);
        let _ = osam_plus.write(identifier, position, random_block_value, &mut rng);
    }

    // Overwrite all addresses with new values
    for _ in 0..overwrite_cycles {
        for (address, random_block_value) in mirror_hash_map.iter_mut() {
            *random_block_value = rng.gen::<T::V>();
            let _ = osam_plus.write(address.0, address.1, *random_block_value, &mut rng);
        }
    }

    // Assert reads fetch the updated data block
    for (address, random_block_value) in mirror_hash_map.iter() {
        let identifier = address.0;
        let position = address.1;
        assert_eq!(
            osam_plus.read(identifier, position).unwrap().unwrap(),
            *random_block_value
        );
    }
}

/// Tests the correctness of PathOsamPlus on a sequence of all writes then reads
pub(crate) fn local_write_then_read<V: OsamPlusBlock, const Z: BucketSize>(
    osam_plus: &mut PathOsamPlus<V, Z>,
    num_operations: usize,
) where
    Standard: Distribution<V>,
{
    init_logger();
    let mut rng = StdRng::seed_from_u64(0);
    let mut mirror_hash_map = HashMap::new();

    // Generate a sequence of allocs and write a random value
    for _ in 0..num_operations {
        let address = osam_plus.alloc(&mut rng).unwrap();
        let identifier = address.0;
        let position = address.1;
        let random_block_value = rng.gen::<V>();
        mirror_hash_map.insert(address, random_block_value);
        let _ = osam_plus.local_write(identifier, position, random_block_value);
    }

    // Assert reads fetch the proper data block
    for (address, random_block_value) in mirror_hash_map.iter() {
        let identifier = address.0;
        let position = address.1;

        assert_eq!(
            osam_plus.read(identifier, position).unwrap().unwrap(),
            *random_block_value
        );
    }
}

/// Tests the correctness of Path OSAM+ on a sequence where
/// 1) the first quarter of writes are locally to the stash
/// 2) read all of these writes
/// 3) the last three quarters writes are made to the OSAM+
/// 4) read all remaining writes
pub(crate) fn locally_interspersed_write_and_read<V: OsamPlusBlock, const Z: BucketSize>(
    osam_plus: &mut PathOsamPlus<V, Z>,
    num_operations: usize,
) where
    Standard: Distribution<V>,
{
    init_logger();
    let mut rng = StdRng::seed_from_u64(0);
    let mut mirror_hash_map = HashMap::new();
    let quarter = num_operations.checked_div(4).unwrap();

    // Generate the first half of allocs and write a random value
    for _ in 0..quarter {
        let address = osam_plus.alloc(&mut rng).unwrap();
        let identifier = address.0;
        let position = address.1;
        let random_block_value = rng.gen::<V>();
        mirror_hash_map.insert(address, random_block_value);
        let _ = osam_plus.local_write(identifier, position, random_block_value);
    }

    // Assert reads fetch the proper data block for half the first writes (quarter of all)
    for (address, random_block_value) in mirror_hash_map.iter() {
        let identifier = address.0;
        let position = address.1;
        assert_eq!(
            osam_plus.read(identifier, position).unwrap().unwrap(),
            *random_block_value
        );
    }
    mirror_hash_map.clear();

    // Generate the second half of allocs and write a random value
    for _ in quarter..num_operations {
        let address = osam_plus.alloc(&mut rng).unwrap();
        let identifier = address.0;
        let position = address.1;
        let random_block_value = rng.gen::<V>();
        mirror_hash_map.insert(address, random_block_value);
        let _ = osam_plus.write(identifier, position, random_block_value, &mut rng);
    }

    // Assert the remaining three quarters of reads are correct
    for (address, random_block_value) in mirror_hash_map.iter() {
        let identifier = address.0;
        let position = address.1;
        assert_eq!(
            osam_plus.read(identifier, position).unwrap().unwrap(),
            *random_block_value
        );
    }
}

/// Tests the correctness of Path OSAM+ where the values of an address are locally overwritten several times
pub(crate) fn local_overwrite_then_read<V: OsamPlusBlock, const Z: BucketSize>(
    osam_plus: &mut PathOsamPlus<V, Z>,
    num_operations: usize,
) where
    Standard: Distribution<V>,
{
    init_logger();
    let mut rng = StdRng::seed_from_u64(0);
    let mut mirror_hash_map = HashMap::new();

    // Generate a sequence of allocs and write a random value
    for _ in 0..num_operations {
        let address = osam_plus.alloc(&mut rng).unwrap();
        let identifier = address.0;
        let position = address.1;
        let random_block_value = rng.gen::<V>();
        mirror_hash_map.insert(address, random_block_value);
        let _ = osam_plus.local_write(identifier, position, random_block_value);
    }

    // Overwrite all addresses with new values
    for (address, random_block_value) in mirror_hash_map.iter_mut() {
        *random_block_value = rng.gen::<V>();
        let _ = osam_plus.local_write(address.0, address.1, *random_block_value);
    }

    // Assert reads fetch the updated data block
    for (address, random_block_value) in mirror_hash_map.iter() {
        let identifier = address.0;
        let position = address.1;
        assert_eq!(
            osam_plus.read(identifier, position).unwrap().unwrap(),
            *random_block_value,
        );
    }
}

macro_rules! create_path_osam_plus_correctness_tests_all_parameters {
    ($prefix: literal, $block_capacity: expr, $block_size: expr, $bucket_size: expr, $overflow_size: expr, $operation_factor: expr) => {
        paste::paste! {
            #[test]
            fn [<"write_then_read" $prefix $block_capacity _ $block_size _ $bucket_size _ $overflow_size _ $operation_factor>]() {
                let mut osam_plus = PathOsamPlus::<BlockValue<$block_size>, $bucket_size>::new_with_parameters($block_capacity, $overflow_size).unwrap();
                let num_operations = osam_plus.block_capacity() * $bucket_size * $operation_factor + usize::try_from($overflow_size).unwrap().checked_div(2).unwrap();
                write_then_read(&mut osam_plus, num_operations);
            }

            #[test]
            fn [<"read_then_write" $prefix $block_capacity _ $block_size _ $bucket_size _ $overflow_size _ $operation_factor>]() {
                let mut osam_plus = PathOsamPlus::<BlockValue<$block_size>, $bucket_size>::new_with_parameters($block_capacity, $overflow_size).unwrap();
                let num_operations = osam_plus.block_capacity() * $bucket_size * $operation_factor + usize::try_from($overflow_size).unwrap().checked_div(2).unwrap();
                read_then_write(&mut osam_plus, num_operations);
            }

            #[test]
            fn [<"interspersed_write_and_read" $prefix $block_capacity _ $block_size _ $bucket_size _ $overflow_size _ $operation_factor>]() {
                let mut osam_plus = PathOsamPlus::<BlockValue<$block_size>, $bucket_size>::new_with_parameters($block_capacity, $overflow_size).unwrap();
                let num_operations = osam_plus.block_capacity() * $bucket_size * $operation_factor + usize::try_from($overflow_size).unwrap().checked_div(2).unwrap();
                interspersed_write_and_read(&mut osam_plus, num_operations);
            }

            #[test]
            fn [<"overwrite_then_read" $prefix $block_capacity _ $block_size _ $bucket_size _ $overflow_size _ $operation_factor>]() {
                let mut osam_plus = PathOsamPlus::<BlockValue<$block_size>, $bucket_size>::new_with_parameters($block_capacity, $overflow_size).unwrap();
                let num_operations = osam_plus.block_capacity() * $bucket_size * $operation_factor + usize::try_from($overflow_size).unwrap().checked_div(2).unwrap();
                let overwrite_cycles = $operation_factor;
                overwrite_then_read(&mut osam_plus, num_operations, overwrite_cycles);
            }

            #[test]
            fn [<"local_write_then_read" $prefix $block_capacity _ $block_size _ $bucket_size _ $overflow_size _ $operation_factor>]() {
                let mut osam_plus = PathOsamPlus::<BlockValue<$block_size>, $bucket_size>::new_with_parameters($block_capacity, $overflow_size).unwrap();
                let num_operations = osam_plus.block_capacity() * $bucket_size * $operation_factor + usize::try_from($overflow_size).unwrap().checked_div(2).unwrap();
                local_write_then_read(&mut osam_plus, num_operations);
            }

            #[test]
            fn [<"locally_interspersed_write_and_read" $prefix $block_capacity _ $block_size _ $bucket_size _ $overflow_size _ $operation_factor>]() {
                let mut osam_plus = PathOsamPlus::<BlockValue<$block_size>, $bucket_size>::new_with_parameters($block_capacity, $overflow_size).unwrap();
                let num_operations = osam_plus.block_capacity() * $bucket_size * $operation_factor + usize::try_from($overflow_size).unwrap().checked_div(2).unwrap();
                locally_interspersed_write_and_read(&mut osam_plus, num_operations);
            }

            #[test]
            fn [<"local_overwrite_then_read" $prefix $block_capacity _ $block_size _ $bucket_size _ $overflow_size _ $operation_factor>]() {
                let mut osam_plus = PathOsamPlus::<BlockValue<$block_size>, $bucket_size>::new_with_parameters($block_capacity, $overflow_size).unwrap();
                let num_operations = osam_plus.block_capacity() * $bucket_size * $operation_factor + usize::try_from($overflow_size).unwrap().checked_div(2).unwrap();
                local_overwrite_then_read(&mut osam_plus, num_operations);
            }
        }
    };
}

macro_rules! create_path_osam_plus_stash_size_correctness_tests_all_parameters {
    ($prefix: literal, $block_capacity: expr, $block_size: expr, $bucket_size: expr, $overflow_size: expr) => {
        paste::paste! {
            #[test]
            fn [<"write_then_read" $prefix $block_capacity _ $block_size _ $bucket_size _ $overflow_size>]() {
                let mut osam_plus = StashSizeMonitor::<BlockValue<$block_size>, $bucket_size>::new_with_parameters($block_capacity, $overflow_size).unwrap();
                let num_operations = (osam_plus.block_capacity() * $bucket_size).checked_div(2).unwrap();
                write_then_read(&mut osam_plus, num_operations);
            }

            #[test]
            fn [<"read_then_write" $prefix $block_capacity _ $block_size _ $bucket_size _ $overflow_size>]() {
                let mut osam_plus = StashSizeMonitor::<BlockValue<$block_size>, $bucket_size>::new_with_parameters($block_capacity, $overflow_size).unwrap();
                let num_operations = (osam_plus.block_capacity() * $bucket_size).checked_div(2).unwrap();
                read_then_write(&mut osam_plus, num_operations);
            }

            #[test]
            fn [<"interspersed_write_and_read" $prefix $block_capacity _ $block_size _ $bucket_size _ $overflow_size>]() {
                let mut osam_plus = StashSizeMonitor::<BlockValue<$block_size>, $bucket_size>::new_with_parameters($block_capacity, $overflow_size).unwrap();
                let num_operations = (osam_plus.block_capacity() * $bucket_size).checked_div(2).unwrap();
                interspersed_write_and_read(&mut osam_plus, num_operations);
            }
        }
    };
}

macro_rules! create_path_osam_plus_correctness_tests_helper {
    ($prefix: literal, $bucket_size: expr, $overflow_size: expr) => {
        create_path_osam_plus_correctness_tests_all_parameters!(
            $prefix,
            8,
            1,
            $bucket_size,
            $overflow_size,
            1
        );
        create_path_osam_plus_correctness_tests_all_parameters!(
            $prefix,
            4,
            1,
            $bucket_size,
            $overflow_size,
            1
        );
        // Block size 4 blocks, block size 2 bytes, testing with 100 operations
        create_path_osam_plus_correctness_tests_all_parameters!(
            $prefix,
            4,
            2,
            $bucket_size,
            $overflow_size,
            2
        );
        create_path_osam_plus_correctness_tests_all_parameters!(
            $prefix,
            16,
            1,
            $bucket_size,
            $overflow_size,
            3
        );
        create_path_osam_plus_correctness_tests_all_parameters!(
            $prefix,
            2,
            1,
            $bucket_size,
            $overflow_size,
            1
        );
    };
}

macro_rules! create_path_osam_plus_stash_size_correctness_tests_helper {
    ($prefix: literal, $bucket_size: expr, $overflow_size: expr) => {
        create_path_osam_plus_stash_size_correctness_tests_all_parameters!(
            $prefix,
            8,
            1,
            $bucket_size,
            $overflow_size
        );
        create_path_osam_plus_stash_size_correctness_tests_all_parameters!(
            $prefix,
            4,
            1,
            $bucket_size,
            $overflow_size
        );
        // Block size 4 blocks, block size 2 bytes, testing with 100 operations
        create_path_osam_plus_stash_size_correctness_tests_all_parameters!(
            $prefix,
            4,
            2,
            $bucket_size,
            $overflow_size
        );
        create_path_osam_plus_stash_size_correctness_tests_all_parameters!(
            $prefix,
            16,
            1,
            $bucket_size,
            $overflow_size
        );
        create_path_osam_plus_stash_size_correctness_tests_all_parameters!(
            $prefix,
            2,
            1,
            $bucket_size,
            $overflow_size
        );
    };
}

macro_rules! create_path_osam_plus_correctness_tests {
    ($bucket_size: expr, $overflow_size: expr) => {
        create_path_osam_plus_correctness_tests_helper!("_", $bucket_size, $overflow_size);
    };
}

macro_rules! create_path_osam_plus_stash_size_correctness_tests {
    ($bucket_size: expr, $overflow_size: expr) => {
        create_path_osam_plus_stash_size_correctness_tests_helper!(
            "_stash_size_",
            $bucket_size,
            $overflow_size
        );
    };
}

// Interface that shares OsamPlus trait to ensure the stash does not overflow with small enough parameters
#[derive(Debug)]
pub(crate) struct StashSizeMonitor<V: OsamPlusBlock, const Z: BucketSize> {
    osam_plus: PathOsamPlus<V, Z>,
}

impl<V: OsamPlusBlock, const Z: BucketSize> StashSizeMonitor<V, Z> {
    pub(crate) fn new_with_parameters(
        block_capacity: Identifier,
        overflow_size: StashSize,
    ) -> Result<Self, OsamPlusError> {
        Ok(Self {
            osam_plus: PathOsamPlus::new_with_parameters(block_capacity, overflow_size).unwrap(),
        })
    }
}

impl<V: OsamPlusBlock, const Z: BucketSize> OsamPlus for StashSizeMonitor<V, Z> {
    type V = V;

    fn block_capacity(&self) -> usize {
        self.osam_plus.block_capacity()
    }

    fn alloc<R: Rng + CryptoRng>(
        &mut self,
        rng: &mut R,
    ) -> Result<(Identifier, TreeIndex), OsamPlusError> {
        Ok(self.osam_plus.alloc(rng)?)
    }

    fn write<R: Rng + CryptoRng>(
        &mut self,
        identifier: Identifier,
        position: TreeIndex,
        value: V,
        rng: &mut R,
    ) -> Result<(), OsamPlusError> {
        let _ = self.osam_plus.write(identifier, position, value, rng)?;
        let stash_size = self.osam_plus.stash_occupancy();
        assert!(stash_size < 10);
        Ok(())
    }

    fn read(
        &mut self,
        identifier: Identifier,
        position: TreeIndex,
    ) -> Result<Option<V>, OsamPlusError> {
        let result = self.osam_plus.read(identifier, position)?;
        let stash_size = self.osam_plus.stash_occupancy();
        assert!(stash_size < 10);
        Ok(result)
    }
}

pub(crate) use create_path_osam_plus_correctness_tests;
pub(crate) use create_path_osam_plus_correctness_tests_all_parameters;
pub(crate) use create_path_osam_plus_correctness_tests_helper;
pub(crate) use create_path_osam_plus_stash_size_correctness_tests;
pub(crate) use create_path_osam_plus_stash_size_correctness_tests_all_parameters;
pub(crate) use create_path_osam_plus_stash_size_correctness_tests_helper;
