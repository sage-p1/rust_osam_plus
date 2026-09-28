//! Regression test for the in-degree bug: r-ary pointers must stay correct
//! when one object has many more than `b` live aliases and aliases are
//! deleted (the Python backend raised "delete repair found an underfull
//! non-leaf parent group" there). Several objects, random copy/get/delete,
//! every get checked against the alias's own value, bounded path length, and
//! no live blocks once every alias is deleted.

use sam_model::pointer::{RaryPointers, SmartPointerBackend};
use sam_model::{AccessPolicy, DryRunSam, SingleAccessMachine};

fn lcg(state: &mut u64, n: usize) -> usize {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*state >> 33) % n as u64) as usize
}

fn stress(branching_factor: usize, max_aliases: usize, operations: usize, seed: u64) {
    const OBJECTS: usize = 4;
    let mut backend = RaryPointers::new(branching_factor).unwrap();
    let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
    let mut state = seed;
    let mut aliases: Vec<Vec<_>> = (0..OBJECTS)
        .map(|k| vec![backend.new_pointer(&mut sam, 1000 * k as u64 + 7).unwrap()])
        .collect();
    let mut worst_reads = 0;
    let mut peak = 0;
    for _ in 0..operations {
        let k = lcg(&mut state, OBJECTS);
        let expected = 1000 * k as u64 + 7;
        let group = &mut aliases[k];
        let op = lcg(&mut state, 10);
        if (op < 5 && group.len() < max_aliases) || group.len() == 1 {
            let i = lcg(&mut state, group.len());
            let copy = backend.copy_pointer(&mut sam, &mut group[i]).unwrap();
            group.push(copy);
        } else if op < 8 {
            let i = lcg(&mut state, group.len());
            let before = sam.stats().operations.reads;
            let value = backend.get(&mut sam, &mut group[i]).unwrap();
            worst_reads = worst_reads.max(sam.stats().operations.reads - before);
            assert_eq!(value, Some(expected), "b={branching_factor}: wrong value");
        } else {
            let i = lcg(&mut state, group.len());
            let mut pointer = group.swap_remove(i);
            backend.delete(&mut sam, &mut pointer).unwrap();
        }
        peak = peak.max(aliases.iter().map(Vec::len).max().unwrap());
    }
    for (k, group) in aliases.iter_mut().enumerate() {
        for pointer in group.iter_mut() {
            assert_eq!(
                backend.get(&mut sam, pointer).unwrap(),
                Some(1000 * k as u64 + 7)
            );
        }
    }
    for group in &mut aliases {
        while !group.is_empty() {
            let i = lcg(&mut state, group.len());
            let mut pointer = group.swap_remove(i);
            backend.delete(&mut sam, &mut pointer).unwrap();
        }
    }
    assert_eq!(sam.live_blocks(), 0, "b={branching_factor}: leaked blocks");
    // A get walks one root-to-leaf path: O(log_{b/2} n) levels.
    let levels = (peak.max(2) as f64)
        .log((branching_factor / 2).max(2) as f64)
        .ceil() as u64;
    assert!(
        worst_reads <= 2 * levels + 3,
        "b={branching_factor}: {worst_reads} reads for one get with {peak} aliases"
    );
}

#[test]
fn many_aliases_with_deletes_small_fanouts() {
    for b in [4, 6, 8, 16] {
        for seed in 0..10 {
            stress(b, 300, 4000, seed);
        }
    }
}

#[test]
fn many_aliases_with_deletes_fanout_64() {
    for seed in 0..5 {
        stress(64, 3000, 20000, seed);
    }
}
