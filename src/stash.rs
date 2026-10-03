// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! A structure of a Path OSAM+ stash.

use crate::{
    backend::Backend,
    bucket::PathOsamPlusBlock,
    utils::{bitonic_sort_by_keys, CompleteBinaryTreeIndex, TreeHeight, TreeIndex},
    BucketSize, Identifier, OsamPlusBlock, OsamPlusError, PathCount, StashSize,
};
use subtle::{Choice, ConditionallySelectable, ConstantTimeEq};

/// The buckets on the paths to the leaves `positions`, each once, in
/// ascending heap order: level by level from the root, so every single path's
/// buckets appear root to leaf. Path OSAM+ reads and writes them in this order.
pub fn path_buckets(positions: &[TreeIndex], height: TreeHeight) -> Vec<TreeIndex> {
    let mut buckets: Vec<TreeIndex> = positions
        .iter()
        .flat_map(|&position| {
            debug_assert!(position.is_leaf(height));
            (0..=height).map(move |depth| position >> (height - depth))
        })
        .collect();
    buckets.sort_unstable();
    buckets.dedup();
    buckets
}

const STASH_GROWTH_INCREMENT: usize = 10;

#[derive(Debug)]
/// A fixed-size, obliviously accessed Path OSAM+ stash data structure implemented using oblivious sorting.
pub struct ObliviousStash<V: OsamPlusBlock> {
    blocks: Vec<PathOsamPlusBlock<V>>,
    reserve_space: StashSize,
}

impl<V: OsamPlusBlock> ObliviousStash<V> {
    pub fn len(&self) -> usize {
        self.blocks.len()
    }
}

impl<V: OsamPlusBlock> ObliviousStash<V> {
    // Create a stash of size `reserve_space + overflow_size`. The first `reserve_space` indices
    // are used for downloading and uploading paths from the server. Excluding the first
    // `reserve_space` indices, the stash is filled out by real blocks from left to right.
    // `reserve_space` holds up to P+1 paths.
    pub fn new<const Z: BucketSize, const P: PathCount>(
        height: StashSize,
        overflow_size: StashSize,
    ) -> Result<Self, OsamPlusError> {
        // Allocate enough space to contain the maximum number of slots possible along P + 1 paths.
        // Account for the number of times a path length is possible when buckets can only be
        // traversed once.
        let path_size: StashSize = StashSize::try_from(Z)? * (height + 1);
        let mut reserve_space = path_size;
        let mut doubling_factor = 1;
        let mut offset = 1;
        let mut turns_until_double = 1;

        for _ in 0..P {
            reserve_space += StashSize::try_from(Z)? * (height + 1 - offset);
            turns_until_double -= 1;
            if turns_until_double == 0 {
                doubling_factor *= 2;
                turns_until_double = doubling_factor;
                offset += 1;
            }
        }

        let num_stash_blocks = usize::try_from(reserve_space + overflow_size)?;

        Ok(Self {
            blocks: vec![PathOsamPlusBlock::<V>::dummy(); num_stash_blocks],
            reserve_space,
        })
    }

