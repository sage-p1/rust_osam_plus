//! Port of Python's `smart_avl_tree.py` (`SmartAVLTree`).
//!
//! An AVL tree whose nodes live in SAM. Every access reads a node once and
//! writes it back to a freshly allocated address, so the sequence and number
//! of `alloc`/`read`/`write`/`retire` (Python's `return_available_address`)
//! calls below mirror the Python implementation exactly, including its
//! quirks (see the comments marked `Python quirk`).
//!
//! # Keys
//! Callers pass anything convertible into [`TreeKey`] (integers or strings).
//! With hashing enabled (the default) the stored key is
//! `SHA3-256(str(key).encode("utf-8"))`, compared lexicographically like
//! Python `bytes`; integers format as Python's `str(int)` (decimal). With
//! hashing disabled only integer keys are supported (stored as
//! [`AvlKey::Int`]); an unhashed string key returns
//! [`SamError::InvalidParameter`] because [`AvlKey`] has no string variant.
//!
//! # Values and cells
//! Values are [`Item<P>`]; nodes are stored as
//! `B::raw_cell(GraphObject::Avl(..))` of a pointer backend `B`, named with a
//! turbofish because it cannot be inferred:
//! `tree.get::<MultiWritePointers, _, _>(&mut sam, 5)`. Methods whose
//! arguments/results do not mention `P` need it spelled out too
//! (`tree.clear::<B, P, _>(&mut sam)`).
//!
//! Python's `__getitem__`/`search`/`delete` return `None` both for a missing
//! key and for a stored `None`; the port returns `Option<Item<P>>` (`None` =
//! missing) and [`SmartAvlTree::contains`] reproduces Python's
//! `search(key) is not None`.

use super::{AvlKey, AvlNode, Item};
use crate::pointer::RawValueCells;
use crate::{Address, GraphObject, MemoryClass, SamError, SingleAccessMachine};
use sha3::{Digest, Sha3_256};
use std::collections::{HashMap, VecDeque};
use std::marker::PhantomData;

/// Structure label used for SAM statistics (Python's `self.structure`).
pub const STRUCTURE: &str = "SmartAVLTree";

/// A caller-side key: Python's `int | str`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum TreeKey {
    Int(i128),
    Str(String),
}

impl TreeKey {
    /// Python's `str(key)`.
    pub fn python_str(&self) -> String {
        match self {
            Self::Int(value) => value.to_string(),
            Self::Str(value) => value.clone(),
        }
    }
}

macro_rules! tree_key_from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for TreeKey {
            fn from(value: $t) -> Self {
                Self::Int(i128::from(value))
            }
        }
    )*};
}
tree_key_from_int!(i8, i16, i32, i64, u8, u16, u32, u64, i128);

impl From<usize> for TreeKey {
    fn from(value: usize) -> Self {
        Self::Int(value as i128)
    }
}

impl From<&str> for TreeKey {
    fn from(value: &str) -> Self {
        Self::Str(value.to_owned())
    }
}

impl From<String> for TreeKey {
    fn from(value: String) -> Self {
        Self::Str(value)
    }
}

impl From<&String> for TreeKey {
    fn from(value: &String) -> Self {
        Self::Str(value.clone())
    }
}

/// A child slot of a client-side node: Python's `None | int | "REPLACE"`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Child {
    Empty,
    At(Address),
    /// Python's `"REPLACE"` marker: the child moved and gets a new address
    /// when it is written back. Never stored in SAM.
    Replace,
}

impl From<Option<Address>> for Child {
    fn from(address: Option<Address>) -> Self {
        address.map_or(Child::Empty, Child::At)
    }
}

impl Child {
    fn address(self) -> Result<Address, SamError> {
        match self {
            Child::At(address) => Ok(address),
            Child::Empty => Err(SamError::InvalidParameter("SmartAVLTree: missing child")),
            Child::Replace => Err(SamError::InvalidParameter(
                "SmartAVLTree: unresolved REPLACE child",
            )),
        }
    }

    fn optional(self) -> Result<Option<Address>, SamError> {
        match self {
            Child::Empty => Ok(None),
            Child::At(address) => Ok(Some(address)),
            Child::Replace => Err(SamError::InvalidParameter(
                "SmartAVLTree: unresolved REPLACE child",
            )),
        }
    }
}

/// Client-side working copy of an [`AvlNode`] (children may be `Replace`).
#[derive(Clone, Debug)]
struct Node<P> {
    key: AvlKey,
    value: Item<P>,
    children: [Child; 2],
    height: i64,
    left_height: i64,
    right_height: i64,
    balance: i64,
    left_balance: Option<i64>,
    right_balance: Option<i64>,
}

impl<P> Node<P> {
    fn leaf(key: AvlKey, value: Item<P>) -> Self {
        Self {
            key,
            value,
            children: [Child::Empty; 2],
            height: 1,
            left_height: 0,
            right_height: 0,
            balance: 0,
            left_balance: None,
            right_balance: None,
        }
    }

    fn from_stored(node: AvlNode<P>) -> Self {
        Self {
            key: node.key,
            value: node.value,
            children: [node.children[0].into(), node.children[1].into()],
            height: node.height,
            left_height: node.left_height,
            right_height: node.right_height,
            balance: node.balance,
            left_balance: node.left_balance,
            right_balance: node.right_balance,
        }
    }

    fn into_stored(self) -> Result<AvlNode<P>, SamError> {
        Ok(AvlNode {
            key: self.key,
            value: self.value,
            children: [self.children[0].optional()?, self.children[1].optional()?],
            height: self.height,
            left_height: self.left_height,
            right_height: self.right_height,
            balance: self.balance,
            left_balance: self.left_balance,
            right_balance: self.right_balance,
        })
    }
}

/// Python's `__right_rotate(node_y, node_z)`: raise `y` (left), lower `z`.
fn right_rotate<P>(y: &mut Node<P>, z: &mut Node<P>) {
    z.children[0] = y.children[1];
    z.left_height = y.right_height;
    z.height = 1 + z.left_height.max(z.right_height);
    z.left_balance = y.right_balance;
    z.balance = z.left_height - z.right_height;

    y.right_height = z.height;
    y.children[1] = Child::Replace;
    y.height = 1 + y.left_height.max(y.right_height);
    y.right_balance = Some(z.balance);
    y.balance = y.left_height - y.right_height;
}

