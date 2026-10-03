// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! Block and bucket structures for OSAM+.

use crate::{utils::TreeIndex, BlockSize, BucketSize, Identifier, OsamPlusBlock};
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use subtle::{Choice, ConditionallySelectable, ConstantTimeEq};

/// A trait that works with datatypes and translates them into or from bytes.
/// Buckets are serialized with it before encryption with `Aes256Gcm`.
pub trait LowLevelBytes: Sized {
    /// Breakdown value to a vector of bytes.
    fn to_bytes_vec(&self) -> Vec<u8> {
        let mut bytes = vec![0u8; Self::byte_count()];
        self.write_bytes(&mut bytes);
        bytes
    }

    /// Writes the value's `byte_count()` bytes into `out`, without allocating.
    fn write_bytes(&self, out: &mut [u8]);

    /// Get the number of bytes required to represent the current value.
    fn byte_count() -> usize;

    /// Given a slice of bytes, restore the original value.
    fn reconstruct(slice: &[u8]) -> Self;
}

macro_rules! impl_low_level_bytes_for_integers {
    ($($t:ty),*) => {$(
        impl LowLevelBytes for $t {
            fn write_bytes(&self, out: &mut [u8]) {
                out.copy_from_slice(&self.to_le_bytes());
            }

            fn byte_count() -> usize {
                std::mem::size_of::<$t>()
            }

            fn reconstruct(slice: &[u8]) -> Self {
                <$t>::from_le_bytes(slice.try_into().unwrap())
            }
        }
    )*};
}

impl_low_level_bytes_for_integers!(u8, u16, u32, u64, i8, i16, i32, i64);

/// The all-ones byte mask if `choice` is set, zero otherwise.
#[inline]
fn byte_mask(choice: Choice) -> u8 {
    0u8.wrapping_sub(choice.unwrap_u8())
}

#[derive(Clone, Copy, Debug, PartialEq)]
/// An `OsamPlusBlock` consisting of unstructured bytes.
pub struct BlockValue<const B: BlockSize> {
    /// The block's data payload.
    pub data: [u8; B],
}

impl<const B: BlockSize> BlockValue<B> {
    /// Instantiates a `BlockValue` from an array of `BLOCK_SIZE` bytes.
    pub fn new(data: [u8; B]) -> Self {
        Self { data }
    }
}

impl<const B: BlockSize> Default for BlockValue<B> {
    fn default() -> Self {
        BlockValue::<B> { data: [0u8; B] }
    }
}

impl<const B: BlockSize> OsamPlusBlock for BlockValue<B> {}

// Masked byte loops: constant time like `u8::conditional_select`, but in
// place and without building a temporary block, so the stash's oblivious
// scans and sort move each byte once.
impl<const B: BlockSize> ConditionallySelectable for BlockValue<B> {
    fn conditional_select(a: &Self, b: &Self, choice: Choice) -> Self {
        let mut result = *a;
        result.conditional_assign(b, choice);
        result
    }

    fn conditional_assign(&mut self, other: &Self, choice: Choice) {
        let mask = byte_mask(choice);
        for (x, y) in self.data.iter_mut().zip(other.data.iter()) {
            *x ^= mask & (*x ^ *y);
        }
    }

    fn conditional_swap(a: &mut Self, b: &mut Self, choice: Choice) {
        let mask = byte_mask(choice);
        for (x, y) in a.data.iter_mut().zip(b.data.iter_mut()) {
            let t = mask & (*x ^ *y);
            *x ^= t;
            *y ^= t;
        }
    }
}

impl<const B: usize> LowLevelBytes for BlockValue<B> {
    fn write_bytes(&self, out: &mut [u8]) {
        out.copy_from_slice(&self.data);
    }

    fn byte_count() -> usize {
        B
    }

    fn reconstruct(slice: &[u8]) -> Self {
        let data = slice.try_into().unwrap();
        Self { data }
    }
}

impl<const B: BlockSize> Distribution<BlockValue<B>> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> BlockValue<B> {
        let mut result = BlockValue::default();
        for i in 0..B {
            result.data[i] = rng.gen();
        }
        result
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
/// A Path OSAM+ block combines an `OsamPlusBlock` V with two metadata fields; its OSAM+ `identifier` and its `position` in the tree.
pub(crate) struct PathOsamPlusBlock<V> {
    pub value: V,
    pub identifier: Identifier,
    pub position: TreeIndex,
}

impl<V: OsamPlusBlock> PathOsamPlusBlock<V> {
    const DUMMY_IDENTIFIER: Identifier = Identifier::MAX;
    const DUMMY_POSITION: TreeIndex = 0;

    pub fn dummy() -> Self {
        Self {
            value: V::default(),
            identifier: Self::DUMMY_IDENTIFIER,
            position: Self::DUMMY_POSITION,
        }
    }

    /// Turns this block into a dummy in place. Only the metadata changes: a
    /// dummy's value is never read, so the payload is left as it was rather
    /// than zeroed (which costs a full block write at large block sizes).
    pub fn set_dummy(&mut self) {
        self.identifier = Self::DUMMY_IDENTIFIER;
        self.position = Self::DUMMY_POSITION;
    }

    /// `set_dummy` if `choice` is set, in constant time.
    pub fn conditional_set_dummy(&mut self, choice: Choice) {
        self.identifier
            .conditional_assign(&Self::DUMMY_IDENTIFIER, choice);
        self.position
            .conditional_assign(&Self::DUMMY_POSITION, choice);
    }

