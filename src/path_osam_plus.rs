// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! An implementation of Path OSAM+.

use super::stash::{path_buckets, ObliviousStash};
use crate::bucket::PathOsamPlusBlock;
use crate::{
    backend::Backend,
    utils::{CompleteBinaryTreeIndex, TreeHeight, TreeIndex},
    BucketSize, CounterSize, Identifier, OsamPlus, OsamPlusBlock, OsamPlusError, PathCount,
    StashSize,
};
use bit_reverse::ParallelReverse;
use rand::{CryptoRng, Rng};
use std::collections::HashMap;

/// The parameter "Z" from the Path ORAM literature that sets the number of blocks per bucket; typical values are 3 or 4.
/// Here we adopt the more conservative setting of 4.
pub const DEFAULT_BLOCKS_PER_BUCKET: BucketSize = 4;

/// The default number of paths that Path OSAM+ can evict simultaneously.
pub const DEFAULT_PATH_COUNT: PathCount = 1;

/// The default number of overflow blocks that the Path OSAM+ stash can store.
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
/// - Path Count `P`: the number of paths evicted during `read_multi_paths`.
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
///
/// Under OSAM+, we introduce a function `local_write` that allows writing to the stash without evicting.
/// This lets us save on round-trips and more easily delete stale versions that are immediately detected
/// in the stash. Secondly, we introduce another function `read_multi_paths` that downloads and evicts P
/// paths every round-trip, instead of just one, to greatly improve how often we merge old blocks.
/// `read_multi_paths` downloads the P+1 paths (requested path and P evict paths). P is a constant
/// that is set upon initialization such that 1 <= P <= (N / 2) - 1. When P = 1, `read_multi_paths` and
/// `read` behave the same. When P = (N / 2) - 1, P + 1 = N / 2 paths are read, which means the entire server.
/// A special exception for N = 2 is that P can only be 0.
#[derive(Debug)]
pub struct PathOsamPlus<V: OsamPlusBlock, const Z: BucketSize, const P: PathCount> {
    /// The underlying untrusted memory that the OSAM+ is obliviously accessing on behalf of its client.
    /// Buckets are either encrypted using `Aes256Gcm` or stored as plaintext.
    backend: Backend<V, Z>,
    /// The Path OSAM+ stash.
    stash: ObliviousStash<V>,
    /// The height of the Path OSAM+ tree data structure.
    height: TreeHeight,
    /// The counter that assigns identifiers to Path OSAM+ blocks.
    /// Also serves as the alloc counter.
    identifier_counter: Identifier,
    /// The counter that deterministically picks which root-to-leaf path to evict.
    position_counter: CounterSize,
    /// The maximum occupancy (number of real blocks) observed in the stash at once.
    max_occupancy: StashSize,
    /// A mapping of occupancies to the number of occurrences.
    all_occupancies: HashMap<StashSize, StashSize>,
    /// The counter tracking the number of writes made with eviction.
    write_counter: CounterSize,
    /// The counter tracking the number of writes made without eviction.
    local_write_counter: CounterSize,
    /// The counter tracking the number of batch writes made without eviction.
    local_write_batch_counter: CounterSize,
    /// The counter tracking the number of reads.
    read_counter: CounterSize,
    /// The counter tracking the number of reads with multi-path eviction.
    read_multi_paths_counter: CounterSize,
    /// The counter tracking the number of evicts.
    evict_counter: CounterSize,
    /// The counter tracking the number of multi-path evictions.
    evict_multi_paths_counter: CounterSize,
    /// The counter tracking the number of round-trips, which is a defined as one
    /// instance of reading a path and then writing a path.
    round_trip_counter: CounterSize,
}

