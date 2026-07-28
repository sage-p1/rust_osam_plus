// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! Encrypted and plaintext backend options for Path OSAM+.

use crate::{
    bucket::{Bucket, LowLevelBytes, PathOsamPlusBlock},
    BucketSize, Identifier, OsamPlusBlock, OsamPlusError,
};
use aes_gcm::{
    aead::{Aead, Generate, Key, KeyInit},
    Aes256Gcm, Nonce,
};
use cipher::typenum::U12;

#[derive(Debug)]
/// The physical memory the OSAM+ interacts with that is encrypted using `Aes256Gcm`.
struct EncryptedBackend {
    physical_memory: Vec<Vec<u8>>,
    cipher: Aes256Gcm,
    nonces: Vec<Nonce<U12>>,
}

impl EncryptedBackend {
    pub fn new<V: OsamPlusBlock, const Z: BucketSize>(
        block_capacity: Identifier,
    ) -> Result<Box<Self>, OsamPlusError> {
        // Convert this number to usize for reuse several times later.
        let backend_size = usize::try_from(block_capacity - 1)?;

        // Generate key, cipher, and vector of unique nonces (one for each bucket) for encryption.
        let key = Key::<Aes256Gcm>::generate();
        let cipher = Aes256Gcm::new(&key);
        let mut nonces = Vec::new();
        while nonces.len() < backend_size {
            let nonce = Nonce::generate();
            if !nonces.contains(&nonce) {
                nonces.push(nonce);
            }
        }

        // Intialize physical memory in backend so `encrypt_bucket` can be called correctly.
        let physical_memory = Vec::new();
        let mut backend = Self {
            physical_memory,
            cipher,
            nonces,
        };

        // `physical_memory` holds `block_capacity - 1` buckets, each storing up to Z blocks.
        // The number of leaves is `block_capacity` / 2, which the original Path ORAM paper's experiments
        // found was sufficient to keep the stash size small with high probability.
        let mut buckets = Vec::new();
        buckets.resize(backend_size, Bucket::<V, Z>::default());
        for (i, bucket) in buckets.iter().enumerate().take(backend_size) {
            let ciphertext = backend.encrypt_bucket(*bucket, i);
            backend.physical_memory.push(ciphertext);
        }

        Ok(Box::new(backend))
    }

    pub fn block_capacity(&self) -> usize {
        self.physical_memory.len()
    }

