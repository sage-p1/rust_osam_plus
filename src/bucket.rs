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
use subtle::ConstantTimeEq;
use subtle::{Choice, ConditionallySelectable};

/// A trait that works with datatypes and translates them into or from byte vectors/arrays.
/// This is necessary to work with `Aes256Gcm`, which only seems to encrypt references to `Vec<u8>` objects.
pub trait LowLevelBytes: Sized {
    /// Breakdown value to a vector of bytes.
    fn to_bytes_vec(&self) -> Vec<u8>;

    /// Get the number of bytes required to represent the current value.
    fn byte_count() -> usize;

    /// Given a slice of bytes, restore the original value.
    fn reconstruct(slice: &[u8]) -> Self;
}

/// Implement trait for all possible `OsamPlusBlock` V values.
impl LowLevelBytes for u8 {
    fn to_bytes_vec(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }

    fn byte_count() -> usize {
        1
    }

    fn reconstruct(slice: &[u8]) -> Self {
        u8::from_le_bytes(slice.try_into().unwrap())
    }
}

impl LowLevelBytes for u16 {
    fn to_bytes_vec(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }

    fn byte_count() -> usize {
        2
    }

    fn reconstruct(slice: &[u8]) -> Self {
        u16::from_le_bytes(slice.try_into().unwrap())
    }
}

impl LowLevelBytes for u32 {
    fn to_bytes_vec(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }

    fn byte_count() -> usize {
        4
    }

    fn reconstruct(slice: &[u8]) -> Self {
        u32::from_le_bytes(slice.try_into().unwrap())
    }
}

impl LowLevelBytes for u64 {
    fn to_bytes_vec(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }

    fn byte_count() -> usize {
        8
    }

    fn reconstruct(slice: &[u8]) -> Self {
        u64::from_le_bytes(slice.try_into().unwrap())
    }
}

impl LowLevelBytes for i8 {
    fn to_bytes_vec(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }

    fn byte_count() -> usize {
        1
    }

    fn reconstruct(slice: &[u8]) -> Self {
        i8::from_le_bytes(slice.try_into().unwrap())
    }
}

impl LowLevelBytes for i16 {
    fn to_bytes_vec(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }

    fn byte_count() -> usize {
        2
    }

    fn reconstruct(slice: &[u8]) -> Self {
        i16::from_le_bytes(slice.try_into().unwrap())
    }
}

impl LowLevelBytes for i32 {
    fn to_bytes_vec(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }

    fn byte_count() -> usize {
        4
    }

    fn reconstruct(slice: &[u8]) -> Self {
        i32::from_le_bytes(slice.try_into().unwrap())
    }
}

impl LowLevelBytes for i64 {
    fn to_bytes_vec(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }

    fn byte_count() -> usize {
        8
    }

    fn reconstruct(slice: &[u8]) -> Self {
        i64::from_le_bytes(slice.try_into().unwrap())
    }
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

impl<const B: BlockSize> ConditionallySelectable for BlockValue<B> {
    fn conditional_select(a: &Self, b: &Self, choice: Choice) -> Self {
        let mut result = BlockValue::default();
        for i in 0..B {
            result.data[i] = u8::conditional_select(&a.data[i], &b.data[i], choice);
        }
        result
    }
}

impl<const B: usize> LowLevelBytes for BlockValue<B> {
    fn to_bytes_vec(&self) -> Vec<u8> {
        self.data.to_vec()
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
/// An OSAM+ block combines an `OsamPlusBlock` V with two metadata fields; its OSAM+ `identifier` and its `position` in the tree.
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
}

impl<V: OsamPlusBlock> LowLevelBytes for PathOsamPlusBlock<V> {
    // Convert `PathOsamPlusBlock` structure into a byte vector containing its identifier, position, and value.
    fn to_bytes_vec(&self) -> Vec<u8> {
        let mut identifier_bytes = self.identifier.to_le_bytes().to_vec();
        let mut position_bytes = self.position.to_le_bytes().to_vec();
        let mut value_bytes = self.value.to_bytes_vec();

        identifier_bytes.append(&mut position_bytes);
        identifier_bytes.append(&mut value_bytes);
        identifier_bytes
    }

    // Compute the total byte count incurred by the identifier, position, and value.
    fn byte_count() -> usize {
        let identifier_count = usize::try_from(Identifier::BITS).unwrap() / 8;
        let position_count = usize::try_from(TreeIndex::BITS).unwrap() / 8;
        let value_count = V::byte_count();
        identifier_count + position_count + value_count
    }

    // Rebuild a `PathOsamPlusBlock` object from a byte slice.
    fn reconstruct(block_slice: &[u8]) -> Self {
        let identifier_end = Identifier::byte_count();
        let position_end = identifier_end + TreeIndex::byte_count();
        let value_end = position_end + V::byte_count();

        let identifier_slice = &block_slice[0..identifier_end];
        let position_slice = &block_slice[identifier_end..position_end];
        let value_slice = &block_slice[position_end..value_end];

        let identifier = u64::from_le_bytes(identifier_slice.try_into().unwrap());
        let position = u64::from_le_bytes(position_slice.try_into().unwrap());
        let value = V::reconstruct(value_slice);

        Self {
            value,
            identifier,
            position,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
/// A OSAM+ bucket.
pub struct Bucket<V: OsamPlusBlock, const Z: BucketSize> {
    /// The OSAM+ blocks stored by this bucket.
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
    fn to_bytes_vec(&self) -> Vec<u8> {
        let mut bucket_vec = Vec::new();
        for block in self.blocks.iter() {
            let mut block_vec = block.to_bytes_vec();
            bucket_vec.append(&mut block_vec);
        }
        bucket_vec
    }

    fn byte_count() -> usize {
        let block_byte_count = PathOsamPlusBlock::<V>::byte_count();
        block_byte_count * Z
    }

    fn reconstruct(bucket_slice: &[u8]) -> Self {
        assert_eq!(bucket_slice.len(), Bucket::<V, Z>::byte_count());
        let mut blocks = [PathOsamPlusBlock::<V>::dummy(); Z];
        for i in 0..Z {
            let block_byte_count = PathOsamPlusBlock::<V>::byte_count();
            let block_slice =
                &bucket_slice[(i * block_byte_count)..(i * block_byte_count + block_byte_count)];
            let block = PathOsamPlusBlock::<V>::reconstruct(block_slice);
            blocks[i] = block;
        }

        Self { blocks }
    }
}