impl<V: OsamPlusBlock, const Z: BucketSize, const P: PathCount> PathOsamPlus<V, Z, P> {
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
    /// - `P`
    ///     - is not 0 when `block_capacity` is 2, or
    ///     - is 0 or exceeds the number of paths possible under (`block_capacity` / 2) - 1.
    pub fn new(
        block_capacity: Identifier,
        overflow_size: StashSize,
        is_encrypted: bool,
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

        let leaf_count = usize::try_from(block_capacity.checked_div(2).unwrap())?;
        if !(P == 0 && block_capacity == 2 || P != 0 && P < leaf_count && block_capacity > 2) {
            return Err(OsamPlusError::InvalidConfigurationError {
                parameter_name: "Path Count P".to_string(),
                parameter_value: P.to_string(),
            });
        }

        // Initialize backend method for storing physical memory (encrypted or plaintext).
        let backend = Backend::<V, Z>::new(block_capacity, is_encrypted)?;

        // Initialize stash.
        let height: StashSize = (block_capacity.ilog2() - 1).into();
        let stash = ObliviousStash::new::<Z, P>(height, overflow_size)?;

        // Initialize other parameters.
        let identifier_counter: Identifier = 1;
        let position_counter: CounterSize = 0;
        let max_occupancy: StashSize = 0;
        let all_occupancies: HashMap<StashSize, StashSize> = HashMap::new();
        let write_counter: CounterSize = 0;
        let local_write_counter: CounterSize = 0;
        let local_write_batch_counter: CounterSize = 0;
        let read_counter: CounterSize = 0;
        let read_multi_paths_counter: CounterSize = 0;
        let evict_counter: CounterSize = 0;
        let evict_multi_paths_counter: CounterSize = 0;
        let round_trip_counter: CounterSize = 0;

        Ok(Self {
            backend,
            stash,
            height,
            identifier_counter,
            position_counter,
            max_occupancy,
            all_occupancies,
            write_counter,
            local_write_counter,
            local_write_batch_counter,
            read_counter,
            read_multi_paths_counter,
            evict_counter,
            evict_multi_paths_counter,
            round_trip_counter,
        })
    }

    /// Initializes an empty tree with `blocks` in one client-side pass, as when
    /// the client has built its memory locally and uploads it once.
    ///
    /// Each block is placed in the deepest bucket on the path to its leaf that
    /// still has a free slot (the placement one greedy eviction of every path
    /// would give), and every bucket is written exactly once. Blocks that do
    /// not fit on their path go to the stash. This costs O(N log N) time,
    /// where writing the blocks one by one with `write` fills the stash while
    /// the tree is empty and costs time quadratic in N.
    ///
    /// The server sees only the upload of every bucket, so the placement is
    /// not required to be oblivious. The tree must be empty.
    pub fn bulk_load(
        &mut self,
        blocks: Vec<(Identifier, TreeIndex, V)>,
    ) -> Result<(), OsamPlusError> {
        let height = self.height;
        let leaves: u64 = 1 << height;
        let buckets = usize::try_from(2 * leaves - 1)?;
        // pending[node]: blocks waiting to be placed at or above `node`.
        let mut pending: Vec<Vec<PathOsamPlusBlock<V>>> = Vec::new();
        pending.resize_with(buckets + 1, Vec::new);
        for (identifier, position, value) in blocks {
            assert_ne!(identifier, Identifier::MAX);
            assert!(position.is_leaf(height));
            pending[usize::try_from(position)?].push(PathOsamPlusBlock {
                value,
                identifier,
                position,
            });
        }
        // Fill buckets from the leaves up; what does not fit moves to the parent.
        let mut slots = vec![PathOsamPlusBlock::<V>::dummy(); Z];
        for node in (1..=buckets).rev() {
            let mut waiting = std::mem::take(&mut pending[node]);
            let placed = waiting.len().min(Z);
            for (slot, block) in waiting.drain(..placed).enumerate() {
                slots[slot] = block;
            }
            for slot in slots.iter_mut().skip(placed) {
                slot.set_dummy();
            }
            self.backend.write_bucket_from_stash(&mut slots, node, 0);
            if node > 1 {
                pending[node / 2].append(&mut waiting);
            } else if !waiting.is_empty() {
                let overflow = waiting
                    .into_iter()
                    .map(|block| (block.identifier, block.position, block.value))
                    .collect();
                self.stash.write_batch_to_stash(overflow)?;
            }
        }
        self.update_stash_stats();
        Ok(())
    }

