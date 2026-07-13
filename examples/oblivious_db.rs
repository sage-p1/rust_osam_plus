// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! An example of using Path OSAM+ to obliviously serve an indexed database.

extern crate osam_plus;
use osam_plus::path_osam_plus::{DEFAULT_BLOCKS_PER_BUCKET, DEFAULT_STASH_OVERFLOW_SIZE};
use osam_plus::{
    BlockSize, BlockValue, Identifier, OsamPlus, OsamPlusError, PathOsamPlus, TreeIndex,
};
use rand::{rngs::OsRng, Rng};

const BLOCK_SIZE: BlockSize = 4096;
const DB_SIZE: Identifier = 64;
// A stand-in for the indexed database you want to obliviously serve.
const DATABASE: [[u8; BLOCK_SIZE as usize]; DB_SIZE as usize] =
    [[0; BLOCK_SIZE as usize]; DB_SIZE as usize];

fn main() -> Result<(), OsamPlusError> {
    let mut rng = OsRng;
    let mut osam_plus =
        PathOsamPlus::<BlockValue<BLOCK_SIZE>, DEFAULT_BLOCKS_PER_BUCKET>::new_with_parameters(
            DB_SIZE,
            DEFAULT_STASH_OVERFLOW_SIZE,
        )?;

    let mut addresses: [(Identifier, TreeIndex); DB_SIZE as usize] =
        [(Identifier::MAX, 0); DB_SIZE as usize];

    // Read DATABASE into OSAM+.
    for (i, bytes) in DATABASE.iter().enumerate() {
        let address = osam_plus.alloc(&mut rng)?;
        addresses[i] = address;
        let identifier = address.0;
        let position = address.1;
        let _ = osam_plus.write(identifier, position, BlockValue::new(*bytes), &mut rng)?;
    }

    // Now OSAM+ can be used to obliviously serve the contents of DATABASE.
    let num_operations = 100;
    for _ in 0..num_operations {
        // Assert addresses correctly map to DATABASE
        let random_index = rng.gen_range(0..DB_SIZE) as usize;
        let address = addresses[random_index];
        let identifier = address.0;
        let position = address.1;
        let value = (osam_plus.read(identifier, position)?).unwrap();
        assert_eq!(value, BlockValue::new(DATABASE[random_index]));

        // Write DATABASE item back
        let address = osam_plus.alloc(&mut rng)?;
        addresses[random_index] = address;
        let identifier = address.0;
        let position = address.1;
        let _ = osam_plus.write(identifier, position, value, &mut rng)?;
    }

    Ok(())
}
