// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! A structure of a Path OSAM+ stash.

use crate::{
    bucket::{Bucket, PathOsamPlusBlock},
    utils::{bitonic_sort_by_keys, CompleteBinaryTreeIndex, TreeHeight, TreeIndex},
    BucketSize, Identifier, LowLevelBytes, OsamPlusBlock, OsamPlusError, PathCount, StashSize,
};
use aes_gcm::{
    aead::{Aead, Generate},
    Aes256Gcm, Nonce,
};
use cipher::typenum::U12;
use std::collections::HashMap;
use std::collections::HashSet;
use subtle::{Choice, ConditionallySelectable, ConstantTimeEq};

const STASH_GROWTH_INCREMENT: usize = 10;

#[derive(Debug)]
/// A fixed-size, obliviously accessed Path OSAM+ stash data structure implemented using oblivious sorting.
pub struct ObliviousStash<V: OsamPlusBlock> {
    blocks: Vec<PathOsamPlusBlock<V>>,
    path_size: StashSize,
    reserve_space: StashSize,
    cipher: Aes256Gcm,
    nonces: Vec<Nonce<U12>>,
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
        cipher: Aes256Gcm,
        nonces: Vec<Nonce<U12>>,
    ) -> Result<Self, OsamPlusError> {
        // Allocate enough space to contain the maximum number of slots possible along P+1 paths.
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
            path_size,
            reserve_space,
            cipher,
            nonces,
        })
    }

    /// Encrypt a bucket with a nonce.
    pub fn encrypt_bucket<const Z: BucketSize>(
        &mut self,
        bucket: Bucket<V, Z>,
        index: usize,
    ) -> Vec<u8> {
        let nonce = self.nonces[index];
        let plaintext = bucket.to_bytes_vec();
        let ciphertext = self.cipher.encrypt(&nonce, plaintext.as_ref()).unwrap();

        ciphertext
    }

    /// Decrypt a ciphertext with its nonce to produce a bucket.
    pub fn decrypt_bucket<const Z: BucketSize>(
        &mut self,
        ciphertext: Vec<u8>,
        index: usize,
    ) -> Option<Bucket<V, Z>> {
        let nonce = self.nonces[index];
        // Attempt to decrypt ciphertext from nonce.
        // Decryption may fail if the last time this bucket was decrypted, it was along a path
        // that was not evicted. That is, the bucket was downloaded but not reuploaded back to
        // `physical_memory`. `decrypt_bucket` always chooses a new unique nonce to avoid repetition.
        // Because the bucket is not uploaded back to the server, `physical_memory` still has the old
        // data corresponding to the old nonce. Decrypting the old data with a new nonce fails, but this
        // behavior is fine because the data is outdated and need not be recovered.
        let output: Option<Bucket<V, Z>>;
        let result = self.cipher.decrypt(&nonce, ciphertext.as_ref());
        match result {
            Ok(plaintext) => {
                let bucket = Bucket::<V, Z>::reconstruct(&plaintext);
                output = Some(bucket);

                // Generate a new unique nonce for the future.
                loop {
                    let nonce = Nonce::generate();
                    if !self.nonces.contains(&nonce) {
                        self.nonces[index] = nonce;
                        break;
                    }
                }
            }
            Err(_) => {
                output = None;
            }
        }

        output
    }

    /// Write server-side path(s) from root to leaf.
    pub fn write_to_paths<const Z: BucketSize, const P: PathCount>(
        &mut self,
        height: TreeHeight,
        physical_memory: &mut [Vec<u8>],
        positions: HashSet<TreeIndex>,
    ) -> Result<(), OsamPlusError> {
        // This function is called by `write`, `read`, and `read_multi_paths` to evict either
        // 1 or P paths to the server.
        assert!(positions.len() == 1 || positions.len() == P);

        // Create sorted vector of all unique buckets
        // and map of buckets to number of assigned blocks.
        let mut bucket_counts: HashMap<u64, u64> = HashMap::new();
        for position in positions.iter() {
            for i in 0..(self.path_size / u64::try_from(Z)?) {
                let bucket = position.ct_node_on_path(i, height);
                bucket_counts.entry(bucket).or_insert(0);
            }
        }
        let mut buckets: Vec<u64> = bucket_counts.keys().copied().collect();
        buckets.sort();

        // Create vector of that hold bucket / overflow assignments.
        let mut bucket_assignments = vec![TreeIndex::MAX; self.len()];

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
            for &bucket in buckets.iter().rev() {
                let count = bucket_counts.get_mut(&bucket).unwrap();
                let bucket_full: Choice = count.ct_eq(&(u64::try_from(Z)?));

                let mut bucket_satisfies_invariant = Choice::from(0);
                for level in 0..(height + 1) {
                    bucket_satisfies_invariant |=
                        block_position.ct_node_on_path(level, height).ct_eq(&bucket);
                }

                let should_assign =
                    bucket_satisfies_invariant & (!bucket_full) & (!block_is_dummy) & (!assigned);
                assigned |= should_assign;

                count.conditional_assign(&(*count + 1), should_assign);
                bucket_assignments[i].conditional_assign(&bucket, should_assign);
            }
            // If the block was not able to be assigned to any bucket, assign it to the overflow.
            bucket_assignments[i]
                .conditional_assign(&(TreeIndex::MAX - 1), (!assigned) & (!block_is_dummy));
        }

        // Assign dummy blocks to the remaining non-full buckets until all buckets are full.
        let mut exists_unfilled_buckets: Choice = 1.into();
        let mut first_unassigned_block_index: usize = 0;
        // Also need to pad any left over `reserve_space` with dummy blocks so real
        // blocks are not overwritten by future path downloads.
        let mut reserve_to_fill = self.reserve_space - StashSize::try_from(buckets.len() * Z)?;
        // Unless the stash overflows, this loop will execute exactly once, and the inner `if` will not execute.
        // If the stash overflows, this loop will execute twice and the inner `if` will execute.
        // This difference in control flow will leak the fact that the stash has overflowed.
        // This is a violation of obliviousness, but the alternative is simply to fail.
        // If the stash is set large enough when the OSAM+ is initialized,
        // stash overflow will occur only with negligible probability.
        while exists_unfilled_buckets.into() {
            // Make a pass over the stash, assigning dummy blocks to unfilled buckets in the paths.
            for (i, block) in self
                .blocks
                .iter()
                .enumerate()
                .skip(first_unassigned_block_index)
            {
                let block_free = block.ct_is_dummy();

                // Assign to buckets that are not full.
                let mut assigned: Choice = 0.into();
                for &bucket in buckets.iter().rev() {
                    let count = bucket_counts.get_mut(&bucket).unwrap();
                    let full = count.ct_eq(&(u64::try_from(Z)?));
                    let no_op = assigned | full | !block_free;

                    bucket_assignments[i].conditional_assign(&bucket, !no_op);
                    count.conditional_assign(&(*count + 1), !no_op);
                    assigned |= !no_op;
                }

                // Real blocks that are assigned to the overflow have the assignment `TreeIndex::Max - 1` so
                // they appear at the start of the stash / end of `reserve_space` before any dummy blocks.
                // Assign dummy blocks to `TreeIndex::Max - 2` to pad out `reserve_space` so they appear
                // before any real blocks that were assigned to the overflow.
                let open_reserve_space = reserve_to_fill.ct_ne(&0);
                let reserve_to_fill_decremented = reserve_to_fill.saturating_sub(1);
                let assign_to_reserve = (!assigned) & open_reserve_space & block_free;
                bucket_assignments[i].conditional_assign(&(TreeIndex::MAX - 2), assign_to_reserve);
                reserve_to_fill.conditional_assign(&reserve_to_fill_decremented, assign_to_reserve);
            }

            // Check that all buckets have been filled.
            exists_unfilled_buckets = reserve_to_fill.ct_ne(&0);
            for count in bucket_counts.values() {
                let full = count.ct_eq(&(u64::try_from(Z)?));
                exists_unfilled_buckets |= !full;
            }

            // If not, there must not have been enough dummy blocks remaining in the stash.
            // That is, the stash has overflowed.
            // So, extend the stash with STASH_GROWTH_INCREMENT more dummy blocks,
            // and repeat the process of trying to fill all unfilled buckets with dummy blocks.
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

        // Encrypt and write the number of blocks downloaded from P paths back to the server.
        let mut offset: usize = 0;
        for &bucket in buckets.iter() {
            // let bucket_to_write = &mut physical_memory[usize::try_from(bucket)?];
            let i = usize::try_from(bucket)? - 1;
            let mut bucket_to_write = Bucket::<V, Z>::default();
            for slot_number in 0..Z {
                let stash_index = offset + slot_number;
                bucket_to_write.blocks[slot_number] = self.blocks[stash_index];
                self.blocks[stash_index] = PathOsamPlusBlock::<V>::dummy();
            }
            let ciphertext = self.encrypt_bucket(bucket_to_write, i);
            physical_memory[i] = ciphertext;
            offset += Z;
        }

        Ok(())
    }

    /// Read several server-side paths from root to leaf into the first `reserve_space`
    /// indices, which are slots reserved for downloading and uploading blocks.
    /// Ensures buckets in overlapping paths are read exactly once.
    pub fn read_from_paths<const Z: BucketSize, const P: PathCount>(
        &mut self,
        height: TreeHeight,
        physical_memory: &mut [Vec<u8>],
        positions: &HashSet<TreeIndex>,
    ) -> Result<(), OsamPlusError> {
        // This function can be called by `read` or `read_multi_paths`, which read either 2 or P+1 paths.
        // If the path to read is the same as the evict path(s), then this drops to 1 or P paths.
        assert!(
            positions.len() == 1
                || positions.len() == 2
                || positions.len() == P
                || positions.len() == P + 1
        );

        // Download from all buckets along all specified paths.
        // Buckets only need to be accesses once.
        let mut checked_buckets = HashSet::new();
        let mut offset: usize = 0;
        for position in positions.iter() {
            // Download physical memory to stash and replace with dummy blocks.
            for i in 0..(self.path_size / u64::try_from(Z)?) {
                let bucket_index = usize::try_from(position.ct_node_on_path(i, height))?;
                // Only traverse a bucket once.
                if !checked_buckets.contains(&bucket_index) {
                    checked_buckets.insert(bucket_index);
                    let ciphertext = physical_memory[bucket_index - 1].clone();

                    // Ignore bucket if decryption fails.
                    if let Some(bucket) = self.decrypt_bucket::<Z>(ciphertext, bucket_index - 1) {
                        for slot_index in 0..Z {
                            self.blocks[Z * offset + slot_index] = bucket.blocks[slot_index];
                        }
                        offset += 1;
                    }
                }
            }
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
            block.conditional_assign(&PathOsamPlusBlock::<V>::dummy(), is_requested_index);
        }

        // Return the value of the found block or None.
        let mut output: Option<V> = None;
        if found.into() {
            output = Some(result);
        }

        Ok(output)
    }

    /// Delete stale versions of blocks with multiple entries.
    pub fn merge(&mut self) -> Result<(), OsamPlusError> {
        // This function is used after `read_from_paths` is called and the first `reserve_space`
        // indices are occupied with real blocks. It collects the newest version of blocks
        // from the stash and then reinserts them from left to right. Since all the keys are
        // then sorted and assigned to buckets, temporarily writing the location where downloaded
        // blocks go is fine.

        // Path OSAM+ enforces two rules about duplicates:
        //  - On the server, the most recent versions of data blocks are kept closer to the root
        //  - Buckets can have at most one version of a data block at any time.
        let mut identifier_map = HashMap::new();

        // Map identifiers to blocks to delete stale versions. Start with blocks that
        // have remained in the stash since these are more recent than anything downloaded
        // from the server. Go in reverse order because `write_to_stash` puts the newest
        // items to the right.
        for i in ((usize::try_from(self.reserve_space)?)..(self.blocks.len())).rev() {
            let block = self.blocks[i];
            identifier_map.entry(block.identifier).or_insert(block);
            self.blocks[i] = PathOsamPlusBlock::<V>::dummy();
        }

        // Then, process blocks from the downloaded path from root to leaf,
        // as the latest version of a block is kept closest to the root.
        // Outdated duplicates will be ignored here if they showed up in the stash.
        for i in 0..(usize::try_from(self.reserve_space)?) {
            let block = self.blocks[i];
            identifier_map.entry(block.identifier).or_insert(block);
            self.blocks[i] = PathOsamPlusBlock::<V>::dummy();
        }

        // Add all blocks back to the stash.
        for (i, block) in identifier_map.values().enumerate() {
            self.blocks[i] = *block;
        }

        Ok(())
    }

    /// Delete stale versions of blocks with multiple entries.
    pub fn local_merge(&mut self) -> Result<(), OsamPlusError> {
        // The first `reserve_space` blocks in the stash are dummy and overwritten in `read_from_paths`.
        // `local_merge` is `merge` but without reinserting blocks in the first `reserve_space` indices
        // so real blocks aren't overwritten when a path is downloaded.
        let mut identifier_map = HashMap::new();

        // Map identifiers to blocks to delete stale versions. Start with blocks that
        // have remained in the stash since these are more recent than anything downloaded
        // from the server.
        for i in (usize::try_from(self.reserve_space)?..self.blocks.len()).rev() {
            let block = self.blocks[i];
            identifier_map.entry(block.identifier).or_insert(block);
            self.blocks[i] = PathOsamPlusBlock::<V>::dummy();
        }

        // Add all blocks besides dummy back to the stash, as the first dummy block
        // appears to the right of all real blocks and signals it is safe to write.
        let mut i = usize::try_from(self.reserve_space)?;
        for block in identifier_map.values() {
            if (!(*block).ct_is_dummy()).into() {
                self.blocks[i] = *block;
                i += 1;
            }
        }

        Ok(())
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
        print!("STASH: ");
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
