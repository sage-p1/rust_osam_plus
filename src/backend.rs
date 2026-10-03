// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! Encrypted and plaintext backend options for Path OSAM+.
//!
//! Buckets are numbered 1..=capacity-1 in heap order (bucket `i` has children
//! `2i` and `2i + 1`), as in the rest of the crate.

use crate::{
    bucket::{Bucket, LowLevelBytes, PathOsamPlusBlock},
    BucketSize, Identifier, OsamPlusBlock, OsamPlusError,
};
use aes_gcm::{
    aead::{AeadInOut, Generate, Key, KeyInit, Nonce, Tag},
    Aes256Gcm,
};

/// Bytes of the AES-GCM tag stored after each encrypted bucket.
const TAG_BYTES: usize = 16;

/// The physical memory the OSAM+ interacts with, encrypted with `Aes256Gcm`.
///
/// The server's memory is one flat arena of fixed-size slots, one per bucket:
/// the bucket's ciphertext followed by its tag. The client keeps the nonce of
/// each bucket's current ciphertext (a 64-bit counter value, so nonces never
/// repeat under the backend's random key); a server that returns an old or
/// altered ciphertext fails authentication. Nonce 0 marks a bucket that was
/// never written, which reads as a bucket of dummies, so creating a backend
/// costs no encryption: `bulk_load` or the first eviction writes each bucket.
struct EncryptedBackend<V: OsamPlusBlock, const Z: BucketSize> {
    arena: Vec<u8>,
    nonces: Vec<u64>,
    cipher: Aes256Gcm,
    next_nonce: u64,
    /// Bytes of one serialized bucket; a slot adds the tag.
    plaintext_bytes: usize,
    /// Client-side buffer the downloaded ciphertext is decrypted in.
    scratch: Vec<u8>,
    _blocks: std::marker::PhantomData<V>,
}

impl<V: OsamPlusBlock, const Z: BucketSize> std::fmt::Debug for EncryptedBackend<V, Z> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptedBackend")
            .field("buckets", &self.nonces.len())
            .field("next_nonce", &self.next_nonce)
            .finish()
    }
}

impl<V: OsamPlusBlock, const Z: BucketSize> EncryptedBackend<V, Z> {
    fn new(block_capacity: Identifier) -> Result<Self, OsamPlusError> {
        let buckets = usize::try_from(block_capacity - 1)?;
        let plaintext_bytes = Bucket::<V, Z>::byte_count();
        Ok(Self {
            arena: vec![0u8; buckets * (plaintext_bytes + TAG_BYTES)],
            nonces: vec![0; buckets],
            cipher: Aes256Gcm::new(&Key::<Aes256Gcm>::generate()),
            next_nonce: 1,
            plaintext_bytes,
            scratch: vec![0u8; plaintext_bytes],
            _blocks: std::marker::PhantomData,
        })
    }

    fn block_capacity(&self) -> usize {
        self.nonces.len()
    }

    fn nonce(counter: u64) -> Nonce<Aes256Gcm> {
        let mut bytes = [0u8; 12];
        bytes[..8].copy_from_slice(&counter.to_le_bytes());
        Nonce::<Aes256Gcm>::from(bytes)
    }

    /// Decrypts bucket `bucket_index` into `blocks[Z * offset..]`.
    fn read_bucket_to_stash(
        &mut self,
        blocks: &mut [PathOsamPlusBlock<V>],
        bucket_index: usize,
        offset: usize,
    ) -> usize {
        let slots = &mut blocks[Z * offset..Z * (offset + 1)];
        let counter = self.nonces[bucket_index - 1];
        if counter == 0 {
            slots.fill(PathOsamPlusBlock::<V>::dummy());
            return offset + 1;
        }
        let slot_bytes = self.plaintext_bytes + TAG_BYTES;
        let slot = &self.arena[(bucket_index - 1) * slot_bytes..][..slot_bytes];
        let (ciphertext, tag) = slot.split_at(self.plaintext_bytes);
        self.scratch.copy_from_slice(ciphertext);
        let tag = Tag::<Aes256Gcm>::try_from(tag).expect("tag length");
        self.cipher
            .decrypt_inout_detached(
                &Self::nonce(counter),
                b"",
                self.scratch.as_mut_slice().into(),
                &tag,
            )
            .expect("bucket failed authentication");
        let size = PathOsamPlusBlock::<V>::byte_count();
        for (block, chunk) in slots.iter_mut().zip(self.scratch.chunks_exact(size)) {
            *block = PathOsamPlusBlock::<V>::reconstruct(chunk);
        }
        offset + 1
    }

    /// Encrypts `blocks[offset..offset + Z]` into bucket `bucket_index` under a
    /// fresh nonce and replaces those stash slots with dummies.
    fn write_bucket_from_stash(
        &mut self,
        blocks: &mut [PathOsamPlusBlock<V>],
        bucket_index: usize,
        offset: usize,
    ) {
        let counter = self.next_nonce;
        self.next_nonce += 1;
        self.nonces[bucket_index - 1] = counter;
        let slot_bytes = self.plaintext_bytes + TAG_BYTES;
        let slot = &mut self.arena[(bucket_index - 1) * slot_bytes..][..slot_bytes];
        let (plaintext, tag_out) = slot.split_at_mut(self.plaintext_bytes);
        let size = PathOsamPlusBlock::<V>::byte_count();
        for (block, chunk) in blocks[offset..offset + Z]
            .iter_mut()
            .zip(plaintext.chunks_exact_mut(size))
        {
            block.write_bytes(chunk);
            block.set_dummy();
        }
        let tag = self
            .cipher
            .encrypt_inout_detached(&Self::nonce(counter), b"", plaintext.into())
            .expect("bucket encryption");
        tag_out.copy_from_slice(&tag);
    }
}