    /// Write server-side path(s) from root to leaf: evict the stash into
    /// `buckets` (from [`path_buckets`]) and upload them.
    ///
    /// Every real block goes to the deepest non-full bucket on its own path,
    /// or stays in the stash. The assignment scans every (block, bucket)
    /// pair in a fixed order with constant-time selects, so its control flow
    /// and memory accesses depend only on the public bucket list; a bucket at
    /// depth `d` is on a block's path exactly when the block's leaf shifted
    /// right by `height - d` equals the bucket.
    pub fn write_to_paths<const Z: BucketSize, const P: PathCount>(
        &mut self,
        height: TreeHeight,
        backend: &mut Backend<V, Z>,
        buckets: &[TreeIndex],
    ) -> Result<(), OsamPlusError> {
        let z = u64::try_from(Z)?;
        let reserved = buckets.len() * Z;
        assert!(reserved <= usize::try_from(self.reserve_space)?);
        let shifts: Vec<u64> = buckets
            .iter()
            .map(|bucket| height - bucket.ct_depth())
            .collect();
        let mut counts = vec![0u64; buckets.len()];

        // Bucket / overflow assignment of every stash slot (the sort keys).
        let mut bucket_assignments = vec![TreeIndex::MAX; self.len()];

        // Assign all non-dummy blocks in the stash to either the path or the overflow.
        let an_arbitrary_leaf: TreeIndex = 1 << height;
        for (block, assignment) in self.blocks.iter().zip(bucket_assignments.iter_mut()) {
            // If `block` is a dummy, the rest of this iteration is a no-op.
            let block_is_dummy = block.ct_is_dummy();
            let block_position =
                TreeIndex::conditional_select(&block.position, &an_arbitrary_leaf, block_is_dummy);

            // Scan the buckets from leaf to root, assigning the block to the
            // first non-full bucket on its path.
            let mut assigned = Choice::from(0);
            for j in (0..buckets.len()).rev() {
                let on_path = (block_position >> shifts[j]).ct_eq(&buckets[j]);
                let full = counts[j].ct_eq(&z);
                let should_assign = on_path & !full & !block_is_dummy & !assigned;
                assigned |= should_assign;
                let incremented = counts[j] + 1;
                counts[j].conditional_assign(&incremented, should_assign);
                assignment.conditional_assign(&buckets[j], should_assign);
            }
            // A real block that fits nowhere on the paths stays in the stash.
            assignment.conditional_assign(&(TreeIndex::MAX - 1), (!assigned) & (!block_is_dummy));
        }

        // Assign dummy blocks to the remaining non-full buckets until all buckets are full.
        let mut exists_unfilled_buckets: Choice = 1.into();
        let mut first_unassigned_block_index: usize = 0;
        // Also pad any leftover `reserve_space` with dummy blocks so real
        // blocks are not overwritten by future path downloads.
        let mut reserve_to_fill = self.reserve_space - StashSize::try_from(reserved)?;
        // Unless the stash overflows, this loop executes exactly once and the inner `if` does not.
        // If the stash overflows, it executes twice, which leaks that the stash overflowed.
        // That violates obliviousness, but the alternative is simply to fail;
        // with a large enough stash, overflow occurs only with negligible probability.
        while exists_unfilled_buckets.into() {
            for (block, assignment) in self
                .blocks
                .iter()
                .zip(bucket_assignments.iter_mut())
                .skip(first_unassigned_block_index)
            {
                let block_free = block.ct_is_dummy();

                // Assign to buckets that are not full.
                let mut assigned: Choice = 0.into();
                for j in (0..buckets.len()).rev() {
                    let full = counts[j].ct_eq(&z);
                    let no_op = assigned | full | !block_free;
                    assignment.conditional_assign(&buckets[j], !no_op);
                    let incremented = counts[j] + 1;
                    counts[j].conditional_assign(&incremented, !no_op);
                    assigned |= !no_op;
                }

                // Real blocks assigned to the overflow have key `TreeIndex::MAX - 1`, so they
                // sort to the start of the stash, right after `reserve_space`. Dummy blocks that
                // pad `reserve_space` get `TreeIndex::MAX - 2` so they sort before them;
                // otherwise real blocks would land in `reserve_space` and be overwritten by
                // future downloads.
                let open_reserve_space = reserve_to_fill.ct_ne(&0);
                let reserve_to_fill_decremented = reserve_to_fill.saturating_sub(1);
                let assign_to_reserve = (!assigned) & open_reserve_space & block_free;
                assignment.conditional_assign(&(TreeIndex::MAX - 2), assign_to_reserve);
                reserve_to_fill.conditional_assign(&reserve_to_fill_decremented, assign_to_reserve);
            }

            // Check that all buckets have been filled.
            exists_unfilled_buckets = reserve_to_fill.ct_ne(&0);
            for count in counts.iter() {
                exists_unfilled_buckets |= !count.ct_eq(&z);
            }

            // If not, the stash ran out of dummy blocks: it overflowed. Extend it with
            // STASH_GROWTH_INCREMENT dummy blocks and fill the remaining buckets.
            if exists_unfilled_buckets.into() {
                first_unassigned_block_index = self.blocks.len();
                self.blocks.resize(
                    self.blocks.len() + STASH_GROWTH_INCREMENT,
                    PathOsamPlusBlock::<V>::dummy(),
                );
                bucket_assignments.resize(
                    bucket_assignments.len() + STASH_GROWTH_INCREMENT,
                    TreeIndex::MAX,
                );
                log::warn!(
                    "Stash overflow occurred. Stash resized to {} blocks.",
                    self.blocks.len()
                );
            }
        }

        // Sort stash so the first `reserve_space` blocks align with their assigned buckets.
        bitonic_sort_by_keys(&mut self.blocks, &mut bucket_assignments);

        // Upload the buckets, Z stash slots each, in ascending order.
        for (j, &bucket) in buckets.iter().enumerate() {
            backend.write_bucket_from_stash(&mut self.blocks, usize::try_from(bucket)?, j * Z);
        }

        Ok(())
    }

