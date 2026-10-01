//! An oblivious B-tree: an opt-in, wider alternative to [`SmartAvlTree`].
//!
//! [`SmartAvlTree`] is binary, so a lookup reads about `log2(n)` nodes. A
//! B-tree node holds up to `fanout - 1` keys and is sized to fill a block, so
//! a lookup reads about `log_fanout(n)` nodes: 3 instead of 17 for 128k keys
//! at a fanout of 80. Nothing uses it unless a caller chooses it; the AVL
//! tree, and every number produced with it, is unchanged.
//!
//! # Access pattern
//! Every operation reads one root-to-node path, each node once, then writes
//! the path back bottom-up to freshly allocated addresses (and retires the
//! old ones, as [`SmartAvlTree`] does). A lookup at depth `d` (root = 1)
//! therefore costs `d` reads, `d` allocations and `d` writes, the same per
//! level as the AVL tree. An insert additionally writes one new node per
//! split. Bulk construction ([`SmartBTree::build_tree`]) writes every node
//! once and reads nothing.
//!
//! Keys, values and cells follow [`SmartAvlTree`]: callers pass [`TreeKey`]s
//! (hashed with SHA3-256 unless hashing is off), values are [`Item<P>`], and
//! nodes are stored as `B::raw_cell(GraphObject::BTree(..))` of a pointer
//! backend `B`. Deletion is not supported.
//!
//! [`SmartAvlTree`]: super::avl::SmartAvlTree

use super::avl::TreeKey;
use super::{AvlKey, Item};
use crate::pointer::RawValueCells;
use crate::{Address, GraphObject, MemoryClass, SamError, SingleAccessMachine};
use sha3::{Digest, Sha3_256};
use std::marker::PhantomData;

/// Structure label used for SAM statistics.
pub const STRUCTURE: &str = "SmartBTree";

/// One B-tree node: sorted keys with their values and, for an inner node,
/// `keys.len() + 1` children.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BTreeNode<P> {
    pub keys: Vec<AvlKey>,
    pub values: Vec<Item<P>>,
    /// Empty for a leaf.
    pub children: Vec<Address>,
}

impl<P> BTreeNode<P> {
    fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }
}

/// A SAM-resident B-tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmartBTree {
    /// Address of the root node; `None` for an empty tree.
    pub root_address: Option<Address>,
    /// Maximum children per node; a node holds at most `fanout - 1` keys.
    pub fanout: usize,
    /// Hash keys with SHA3-256, as [`SmartAvlTree`](super::avl::SmartAvlTree) does by default.
    pub hash_enabled: bool,
    /// Allocate oblivious (`true`) or plaintext addresses.
    pub osam: bool,
    len: usize,
}

/// Encoded bytes of one key, value and child slot in
/// [`crate::GraphValueCodec`]'s format: key tag + key, item tag + pointer or
/// integer, child identifier.
pub fn bytes_per_key(hash_enabled: bool) -> usize {
    (1 + if hash_enabled { 32 } else { 8 }) + (1 + 8) + 8
}

/// Fixed bytes of one encoded node besides its keys: object tag, key count,
/// leaf flag, the extra child slot, a one-byte cell tag and the 4-byte
/// fixed-size envelope.
pub const NODE_OVERHEAD_BYTES: usize = 1 + 2 + 1 + 8 + 1 + 4;

impl SmartBTree {
    /// A tree with the given maximum children per node (at least 3).
    pub fn new(fanout: usize, hash_enabled: bool, osam: bool) -> Result<Self, SamError> {
        if fanout < 3 {
            return Err(SamError::InvalidParameter(
                "SmartBTree fanout must be at least 3",
            ));
        }
        if fanout > usize::from(u16::MAX) {
            return Err(SamError::InvalidParameter(
                "SmartBTree fanout must fit in a u16 key count",
            ));
        }
        Ok(Self {
            root_address: None,
            fanout,
            hash_enabled,
            osam,
            len: 0,
        })
    }

    /// The widest fanout whose nodes fit in `block_size` bytes (at least 3,
    /// whether or not that fits).
    pub fn fanout_for_block(block_size: usize, hash_enabled: bool) -> usize {
        let keys = block_size.saturating_sub(NODE_OVERHEAD_BYTES) / bytes_per_key(hash_enabled);
        (keys + 1).clamp(3, usize::from(u16::MAX))
    }

    pub fn is_empty(&self) -> bool {
        self.root_address.is_none()
    }

    /// Number of keys stored.
    pub fn len(&self) -> usize {
        self.len
    }

