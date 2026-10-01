//! Oblivious data structures stored directly in SAM, ported from the Python
//! implementation (`smart_queue.py`, `smart_stack.py`, `smart_avl_tree.py`), plus an
//! opt-in B-tree ([`btree::SmartBTree`]) with no Python counterpart.
//!
//! As in Python, which has one global SAM, their entries live in the same SAM
//! as the smart pointers: each entry is a *raw* cell of the pointer backend
//! (see [`crate::pointer::RawValueCells`]) holding a [`crate::GraphObject`]
//! variant. Structure labels match Python's, so per-structure counts compare
//! directly.

use crate::Address;

pub use btree::{BTreeNode, SmartBTree};

pub mod avl;
pub mod btree;
pub mod queue;
pub mod stack;

/// A value held by an oblivious data-structure entry. Python stores arbitrary
/// objects; the graph stores integers, smart pointers and small tuples.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Item<P> {
    None,
    Int(i64),
    Pointer(P),
    Tuple(Vec<Item<P>>),
}

/// One `SmartQueue` cell: Python's `(new_tail, value)` tuple.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueEntry<P> {
    pub next: Address,
    pub value: Item<P>,
}

/// One `SmartStack` cell: Python's `(value, previous_top)` tuple.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StackEntry<P> {
    pub value: Item<P>,
    pub next: Option<Address>,
}

/// An AVL-tree key: Python hashes keys with SHA3-256 unless hashing is off.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AvlKey {
    Hash([u8; 32]),
    Int(i64),
}

/// One `SmartAVLTree` node (Python's `AVLNode`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvlNode<P> {
    pub key: AvlKey,
    pub value: Item<P>,
    /// SAM addresses of the left and right children.
    pub children: [Option<Address>; 2],
    pub height: i64,
    pub left_height: i64,
    pub right_height: i64,
    pub balance: i64,
    pub left_balance: Option<i64>,
    pub right_balance: Option<i64>,
}