#[derive(Debug)]
/// The physical memory the OSAM+ interacts with that is not encrypted.
struct PlaintextBackend<V: OsamPlusBlock, const Z: BucketSize> {
    physical_memory: Vec<Bucket<V, Z>>,
}

impl<V: OsamPlusBlock, const Z: BucketSize> PlaintextBackend<V, Z> {
    fn new(block_capacity: Identifier) -> Result<Self, OsamPlusError> {
        Ok(Self {
            physical_memory: vec![Bucket::<V, Z>::default(); usize::try_from(block_capacity - 1)?],
        })
    }

    fn read_bucket_to_stash(
        &mut self,
        blocks: &mut [PathOsamPlusBlock<V>],
        bucket_index: usize,
        offset: usize,
    ) -> usize {
        let bucket = &mut self.physical_memory[bucket_index - 1];
        for (slot, block) in blocks[Z * offset..Z * (offset + 1)]
            .iter_mut()
            .zip(bucket.blocks.iter_mut())
        {
            *slot = std::mem::replace(block, PathOsamPlusBlock::<V>::dummy());
        }
        offset + 1
    }

    fn write_bucket_from_stash(
        &mut self,
        blocks: &mut [PathOsamPlusBlock<V>],
        bucket_index: usize,
        offset: usize,
    ) {
        let bucket = &mut self.physical_memory[bucket_index - 1];
        for (block, slot) in bucket
            .blocks
            .iter_mut()
            .zip(blocks[offset..offset + Z].iter_mut())
        {
            *block = std::mem::replace(slot, PathOsamPlusBlock::<V>::dummy());
        }
    }
}

#[derive(Debug)]
enum BackendMethod<V: OsamPlusBlock, const Z: BucketSize> {
    Encrypted(Box<EncryptedBackend<V, Z>>),
    Plaintext(PlaintextBackend<V, Z>),
}

/// The server-side tree of buckets, encrypted or in plaintext.
#[derive(Debug)]
pub struct Backend<V: OsamPlusBlock, const Z: BucketSize>(BackendMethod<V, Z>);

impl<V: OsamPlusBlock, const Z: BucketSize> Backend<V, Z> {
    pub fn new(block_capacity: Identifier, is_encrypted: bool) -> Result<Self, OsamPlusError> {
        Ok(Self(if is_encrypted {
            BackendMethod::Encrypted(Box::new(EncryptedBackend::new(block_capacity)?))
        } else {
            BackendMethod::Plaintext(PlaintextBackend::new(block_capacity)?)
        }))
    }

    /// The number of buckets.
    pub fn block_capacity(&self) -> usize {
        match &self.0 {
            BackendMethod::Encrypted(e) => e.block_capacity(),
            BackendMethod::Plaintext(p) => p.physical_memory.len(),
        }
    }

    /// Downloads bucket `bucket_index` into stash slots `Z * offset..Z * (offset + 1)`
    /// and returns `offset + 1`.
    pub fn read_bucket_to_stash(
        &mut self,
        blocks: &mut [PathOsamPlusBlock<V>],
        bucket_index: usize,
        offset: usize,
    ) -> usize {
        match &mut self.0 {
            BackendMethod::Encrypted(e) => e.read_bucket_to_stash(blocks, bucket_index, offset),
            BackendMethod::Plaintext(p) => p.read_bucket_to_stash(blocks, bucket_index, offset),
        }
    }

    /// Uploads stash slots `offset..offset + Z` as bucket `bucket_index`,
    /// leaving dummies in their place.
    pub fn write_bucket_from_stash(
        &mut self,
        blocks: &mut [PathOsamPlusBlock<V>],
        bucket_index: usize,
        offset: usize,
    ) {
        match &mut self.0 {
            BackendMethod::Encrypted(e) => e.write_bucket_from_stash(blocks, bucket_index, offset),
            BackendMethod::Plaintext(p) => p.write_bucket_from_stash(blocks, bucket_index, offset),
        }
    }

    pub fn print_physical_memory(&mut self) {
        println!("Physical Memory: ");
        let mut blocks = vec![PathOsamPlusBlock::<V>::dummy(); Z];
        for i in 1..=self.block_capacity() {
            print!("Bucket {}: ", i);
            self.read_bucket_to_stash(&mut blocks, i, 0);
            for block in blocks.iter() {
                if block.ct_is_dummy().into() {
                    print!("(dummy) ");
                } else {
                    print!(
                        "({}, {}, {:?}) ",
                        block.identifier, block.position, block.value
                    );
                }
            }
            self.write_bucket_from_stash(&mut blocks, i, 0);
            println!();
        }
    }
}
