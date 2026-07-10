// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! A trait representing a Path OSAM+ stash.

use std::collections::HashMap;
use crate::{
    bucket::{Bucket, PathOsamPlusBlock},
    utils::{bitonic_sort_by_keys, CompleteBinaryTreeIndex, TreeIndex},
    BucketSize, Identifier, OsamPlusBlock, OsamPlusError, StashSize,
};
use subtle::{Choice, ConditionallySelectable, ConstantTimeEq};

const STASH_GROWTH_INCREMENT: usize = 10;

#[derive(Debug)]
/// A fixed-size, obliviously accessed Path OSAM+ stash data structure implemented using oblivious sorting.
pub struct ObliviousStash<V: OsamPlusBlock> {
    blocks: Vec<PathOsamPlusBlock<V>>,
    path_size: StashSize,
}

impl<V: OsamPlusBlock> ObliviousStash<V> {
    pub fn len(&self) -> usize {
        self.blocks.len()
    }
}

impl<V: OsamPlusBlock> ObliviousStash<V> {
    // Create a stash of size `path_size + overflow_size`. The first `path_size` indices
    // are used for downloading and uploading paths from the server. Excluding the first
    // `path_size` indices, the stash is filled out by real blocks from left to right
    pub fn new(path_size: StashSize, overflow_size: StashSize) -> Result<Self, OsamPlusError> {
        let num_stash_blocks: usize = (path_size + overflow_size).try_into()?;

        Ok(Self {
            blocks: vec![PathOsamPlusBlock::<V>::dummy(); num_stash_blocks],
            path_size,
        })
    }