    /// One round trip: download the paths to `positions`, drop stale
    /// versions, take the block `identifier` out of the stash if one is
    /// given, and evict the stash back into the same paths.
    fn round_trip(
        &mut self,
        positions: &[TreeIndex],
        identifier: Option<Identifier>,
    ) -> Result<Option<V>, OsamPlusError> {
        let buckets = path_buckets(positions, self.height);
        self.stash
            .read_from_paths::<Z, P>(&mut self.backend, &buckets)?;
        self.stash.merge()?;
        let result = match identifier {
            Some(identifier) => self.stash.read_from_stash(identifier)?,
            None => None,
        };
        // The read path is written back as well (as in Path ORAM), so its
        // other blocks stay in the tree instead of accumulating in the stash.
        self.stash
            .write_to_paths::<Z, P>(self.height, &mut self.backend, &buckets)?;
        self.update_stash_stats();
        self.round_trip_counter += 1;
        Ok(result)
    }

    /// `count` distinct eviction leaves: the next ones in reverse-lexicographic
    /// order, or uniformly random ones.
    fn evict_positions<R: Rng + CryptoRng>(
        &mut self,
        count: usize,
        ordered_evict: bool,
        rng: &mut R,
    ) -> Result<Vec<TreeIndex>, OsamPlusError> {
        let mut positions = Vec::with_capacity(count + 1);
        if ordered_evict {
            for _ in 0..count {
                positions.push(self.evict_position()?);
            }
        } else {
            while positions.len() < count {
                let leaf = CompleteBinaryTreeIndex::random_leaf(self.height, rng)?;
                if !positions.contains(&leaf) {
                    positions.push(leaf);
                }
            }
        }
        Ok(positions)
    }

    /// Reads a single value while evicting P paths. Downloads P + 1, or P if the
    /// read path is also an evict path, paths.
    pub fn read_multi_paths<R: Rng + CryptoRng>(
        &mut self,
        identifier: Identifier,
        position: TreeIndex,
        ordered_evict: bool,
        rng: &mut R,
    ) -> Result<Option<V>, OsamPlusError> {
        assert_ne!(identifier, Identifier::MAX);
        assert!(position.is_leaf(self.height));
        let mut positions = self.evict_positions(P, ordered_evict, rng)?;
        positions.push(position);
        let result = self.round_trip(&positions, Some(identifier))?;
        self.read_multi_paths_counter += 1;
        Ok(result)
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

        // Add the block to the stash without interacting with the server,
        // replacing an older version if there is one.
        self.stash.write_to_stash(identifier, position, value)?;
        self.stash.local_merge()?;

        self.update_stash_stats();
        self.local_write_counter += 1;
        Ok(())
    }

    /// Locally writes several data blocks to the stash. Does not evict to server.
    pub fn local_write_batch(
        &mut self,
        batch: Vec<(Identifier, TreeIndex, V)>,
    ) -> Result<(), OsamPlusError> {
        for data in batch.iter() {
            assert_ne!(data.0, Identifier::MAX);
            assert!(data.1.is_leaf(self.height));
        }
        self.stash.write_batch_to_stash(batch)?;
        self.stash.local_merge()?;

        self.update_stash_stats();
        self.local_write_batch_counter += 1;
        Ok(())
    }

    /// Evicts a single path without reading any value to return to the user.
    pub fn evict<R: Rng + CryptoRng>(
        &mut self,
        ordered_evict: bool,
        rng: &mut R,
    ) -> Result<(), OsamPlusError> {
        let positions = self.evict_positions(1, ordered_evict, rng)?;
        self.round_trip(&positions, None)?;
        self.evict_counter += 1;
        Ok(())
    }