    /// The stored key for a caller key (SHA3-256 of Python's `str(key)` when
    /// hashing is enabled, as in the AVL tree).
    pub fn tree_key(&self, key: &TreeKey) -> Result<AvlKey, SamError> {
        if self.hash_enabled {
            let digest = Sha3_256::digest(key.python_str().as_bytes());
            let mut bytes = [0_u8; 32];
            bytes.copy_from_slice(&digest);
            return Ok(AvlKey::Hash(bytes));
        }
        match key {
            TreeKey::Int(value) => i64::try_from(*value)
                .map(AvlKey::Int)
                .map_err(|_| SamError::InvalidParameter("unhashed SmartBTree key exceeds i64")),
            TreeKey::Str(_) => Err(SamError::InvalidParameter(
                "unhashed SmartBTree keys must be integers",
            )),
        }
    }

    fn op<'a, B, P, S>(&'a mut self, sam: &'a mut S) -> Op<'a, B, P, S> {
        let class = if self.osam {
            MemoryClass::Oblivious
        } else {
            MemoryClass::Plaintext
        };
        Op {
            tree: self,
            sam,
            class,
            _marker: PhantomData,
        }
    }

    /// Builds the tree from `items` in one pass: one allocation and one write
    /// per node, no reads. The tree must be empty; duplicate keys keep their
    /// last value.
    pub fn build_tree<B, P, S, K>(
        &mut self,
        sam: &mut S,
        items: impl IntoIterator<Item = (K, Item<P>)>,
    ) -> Result<(), SamError>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
        K: Into<TreeKey>,
    {
        if !self.is_empty() {
            return Err(SamError::InvalidParameter(
                "SmartBTree::build_tree needs an empty tree",
            ));
        }
        let mut pairs = Vec::new();
        for (key, value) in items {
            pairs.push((self.tree_key(&key.into())?, value));
        }
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        // Last value wins for duplicate keys.
        let mut dedup: Vec<(AvlKey, Item<P>)> = Vec::with_capacity(pairs.len());
        for (k, v) in pairs {
            match dedup.last_mut() {
                Some(last) if last.0 == k => last.1 = v,
                _ => dedup.push((k, v)),
            }
        }
        if dedup.is_empty() {
            return Ok(());
        }
        self.len = dedup.len();
        // Smallest height whose full tree holds every key.
        let mut height = 1;
        while capacity(self.fanout, height) < dedup.len() {
            height += 1;
        }
        let mut op = self.op::<B, P, S>(sam);
        let root = op.build(dedup, height)?;
        op.tree.root_address = Some(root);
        Ok(())
    }

    /// The value stored under `key`. A node at depth `d` costs `d` reads,
    /// allocations and writes; a missing key costs the same along the
    /// searched path. An empty tree costs nothing.
    pub fn get<B, P, S>(
        &mut self,
        sam: &mut S,
        key: impl Into<TreeKey>,
    ) -> Result<Option<Item<P>>, SamError>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
    {
        let key = self.tree_key(&key.into())?;
        self.op::<B, P, S>(sam)
            .access(&key, |item| Ok(item.clone()))
    }

    /// Runs `update` on the value stored under `key` while its node is held,
    /// then writes the path back with the possibly modified value. Costs
    /// exactly what [`Self::get`] costs; `update` may use the backend and the
    /// SAM, e.g. to dereference a stored pointer, whose refreshed state is
    /// then persisted with the node. Returns `None` (without calling
    /// `update`) when the key is absent.
    pub fn update_with<B, P, S, T, F>(
        &mut self,
        backend: &mut B,
        sam: &mut S,
        key: impl Into<TreeKey>,
        update: F,
    ) -> Result<Option<T>, SamError>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
        F: FnOnce(&mut Item<P>, &mut B, &mut S) -> Result<T, SamError>,
    {
        let key = self.tree_key(&key.into())?;
        let mut op = self.op::<B, P, S>(sam);
        let Some(mut path) = op.read_path(&key)? else {
            return Ok(None);
        };
        let result = match path.found {
            Some(i) => {
                let node = path.nodes.last_mut().expect("path ends at the key's node");
                Some(update(&mut node.values[i], backend, op.sam))
            }
            None => None,
        };
        op.write_path(path.nodes, path.slots)?;
        result.transpose()
    }