    // Write server-side path from root to leaf while accounting for existing blocks
    pub fn write_to_path<const Z: BucketSize>(
        &mut self,
        physical_memory: &mut [Bucket<V, Z>],
        position: TreeIndex,
    ) -> Result<(), OsamPlusError> {
        let height = position.ct_depth();
        let mut level_assignments = vec![TreeIndex::MAX; self.len()];
        let mut level_counts = vec![0; usize::try_from(height)? + 1];

        // Assign all non-dummy blocks in the stash to either the path or the overflow.
        for (i, block) in self.blocks.iter().enumerate() {
            // If `block` is a dummy, the rest of this loop iteration will be a no-op, and the values don't matter.
            let block_is_dummy = block.ct_is_dummy();

            // Set up valid but meaningless input to the computation in case `block` is a dummy.
            let an_arbitrary_leaf: TreeIndex = 1 << height;
            let block_position =
                TreeIndex::conditional_select(&block.position, &an_arbitrary_leaf, block_is_dummy);

            // Assign the block to a bucket or to the overflow.
            let mut assigned = Choice::from(0);
            // Obliviously scan through the buckets from leaf to root,
            // assigning the block to the first empty bucket satisfying the invariant.
            for (level, count) in level_counts.iter_mut().enumerate().rev() {
                let level_bucket_full: Choice = count.ct_eq(&(u64::try_from(Z)?));

                let level_u64 = u64::try_from(level)?;
                let level_satisfies_invariant = block_position
                    .ct_node_on_path(level_u64, height)
                    .ct_eq(&position.ct_node_on_path(level_u64, height));

                let should_assign = level_satisfies_invariant
                    & (!level_bucket_full)
                    & (!block_is_dummy)
                    & (!assigned);
                assigned |= should_assign;

                let level_count_incremented = *count + 1;
                count.conditional_assign(&level_count_incremented, should_assign);
                level_assignments[i].conditional_assign(&level_u64, should_assign);
            }
            // If the block was not able to be assigned to any bucket, assign it to the overflow.
            level_assignments[i]
                .conditional_assign(&(TreeIndex::MAX - 1), (!assigned) & (!block_is_dummy));
        }

        // Assign dummy blocks to the remaining non-full buckets until all buckets are full.
        let mut exists_unfilled_levels: Choice = 1.into();
        let mut first_unassigned_block_index: usize = 0;
        // Unless the stash overflows, this loop will execute exactly once, and the inner `if` will not execute.
        // If the stash overflows, this loop will execute twice and the inner `if` will execute.
        // This difference in control flow will leak the fact that the stash has overflowed.
        // This is a violation of obliviousness, but the alternative is simply to fail.
        // If the stash is set large enough when the OSAM+ is initialized,
        // stash overflow will occur only with negligible probability.
        while exists_unfilled_levels.into() {
            // Make a pass over the stash, assigning dummy blocks to unfilled levels in the path.
            for (i, block) in self
                .blocks
                .iter()
                .enumerate()
                .skip(first_unassigned_block_index)
            {
                // Skip the last block. It is reserved for handling writes to uninitialized addresses.
                if i == self.blocks.len() - 1 {
                    break;
                }

                let block_free = block.ct_is_dummy();

                let mut assigned: Choice = 0.into();
                for (level, count) in level_counts.iter_mut().enumerate() {
                    let full = count.ct_eq(&(u64::try_from(Z)?));
                    let no_op = assigned | full | !block_free;

                    level_assignments[i].conditional_assign(&(u64::try_from(level))?, !no_op);
                    count.conditional_assign(&(*count + 1), !no_op);
                    assigned |= !no_op;
                }
            }

            // Check that all levels have been filled.
            exists_unfilled_levels = 0.into();
            for count in level_counts.iter() {
                let full = count.ct_eq(&(u64::try_from(Z)?));
                exists_unfilled_levels |= !full;
            }

            // If not, there must not have been enough dummy blocks remaining in the stash.
            // That is, the stash has overflowed.
            // So, extend the stash with STASH_GROWTH_INCREMENT more dummy blocks,
            // and repeat the process of trying to fill all unfilled levels with dummy blocks.
            if exists_unfilled_levels.into() {
                first_unassigned_block_index = self.blocks.len() - 1;

                self.blocks.resize(
                    self.blocks.len() + STASH_GROWTH_INCREMENT,
                    PathOsamPlusBlock::<V>::dummy(),
                );
                level_assignments.resize(
                    level_assignments.len() + STASH_GROWTH_INCREMENT,
                    TreeIndex::MAX,
                );

                log::warn!(
                    "Stash overflow occurred. Stash resized to {} blocks.",
                    self.blocks.len()
                );
            }
        }

        // Sort stash so the first `path_size` blocks align with their assigned buckets
        bitonic_sort_by_keys(&mut self.blocks, &mut level_assignments);

        // Write the first Z * height blocks into slots in the tree
        for depth in 0..=height {
            let bucket_to_write =
                &mut physical_memory[usize::try_from(position.ct_node_on_path(depth, height))?];
            for slot_number in 0..Z {
                let stash_index = (usize::try_from(depth)?) * Z + slot_number;

                bucket_to_write.blocks[slot_number] = self.blocks[stash_index];
                self.blocks[stash_index] = PathOsamPlusBlock::<V>::dummy();
            }
        }

        Ok(())
    }

    // Read server-side path from root to leaf into the first `path_size` indices,
    // which are slots reserved for downloading and uploading blocks
    pub fn read_from_path<const Z: BucketSize>(
        &mut self,
        physical_memory: &mut [Bucket<V, Z>],
        position: TreeIndex,
    ) -> Result<(), OsamPlusError> {
        let height = position.ct_depth();

        // Download physical memory to stash and replace with dummy blocks
        for i in (0..(self.path_size / u64::try_from(Z)?)).rev() {
            let bucket_index = usize::try_from(position.ct_node_on_path(i, height))?;
            let mut bucket = physical_memory[bucket_index];
            for slot_index in 0..Z {
                self.blocks[Z * (usize::try_from(i)?) + slot_index] = bucket.blocks[slot_index];
                bucket.blocks[slot_index] = PathOsamPlusBlock::<V>::dummy();
            }
            physical_memory[bucket_index] = bucket;
        }

        Ok(())
    }