/// Python's `__left_rotate(node_x, node_y)`: lower `x` (left), raise `y`.
fn left_rotate<P>(x: &mut Node<P>, y: &mut Node<P>) {
    x.children[1] = y.children[0];
    x.right_height = y.left_height;
    x.height = 1 + x.left_height.max(x.right_height);
    x.right_balance = y.left_balance;
    x.balance = x.left_height - x.right_height;

    y.left_height = x.height;
    y.children[0] = Child::Replace;
    y.height = 1 + y.left_height.max(y.right_height);
    y.left_balance = Some(x.balance);
    y.balance = y.left_height - y.right_height;
}

/// Two distinct mutable elements of a slice.
fn pair_mut<T>(items: &mut [T], a: usize, b: usize) -> (&mut T, &mut T) {
    assert_ne!(a, b);
    if a < b {
        let (low, high) = items.split_at_mut(b);
        (&mut low[a], &mut high[0])
    } else {
        let (low, high) = items.split_at_mut(a);
        (&mut high[0], &mut low[b])
    }
}

fn pop<P>(nodes: &mut Vec<Node<P>>) -> Result<Node<P>, SamError> {
    nodes
        .pop()
        .ok_or(SamError::InvalidParameter("SmartAVLTree: empty node path"))
}

fn missing_child_key() -> SamError {
    SamError::InvalidParameter("SmartAVLTree: child key used before assignment")
}

/// A SAM-built AVL tree (Python's `SmartAVLTree`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmartAvlTree {
    /// Address of the root node; `None` for an empty tree.
    pub root_address: Option<Address>,
    /// Hash keys with SHA3-256 (Python default `True`).
    pub hash_enabled: bool,
    /// Allocate oblivious (`true`, Python default) or plaintext addresses.
    pub avl_tree_osam: bool,
}

impl Default for SmartAvlTree {
    fn default() -> Self {
        Self::new(true, true)
    }
}

impl SmartAvlTree {
    /// Python's `SmartAVLTree(hash_enabled, avl_tree_osam)`.
    pub fn new(hash_enabled: bool, avl_tree_osam: bool) -> Self {
        Self {
            root_address: None,
            hash_enabled,
            avl_tree_osam,
        }
    }

    /// True when the tree has no root.
    pub fn is_empty(&self) -> bool {
        self.root_address.is_none()
    }

    /// Python's `__get_key`: the stored key for a caller key.
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
                .map_err(|_| SamError::InvalidParameter("unhashed SmartAVLTree key exceeds i64")),
            TreeKey::Str(_) => Err(SamError::InvalidParameter(
                "unhashed SmartAVLTree keys must be integers",
            )),
        }
    }

    fn op<'a, B, P, S>(&'a mut self, sam: &'a mut S) -> Op<'a, B, P, S>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
    {
        let class = if self.avl_tree_osam {
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

    /// Python's `build_tree(dict)`: `items` are the dict's `(key, value)`
    /// pairs in insertion order (duplicate keys behave like a dict literal:
    /// first position, last value). Python's list/set inputs map to pairs as
    /// `(e[0], e[1])` for 2+-element sequences and `(e, e)` otherwise.
    ///
    /// Builds a perfectly balanced tree when empty (one allocation and one
    /// write per element, no reads); otherwise inserts each pair.
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
        let mut dict: Vec<(TreeKey, Item<P>)> = Vec::new();
        let mut index: HashMap<TreeKey, usize> = HashMap::new();
        for (key, value) in items {
            let key = key.into();
            match index.get(&key) {
                Some(&position) => dict[position].1 = value,
                None => {
                    index.insert(key.clone(), dict.len());
                    dict.push((key, value));
                }
            }
        }
        self.op::<B, P, S>(sam).build_tree(dict)
    }

    /// Python's `__contains__`: `search(key) is not None`.
    pub fn contains<B, P, S>(
        &mut self,
        sam: &mut S,
        key: impl Into<TreeKey>,
    ) -> Result<bool, SamError>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
    {
        Ok(!matches!(
            self.get::<B, P, S>(sam, key)?,
            None | Some(Item::None)
        ))
    }

    /// Python's `search(key)` (same as [`Self::get`]).
    pub fn search<B, P, S>(
        &mut self,
        sam: &mut S,
        key: impl Into<TreeKey>,
    ) -> Result<Option<Item<P>>, SamError>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
    {
        self.get::<B, P, S>(sam, key)
    }

    /// Python's `__getitem__(key)`. For a node at depth `d` (root = 1):
    /// `d` reads, `d` allocations, `d` writes. A missing key costs the same
    /// along the searched path. An empty tree costs nothing.
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
        let key = key.into();
        let mut op = self.op::<B, P, S>(sam);
        match op.get_node(&key)? {
            Some((address, node)) => {
                let value = node.value.clone();
                op.write(address, node)?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Runs `update` on the value stored under `key` while its node is held
    /// (the search path is already rewritten), then writes the node back
    /// with the possibly modified value. Costs exactly what [`Self::get`]
    /// costs; `update` may use the backend and the SAM, e.g. to smart-copy a
    /// stored pointer, whose refreshed state is then persisted with the node.
    /// Returns `None` (without calling `update`) when the key is absent.
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
        let key = key.into();
        let mut op = self.op::<B, P, S>(sam);
        let Some((address, mut node)) = op.get_node(&key)? else {
            return Ok(None);
        };
        let result = update(&mut node.value, backend, op.sam);
        op.write(address, node)?;
        result.map(Some)
    }

    /// Python's `height(key)`: the stored height of the key's node.
    pub fn height<B, P, S>(
        &mut self,
        sam: &mut S,
        key: impl Into<TreeKey>,
    ) -> Result<Option<i64>, SamError>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
    {
        let key = key.into();
        let mut op = self.op::<B, P, S>(sam);
        match op.get_node(&key)? {
            Some((address, node)) => {
                let height = node.height;
                op.write(address, node)?;
                Ok(Some(height))
            }
            None => Ok(None),
        }
    }

    /// Python's `insert(key, value)` / `__setitem__`: insert or update.
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
        let key = key.into();
        self.op::<B, P, S>(sam).set_item(&key, value)
    }

    /// Python's `delete(key)` / `__delitem__`: removes the key and returns
    /// its value (`None` when absent or the tree is empty).
    pub fn delete<B, P, S>(
        &mut self,
        sam: &mut S,
        key: impl Into<TreeKey>,
    ) -> Result<Option<Item<P>>, SamError>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
    {
        let key = key.into();
        self.op::<B, P, S>(sam).delete(&key)
    }

    /// Python's `bfs()`: all values in breadth-first order.
    pub fn bfs<B, P, S>(&mut self, sam: &mut S) -> Result<Vec<Item<P>>, SamError>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
    {
        self.op::<B, P, S>(sam).bfs(None)
    }

    /// Python's `bfs(key)`: values of `key`'s subtree in breadth-first order
    /// (empty, after visiting every node, when the key is absent).
    pub fn bfs_from<B, P, S>(
        &mut self,
        sam: &mut S,
        key: impl Into<TreeKey>,
    ) -> Result<Vec<Item<P>>, SamError>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
    {
        let key = key.into();
        self.op::<B, P, S>(sam).bfs(Some(&key))
    }

    /// Python's `dfs()`: all values in pre-order (left before right).
    pub fn dfs<B, P, S>(&mut self, sam: &mut S) -> Result<Vec<Item<P>>, SamError>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
    {
        self.op::<B, P, S>(sam).dfs(None)
    }

    /// Python's `dfs(key)`: pre-order values of `key`'s subtree.
    pub fn dfs_from<B, P, S>(
        &mut self,
        sam: &mut S,
        key: impl Into<TreeKey>,
    ) -> Result<Vec<Item<P>>, SamError>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
    {
        let key = key.into();
        self.op::<B, P, S>(sam).dfs(Some(&key))
    }

    /// Python's `clear()`: reads (and retires) every node, writes nothing.
    pub fn clear<B, P, S>(&mut self, sam: &mut S) -> Result<(), SamError>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
    {
        self.op::<B, P, S>(sam).clear()
    }
}