    /// Download `buckets` (from [`path_buckets`]) into the first
    /// `reserve_space` stash slots, which are reserved for downloads and uploads.
    /// The buckets come root first, so the versions of a block on its path
    /// arrive newest first.
    pub fn read_from_paths<const Z: BucketSize, const P: PathCount>(
        &mut self,
        backend: &mut Backend<V, Z>,
        buckets: &[TreeIndex],
    ) -> Result<(), OsamPlusError> {
        assert!(buckets.len() * Z <= usize::try_from(self.reserve_space)?);
        let mut offset: usize = 0;
        for &bucket in buckets {
            offset =
                backend.read_bucket_to_stash(&mut self.blocks, usize::try_from(bucket)?, offset);
        }
        Ok(())
    }

    /// Write block to stash by overwriting the leftmost dummy block.
    pub fn write_to_stash(
        &mut self,
        identifier: Identifier,
        position: TreeIndex,
        value: V,
    ) -> Result<(), OsamPlusError> {
        // Create block with new values.
        let new_block = PathOsamPlusBlock {
            value,
            identifier,
            position,
        };

        // Overwrite the first dummy block.
        let mut assigned = Choice::from(0);

        // Skip the first `reserve_space` indices in the stash, since these are
        // overwritten upon calling `read_from_paths`.
        for block in self
            .blocks
            .iter_mut()
            .skip(usize::try_from(self.reserve_space)?)
        {
            let block_is_dummy = block.ct_is_dummy();
            let should_assign = block_is_dummy & (!assigned);
            assigned |= should_assign;
            block.conditional_assign(&new_block, should_assign);
        }

        // Add block to stash and resize if there no room currently (stash overflow).
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

    /// Write several data blocks to the stash in one call.
    pub fn write_batch_to_stash(
        &mut self,
        batch: Vec<(Identifier, TreeIndex, V)>,
    ) -> Result<(), OsamPlusError> {
        // Determine the first dummy index in the stash as all blocks to the right
        // are also dummy and can be safely overwritten.
        let mut dummy_index = self.blocks.len();
        for (i, block) in self
            .blocks
            .iter()
            .enumerate()
            .skip(usize::try_from(self.reserve_space)?)
        {
            if (block.ct_is_dummy()).into() {
                dummy_index = i;
                break;
            }
        }

        for data in batch.iter() {
            // Resize if too much data is added to the stash at once.
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

            // Create block and add to stash.
            let new_block = PathOsamPlusBlock {
                value: data.2,
                identifier: data.0,
                position: data.1,
            };
            self.blocks[dummy_index] = new_block;
            dummy_index += 1;
        }

        Ok(())
    }

    /// Read block from stash and replace with dummy.
    pub fn read_from_stash(&mut self, identifier: Identifier) -> Result<Option<V>, OsamPlusError> {
        let mut result: V = V::default();
        let mut found: Choice = 0.into();

        // Iterate over stash, updating the block with identifier `identifier` if one exists.
        for block in &mut self.blocks {
            let is_requested_index = block.identifier.ct_eq(&identifier);
            found.conditional_assign(&1.into(), is_requested_index);

            // Read current value of target block into `result` and replace with dummy.
            result.conditional_assign(&block.value, is_requested_index);
            block.conditional_set_dummy(is_requested_index);
        }

        // Return the value of the found block or None.
        let mut output: Option<V> = None;
        if found.into() {
            output = Some(result);
        }

        Ok(output)
    }

    /// Delete stale versions of blocks with multiple entries.
    ///
    /// Used after `read_from_paths`, when the first `reserve_space` slots
    /// hold the downloaded paths. Path OSAM+ keeps the newest version of a
    /// block closest to the root, and the stash is newer than the server, so
    /// the newest version is the first one in this order: the stash from right
    /// to left (`write_to_stash` appends newer blocks to the right), then the
    /// downloads from root to leaf. The surviving blocks are packed from slot
    /// 0 in that order; `write_to_paths` reassigns every slot.
    pub fn merge(&mut self) -> Result<(), OsamPlusError> {
        let reserve = usize::try_from(self.reserve_space)?;
        let order: Vec<usize> = (reserve..self.blocks.len())
            .rev()
            .chain(0..reserve)
            .collect();
        self.keep_newest(&order, 0);
        Ok(())
    }

    /// `merge` for the stash alone: the slots after `reserve_space` (the
    /// reserve holds only dummies between accesses), packed from
    /// `reserve_space`, which keeps real blocks out of the next download.
    pub fn local_merge(&mut self) -> Result<(), OsamPlusError> {
        let reserve = usize::try_from(self.reserve_space)?;
        let order: Vec<usize> = (reserve..self.blocks.len()).rev().collect();
        self.keep_newest(&order, reserve);
        Ok(())
    }

    /// Keeps the first block per identifier among the real blocks at `order`
    /// (slots listed newest first), empties those slots, and packs the kept
    /// blocks from slot `start` in `order`.
    fn keep_newest(&mut self, order: &[usize], start: usize) {
        // (identifier, rank) of every real block; the lowest rank wins.
        let mut real: Vec<(Identifier, usize)> = order
            .iter()
            .enumerate()
            .filter(|&(_, &slot)| !bool::from(self.blocks[slot].ct_is_dummy()))
            .map(|(rank, &slot)| (self.blocks[slot].identifier, rank))
            .collect();
        real.sort_unstable();
        real.dedup_by_key(|&mut (identifier, _)| identifier);
        let mut ranks: Vec<usize> = real.into_iter().map(|(_, rank)| rank).collect();
        ranks.sort_unstable();
        let kept: Vec<PathOsamPlusBlock<V>> =
            ranks.iter().map(|&rank| self.blocks[order[rank]]).collect();
        for &slot in order {
            self.blocks[slot].set_dummy();
        }
        self.blocks[start..start + kept.len()].copy_from_slice(&kept);
    }

    /// Outputs the number of real blocks in the stash.
    pub fn occupancy(&self) -> StashSize {
        let mut result = 0;
        for i in self.reserve_space.try_into().unwrap()..(self.blocks.len()) {
            if (!self.blocks[i].ct_is_dummy()).into() {
                result += 1;
            }
        }
        result
    }

    /// Print blocks in stash for debug purposes.
    pub fn print_stash(&self) {
        print!("Stash: ");
        for i in self.reserve_space.try_into().unwrap()..self.blocks.len() {
            let block = self.blocks[i];
            if (!block.ct_is_dummy()).into() {
                print!(
                    "({}, {}, {:?}) ",
                    block.identifier, block.position, block.value
                );
            }
        }
        println!();
    }
}