    // Downloads all blocks along the eviction path to the stash. These blocks are added
    // left to right in the stash wherever free space exists instead of occupying a reserved
    // `path_size` slots. Skips any buckets along the first path read, as it is known those  
    // buckets are already empty. At least the root bucket is skipped.
    // It is important that any blocks added here are to the right of any blocks currently
    // in the stash.
    pub fn read_from_eviction_path<const Z: BucketSize>(
        &mut self,
        physical_memory: &mut [Bucket<V, Z>],
        read_position: TreeIndex,
        evict_position: TreeIndex,
    ) -> Result<usize, OsamPlusError> {
        let height = evict_position.ct_depth();

        // Determine the first dummy index in the stash as all blocks to the right
        // are also dummy and can be safely overwritten
        let mut dummy_index = self.blocks.len();
        let read_from_path_indices = usize::try_from(self.path_size)?;
        let mut assigned = Choice::from(0);
        for (i, block) in self.blocks.iter().enumerate().skip(read_from_path_indices) {
            let block_is_dummy = block.ct_is_dummy();
            let should_assign = block_is_dummy & (!assigned);
            assigned |= should_assign;
            if should_assign.into() {
                dummy_index = i;
                break;
            }
        }

        // Save the index of where the evict path begins in the stash
        let evict_path_index = dummy_index;

        // Download physical memory to stash and replace with dummy blocks
        for i in 1..(self.path_size / u64::try_from(Z)?) {
            let read_index = read_position.ct_node_on_path(i, height);
            let evict_index = evict_position.ct_node_on_path(i, height);
            
            // Ignore buckets that were downloaded in the first read
            if read_index.ct_ne(&evict_index).into() {
                let evict_index = usize::try_from(evict_index)?;
                let mut bucket = physical_memory[evict_index];
                for slot_index in 0..Z {
                    // Resize if too much data is downloaded
                    if dummy_index >= self.blocks.len() {
                        self.blocks.resize(
                            self.blocks.len() + STASH_GROWTH_INCREMENT,
                            PathOsamPlusBlock::<V>::dummy(),
                        );

                        log::warn!(
                            "Stash overflow occurred. Stash resized to {} blocks.",
                            self.blocks.len()
                        );
                    }

                    self.blocks[dummy_index] = bucket.blocks[slot_index];
                    dummy_index += 1;
                    bucket.blocks[slot_index] = PathOsamPlusBlock::<V>::dummy();
                }
                physical_memory[evict_index] = bucket;
            }
        }

        Ok(evict_path_index)
    }

    // Write block to stash
    pub fn write_to_stash(
        &mut self,
        identifier: Identifier,
        position: TreeIndex,
        value: V,
    ) -> Result<(), OsamPlusError> {
        // Create block with new values
        let new_block = PathOsamPlusBlock {
            value,
            identifier,
            position,
        };

        // Overwrite the first dummy block
        let mut assigned = Choice::from(0);
        let read_from_path_indices = usize::try_from(self.path_size)?;

        // Skip the first `path_size` indices in the stash, since these are
        // overwritten upon calling read_from_path
        for block in self.blocks.iter_mut().skip(read_from_path_indices) {
            let block_is_dummy = block.ct_is_dummy();
            let should_assign = block_is_dummy & (!assigned);
            assigned |= should_assign;
            block.conditional_assign(&new_block, should_assign);
        }

        // Add block to stash and resize if there no room currently (stash overflow)
        if (!assigned).into() {
            let i = self.blocks.len();

            self.blocks.resize(
                self.blocks.len() + STASH_GROWTH_INCREMENT,
                PathOsamPlusBlock::<V>::dummy(),
            );

            log::warn!(
                "Stash overflow occurred. Stash resized to {} blocks.",
                self.blocks.len()
            );

            self.blocks[i] = new_block;
        }

        Ok(())
    }

