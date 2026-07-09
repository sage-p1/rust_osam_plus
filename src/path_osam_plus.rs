// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! An implementation of Path OSAM+.

use super::stash::ObliviousStash;
use crate::{
    bucket::Bucket,
    utils::{CompleteBinaryTreeIndex, TreeHeight, TreeIndex},
    BucketSize, CounterSize, Identifier, OsamPlus, OsamPlusBlock, OsamPlusError, StashSize,
};
use bit_reverse::ParallelReverse;
use rand::{CryptoRng, Rng};
use std::collections::HashMap;

/// The parameter "Z" from the Path OSAM+ literature that sets the number of blocks per bucket; typical values are 3 or 4.
/// Here we adopt the more conservative setting of 4.
pub const DEFAULT_BLOCKS_PER_BUCKET: BucketSize = 4;

/// The default number of overflow blocks that the Path OSAM+ stash (and recursive stashes) can store.
pub const DEFAULT_STASH_OVERFLOW_SIZE: StashSize = 40;

/// A doubly oblivious Path OSAM+.
///
/// ## Parameters
///
/// - Block type `V`: the type of elements stored by the OSAM+.
/// - Bucket size `Z`: the number of blocks per Path OSAM+ bucket.
///   Must be at least 2. Typical values are 3, 4, or 5.
///   Along with the overflow size, this value affects the probability
///   of stash overflow (see below) and should be set with care.
/// - Overflow size: The number of blocks that the stash can store between OSAM+ accesses without overflowing.
///   Along with the bucket size, this value affects the probability of stash overflow (see below)
///   and should be set with care.
///
/// ## Security
///
/// OSAM+ operations are guaranteed to be oblivious, *unless* the stash overflows.
/// In this case, the stash will grow, which reveals that the overflow occurred.
/// This is a violation of obliviousness, but a mild one in several ways.
/// The stash overflow is very likely to reset to empty after the overflow,
/// and stash overflows are isolated events. It is not at all obvious
/// how an attacker might use a stash overflow to infer properties of the access pattern.
///
/// That said, it is best to choose parameters so that the stash does not ever overflow.
/// With Z = 4, experiments from the [original Path ORAM paper](https://eprint.iacr.org/2013/280.pdf)
/// indicate that the probability of overflow is independent of the number N of blocks stored,
/// and that setting SO = 40 is enough to reduce this probability to below 2^{-50} (Figure 3).
/// The authors conservatively estimate that setting SO = 89 suffices for 2^{-80} overflow probability.
/// The choice Z = 3 is also popular, although the probability of overflow is less well understood.
#[derive(Debug)]
pub struct PathOsamPlus<V: OsamPlusBlock, const Z: BucketSize> {
    /// The underlying untrusted memory that the OSAM+ is obliviously accessing on behalf of its client.
    physical_memory: Vec<Bucket<V, Z>>,
    /// The Path OSAM+ stash.
    stash: ObliviousStash<V>,
    /// The height of the Path OSAM+ tree data structure.
    height: TreeHeight,
    /// The counter that assigns identifiers to Path OSAM+ blocks.
    // Also serves as the alloc counter.
    identifier_counter: Identifier,
    /// The counter that deterministically picks which path evict.
    evict_counter: CounterSize,
    /// The maximum occupancy (number of real blocks) observed in the stash at once.
    max_occupancy: StashSize,
    /// A mapping of occupancies to the number of occurrences.
    all_occupancies: HashMap<StashSize, StashSize>,
    /// The counter tracking the number of writes made with eviction.
    write_counter: CounterSize,
    /// The counter tracking the number of writes made without eviction.
    local_write_counter: CounterSize,
    /// The counter tracking the number of reads.
    read_counter: CounterSize,
    /// The counter tracking the number of round-trips, which is a defined as one
    /// instance of reading a path and then writing a path. This should equal
    /// `write_counter` + `read_counter`
    round_trip_counter: CounterSize,
}

