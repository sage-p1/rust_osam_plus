// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! An implementation of Oblivious SAM (OSAM+) for the secure enclave setting.
//!
//! ⚠️ **Warning**: This implementation has not been audited. Use at your own risk!
//!
//! # Overview
//!
//! This crate implements an oblivious SAM protocol (OSAM+) for (secure) enclave applications.
//!
//! This crate assumes that OSAM+ clients are running inside a secure enclave architecture that provides memory encryption.
//! It does not perform encryption-on-write and thus is **not** secure without memory encryption.
//!
//! # Design
//!
//! This crate implements the Path OSAM+ protocol and is derived from
//! [Facebook's Path ORAM](https://github.com/facebook/oram) implementation.
//! See [Concrete OSAM](https://eprint.iacr.org/2026/451.pdf) for an introduction
//! to the SAM+ and Path OSAM+ implementation. This work is adapted from the
//! [OSAM Model](https://eprint.iacr.org/2024/1029.pdf).
//! Facebook's original oblivious client data structures are based on the
//! [Oblix paper](https://people.eecs.berkeley.edu/~raluca/oblix.pdf). See the
//! [Path ORAM retrospective paper](http://elaineshi.com/docs/pathoram-retro.pdf)
//! for a high-level introduction to Path ORAM. The SAM+ framework is adapted from
//!
//! # Example
//!
//! The below example reads a database from memory into a Path OSAM+, thus permitting secret-dependent accesses.
//!
//! ```
//! use osam_plus::{BlockSize, BlockValue, Identifier, OsamPlus, PathOsamPlus, TreeIndex};
//! use osam_plus::path_osam_plus::{DEFAULT_BLOCKS_PER_BUCKET, DEFAULT_STASH_OVERFLOW_SIZE};
//! # use osam_plus::OsamPlusError;
//!
//! const BLOCK_SIZE: BlockSize = 64;
//! const DB_SIZE: Identifier = 64;
//! const DATABASE: [[u8; BLOCK_SIZE as usize]; DB_SIZE as usize] =
//! [[0; BLOCK_SIZE as usize]; DB_SIZE as usize];
//! let mut rng = rand::rngs::OsRng;
//! let mut addresses: [(Identifier, TreeIndex); DB_SIZE as usize] =  
//! [(Identifier::MAX, 0); DB_SIZE as usize];
//!
//! // Initialize a Path OSAM+ to store 64 blocks of 64 bytes each.
//! let mut osam_plus = PathOsamPlus::<
//!     BlockValue<BLOCK_SIZE>,
//!     DEFAULT_BLOCKS_PER_BUCKET,
//!     >::new_with_parameters(DB_SIZE, DEFAULT_STASH_OVERFLOW_SIZE)?;
//!
//! // Read a database (here, an array of byte arrays) into Path OSAM+.
//! for (i, bytes) in DATABASE.iter().enumerate() {
//!     let address = osam_plus.alloc(&mut rng)?;
//!     addresses[i] = address;
//!     let identifier = address.0;
//!     let position = address.1;
//!     let _ = osam_plus.write(identifier, position, BlockValue::new(*bytes), &mut rng)?;
//! }
//!
//! // Now you can safely make secret-dependent accesses to your database.
//! for (i, address) in addresses.iter().enumerate() {
//!     let address = addresses[i];
//!     let identifier = address.0;
//!     let position = address.1;
//!     let bytes = osam_plus.read(identifier, position)?.unwrap();
//!     assert_eq!(bytes, BlockValue::new(DATABASE[i]));
//! }
//!
//! # Ok::<(), OsamPlusError>(())
//! ```
//!
//! # Advanced
//!
//! Path OSAM+ can store arbitrary structs implementing `OsamPlusBlock`.
//! We provide implementations of `OsamPlusBlock` for `u8`, `u16`, `u32`, `u64`, `i8`, `i16`, `i32`, `i64`,
//! and `BlockValue<const B: BlockSize>`.
//!
//! The `DefaultOsam` used in the above example should have good performance in most use cases.
//! But the underlying algorithms have several tunable parameters that impact performance.
//! The following example instantiates the same Path OSAM+ struct as above, but using the `PathOsamPlus`
//! interface which exposes these parameters.
//!
//! ```
//! use osam_plus::{BlockSize, BlockValue, BucketSize,
//!             Identifier, OsamPlus, PathOsamPlus, StashSize};
//! use osam_plus::path_osam_plus::{DEFAULT_BLOCKS_PER_BUCKET, DEFAULT_STASH_OVERFLOW_SIZE};
//! # use osam_plus::OsamPlusError;
//! # let mut rng = rand::rngs::OsRng;
//! # const BLOCK_SIZE: BlockSize = 64;
//! # const DB_SIZE: Identifier = 64;
//!
//! const BUCKET_SIZE: BucketSize = DEFAULT_BLOCKS_PER_BUCKET;
//! const INITIAL_STASH_OVERFLOW_SIZE: StashSize = DEFAULT_STASH_OVERFLOW_SIZE;
//!
//! let mut osam_plus = PathOsamPlus::<
//!     BlockValue<BLOCK_SIZE>,
//!     DEFAULT_BLOCKS_PER_BUCKET,
//!     >::new_with_parameters(DB_SIZE, DEFAULT_STASH_OVERFLOW_SIZE)?;
//! # Ok::<(), OsamPlusError>(())
//! ```
//!
//! See [`PathOsamPlus`] for an explanation of these parameters and their possible settings.