    /// Evicts P paths without reading any value to return to the user.
    pub fn evict_multi_paths<R: Rng + CryptoRng>(
        &mut self,
        ordered_evict: bool,
        rng: &mut R,
    ) -> Result<(), OsamPlusError> {
        let positions = self.evict_positions(P, ordered_evict, rng)?;
        self.round_trip(&positions, None)?;
        self.evict_multi_paths_counter += 1;
        Ok(())
    }

    /// A public flush: evicts P paths and also downloads one uniformly random
    /// dummy path, so that on the wire a flush has the same shape as a read
    /// (one random path plus P eviction paths).
    pub fn flush_multi_paths<R: Rng + CryptoRng>(
        &mut self,
        ordered_evict: bool,
        rng: &mut R,
    ) -> Result<(), OsamPlusError> {
        let mut positions = self.evict_positions(P, ordered_evict, rng)?;
        positions.push(CompleteBinaryTreeIndex::random_leaf(self.height, rng)?);
        self.round_trip(&positions, None)?;
        self.evict_multi_paths_counter += 1;
        Ok(())
    }

    /// Calculates the next position to evict via reverse-lexicographic ordering.
    fn evict_position(&mut self) -> Result<TreeIndex, OsamPlusError> {
        let mut evict_position: TreeIndex = self.position_counter;
        let height: u32 = self.height.try_into()?;
        let number_of_leaves = 2u64.pow(height);

        // Map to bucket indices, perform bit reversal, move bits over to
        // leaf indices, and add bucket offset.
        evict_position %= number_of_leaves;
        evict_position = evict_position.swap_bits();
        evict_position = evict_position.checked_shr(64 - height).unwrap_or(0);
        evict_position += number_of_leaves;

        self.position_counter += 1;
        assert!(evict_position.is_leaf(self.height));
        Ok(evict_position)
    }

    /// Outputs the number of real blocks in the stash.
    pub fn stash_occupancy(&self) -> StashSize {
        self.stash.occupancy()
    }

    /// Outputs the total size of the stash.
    pub fn stash_size(&self) -> StashSize {
        StashSize::try_from(self.stash.len()).unwrap()
    }

    /// Updates maximum occupancy and bookmarks current occupancy.
    fn update_stash_stats(&mut self) {
        let current_occupancy = self.stash_occupancy();
        if current_occupancy > self.max_occupancy {
            self.max_occupancy = current_occupancy;
        }
        let count = self.all_occupancies.entry(current_occupancy).or_insert(0);
        *count += 1;
    }

    /// Outputs the maximum stash occupancy.
    pub fn max_occupancy(&self) -> StashSize {
        self.max_occupancy
    }

    /// Calculates and outputs variance of stash occupancy.
    pub fn variance(&self) -> f64 {
        // Calculate average occupancy.
        let mut sum = 0;
        let mut occurrences = 0;
        for (occupancy, count) in self.all_occupancies.iter() {
            sum += occupancy * count;
            occurrences += count;
        }
        let average = (sum as f64) / (occurrences as f64);

        // Calculate probability and variance per occupancy.
        let mut variance: f64 = 0.0;
        for (occupancy, count) in self.all_occupancies.iter() {
            let squared_term = ((*occupancy as f64) - average).powi(2);
            let probability = (*count as f64) / (occurrences as f64);
            variance += squared_term * probability;
        }
        variance
    }

    /// Calculates and outputs standard deviation of stash occupancy.
    pub fn standard_deviation(&self) -> f64 {
        self.variance().powf(0.5)
    }

    /// Outputs variance and standard deviation of stash occupancy together.
    pub fn variance_and_standard_deviation(&self) -> (f64, f64) {
        let variance = self.variance();
        let standard_deviation = variance.powf(0.5);
        (variance, standard_deviation)
    }

    /// Outputs the number of allocs.
    pub fn alloc_counter(&self) -> Identifier {
        self.identifier_counter - 1
    }