    // Read and remove block from stash
    pub fn read_from_stash(&mut self, identifier: Identifier) -> Result<Option<V>, OsamPlusError> {
        let mut result: V = V::default();
        let mut found: Choice = 0.into();

        // Iterate over stash, updating the block with identifier `identifier` if one exists.
        for block in &mut self.blocks {
            let is_requested_index = block.identifier.ct_eq(&identifier);
            found.conditional_assign(&1.into(), is_requested_index);

            // Read current value of target block into `result`.
            result.conditional_assign(&block.value, is_requested_index);
            // Write new position into target block.
            block.conditional_assign(&PathOsamPlusBlock::<V>::dummy(), is_requested_index);
        }

        let mut output: Option<V> = None;
        if found.into() {
            output = Some(result);
        }

        // Return the value of the found block (or the default value, if no block was found)
        Ok(output)
    }

    // Delete stale versions of blocks with multiple entries
    pub fn merge(&mut self, evict_path_index: usize) -> Result<(), OsamPlusError> {
        // This function is used after `read_from_path` is called and the first `path_size`
        // indices are occupied with real blocks. It collects the newest version of blocks 
        // from the stash and then reinserts them from left to right. Since all the keys are
        // then sorted and assigned to buckets, temporarily writing the location where downloaded
        // blocks go is fine.
        let mut identifier_map = HashMap::new();

        // Map identifiers to blocks to delete stale versions. Start with blocks that
        // have remained in the stash since these are more recent than anything downloaded
        // from the server. 
        for i in (usize::try_from(self.path_size)?..evict_path_index).rev() {
            let block = self.blocks[i];
            identifier_map.entry(block.identifier).or_insert(block);
            self.blocks[i] = PathOsamPlusBlock::<V>::dummy();
        }

        // Then, process blocks from the downloaded path from root to leaf,
        // as the latest version of a block is kept closest to the root.
        // Duplicates here will be replaced by their newer counterparts.
        for i in 0..usize::try_from(self.path_size)? {
            let block = self.blocks[i];
            identifier_map.entry(block.identifier).or_insert(block);
            self.blocks[i] = PathOsamPlusBlock::<V>::dummy();
        }

        // Process blocks from the evict path so that more recent versions from the 
        // stash or closer to the root take precedence.
        for i in evict_path_index..self.blocks.len() {
            let block = self.blocks[i];
            identifier_map.entry(block.identifier).or_insert(block);
            self.blocks[i] = PathOsamPlusBlock::<V>::dummy();
        }

        // Add all blocks back to the stash
        for (i, block) in identifier_map.values().enumerate() {
            self.blocks[i] = *block;
        }

        Ok(())
    }

    // Delete stale versions of blocks with multiple entries
    pub fn local_merge(&mut self) -> Result<(), OsamPlusError>{
        // The first `path_size` blocks in the stash are dummy and overwritten in `read_from_path`.
        // `local_merge` is `merge` but without reinserting blocks in the first `path_size` indices
        // so real blocks aren't overwritten.
        let mut identifier_map = HashMap::new();

        // Map identifiers to blocks to delete stale versions. Start with blocks that
        // have remained in the stash since these are more recent than anything downloaded
        // from the server.
        for i in (usize::try_from(self.path_size)?..self.blocks.len()).rev() {
            let block = self.blocks[i];
            identifier_map.entry(block.identifier).or_insert(block);
            self.blocks[i] = PathOsamPlusBlock::<V>::dummy();
        }

        // Add all blocks besides dummy back to the stash, as because the first dummy block 
        // appears to the right of all real blocks
        let mut i = usize::try_from(self.path_size)?;
        for block in identifier_map.values() {
            if (!(*block).ct_is_dummy()).into() {
                self.blocks[i] = *block;
                i += 1;
            }
        }

        Ok(())
    }

    pub fn occupancy(&self) -> StashSize {
        let mut result = 0;
        for i in self.path_size.try_into().unwrap()..(self.blocks.len()) {
            if (!self.blocks[i].ct_is_dummy()).into() {
                result += 1;
            }
        }
        result
    }
}