impl<V: OsamPlusBlock, const Z: BucketSize> PathOsamPlus<V, Z> {
    /// Returns a new `PathOsamPlus` of default `V` values
    /// with a stash overflow size of `overflow_size` blocks
    /// (See [`PathOsamPlus`]) for a description of these parameters).
    ///
    /// # Errors
    ///
    /// Returns an `InvalidConfigurationError` in the following cases.
    ///
    /// - `block_capacity` is 0, 1, or is not a power of two.
    /// - `Z` is 0 or 1.
    /// - `overflow_size` is 0.
    pub fn new_with_parameters(
        block_capacity: Identifier,
        overflow_size: StashSize,
    ) -> Result<Self, OsamPlusError> {
        log::info!("PathOsamPlus::new(capacity = {})", block_capacity,);

        if !block_capacity.is_power_of_two() | (block_capacity <= 1) {
            return Err(OsamPlusError::InvalidConfigurationError {
                parameter_name: "OSAM+ capacity".to_string(),
                parameter_value: block_capacity.to_string(),
            });
        }

        if Z <= 1 {
            return Err(OsamPlusError::InvalidConfigurationError {
                parameter_name: "Bucket size Z".to_string(),
                parameter_value: Z.to_string(),
            });
        }

        if overflow_size == 0 {
            return Err(OsamPlusError::InvalidConfigurationError {
                parameter_name: "Overflow size".to_string(),
                parameter_value: overflow_size.to_string(),
            });
        }

        let number_of_nodes = block_capacity;
        let height: u64 = (block_capacity.ilog2() - 1).into();
        let path_size = u64::try_from(Z)? * (height + 1);
        let stash = ObliviousStash::new(path_size, overflow_size)?;

        // physical_memory holds `block_capacity` buckets, each storing up to Z blocks.
        // The number of leaves is `block_capacity` / 2, which the original Path OSAM+ paper's experiments
        // found was sufficient to keep the stash size small with high probability.
        let mut physical_memory = Vec::new();
        physical_memory.resize(usize::try_from(number_of_nodes)?, Bucket::<V, Z>::default());

        // Initialize other parameters
        let identifier_counter: Identifier = 1;
        let evict_counter: CounterSize = 0;
        let max_occupancy: StashSize = 0;
        let all_occupancies: HashMap<StashSize, StashSize> = HashMap::new();
        let write_counter: CounterSize = 0;
        let local_write_counter: CounterSize = 0;
        let read_counter: CounterSize = 0;
        let round_trip_counter: CounterSize = 0;

        Ok(Self {
            physical_memory,
            stash,
            height,
            identifier_counter,
            evict_counter,
            max_occupancy,
            all_occupancies,
            write_counter,
            local_write_counter,
            read_counter,
            round_trip_counter,
        })
    }

    /// Locally writes the value stored `identifier` and `position` to stash. Does not evict to server.
    pub fn local_write(
        &mut self,
        identifier: Identifier,
        position: TreeIndex,
        value: V,
    ) -> Result<(), OsamPlusError> {
        assert_ne!(identifier, Identifier::MAX);
        assert!(position.is_leaf(self.height));

        // Add new block to stash by replacing a dummy block
        // Do this locally without interacting with the server
        self.stash.write_to_stash(identifier, position, value)?;

        // Remove duplicates
        let _ = self.stash.local_merge();

        // Bookkeeping of OSAM+ stats
        self.update_stash_stats();
        self.local_write_counter += 1;

        Ok(())
    }

    /// Calculates the next position to evict
    fn evict_position(&mut self) -> Result<TreeIndex, OsamPlusError> {
        // Deterministically evict buckets in reverse-lexicographic ordering
        let mut evict_position: TreeIndex = self.evict_counter;
        let height: u32 = self.height.try_into()?;
        let num_leaves = 2u64.pow(height);
        evict_position %= num_leaves; // Map to bucket indices
        evict_position = evict_position.swap_bits(); // Bit reversal
        evict_position = evict_position.checked_shr(64 - height).unwrap_or(0); // Move bits over to leaf indices
        evict_position += num_leaves; // Add bucket offset
        self.evict_counter += 1;
        Ok(evict_position)
    }

    /// Outputs the number of real blocks in the stash
    pub fn stash_occupancy(&self) -> StashSize {
        self.stash.occupancy()
    }

    /// Outputs the total size of the stash
    pub fn stash_size(&self) -> usize {
        self.stash.len()
    }

    /// Updates maximum occupancy and bookmarks current occupancy
    fn update_stash_stats(&mut self) {
        let current_occupancy = self.stash_occupancy();
        if current_occupancy > self.max_occupancy {
            self.max_occupancy = current_occupancy;
        }
        let count = self.all_occupancies.entry(current_occupancy).or_insert(0);
        *count += 1;
    }

    /// Outputs the maximum stash occupancy
    pub fn max_occupancy(&self) -> StashSize {
        self.max_occupancy
    }

    /// Calculates and outputs variance of stash occupancy
    pub fn variance(&self) -> f64 {
        // Calculate average occupancy
        let mut sum = 0;
        let mut num_occurrences = 0;
        for (occupancy, count) in self.all_occupancies.iter() {
            sum += occupancy * count;
            num_occurrences += count;
        }
        let average = (sum as f64) / (num_occurrences as f64);

        // Calculate probability and variance per occupancy
        let mut variance: f64 = 0.0;
        for (occupancy, count) in self.all_occupancies.iter() {
            let squared_term = ((*occupancy as f64) - average).powi(2);
            let probability = (*count as f64) / (num_occurrences as f64);
            variance += squared_term * probability;
        }
        variance
    }