    /// Inserts or replaces `key`. Reads the root-to-leaf path once, splits
    /// full nodes bottom-up, and writes the path (plus one node per split, and
    /// a new root if the root splits) back to fresh addresses.
    pub fn insert<B, P, S>(
        &mut self,
        sam: &mut S,
        key: impl Into<TreeKey>,
        value: Item<P>,
    ) -> Result<(), SamError>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
    {
        let key = self.tree_key(&key.into())?;
        let mut op = self.op::<B, P, S>(sam);
        let Some(path) = op.read_path(&key)? else {
            // Empty tree: a single leaf.
            let address = op.alloc();
            op.write(
                address,
                BTreeNode {
                    keys: vec![key],
                    values: vec![value],
                    children: Vec::new(),
                },
            )?;
            op.tree.root_address = Some(address);
            op.tree.len = 1;
            return Ok(());
        };
        op.insert_on_path(path, key, value)
    }
}

/// Keys a full tree of `height` levels holds.
fn capacity(fanout: usize, height: usize) -> usize {
    let mut total: usize = 0;
    let mut level: usize = 1;
    for _ in 0..height {
        total = total.saturating_add(level.saturating_mul(fanout - 1));
        level = level.saturating_mul(fanout);
    }
    total
}

/// The nodes read on the way to a key, root first. `slots[i]` is the child
/// index taken out of `nodes[i]` (one fewer than `nodes`); `found` is the
/// key's index in the last node when present.
struct Path<P> {
    nodes: Vec<BTreeNode<P>>,
    slots: Vec<usize>,
    found: Option<usize>,
}

struct Op<'a, B, P, S> {
    tree: &'a mut SmartBTree,
    sam: &'a mut S,
    class: MemoryClass,
    _marker: PhantomData<fn() -> (B, P)>,
}