    pub fn ct_is_dummy(&self) -> Choice {
        self.position.ct_eq(&Self::DUMMY_POSITION)
    }
}

impl<V: OsamPlusBlock> std::fmt::Debug for PathOsamPlusBlock<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.ct_is_dummy().into() {
            write!(f, "PathOsamPlusBlock::Dummy")
        } else {
            f.debug_struct("PathOsamPlusBlock")
                .field("value", &self.value)
                .field("identifier", &self.identifier)
                .field("position", &self.position)
                .finish()
        }
    }
}

impl<V: OsamPlusBlock> OsamPlusBlock for PathOsamPlusBlock<V> {}

impl<V: ConditionallySelectable> ConditionallySelectable for PathOsamPlusBlock<V> {
    fn conditional_select(a: &Self, b: &Self, choice: Choice) -> Self {
        let value = V::conditional_select(&a.value, &b.value, choice);
        let identifier = Identifier::conditional_select(&a.identifier, &b.identifier, choice);
        let position = TreeIndex::conditional_select(&a.position, &b.position, choice);
        PathOsamPlusBlock::<V> {
            value,
            identifier,
            position,
        }
    }

    fn conditional_assign(&mut self, other: &Self, choice: Choice) {
        self.value.conditional_assign(&other.value, choice);
        self.identifier
            .conditional_assign(&other.identifier, choice);
        self.position.conditional_assign(&other.position, choice);
    }

    fn conditional_swap(a: &mut Self, b: &mut Self, choice: Choice) {
        V::conditional_swap(&mut a.value, &mut b.value, choice);
        Identifier::conditional_swap(&mut a.identifier, &mut b.identifier, choice);
        TreeIndex::conditional_swap(&mut a.position, &mut b.position, choice);
    }
}

impl<V: OsamPlusBlock> LowLevelBytes for PathOsamPlusBlock<V> {
    // Layout: identifier, position, value (little endian).
    fn write_bytes(&self, out: &mut [u8]) {
        out[0..8].copy_from_slice(&self.identifier.to_le_bytes());
        out[8..16].copy_from_slice(&self.position.to_le_bytes());
        self.value.write_bytes(&mut out[16..]);
    }

    fn byte_count() -> usize {
        Identifier::byte_count() + TreeIndex::byte_count() + V::byte_count()
    }

    fn reconstruct(block_slice: &[u8]) -> Self {
        Self {
            identifier: u64::from_le_bytes(block_slice[0..8].try_into().unwrap()),
            position: u64::from_le_bytes(block_slice[8..16].try_into().unwrap()),
            value: V::reconstruct(&block_slice[16..16 + V::byte_count()]),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
/// A Path OSAM+ bucket.
pub struct Bucket<V: OsamPlusBlock, const Z: BucketSize> {
    /// The Path OSAM+ blocks stored by this bucket.
    pub(crate) blocks: [PathOsamPlusBlock<V>; Z],
}

impl<V: OsamPlusBlock, const Z: BucketSize> std::fmt::Debug for Bucket<V, Z> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut self_is_dummy = true;

        for block in self.blocks {
            if (!block.ct_is_dummy()).into() {
                self_is_dummy = false;
            }
        }

        if self_is_dummy {
            write!(f, "Bucket::Dummy")
        } else {
            f.debug_struct("Bucket")
                .field("blocks", &self.blocks)
                .finish()
        }
    }
}

impl<V: OsamPlusBlock, const Z: BucketSize> Default for Bucket<V, Z> {
    fn default() -> Self {
        Self {
            blocks: [PathOsamPlusBlock::<V>::dummy(); Z],
        }
    }
}

impl<V: OsamPlusBlock, const Z: BucketSize> ConditionallySelectable for Bucket<V, Z> {
    fn conditional_select(a: &Self, b: &Self, choice: Choice) -> Self {
        let mut result = Self::default();
        for i in 0..result.blocks.len() {
            result.blocks[i] =
                PathOsamPlusBlock::<V>::conditional_select(&a.blocks[i], &b.blocks[i], choice)
        }
        result
    }
}

impl<V: OsamPlusBlock, const Z: BucketSize> OsamPlusBlock for Bucket<V, Z> {}

impl<V: OsamPlusBlock, const Z: BucketSize> LowLevelBytes for Bucket<V, Z> {
    fn write_bytes(&self, out: &mut [u8]) {
        let size = PathOsamPlusBlock::<V>::byte_count();
        for (block, chunk) in self.blocks.iter().zip(out.chunks_exact_mut(size)) {
            block.write_bytes(chunk);
        }
    }

    fn byte_count() -> usize {
        PathOsamPlusBlock::<V>::byte_count() * Z
    }

    fn reconstruct(bucket_slice: &[u8]) -> Self {
        assert_eq!(bucket_slice.len(), Bucket::<V, Z>::byte_count());
        let size = PathOsamPlusBlock::<V>::byte_count();
        let mut blocks = [PathOsamPlusBlock::<V>::dummy(); Z];
        for (block, chunk) in blocks.iter_mut().zip(bucket_slice.chunks_exact(size)) {
            *block = PathOsamPlusBlock::<V>::reconstruct(chunk);
        }
        Self { blocks }
    }
}
