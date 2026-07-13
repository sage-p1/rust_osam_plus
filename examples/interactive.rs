// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! A simple interactive demonstration of OSAM+.

use osam_plus::path_osam_plus::{DEFAULT_BLOCKS_PER_BUCKET, DEFAULT_STASH_OVERFLOW_SIZE};
use osam_plus::{OsamPlus, PathOsamPlus};
use rand::rngs::OsRng;
use rustyline::history::FileHistory;
use rustyline::Editor;

fn parse_u64(
    prompt: &str,
    rl: &mut Editor<(), FileHistory>,
) -> Result<u64, Box<dyn std::error::Error>> {
    Ok(loop {
        println!("{}", prompt);
        println!();
        let readline: String = rl.readline("> ")?;
        let number_parse = readline.parse::<u64>();
        match number_parse {
            Ok(number) => break number,
            Err(_) => {
                println!("\nExpected a u64. Try again.");
                continue;
            }
        }
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = OsRng;

    let mut rl = Editor::<(), _>::new().unwrap();

    println!("In this example, we initialize and interact with an oblivious SAM+ storing u64s.");
    println!("How many u64 blocks would you like the OSAM+ to support?");

    let capacity = parse_u64("\nEnter a power of two:", &mut rl)?;

    // Initialize an OSAM+ storing `capacity` u64s.
    let mut osam_plus = PathOsamPlus::<u64, DEFAULT_BLOCKS_PER_BUCKET>::new_with_parameters(
        capacity,
        DEFAULT_STASH_OVERFLOW_SIZE,
    )?;

    loop {
        let action = loop {
            println!("\nEnter an option (a, r, w, or q):");
            println!("a) Alloc");
            println!("w) Write");
            println!("r) Read");
            println!("q) Quit");
            let action: String = rl.readline("\n> ")?;
            if (action != "a") & (action != "w") & (action != "r") & (action != "q") {
                println!("\nTry again.");
                continue;
            }
            break action;
        };

        if action == "q" {
            println!("\nQuitting program...");
            break;
        } else if action == "a" {
            let address = osam_plus.alloc(&mut rng)?;
            println!(
                "\nThe allocated address is (identifier: {}, position: {})",
                address.0, address.1
            );
        } else {
            let identifier = parse_u64("\nEnter identifier: ", &mut rl)?;
            let position = parse_u64("\nEnter position: ", &mut rl)?;

            if action == "w" {
                let value = parse_u64("\nEnter value: ", &mut rl)?;
                let _ = osam_plus.write(identifier, position, value, &mut rng)?;
                println!(
                    "\nWrote value {} to address (identifier: {}, position: {}).",
                    value, identifier, position
                );
            } else {
                let value = osam_plus.read(identifier, position)?;
                match value {
                    Some(v) => println!(
                        "\nValue at address (identifier: {}, position: {}) is {}",
                        identifier, position, v
                    ),
                    None => println!(
                        "\nCould not find a value at address (identifier: {}, position: {})",
                        identifier, position
                    ),
                }
            }
        }
    }

    Ok(())
}