impl<B, P, S> Op<'_, B, P, S>
where
    P: Clone,
    B: RawValueCells<GraphObject<P>>,
    S: SingleAccessMachine<B::Cell>,
{
    fn alloc(&mut self) -> Address {
        self.sam.alloc(self.class, STRUCTURE)
    }

    fn write(&mut self, address: Address, node: BTreeNode<P>) -> Result<(), SamError> {
        let cell = B::raw_cell(GraphObject::BTree(Box::new(node)));
        self.sam.write(address, cell, STRUCTURE)
    }

    fn read(&mut self, address: Address) -> Result<BTreeNode<P>, SamError> {
        let node = match self.sam.read(address, STRUCTURE)? {
            Some(cell) => match B::raw_value(cell)? {
                GraphObject::BTree(node) => *node,
                _ => return Err(SamError::InvalidPointerCell("expected a SmartBTree node")),
            },
            None => {
                return Err(SamError::InvalidParameter(
                    "SmartBTree: read an unwritten node",
                ))
            }
        };
        self.sam.retire(address);
        Ok(node)
    }

    /// Builds a subtree of exactly `height` levels over sorted `pairs`,
    /// children before parents, and returns its root's address.
    fn build(&mut self, pairs: Vec<(AvlKey, Item<P>)>, height: usize) -> Result<Address, SamError> {
        let fanout = self.tree.fanout;
        if height == 1 {
            let (keys, values) = pairs.into_iter().unzip();
            let address = self.alloc();
            self.write(
                address,
                BTreeNode {
                    keys,
                    values,
                    children: Vec::new(),
                },
            )?;
            return Ok(address);
        }
        let n = pairs.len();
        let child_cap = capacity(fanout, height - 1);
        // c children need c - 1 separators: n - (c - 1) <= c * child_cap.
        let c = (n + 1).div_ceil(child_cap + 1).max(2);
        let in_children = n - (c - 1);
        let (base, extra) = (in_children / c, in_children % c);
        let mut it = pairs.into_iter();
        let mut keys = Vec::with_capacity(c - 1);
        let mut values = Vec::with_capacity(c - 1);
        let mut children = Vec::with_capacity(c);
        for i in 0..c {
            let size = base + usize::from(i < extra);
            let chunk: Vec<_> = it.by_ref().take(size).collect();
            children.push(self.build(chunk, height - 1)?);
            if i + 1 < c {
                let (k, v) = it.next().expect("separator");
                keys.push(k);
                values.push(v);
            }
        }
        let address = self.alloc();
        self.write(
            address,
            BTreeNode {
                keys,
                values,
                children,
            },
        )?;
        Ok(address)
    }

    /// Reads the path from the root towards `key`, stopping at the node that
    /// holds it or at a leaf. `None` for an empty tree.
    fn read_path(&mut self, key: &AvlKey) -> Result<Option<Path<P>>, SamError> {
        let Some(mut address) = self.tree.root_address else {
            return Ok(None);
        };
        let mut nodes = Vec::new();
        let mut slots = Vec::new();
        loop {
            let node = self.read(address)?;
            let position = node.keys.binary_search(key);
            match position {
                Ok(i) => {
                    nodes.push(node);
                    return Ok(Some(Path {
                        nodes,
                        slots,
                        found: Some(i),
                    }));
                }
                Err(_) if node.is_leaf() => {
                    nodes.push(node);
                    return Ok(Some(Path {
                        nodes,
                        slots,
                        found: None,
                    }));
                }
                Err(i) => {
                    address = node.children[i];
                    slots.push(i);
                    nodes.push(node);
                }
            }
        }
    }

    /// Writes a read path back bottom-up to fresh addresses.
    fn write_path(
        &mut self,
        mut nodes: Vec<BTreeNode<P>>,
        slots: Vec<usize>,
    ) -> Result<(), SamError> {
        let mut child: Option<Address> = None;
        while let Some(mut node) = nodes.pop() {
            if let Some(address) = child {
                node.children[slots[nodes.len()]] = address;
            }
            let address = self.alloc();
            self.write(address, node)?;
            child = Some(address);
        }
        self.tree.root_address = child;
        Ok(())
    }

    fn access<T>(
        &mut self,
        key: &AvlKey,
        f: impl FnOnce(&Item<P>) -> Result<T, SamError>,
    ) -> Result<Option<T>, SamError> {
        let Some(path) = self.read_path(key)? else {
            return Ok(None);
        };
        let result = match path.found {
            Some(i) => Some(f(&path.nodes.last().expect("non-empty path").values[i])?),
            None => None,
        };
        self.write_path(path.nodes, path.slots)?;
        Ok(result)
    }

    fn insert_on_path(
        &mut self,
        path: Path<P>,
        key: AvlKey,
        value: Item<P>,
    ) -> Result<(), SamError> {
        let Path {
            mut nodes,
            slots,
            found,
        } = path;
        if let Some(i) = found {
            nodes.last_mut().expect("non-empty path").values[i] = value;
            return self.write_path(nodes, slots);
        }
        let max_keys = self.tree.fanout - 1;
        // Insert into the leaf, then carry splits upwards. `pending` is the
        // (separator, right sibling address) produced by a split below.
        let mut pending: Option<(AvlKey, Item<P>, Address)> = None;
        let mut child: Option<Address> = None;
        let mut inserted = Some((key, value));
        while let Some(mut node) = nodes.pop() {
            let depth = nodes.len();
            if let Some(address) = child {
                node.children[slots[depth]] = address;
            }
            if let Some((k, v)) = inserted.take() {
                let i = node.keys.binary_search(&k).unwrap_err();
                node.keys.insert(i, k);
                node.values.insert(i, v);
            }
            if let Some((k, v, right)) = pending.take() {
                let i = slots[depth];
                node.keys.insert(i, k);
                node.values.insert(i, v);
                node.children.insert(i + 1, right);
            }
            if node.keys.len() > max_keys {
                // Split around the median; the right half goes to a new node.
                let mid = node.keys.len() / 2;
                let right_keys = node.keys.split_off(mid + 1);
                let right_values = node.values.split_off(mid + 1);
                let right_children = if node.is_leaf() {
                    Vec::new()
                } else {
                    node.children.split_off(mid + 1)
                };
                let median_key = node.keys.pop().expect("median");
                let median_value = node.values.pop().expect("median");
                let right_address = self.alloc();
                self.write(
                    right_address,
                    BTreeNode {
                        keys: right_keys,
                        values: right_values,
                        children: right_children,
                    },
                )?;
                pending = Some((median_key, median_value, right_address));
            }
            let address = self.alloc();
            self.write(address, node)?;
            child = Some(address);
        }
        if let Some((k, v, right)) = pending {
            // The root split: a new root above the two halves.
            let address = self.alloc();
            self.write(
                address,
                BTreeNode {
                    keys: vec![k],
                    values: vec![v],
                    children: vec![child.expect("old root"), right],
                },
            )?;
            child = Some(address);
        }
        self.tree.root_address = child;
        self.tree.len += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pointer::RecursivePointers;
    use crate::{AccessPolicy, DryRunSam};

    type B = RecursivePointers;
    type P = crate::pointer::RecursivePointer;

    fn sam() -> DryRunSam<GraphObject<P>> {
        DryRunSam::new(AccessPolicy::MULTI_WRITE)
    }

    fn depth_of(tree: &SmartBTree, sam: &mut DryRunSam<GraphObject<P>>, key: u64) -> u64 {
        let mut t = *tree;
        let before = sam.stats().operations.reads;
        t.get::<B, P, _>(sam, key).unwrap();
        sam.stats().operations.reads - before
    }

    #[test]
    fn bulk_build_finds_every_key_and_stays_shallow() {
        for (n, fanout) in [(1usize, 3usize), (10, 3), (1000, 4), (128_000, 81)] {
            let mut s = sam();
            let mut tree = SmartBTree::new(fanout, true, true).unwrap();
            tree.build_tree::<B, P, _, _>(
                &mut s,
                (0..n as u64).map(|k| (k, Item::Int(k as i64 * 3))),
            )
            .unwrap();
            assert_eq!(s.stats().operations.reads, 0, "build reads nothing");
            let mut height = 1;
            while capacity(fanout, height) < n {
                height += 1;
            }
            for k in (0..n as u64).step_by((n / 97).max(1)) {
                assert_eq!(
                    tree.get::<B, P, _>(&mut s, k).unwrap(),
                    Some(Item::Int(k as i64 * 3))
                );
            }
            assert_eq!(tree.get::<B, P, _>(&mut s, n as u64 + 5).unwrap(), None);
            assert!(depth_of(&tree, &mut s, 0) <= height as u64);
        }
    }

    #[test]
    fn a_lookup_costs_one_read_alloc_and_write_per_level() {
        let mut s = sam();
        let mut tree = SmartBTree::new(81, true, true).unwrap();
        tree.build_tree::<B, P, _, _>(&mut s, (0..128_000u64).map(|k| (k, Item::Int(0))))
            .unwrap();
        let before = s.stats().operations;
        tree.get::<B, P, _>(&mut s, 12_345u64).unwrap();
        let after = s.stats().operations;
        let reads = after.reads - before.reads;
        assert!(
            (1..=3).contains(&reads),
            "fanout 81 over 128k keys: {reads} levels"
        );
        assert_eq!(after.writes - before.writes, reads);
        // Recycled addresses are not counted as allocations, and every read
        // address is retired, so the path rewrite reuses them.
        assert!(after.allocations - before.allocations <= reads);
    }

    #[test]
    fn inserts_match_a_map_and_keep_lookups_working() {
        use std::collections::BTreeMap;
        for fanout in [3, 4, 7] {
            let mut s = sam();
            let mut tree = SmartBTree::new(fanout, false, true).unwrap();
            let mut model = BTreeMap::new();
            let mut x: u64 = 1;
            for _ in 0..600 {
                x = x
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let k = (x >> 40) % 400;
                tree.insert::<B, P, _>(&mut s, k, Item::Int(k as i64 + 1))
                    .unwrap();
                model.insert(k, k as i64 + 1);
            }
            assert_eq!(tree.len(), model.len());
            for k in 0..420u64 {
                let want = model.get(&k).map(|&v| Item::Int(v));
                assert_eq!(
                    tree.get::<B, P, _>(&mut s, k).unwrap(),
                    want,
                    "fanout {fanout} key {k}"
                );
            }
        }
    }

    #[test]
    fn update_with_persists_the_change() {
        let mut s = sam();
        let mut tree = SmartBTree::new(5, true, true).unwrap();
        tree.build_tree::<B, P, _, _>(&mut s, (0..50u64).map(|k| (k, Item::Int(k as i64))))
            .unwrap();
        let mut backend = RecursivePointers::default();
        let got = tree
            .update_with(&mut backend, &mut s, 7u64, |item, _, _| {
                *item = Item::Int(700);
                Ok(1)
            })
            .unwrap();
        assert_eq!(got, Some(1));
        assert_eq!(
            tree.get::<B, P, _>(&mut s, 7u64).unwrap(),
            Some(Item::Int(700))
        );
        assert_eq!(
            tree.update_with(&mut backend, &mut s, 999u64, |_, _, _| Ok(0))
                .unwrap(),
            None
        );
    }

    #[test]
    fn fanout_for_block_fits_the_block() {
        for bs in [256usize, 4096, 32768] {
            let f = SmartBTree::fanout_for_block(bs, true);
            assert!(NODE_OVERHEAD_BYTES + (f - 1) * bytes_per_key(true) <= bs);
            assert!(NODE_OVERHEAD_BYTES + f * bytes_per_key(true) > bs);
        }
        assert_eq!(SmartBTree::fanout_for_block(64, true), 3);
    }
}