#![warn(clippy::cargo, clippy::doc_markdown, missing_docs, rustdoc::all)]

use rand::{CryptoRng, Rng};
use std::num::TryFromIntError;
use subtle::ConditionallySelectable;
use thiserror::Error;

pub(crate) mod bucket;
pub mod path_osam_plus;
pub(crate) mod stash;
#[cfg(test)]
mod test_utils;
pub(crate) mod utils;

pub use crate::bucket::BlockValue;
pub use crate::path_osam_plus::PathOsamPlus;
pub use crate::utils::TreeIndex;

/// The numeric type used to specify the size of an OSAM+ block in bytes.
pub type BlockSize = usize;
/// The numeric type used to assign a unique identifier to a block.
pub type Identifier = u64;
/// The numeric type used to specify the size of an OSAM+ bucket in blocks.
pub type BucketSize = usize;
/// Numeric type used to represent the size of a Path OSAM+ stash in blocks.
pub type StashSize = u64;
/// Numeric type used to represent the evict counter in Path OSAM+.
pub type CounterSize = u64;

/// A "trait alias" for OSAM+ blocks: the values read and written by Path OSAM+s.
pub trait OsamPlusBlock:
    Copy + Clone + std::fmt::Debug + Default + PartialEq + ConditionallySelectable
{
}

impl OsamPlusBlock for u8 {}
impl OsamPlusBlock for u16 {}
impl OsamPlusBlock for u32 {}
impl OsamPlusBlock for u64 {}
impl OsamPlusBlock for i8 {}
impl OsamPlusBlock for i16 {}
impl OsamPlusBlock for i32 {}
impl OsamPlusBlock for i64 {}

/// A list of error types which are produced during OSAM+ protocol execution.
#[derive(Error, Debug)]
pub enum OsamPlusError {
    /// Errors arising from conversions between integer types.
    #[error("Arithmetic error encountered.")]
    IntegerConversionError(#[from] TryFromIntError),
    /// Errors arising from invalid parameters or configuration.
    #[error("Invalid configuration. {parameter_name} cannot have value {parameter_value}.")]
    InvalidConfigurationError {
        /// The misconfigured parameter.
        parameter_name: String,
        /// Its invalid value.
        parameter_value: String,
    },
}

/// Represents an oblivious SAM+ (OSAM+) mapping identifiers of type `Identifier`
/// and position of type `TreeIndex` to values of type `V: OsamPlusBlock`.
pub trait OsamPlus
where
    Self: Sized,
{
    /// The type of elements stored in the OSAM+.
    type V: OsamPlusBlock;

    /// Returns the capacity in blocks of this OSAM+.
    fn block_capacity(&self) -> usize;

    /// Allocates a valid `Identifier` and random`TreeIndex` to be used for reading and writing
    fn alloc<R: Rng + CryptoRng>(
        &mut self,
        rng: &mut R,
    ) -> Result<(Identifier, TreeIndex), OsamPlusError>;

    /// Obliviously writes the value stored `identifier` and `position`. Evicts blocks to server.
    fn write<R: Rng + CryptoRng>(
        &mut self,
        identifier: Identifier,
        position: TreeIndex,
        value: Self::V,
        rng: &mut R,
    ) -> Result<(), OsamPlusError>;

    /// Obliviously reads the value stored at `index`.
    fn read(
        &mut self,
        identifier: Identifier,
        position: TreeIndex,
    ) -> Result<Option<Self::V>, OsamPlusError>;
}
