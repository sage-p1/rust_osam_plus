## osam_plus

This library implements an Oblivious SAM+ (OSAM+) derived from [Facebook's ORAM implementation](https://github.com/facebook/oram).

⚠️ **Warning**: This implementation has not been audited. Use at your own risk!

Documentation
-------------

TODO: Write documentation

Installation
------------

TODO: Figure out how to make crate installable 

### Minimum Supported Rust Version

Rust **1.88** or higher.

Resources
---------

- [Original Path ORAM paper](https://eprint.iacr.org/2013/280.pdf), which introduced the standard "vanilla" variant of Path ORAM on which this library is based.
- [Path ORAM retrospective paper](http://elaineshi.com/docs/pathoram-retro.pdf), containing a high-level overview of developments related to Path ORAM.
- [Oblix paper](https://people.eecs.berkeley.edu/~raluca/oblix.pdf), which describes the oblivious stash data structure this library implements. 
- [Oblivious Single Access Machines](https://eprint.iacr.org/2024/1029.pdf), which derives the SAM model from RAM, defines the behavior of an OSAM, and modifies Path ORAM to create a backend called Path OSAM.
- [Reverse-lexicographic Eviction](https://eprint.iacr.org/2013/239.pdf), referring to the deterministic order buckets in which blocks are evicted to buckets. This differs from Path ORAM, which evicts blocks to the path it reads. 
- [Oblivious Single Access Machines+](https://eprint.iacr.org/2026/451.pdf), a relaxation of the OSAM model which allows addresses to be written to multiple times. Single reads are still enforced.

Code Organization
--------------------
Within `src/`:
- `lib.rs` defines public API.
- `path_oram.rs` defines the main OSAM implementation.
- `bucket.rs` defines low-level block and bucket structs.
- `utils.rs` contains utilities related to oblivious sorting and tree index calculations.
- `test_utils.rs` contains code shared between tests.

License
-------

This project is dual-licensed under either the [MIT license](https://github.com/facebook/oram/main/LICENSE-MIT)
or the [Apache License, Version 2.0](https://github.com/facebook/oram/blob/main/LICENSE-APACHE).
You may select, at your option, one of the above-listed licenses.