    /// Calculates and outputs standard deviation of stash occupancy
    pub fn standard_deviation(&self) -> f64 {
        self.variance().powf(0.5)
    }

    /// Outputs variance and standard deviation of stash occupancy together
    pub fn variance_and_standard_deviation(&self) -> (f64, f64) {
        let variance = self.variance();
        let standard_deviation = variance.powf(0.5);
        (variance, standard_deviation)
    }

    /// Outputs the number of allocs
    pub fn alloc_counter(&self) -> Identifier {
        self.identifier_counter - 1
    }

    /// Outputs the number of writes with eviction
    pub fn write_counter(&self) -> StashSize {
        self.write_counter
    }

    /// Outputs the number of local writes without eviction
    pub fn local_write_counter(&self) -> StashSize {
        self.local_write_counter
    }

    /// Outputs the number of reads
    pub fn read_counter(&self) -> StashSize {
        self.read_counter
    }

    /// Outputs the number of round trips
    pub fn round_trip_counter(&self) -> StashSize {
        self.round_trip_counter
    }
}

impl<V: OsamPlusBlock, const Z: BucketSize> OsamPlus for PathOsamPlus<V, Z> {
    type V = V;

    /// Returns the capacity in blocks of this OSAM+.
    fn block_capacity(&self) -> usize {
        self.physical_memory.len()
    }

    /// Allocates a valid `Identifier` and `TreeIndex` to be used for reading and writing
    fn alloc<R: Rng + CryptoRng>(
        &mut self,
        rng: &mut R,
    ) -> Result<(Identifier, TreeIndex), OsamPlusError> {
        // Assign unique identifier from counter
        let identifier = self.identifier_counter;
        self.identifier_counter += 1;

        // Randomly select leaf position
        let position = CompleteBinaryTreeIndex::random_leaf(self.height, rng)?;
        Ok((identifier, position))
    }

    /// Obliviously writes the value stored `identifier` and `position`. Evicts blocks to server.
    fn write<R: Rng + CryptoRng>(
        &mut self,
        identifier: Identifier,
        position: TreeIndex,
        value: V,
        rng: &mut R,
    ) -> Result<(), OsamPlusError> {
        assert_ne!(identifier, Identifier::MAX);
        assert!(position.is_leaf(self.height));

        // Read a dummy path to make reads and writes indistinguishable
        let dummy_position: TreeIndex = CompleteBinaryTreeIndex::random_leaf(self.height, rng)?;
        assert!(dummy_position.is_leaf(self.height));
        self.stash
            .read_from_path(&mut self.physical_memory, dummy_position)?;

        // Read eviction path to stash
        let evict_position = self.evict_position()?;
        self.stash
            .read_from_eviction_path(&mut self.physical_memory, dummy_position, evict_position)?;

        // Add new block to stash by replacing a dummy block
        self.stash.write_to_stash(identifier, position, value)?;

        // Remove duplicates
        let _ = self.stash.merge();

        // Evict blocks from the stash along a deterministic path
        self.stash
            .write_to_path(&mut self.physical_memory, evict_position)?;

        // Bookkeeping of OSAM+ stats
        self.update_stash_stats();
        self.write_counter += 1;
        self.round_trip_counter += 1;

        Ok(())
    }

    /// Obliviously reads the value stored at `index`.
    fn read(
        &mut self,
        identifier: Identifier,
        position: TreeIndex,
    ) -> Result<Option<V>, OsamPlusError> {
        assert_ne!(identifier, Identifier::MAX);
        assert!(position.is_leaf(self.height));

        // Read path containing target block
        self.stash
            .read_from_path(&mut self.physical_memory, position)?;

        // Read eviction path to stash
        let evict_position = self.evict_position()?;
        self.stash
            .read_from_eviction_path(&mut self.physical_memory, position, evict_position)?;

        // Remove duplicates
        let _ = self.stash.merge();

        // Remove block from stash (and replace with dummy)
        let result = self.stash.read_from_stash(identifier)?;

        // Evict blocks from the stash along a deterministic path
        self.stash
            .write_to_path(&mut self.physical_memory, evict_position)?;

        // Bookkeeping of OSAM+ stats
        self.update_stash_stats();
        self.read_counter += 1;
        self.round_trip_counter += 1;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{bucket::*, test_utils::*};

    // Test default parameters.
    create_path_osam_plus_correctness_tests!(4, 40);

    // Test small initial stash sizes and correct resizing of stash on overflow.
    create_path_osam_plus_correctness_tests!(4, 10);
    create_path_osam_plus_correctness_tests!(4, 1);

    // Test small and large bucket sizes.
    create_path_osam_plus_correctness_tests!(3, 40);
    create_path_osam_plus_correctness_tests!(5, 40);

    // Check that the stash size stays reasonably small over the test runs.
    create_path_osam_plus_stash_size_correctness_tests!(4, 40);
}