/// One tree operation bound to a SAM and a cell backend.
struct Op<'a, B, P, S> {
    tree: &'a mut SmartAvlTree,
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

    fn write(&mut self, address: Address, node: Node<P>) -> Result<(), SamError> {
        let cell = B::raw_cell(GraphObject::Avl(Box::new(node.into_stored()?)));
        self.sam.write(address, cell, STRUCTURE)
    }

    fn read(&mut self, address: Address) -> Result<Node<P>, SamError> {
        match self.sam.read(address, STRUCTURE)? {
            Some(cell) => match B::raw_value(cell)? {
                GraphObject::Avl(node) => Ok(Node::from_stored(*node)),
                _ => Err(SamError::InvalidPointerCell("expected a SmartAVLTree node")),
            },
            None => Err(SamError::InvalidParameter(
                "SmartAVLTree: read an unwritten node",
            )),
        }
    }

    fn retire(&mut self, address: Address) {
        self.sam.retire(address);
    }

    /// Python's `__write_back_last_node`.
    fn write_back_last_node(
        &mut self,
        child_address: Option<Address>,
        nodes: &mut Vec<Node<P>>,
    ) -> Result<Address, SamError> {
        let mut node = pop(nodes)?;
        for child in &mut node.children {
            if *child == Child::Replace {
                *child = child_address.into();
            }
        }
        let address = self.alloc();
        self.write(address, node)?;
        Ok(address)
    }

    fn build_tree(&mut self, dict: Vec<(TreeKey, Item<P>)>) -> Result<(), SamError> {
        if dict.is_empty() {
            return Ok(());
        }
        if self.tree.root_address.is_some() {
            for (key, value) in dict {
                self.set_item(&key, value)?;
            }
            return Ok(());
        }
        let mut elements = Vec::with_capacity(dict.len());
        for (key, value) in dict {
            elements.push((self.tree.tree_key(&key)?, value));
        }
        // Stable, like Python's list.sort.
        elements.sort_by(|a, b| a.0.cmp(&b.0));
        let root = self.alloc();
        self.tree.root_address = Some(root);
        self.build_range(&elements, 0, elements.len() - 1, root)?;
        Ok(())
    }

    /// Python's `__build_tree(elements, left, right, address)` for
    /// `left <= right`; returns `(height, balance)`.
    fn build_range(
        &mut self,
        elements: &[(AvlKey, Item<P>)],
        left: usize,
        right: usize,
        address: Address,
    ) -> Result<(i64, Option<i64>), SamError> {
        let middle = (left + right) / 2;
        let mut children = [Child::Empty; 2];
        let (mut left_height, mut left_balance) = (0, None);
        if middle > left {
            let left_address = self.alloc();
            children[0] = Child::At(left_address);
            (left_height, left_balance) =
                self.build_range(elements, left, middle - 1, left_address)?;
        }
        let (mut right_height, mut right_balance) = (0, None);
        if right > middle {
            let right_address = self.alloc();
            children[1] = Child::At(right_address);
            (right_height, right_balance) =
                self.build_range(elements, middle + 1, right, right_address)?;
        }
        let height = 1 + left_height.max(right_height);
        let balance = left_height - right_height;
        let node = Node {
            key: elements[middle].0.clone(),
            value: elements[middle].1.clone(),
            children,
            height,
            left_height,
            right_height,
            balance,
            left_balance,
            right_balance,
        };
        self.write(address, node)?;
        Ok((height, Some(balance)))
    }

    /// Python's `__get_node`: returns the matching node and the address it
    /// must be written back to (the caller writes it). Every other node on
    /// the path is written back here.
    fn get_node(&mut self, key: &TreeKey) -> Result<Option<(Address, Node<P>)>, SamError> {
        let Some(root) = self.tree.root_address else {
            return Ok(None);
        };
        let hash_key = self.tree.tree_key(key)?;
        let mut addresses = VecDeque::new();

        let mut current = self.read(root)?;
        self.retire(root);
        let new_root = self.alloc();
        self.tree.root_address = Some(new_root);
        addresses.push_back(new_root);

        loop {
            let current_address = addresses.pop_front().ok_or(SamError::InvalidParameter(
                "SmartAVLTree: empty address queue",
            ))?;
            if hash_key == current.key {
                return Ok(Some((current_address, current)));
            }
            let index = usize::from(hash_key >= current.key);
            let next_address = match current.children[index] {
                Child::Empty => {
                    // leaf reached: write back the final node
                    self.write(current_address, current)?;
                    return Ok(None);
                }
                child => child.address()?,
            };
            let new_next_address = self.alloc();
            current.children[index] = Child::At(new_next_address);
            addresses.push_back(new_next_address);
            self.write(current_address, current)?;
            current = self.read(next_address)?;
            self.retire(next_address);
        }
    }

    /// Python's `__setitem__`.
    fn set_item(&mut self, key: &TreeKey, value: Item<P>) -> Result<(), SamError> {
        let hash_key = self.tree.tree_key(key)?;

        let Some(root) = self.tree.root_address else {
            let address = self.alloc();
            self.tree.root_address = Some(address);
            return self.write(address, Node::leaf(hash_key, value));
        };

        let mut current = self.read(root)?;
        self.retire(root);

        let mut value = Some(value);
        let mut nodes: Vec<Node<P>> = Vec::new();
        let mut is_insert = true;
        loop {
            if hash_key == current.key {
                current.value = value.take().unwrap_or(Item::None);
                is_insert = false;
                nodes.push(current);
                break;
            }
            let index = usize::from(hash_key >= current.key);
            let next = current.children[index];
            current.children[index] = Child::Replace;
            nodes.push(current);
            match next {
                Child::Empty => {
                    let leaf = Node::leaf(hash_key.clone(), value.take().unwrap_or(Item::None));
                    nodes.push(leaf);
                    break;
                }
                child => {
                    let next_address = child.address()?;
                    current = self.read(next_address)?;
                    self.retire(next_address);
                }
            }
        }

        let mut child_address: Option<Address> = None;

        if is_insert {
            let mut i = nodes.len() as isize - 1;
            let mut child_hash_key: Option<AvlKey> = None;
            while i >= 0 {
                let iu = i as usize;
                if nodes[iu].key != hash_key {
                    let child_key = child_hash_key.as_ref().ok_or_else(missing_child_key)?;
                    let (node, child) = pair_mut(&mut nodes, iu, iu + 1);
                    if *child_key < node.key {
                        node.left_height = child.height;
                        node.left_balance = Some(child.balance);
                    } else {
                        node.right_height = child.height;
                        node.right_balance = Some(child.balance);
                    }
                    node.height = 1 + node.left_height.max(node.right_height);
                    node.balance = node.left_height - node.right_height;
                }

                // write back nodes too deep to take part in a rotation
                if iu + 3 < nodes.len() {
                    child_address = Some(self.write_back_last_node(child_address, &mut nodes)?);
                }

                let balance = nodes[iu].balance;
                let key_lt_child = |child: &Option<AvlKey>| -> Result<bool, SamError> {
                    Ok(hash_key < *child.as_ref().ok_or_else(missing_child_key)?)
                };
                let key_gt_child = |child: &Option<AvlKey>| -> Result<bool, SamError> {
                    Ok(hash_key > *child.as_ref().ok_or_else(missing_child_key)?)
                };

                // insertions need at most one rotation
                if balance > 1 && key_lt_child(&child_hash_key)? {
                    // left left
                    let (y, z) = pair_mut(&mut nodes, iu + 1, iu);
                    right_rotate(y, z);
                    let x_address = self.write_back_last_node(child_address, &mut nodes)?;
                    nodes[iu + 1].children[0] = Child::At(x_address);
                    let node_z = nodes.remove(iu);
                    let z_address = self.alloc();
                    child_address = Some(z_address);
                    nodes[iu].children[1] = Child::At(z_address);
                    self.write(z_address, node_z)?;
                    break;
                }
                if balance < -1 && key_gt_child(&child_hash_key)? {
                    // right right
                    let (x, y) = pair_mut(&mut nodes, iu, iu + 1);
                    left_rotate(x, y);
                    let z_address = self.write_back_last_node(child_address, &mut nodes)?;
                    nodes[iu + 1].children[1] = Child::At(z_address);
                    let node_x = nodes.remove(iu);
                    let x_address = self.alloc();
                    child_address = Some(x_address);
                    nodes[iu].children[0] = Child::At(x_address);
                    self.write(x_address, node_x)?;
                    break;
                }
                if balance > 1 && key_gt_child(&child_hash_key)? {
                    // left right
                    {
                        let (x, y) = pair_mut(&mut nodes, iu + 1, iu + 2);
                        left_rotate(x, y);
                    }
                    {
                        let (y, z) = pair_mut(&mut nodes, iu + 2, iu);
                        right_rotate(y, z);
                    }
                    let mut node_z = nodes.remove(iu);
                    let mut node_x = nodes.remove(iu);
                    if node_z.children[0] == Child::Replace {
                        node_z.children[0] = child_address.into();
                    } else {
                        node_x.children[1] = child_address.into();
                    }
                    let x_address = self.alloc();
                    self.write(x_address, node_x)?;
                    nodes[iu].children[0] = Child::At(x_address);
                    let z_address = self.alloc();
                    child_address = Some(z_address);
                    self.write(z_address, node_z)?;
                    break;
                }
                if balance < -1 && key_lt_child(&child_hash_key)? {
                    // right left
                    {
                        let (y, z) = pair_mut(&mut nodes, iu + 2, iu + 1);
                        right_rotate(y, z);
                    }
                    {
                        let (x, y) = pair_mut(&mut nodes, iu, iu + 2);
                        left_rotate(x, y);
                    }
                    let mut node_x = nodes.remove(iu);
                    let mut node_z = nodes.remove(iu);
                    if node_z.children[0] == Child::Replace {
                        node_z.children[0] = child_address.into();
                    } else {
                        node_x.children[1] = child_address.into();
                    }
                    let x_address = self.alloc();
                    self.write(x_address, node_x)?;
                    nodes[iu].children[0] = Child::At(x_address);
                    let z_address = self.alloc();
                    child_address = Some(z_address);
                    self.write(z_address, node_z)?;
                    break;
                }

                child_hash_key = Some(nodes[iu].key.clone());
                i -= 1;
            }

            // update the parent's balance for the last node that may change
            if i >= 1 {
                let iu = i as usize;
                let (parent, node) = pair_mut(&mut nodes, iu - 1, iu);
                if parent.key < node.key {
                    parent.right_balance = Some(node.balance);
                } else {
                    parent.left_balance = Some(node.balance);
                }
            }
        }

        while !nodes.is_empty() {
            child_address = Some(self.write_back_last_node(child_address, &mut nodes)?);
        }
        self.tree.root_address = child_address;
        Ok(())
    }

    /// Python's `__delitem__`.
    fn delete(&mut self, key: &TreeKey) -> Result<Option<Item<P>>, SamError> {
        let Some(root) = self.tree.root_address else {
            return Ok(None);
        };
        let hash_key = self.tree.tree_key(key)?;

        let mut current = self.read(root)?;
        self.retire(root);

        let mut nodes: Vec<Node<P>> = Vec::new();
        let mut result = None;
        let mut found = false;
        loop {
            if hash_key == current.key {
                result = Some(current.value.clone());
                found = true;
                nodes.push(current);
                break;
            }
            let index = usize::from(hash_key >= current.key);
            let next = current.children[index];
            current.children[index] = Child::Replace;
            nodes.push(current);
            match next {
                Child::Empty => break,
                child => {
                    let next_address = child.address()?;
                    current = self.read(next_address)?;
                    self.retire(next_address);
                }
            }
        }

        let mut child_address: Option<Address> = None;

        if found {
            let node = pop(&mut nodes)?;
            if node.children[0] != Child::Empty && node.children[1] != Child::Empty {
                // two children: replace by the in-order predecessor
                nodes.push(node);
                let target = nodes.len() - 1;
                let left_address = nodes[target].children[0].address()?;
                let left = self.read(left_address)?;
                nodes.push(left);
                self.retire(left_address);
                nodes[target].children[0] = Child::Replace;

                // Python quirk: after reading the right child it returns the
                // *new* node's right child (`nodes[-1].children[1]`), i.e. the
                // address it is about to read next, instead of the address it
                // just read. The Rust SAM cannot read a retired address, so
                // that retire is deferred until right after the read; no SAM
                // operation lies in between, so counts and the recycling order
                // are unchanged.
                let mut deferred_retire: Option<Address> = None;
                while nodes[nodes.len() - 1].children[1] != Child::Empty {
                    let address = nodes[nodes.len() - 1].children[1].address()?;
                    let next = self.read(address)?;
                    if deferred_retire == Some(address) {
                        self.retire(address);
                        deferred_retire = None;
                    }
                    let returned = next.children[1];
                    nodes.push(next);
                    if let Some(returned) = returned.optional()? {
                        deferred_retire = Some(returned);
                    }
                    let parent = nodes.len() - 2;
                    nodes[parent].children[1] = Child::Replace;
                }
                if let Some(address) = deferred_retire {
                    self.retire(address);
                }

                let max_child = pop(&mut nodes)?;
                nodes[target].value = max_child.value.clone();
                nodes[target].key = max_child.key.clone();
                child_address = max_child.children[0].optional()?;
            } else if node.children[0] != Child::Empty {
                child_address = node.children[0].optional()?;
            } else if node.children[1] != Child::Empty {
                child_address = node.children[1].optional()?;
            }
            // node.delete(): the removed node is simply dropped

            let mut i = nodes.len() as isize - 1;
            while i >= 0 {
                let iu = i as usize;
                let (child_height, child_balance) = if iu == nodes.len() - 1 {
                    if child_address.is_none() {
                        (0, None)
                    } else {
                        (1, Some(0))
                    }
                } else {
                    (nodes[iu + 1].height, Some(nodes[iu + 1].balance))
                };
                {
                    let node = &mut nodes[iu];
                    if node.children[0] == Child::Replace {
                        node.left_height = child_height;
                        node.left_balance = child_balance;
                    } else {
                        node.right_height = child_height;
                        node.right_balance = child_balance;
                    }
                    node.height = 1 + node.left_height.max(node.right_height);
                    node.balance = node.left_height - node.right_height;
                }
                let balance = nodes[iu].balance;
                let left_balance = nodes[iu].left_balance;
                let right_balance = nodes[iu].right_balance;

                if iu + 3 < nodes.len() {
                    child_address = Some(self.write_back_last_node(child_address, &mut nodes)?);
                }

                if balance > 1 && left_balance.is_some_and(|b| b >= 0) {
                    // left left
                    if nodes.len() - 1 - iu == 2 {
                        child_address = Some(self.write_back_last_node(child_address, &mut nodes)?);
                    }
                    let mut left_child = None;
                    if nodes.len() - 1 - iu == 1 {
                        if nodes[iu + 1].key < nodes[iu].key {
                            let mut y = pop(&mut nodes)?;
                            y.children[0] = child_address.into();
                            left_child = Some(y);
                        } else {
                            child_address =
                                Some(self.write_back_last_node(child_address, &mut nodes)?);
                        }
                    }
                    let mut left_child = match left_child {
                        Some(node) => node,
                        None => {
                            let address = nodes[iu].children[0].address()?;
                            let node = self.read(address)?;
                            self.retire(address);
                            nodes[iu].children[0] = Child::Replace;
                            nodes[iu].children[1] = child_address.into();
                            node
                        }
                    };
                    right_rotate(&mut left_child, &mut nodes[iu]);
                    let address = self.alloc();
                    child_address = Some(address);
                    self.write(address, nodes[iu].clone())?;
                    nodes.pop();
                    nodes.push(left_child);
                } else if balance < -1 && right_balance.is_some_and(|b| b <= 0) {
                    // right right
                    if nodes.len() - 1 - iu == 2 {
                        child_address = Some(self.write_back_last_node(child_address, &mut nodes)?);
                    }
                    let mut right_child = None;
                    if nodes.len() - 1 - iu == 1 {
                        if nodes[iu + 1].key > nodes[iu].key {
                            let mut y = pop(&mut nodes)?;
                            y.children[1] = child_address.into();
                            right_child = Some(y);
                        } else {
                            child_address =
                                Some(self.write_back_last_node(child_address, &mut nodes)?);
                        }
                    }
                    let mut right_child = match right_child {
                        Some(node) => node,
                        None => {
                            let address = nodes[iu].children[1].address()?;
                            let node = self.read(address)?;
                            self.retire(address);
                            nodes[iu].children[1] = Child::Replace;
                            nodes[iu].children[0] = child_address.into();
                            node
                        }
                    };
                    left_rotate(&mut nodes[iu], &mut right_child);
                    let address = self.alloc();
                    child_address = Some(address);
                    self.write(address, nodes[iu].clone())?;
                    nodes.pop();
                    nodes.push(right_child);
                } else if balance > 1 && left_balance.is_some_and(|b| b <= 0) {
                    // left right
                    let mut left_child = None;
                    let mut right_child = None;
                    if nodes.len() - 1 - iu == 2 {
                        if nodes[iu + 1].key < nodes[iu].key
                            && nodes[iu + 1].key < nodes[iu + 2].key
                        {
                            let mut y = pop(&mut nodes)?;
                            if y.children[0] == Child::Replace {
                                y.children[0] = child_address.into();
                            } else {
                                y.children[1] = child_address.into();
                            }
                            right_child = Some(y);
                            left_child = Some(pop(&mut nodes)?);
                        } else if nodes[iu + 1].key < nodes[iu].key {
                            child_address =
                                Some(self.write_back_last_node(child_address, &mut nodes)?);
                            let mut x = pop(&mut nodes)?;
                            x.children[0] = child_address.into();
                            left_child = Some(x);
                        } else {
                            for _ in 0..2 {
                                child_address =
                                    Some(self.write_back_last_node(child_address, &mut nodes)?);
                            }
                        }
                    } else if nodes.len() - 1 - iu == 1 {
                        if nodes[iu + 1].key < nodes[iu].key {
                            let mut x = pop(&mut nodes)?;
                            x.children[0] = child_address.into();
                            left_child = Some(x);
                        } else {
                            child_address =
                                Some(self.write_back_last_node(child_address, &mut nodes)?);
                        }
                    }
                    let mut left_child = match left_child {
                        Some(node) => node,
                        None => {
                            let address = nodes[iu].children[0].address()?;
                            let node = self.read(address)?;
                            self.retire(address);
                            nodes[iu].children[0] = Child::Replace;
                            nodes[iu].children[1] = child_address.into();
                            node
                        }
                    };
                    let mut right_child = match right_child {
                        Some(node) => node,
                        None => {
                            let address = left_child.children[1].address()?;
                            let node = self.read(address)?;
                            self.retire(address);
                            left_child.children[1] = Child::Replace;
                            node
                        }
                    };
                    left_rotate(&mut left_child, &mut right_child);
                    right_rotate(&mut right_child, &mut nodes[iu]);

                    let z_address = self.alloc();
                    self.write(z_address, nodes[iu].clone())?;
                    right_child.children[1] = Child::At(z_address);
                    nodes.pop();

                    let x_address = self.alloc();
                    child_address = Some(x_address);
                    self.write(x_address, left_child)?;
                    nodes.push(right_child);
                } else if balance < -1 && right_balance.is_some_and(|b| b > 0) {
                    // right left
                    let mut left_child = None;
                    let mut right_child = None;
                    if nodes.len() - 1 - iu == 2 {
                        if nodes[iu + 1].key > nodes[iu].key
                            && nodes[iu + 1].key > nodes[iu + 2].key
                        {
                            let mut y = pop(&mut nodes)?;
                            if y.children[0] == Child::Replace {
                                y.children[0] = child_address.into();
                            } else {
                                y.children[1] = child_address.into();
                            }
                            left_child = Some(y);
                            right_child = Some(pop(&mut nodes)?);
                        } else if nodes[iu + 1].key > nodes[iu].key {
                            child_address =
                                Some(self.write_back_last_node(child_address, &mut nodes)?);
                            let mut z = pop(&mut nodes)?;
                            z.children[1] = child_address.into();
                            right_child = Some(z);
                        } else {
                            for _ in 0..2 {
                                child_address =
                                    Some(self.write_back_last_node(child_address, &mut nodes)?);
                            }
                        }
                    } else if nodes.len() - 1 - iu == 1 {
                        if nodes[iu + 1].key > nodes[iu].key {
                            let mut z = pop(&mut nodes)?;
                            z.children[1] = child_address.into();
                            right_child = Some(z);
                        } else {
                            child_address =
                                Some(self.write_back_last_node(child_address, &mut nodes)?);
                        }
                    }
                    let mut right_child = match right_child {
                        Some(node) => node,
                        None => {
                            let address = nodes[iu].children[1].address()?;
                            let node = self.read(address)?;
                            self.retire(address);
                            nodes[iu].children[1] = Child::Replace;
                            nodes[iu].children[0] = child_address.into();
                            node
                        }
                    };
                    let mut left_child = match left_child {
                        Some(node) => node,
                        None => {
                            let address = right_child.children[0].address()?;
                            let node = self.read(address)?;
                            self.retire(address);
                            right_child.children[0] = Child::Replace;
                            node
                        }
                    };
                    right_rotate(&mut left_child, &mut right_child);
                    left_rotate(&mut nodes[iu], &mut left_child);

                    let x_address = self.alloc();
                    self.write(x_address, nodes[iu].clone())?;
                    left_child.children[0] = Child::At(x_address);
                    nodes.pop();

                    let z_address = self.alloc();
                    child_address = Some(z_address);
                    self.write(z_address, right_child)?;
                    nodes.push(left_child);
                }

                i -= 1;
            }
        }

        while !nodes.is_empty() {
            child_address = Some(self.write_back_last_node(child_address, &mut nodes)?);
        }
        self.tree.root_address = child_address;
        Ok(result)
    }

    /// Python's `bfs(key)`.
    fn bfs(&mut self, key: Option<&TreeKey>) -> Result<Vec<Item<P>>, SamError> {
        let mut values = Vec::new();
        let Some(root) = self.tree.root_address else {
            return Ok(values);
        };
        let hash_key = key.map(|key| self.tree.tree_key(key)).transpose()?;

        let mut addresses = VecDeque::new();
        let mut nodes = VecDeque::new();
        nodes.push_back(self.read(root)?);
        self.retire(root);
        let new_root = self.alloc();
        self.tree.root_address = Some(new_root);
        addresses.push_back(new_root);

        let mut can_collect_values = key.is_none();
        while !addresses.is_empty() && !nodes.is_empty() {
            let (Some(mut current), Some(current_address)) =
                (nodes.pop_front(), addresses.pop_front())
            else {
                break;
            };
            if hash_key.as_ref() == Some(&current.key) {
                can_collect_values = true;
                // every other queued node lies on another path: write back
                while let Some(node) = nodes.pop_front() {
                    let address = addresses.pop_front().ok_or(SamError::InvalidParameter(
                        "SmartAVLTree: bfs queue mismatch",
                    ))?;
                    self.write(address, node)?;
                }
            }
            if can_collect_values {
                values.push(current.value.clone());
            }
            for index in 0..2 {
                if let Child::At(address) = current.children[index] {
                    nodes.push_back(self.read(address)?);
                    self.retire(address);
                    let new_address = self.alloc();
                    addresses.push_back(new_address);
                    current.children[index] = Child::At(new_address);
                }
            }
            self.write(current_address, current)?;
        }
        Ok(values)
    }

    /// Python's `dfs(key)`.
    fn dfs(&mut self, key: Option<&TreeKey>) -> Result<Vec<Item<P>>, SamError> {
        let mut values = Vec::new();
        let Some(root) = self.tree.root_address else {
            return Ok(values);
        };
        let hash_key = key.map(|key| self.tree.tree_key(key)).transpose()?;

        let mut addresses = Vec::new();
        let mut nodes = Vec::new();
        nodes.push(self.read(root)?);
        self.retire(root);
        let new_root = self.alloc();
        self.tree.root_address = Some(new_root);
        addresses.push(new_root);

        let mut can_collect_values = key.is_none();
        while !addresses.is_empty() && !nodes.is_empty() {
            let (Some(mut current), Some(current_address)) = (nodes.pop(), addresses.pop()) else {
                break;
            };
            if hash_key.as_ref() == Some(&current.key) {
                can_collect_values = true;
                while let Some(node) = nodes.pop() {
                    let address = addresses.pop().ok_or(SamError::InvalidParameter(
                        "SmartAVLTree: dfs stack mismatch",
                    ))?;
                    self.write(address, node)?;
                }
            }
            if can_collect_values {
                values.push(current.value.clone());
            }
            // right to left so the left subtree is visited first
            for index in [1, 0] {
                if let Child::At(address) = current.children[index] {
                    nodes.push(self.read(address)?);
                    self.retire(address);
                    let new_address = self.alloc();
                    addresses.push(new_address);
                    current.children[index] = Child::At(new_address);
                }
            }
            self.write(current_address, current)?;
        }
        Ok(values)
    }

    /// Python's `clear()`.
    fn clear(&mut self) -> Result<(), SamError> {
        let Some(root) = self.tree.root_address else {
            return Ok(());
        };
        let mut nodes = VecDeque::new();
        nodes.push_back(self.read(root)?);
        self.retire(root);
        self.tree.root_address = None;
        while let Some(current) = nodes.pop_front() {
            for child in current.children {
                if let Child::At(address) = child {
                    nodes.push_back(self.read(address)?);
                    self.retire(address);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pointer::{
        MultiWritePointer, MultiWritePointers, RecursivePointer, RecursivePointers,
    };
    use crate::{AccessPolicy, DryRunSam};
    use std::collections::BTreeMap;

    type MwCell = <MultiWritePointers as crate::pointer::SmartPointerBackend<
        GraphObject<MultiWritePointer>,
    >>::Cell;

    /// Reads the whole tree through the SAM (bfs cost is irrelevant here)
    /// and checks structural AVL invariants via a snapshot walk.
    fn check_invariants(sam: &DryRunSam<MwCell>, tree: &SmartAvlTree) -> usize {
        let snapshot = sam.snapshot();
        let cells: HashMap<u64, AvlNode<MultiWritePointer>> = snapshot
            .blocks
            .into_iter()
            .filter_map(|block| {
                match <MultiWritePointers as RawValueCells<GraphObject<P>>>::raw_value(block.value)
                {
                    Ok(GraphObject::Avl(node)) => Some((block.identifier, *node)),
                    _ => None,
                }
            })
            .collect();
        fn walk(
            cells: &HashMap<u64, AvlNode<MultiWritePointer>>,
            address: Option<Address>,
            low: Option<&AvlKey>,
            high: Option<&AvlKey>,
            count: &mut usize,
        ) -> (i64, Option<i64>) {
            let Some(address) = address else {
                return (0, None);
            };
            let Address::Oblivious(id) = address else {
                panic!("plaintext node")
            };
            let node = &cells[&id];
            *count += 1;
            if let Some(low) = low {
                assert!(node.key > *low);
            }
            if let Some(high) = high {
                assert!(node.key < *high);
            }
            let (lh, lb) = walk(cells, node.children[0], low, Some(&node.key), count);
            let (rh, rb) = walk(cells, node.children[1], Some(&node.key), high, count);
            assert_eq!(node.left_height, lh);
            assert_eq!(node.right_height, rh);
            assert_eq!(node.left_balance, lb);
            assert_eq!(node.right_balance, rb);
            assert_eq!(node.height, 1 + lh.max(rh));
            assert_eq!(node.balance, lh - rh);
            assert!(node.balance.abs() <= 1, "unbalanced node");
            (node.height, Some(node.balance))
        }
        let mut count = 0;
        walk(&cells, tree.root_address, None, None, &mut count);
        count
    }

    type B = MultiWritePointers;
    type P = MultiWritePointer;

    #[test]
    fn hashing_matches_python_sha3_of_str() {
        let tree = SmartAvlTree::default();
        // python3 -c 'from hashlib import sha3_256; print(sha3_256(b"42").hexdigest()[:16])'
        let AvlKey::Hash(bytes) = tree.tree_key(&TreeKey::from(42)).unwrap() else {
            panic!()
        };
        let hex: String = bytes[..8].iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "4e169ddf479c8cd9");
        assert_eq!(
            tree.tree_key(&TreeKey::from(42)).unwrap(),
            tree.tree_key(&TreeKey::from("42")).unwrap()
        );
        let unhashed = SmartAvlTree::new(false, true);
        assert_eq!(
            unhashed.tree_key(&TreeKey::from(-3)).unwrap(),
            AvlKey::Int(-3)
        );
        assert!(unhashed.tree_key(&TreeKey::from("a")).is_err());
    }

    #[test]
    fn unhashed_build_matches_python_unit_test_orders() {
        let mut sam = DryRunSam::<MwCell>::new(AccessPolicy::MULTI_WRITE);
        let mut tree = SmartAvlTree::new(false, true);
        tree.build_tree::<B, P, _, _>(&mut sam, (1..16).map(|i| (i, Item::Int(i))))
            .unwrap();
        let ints = |v: Vec<Item<P>>| -> Vec<i64> {
            v.into_iter()
                .map(|item| match item {
                    Item::Int(i) => i,
                    _ => panic!(),
                })
                .collect()
        };
        assert_eq!(
            ints(tree.bfs::<B, P, _>(&mut sam).unwrap()),
            vec![8, 4, 12, 2, 6, 10, 14, 1, 3, 5, 7, 9, 11, 13, 15]
        );
        assert_eq!(
            ints(tree.bfs_from::<B, P, _>(&mut sam, 12).unwrap()),
            vec![12, 10, 14, 9, 11, 13, 15]
        );
        assert_eq!(
            ints(tree.dfs::<B, P, _>(&mut sam).unwrap()),
            vec![8, 4, 2, 1, 3, 6, 5, 7, 12, 10, 9, 11, 14, 13, 15]
        );
        assert_eq!(
            ints(tree.dfs_from::<B, P, _>(&mut sam, 4).unwrap()),
            vec![4, 2, 1, 3, 6, 5, 7]
        );
        assert_eq!(tree.height::<B, P, _>(&mut sam, 8).unwrap(), Some(4));
        assert_eq!(tree.height::<B, P, _>(&mut sam, 3).unwrap(), Some(1));
        for i in 16..32 {
            tree.insert::<B, P, _>(&mut sam, i, Item::Int(i)).unwrap();
        }
        assert_eq!(
            ints(tree.bfs::<B, P, _>(&mut sam).unwrap()),
            vec![
                16, 8, 24, 4, 12, 20, 28, 2, 6, 10, 14, 18, 22, 26, 30, 1, 3, 5, 7, 9, 11, 13, 15,
                17, 19, 21, 23, 25, 27, 29, 31
            ]
        );
        for i in 16..32 {
            assert_eq!(
                tree.delete::<B, P, _>(&mut sam, i).unwrap(),
                Some(Item::Int(i))
            );
        }
        assert_eq!(
            ints(tree.dfs::<B, P, _>(&mut sam).unwrap()),
            vec![8, 4, 2, 1, 3, 6, 5, 7, 12, 10, 9, 11, 14, 13, 15]
        );
        assert_eq!(check_invariants(&sam, &tree), 15);
        tree.clear::<B, P, _>(&mut sam).unwrap();
        assert!(tree.is_empty());
        assert_eq!(tree.get::<B, P, _>(&mut sam, 3).unwrap(), None);
    }

    #[test]
    fn random_operations_match_btreemap_and_stay_balanced() {
        for (hashed, seed) in [(true, 1_u64), (false, 2), (true, 3), (false, 4)] {
            let mut sam = DryRunSam::<MwCell>::new(AccessPolicy::MULTI_WRITE);
            let mut tree = SmartAvlTree::new(hashed, true);
            let mut model = BTreeMap::new();
            let mut x = seed;
            for step in 0..3000_i64 {
                x = (x * 1103515245 + 12345) % (1 << 31);
                let key = ((x >> 8) % 200) as i64;
                match x % 10 {
                    0..=4 => {
                        tree.insert::<B, P, _>(&mut sam, key, Item::Int(step))
                            .unwrap();
                        model.insert(key, step);
                    }
                    5..=7 => {
                        let got = tree.delete::<B, P, _>(&mut sam, key).unwrap();
                        assert_eq!(got, model.remove(&key).map(Item::Int));
                    }
                    _ => {
                        let got = tree.get::<B, P, _>(&mut sam, key).unwrap();
                        assert_eq!(got, model.get(&key).copied().map(Item::Int));
                        assert_eq!(
                            tree.contains::<B, P, _>(&mut sam, key).unwrap(),
                            model.contains_key(&key)
                        );
                    }
                }
                if step % 97 == 0 {
                    assert_eq!(check_invariants(&sam, &tree), model.len());
                }
            }
            assert_eq!(check_invariants(&sam, &tree), model.len());
            let mut values: Vec<i64> = tree
                .bfs::<B, P, _>(&mut sam)
                .unwrap()
                .into_iter()
                .map(|item| match item {
                    Item::Int(i) => i,
                    _ => panic!(),
                })
                .collect();
            values.sort_unstable();
            let mut expected: Vec<i64> = model.values().copied().collect();
            expected.sort_unstable();
            assert_eq!(values, expected);
        }
    }

    #[test]
    fn recursive_policy_and_string_keys() {
        let mut sam = DryRunSam::new(AccessPolicy::RECURSIVE);
        let mut tree = SmartAvlTree::default();
        for i in 0..50 {
            tree.insert::<RecursivePointers, RecursivePointer, _>(
                &mut sam,
                format!("v{i}"),
                Item::Int(i),
            )
            .unwrap();
        }
        for i in 0..50 {
            assert_eq!(
                tree.get::<RecursivePointers, RecursivePointer, _>(&mut sam, format!("v{i}"))
                    .unwrap(),
                Some(Item::Int(i))
            );
        }
        for i in (0..50).step_by(2) {
            assert_eq!(
                tree.delete::<RecursivePointers, RecursivePointer, _>(&mut sam, format!("v{i}"))
                    .unwrap(),
                Some(Item::Int(i))
            );
        }
        let values = tree
            .dfs::<RecursivePointers, RecursivePointer, _>(&mut sam)
            .unwrap();
        assert_eq!(values.len(), 25);
        // Address recycling keeps fresh allocations near the tree size.
        assert!(sam.stats().operations.allocations < 200);
    }
}