    /// Outputs the number of writes with eviction.
    pub fn write_counter(&self) -> CounterSize {
        self.write_counter
    }

    /// Outputs the number of local writes without eviction.
    pub fn local_write_counter(&self) -> CounterSize {
        self.local_write_counter
    }

    /// Outputs the number of local batch writes without eviction.
    pub fn local_write_batch_counter(&self) -> CounterSize {
        self.local_write_batch_counter
    }

    /// Outputs the number of reads.
    pub fn read_counter(&self) -> CounterSize {
        self.read_counter
    }

    /// Outputs the number of reads with multi-path eviction.
    pub fn read_multi_paths_counter(&self) -> CounterSize {
        self.read_multi_paths_counter
    }

    /// Outputs the number of evicts.
    pub fn evict_counter(&self) -> CounterSize {
        self.evict_counter
    }

    /// Outputs the number of multi-path evictions.
    pub fn evict_multi_paths_counter(&self) -> CounterSize {
        self.evict_multi_paths_counter
    }

    /// Outputs the number of round trips.
    pub fn round_trip_counter(&self) -> CounterSize {
        self.round_trip_counter
    }

    /// Print blocks in physical memory for debug purposes.
    pub fn print_physical_memory(&mut self) {
        self.backend.print_physical_memory();
    }

    /// Print blocks in stash for debug purposes.
    pub fn print_stash(&self) {
        self.stash.print_stash();
    }
}

impl<V: OsamPlusBlock, const Z: BucketSize, const P: PathCount> OsamPlus for PathOsamPlus<V, Z, P> {
    type V = V;

    /// Returns the capacity in blocks of this OSAM+.
    fn block_capacity(&self) -> usize {
        self.backend.block_capacity()
    }

    /// Allocates a valid `Identifier` and `TreeIndex` to be used for reading and writing.
    fn alloc<R: Rng + CryptoRng>(
        &mut self,
        rng: &mut R,
    ) -> Result<(Identifier, TreeIndex), OsamPlusError> {
        // Assign unique identifier from counter.
        let identifier = self.identifier_counter;
        self.identifier_counter += 1;

        // Randomly select leaf position.
        let position: TreeIndex = CompleteBinaryTreeIndex::random_leaf(self.height, rng)?;
        assert!(position.is_leaf(self.height));
        Ok((identifier, position))
    }

    /// Obliviously writes the value stored `identifier` and `position`. Evicts 1 path to the server.
    fn write<R: Rng + CryptoRng>(
        &mut self,
        identifier: Identifier,
        position: TreeIndex,
        value: V,
        ordered_evict: bool,
        rng: &mut R,
    ) -> Result<(), OsamPlusError> {
        assert_ne!(identifier, Identifier::MAX);
        assert!(position.is_leaf(self.height));

        // Add new block to stash by replacing a dummy block.
        self.stash.write_to_stash(identifier, position, value)?;

        // Download a random dummy path, which makes writes look like reads,
        // and the eviction path; write both back.
        let mut positions = self.evict_positions(1, ordered_evict, rng)?;
        positions.push(CompleteBinaryTreeIndex::random_leaf(self.height, rng)?);
        self.round_trip(&positions, None)?;
        self.write_counter += 1;
        Ok(())
    }

    /// Obliviously reads the value stored at `index`. Evict 1 path to the server.
    fn read<R: Rng + CryptoRng>(
        &mut self,
        identifier: Identifier,
        position: TreeIndex,
        ordered_evict: bool,
        rng: &mut R,
    ) -> Result<Option<V>, OsamPlusError> {
        assert_ne!(identifier, Identifier::MAX);
        assert!(position.is_leaf(self.height));
        let mut positions = self.evict_positions(1, ordered_evict, rng)?;
        positions.push(position);
        let result = self.round_trip(&positions, Some(identifier))?;
        self.read_counter += 1;
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