    /// Encrypt a bucket with a nonce.
    pub fn encrypt_bucket<V: OsamPlusBlock, const Z: BucketSize>(
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
    pub fn decrypt_bucket<V: OsamPlusBlock, const Z: BucketSize>(
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

    pub fn read_bucket_to_stash<V: OsamPlusBlock, const Z: BucketSize>(
        &mut self,
        blocks: &mut [PathOsamPlusBlock<V>],
        bucket_index: usize,
        offset: usize,
    ) -> usize {
        let ciphertext = self.physical_memory[bucket_index - 1].clone();
        // Ignore bucket if decryption fails.
        if let Some(bucket) = self.decrypt_bucket::<V, Z>(ciphertext, bucket_index - 1) {
            for slot_index in 0..Z {
                blocks[Z * offset + slot_index] = bucket.blocks[slot_index];
            }
            offset + 1
        } else {
            for slot_index in 0..Z {
                blocks[Z * offset + slot_index] = PathOsamPlusBlock::<V>::dummy();
            }
            offset
        }
    }

    pub fn write_bucket_to_stash<V: OsamPlusBlock, const Z: BucketSize>(
        &mut self,
        blocks: &mut [PathOsamPlusBlock<V>],
        bucket_index: usize,
        offset: usize,
    ) {
        let mut bucket_to_write = Bucket::<V, Z>::default();
        for slot_number in 0..Z {
            let stash_index = offset + slot_number;
            bucket_to_write.blocks[slot_number] = blocks[stash_index];
            blocks[stash_index] = PathOsamPlusBlock::<V>::dummy();
        }
        let ciphertext = self.encrypt_bucket(bucket_to_write, bucket_index - 1);
        self.physical_memory[bucket_index - 1] = ciphertext;
    }

    pub fn print_physical_memory<V: OsamPlusBlock, const Z: BucketSize>(&mut self) {
        println!("Physical Memory: ");
        let mut blocks = vec![PathOsamPlusBlock::<V>::dummy(); Z];
        for i in 1..(self.block_capacity() + 1) {
            print!("BUCKET {}: ", i);
            self.read_bucket_to_stash::<V, Z>(&mut blocks, i, 0);
            for block in blocks.iter().take(Z) {
                if block.ct_is_dummy().into() {
                    print!("(dummy) ");
                } else {
                    print!(
                        "({}, {}, {:?}) ",
                        block.identifier, block.position, block.value
                    );
                }
            }
            self.write_bucket_to_stash::<V, Z>(&mut blocks, i, 0);
            println!();
        }
    }
}

#[derive(Debug)]
/// The physical memory the OSAM+ interacts with that is not encrypted.
struct PlaintextBackend<V: OsamPlusBlock, const Z: BucketSize> {
    physical_memory: Vec<Bucket<V, Z>>,
}

impl<V: OsamPlusBlock, const Z: BucketSize> PlaintextBackend<V, Z> {
    pub fn new(block_capacity: Identifier) -> Result<Self, OsamPlusError> {
        let mut physical_memory = Vec::new();
        physical_memory.resize(
            usize::try_from(block_capacity - 1)?,
            Bucket::<V, Z>::default(),
        );
        Ok(Self { physical_memory })
    }

    pub fn block_capacity(&self) -> usize {
        self.physical_memory.len()
    }

    pub fn read_bucket_to_stash(
        &mut self,
        blocks: &mut [PathOsamPlusBlock<V>],
        bucket_index: usize,
        offset: usize,
    ) -> usize {
        let mut bucket = self.physical_memory[bucket_index - 1];
        for slot_index in 0..Z {
            blocks[Z * offset + slot_index] = bucket.blocks[slot_index];
            bucket.blocks[slot_index] = PathOsamPlusBlock::<V>::dummy();
        }
        self.physical_memory[bucket_index - 1] = bucket;
        offset + 1
    }

    pub fn write_bucket_to_stash(
        &mut self,
        blocks: &mut [PathOsamPlusBlock<V>],
        bucket_index: usize,
        offset: usize,
    ) {
        let bucket_to_write = &mut self.physical_memory[bucket_index - 1];
        for slot_number in 0..Z {
            let stash_index = offset + slot_number;
            bucket_to_write.blocks[slot_number] = blocks[stash_index];
            blocks[stash_index] = PathOsamPlusBlock::<V>::dummy();
        }
    }

    pub fn print_physical_memory(&self) {
        println!("Physical Memory: ");
        for i in 0..(self.physical_memory.len()) {
            print!("BUCKET {}: ", i + 1);
            let bucket = self.physical_memory[i];
            for block in bucket.blocks.iter() {
                if block.ct_is_dummy().into() {
                    print!("(dummy) ");
                } else {
                    print!(
                        "({}, {}, {:?}) ",
                        block.identifier, block.position, block.value
                    );
                }
            }
            println!();
        }
    }
}

#[derive(Debug)]
enum BackendMethod<V: OsamPlusBlock, const Z: BucketSize> {
    Encrypted(Box<EncryptedBackend>),
    Plaintext(PlaintextBackend<V, Z>),
}

#[derive(Debug)]
pub struct Backend<V: OsamPlusBlock, const Z: BucketSize>(BackendMethod<V, Z>);

impl<V: OsamPlusBlock, const Z: BucketSize> Backend<V, Z> {
    pub fn new(block_capacity: Identifier, is_encrypted: bool) -> Result<Self, OsamPlusError> {
        if is_encrypted {
            Ok(Self(BackendMethod::Encrypted(
                EncryptedBackend::new::<V, Z>(block_capacity)?,
            )))
        } else {
            Ok(Self(BackendMethod::Plaintext(PlaintextBackend::new(
                block_capacity,
            )?)))
        }
    }

    pub fn block_capacity(&self) -> usize {
        match &self.0 {
            BackendMethod::Encrypted(e) => e.block_capacity(),
            BackendMethod::Plaintext(p) => p.block_capacity(),
        }
    }

    pub fn read_bucket_to_stash(
        &mut self,
        blocks: &mut [PathOsamPlusBlock<V>],
        bucket_index: usize,
        offset: usize,
    ) -> usize {
        match &mut self.0 {
            BackendMethod::Encrypted(e) => {
                e.read_bucket_to_stash::<V, Z>(blocks, bucket_index, offset)
            }
            BackendMethod::Plaintext(p) => p.read_bucket_to_stash(blocks, bucket_index, offset),
        }
    }

    pub fn write_bucket_to_stash(
        &mut self,
        blocks: &mut [PathOsamPlusBlock<V>],
        bucket_index: usize,
        offset: usize,
    ) {
        match &mut self.0 {
            BackendMethod::Encrypted(e) => {
                e.write_bucket_to_stash::<V, Z>(blocks, bucket_index, offset)
            }
            BackendMethod::Plaintext(p) => p.write_bucket_to_stash(blocks, bucket_index, offset),
        }
    }

    pub fn print_physical_memory(&mut self) {
        match &mut self.0 {
            BackendMethod::Encrypted(e) => e.print_physical_memory::<V, Z>(),
            BackendMethod::Plaintext(p) => p.print_physical_memory(),
        }
    }
}
