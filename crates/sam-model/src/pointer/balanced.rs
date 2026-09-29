//! Balanced r-ary multi-write pointer with downward pointers.
//!
//! The aliases of one value are the leaves of a **complete b-ary tree whose
//! shape is a function of the alias count `f` alone**: all leaves sit at depth
//! `H = ceil(log_b f)` (and `H = 0` for a single alias, which is the root
//! itself), and leaf `j` lies on the path spelled by the base-`b` digits of `j`.
//! Dereferences never restructure the tree; copies and deletes keep it
//! complete by inserting at position `f` and moving the last leaf into a
//! deleted slot.
//!
//! Cells (single read, multiple writes):
//!
//! * `Root { value, meta }`: the shared value, the alias count, the height and
//!   the address of the root's child list.
//! * `Node { parent, group, index }`: one per alias and per internal node. It
//!   stores the parent's address and the parent's whole child group. Each
//!   group entry is `(node, down)`, where `down` is that child's child-list
//!   cell (`None` for aliases). So every member's cell is determined by its
//!   parent and the group, and off-path members can be rewritten without
//!   reading them.
//! * `Kids { group }`: the **downward pointers** of an internal node, i.e. its
//!   child group. Dereferences only rewrite it in place and never read it.
//!   Copies and deletes read it to walk down to the frontier leaf.
//!
//! Costs (reads, which are what the r-ary backend leaks):
//!
//! * dereference: exactly `H + 1` (fewer only when the walk meets a root that
//!   is live in the client cache);
//! * copy and delete: exactly `2H` (padded), or 1 when `H = 0`.
//!
//! Writes per dereference: every group on the path (at most `b` each), one
//! child list per internal path node, and the root. That is at most
//! `(b + 1) H + 1`, and at most `b + 1` per read.

use std::sync::Arc;

use super::{CacheablePointerBackend, CachedDeref, CachedRoots, SmartPointerBackend, ValueCodec};
use crate::{Address, MemoryClass, SamError, SingleAccessMachine};

const STRUCTURE: &str = "SmartPointerBalancedRary";
const UNRESOLVED: usize = usize::MAX;

/// One member of a child group.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BalancedEntry {
    pub node: Address,
    /// The member's child-list cell; `None` for an alias.
    pub down: Option<Address>,
}

/// Root metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BalancedMeta {
    pub count: u32,
    pub height: u8,
    /// Child-list cell of the root (`None` when `height == 0`).
    pub down: Option<Address>,
}

#[derive(Clone, Debug)]
pub enum BalancedCell<V> {
    Root {
        value: V,
        meta: BalancedMeta,
    },
    Node {
        parent: Address,
        group: Arc<[BalancedEntry]>,
        index: usize,
    },
    Kids {
        group: Arc<[BalancedEntry]>,
    },
}

/// Client-side pointer: the address of this alias's cell.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BalancedPointer {
    head: Option<Address>,
}

impl BalancedPointer {
    pub fn head(&self) -> Option<Address> {
        self.head
    }
    pub fn from_head(head: Address) -> Self {
        Self { head: Some(head) }
    }
}

/// Staged write-back of a cached root.
#[derive(Clone, Copy, Debug)]
pub struct BalancedWriteback {
    pub root: Address,
    pub meta: BalancedMeta,
}

/// Height of the complete tree for `f` aliases.
pub fn balanced_height(f: u32, b: usize) -> u8 {
    let (mut h, mut cap) = (0u8, 1u64);
    while cap < f as u64 {
        cap *= b as u64;
        h += 1;
    }
    h
}

/// Exact reads of one dereference of an object with `f` aliases (no cache hit).
pub fn balanced_deref_reads(f: u32, b: usize) -> u64 {
    balanced_height(f.max(1), b) as u64 + 1
}

/// Exact reads of one copy or delete.
pub fn balanced_update_reads(f: u32, b: usize) -> u64 {
    match balanced_height(f.max(1), b) {
        0 => 1,
        h => 2 * h as u64,
    }
}

fn err(msg: &'static str) -> SamError {
    SamError::InvalidPointerCell(msg)
}

fn read_cell<V: Clone, S: SingleAccessMachine<BalancedCell<V>>>(
    sam: &mut S,
    address: Address,
) -> Result<BalancedCell<V>, SamError> {
    let cell = sam
        .read(address, STRUCTURE)?
        .ok_or(err("unwritten balanced-pointer cell"))?;
    sam.retire(address);
    Ok(match cell {
        BalancedCell::Node {
            parent,
            group,
            index,
        } => {
            let index = if index == UNRESOLVED {
                group
                    .iter()
                    .position(|e| e.node == address)
                    .ok_or(err("node missing from its own group"))?
            } else {
                index
            };
            BalancedCell::Node {
                parent,
                group,
                index,
            }
        }
        other => other,
    })
}

fn alloc<V: Clone, S: SingleAccessMachine<BalancedCell<V>>>(sam: &mut S) -> Address {
    sam.alloc(MemoryClass::Oblivious, STRUCTURE)
}

/// Writes every member of `group` under `parent`, and the group's child list.
fn write_group<V: Clone, S: SingleAccessMachine<BalancedCell<V>>>(
    sam: &mut S,
    parent: Address,
    down: Address,
    group: &[BalancedEntry],
) -> Result<(), SamError> {
    let shared: Arc<[BalancedEntry]> = group.into();
    let mut writes: Vec<(Address, BalancedCell<V>)> = group
        .iter()
        .enumerate()
        .map(|(index, e)| {
            (
                e.node,
                BalancedCell::Node {
                    parent,
                    group: shared.clone(),
                    index,
                },
            )
        })
        .collect();
    writes.push((down, BalancedCell::Kids { group: shared }));
    sam.write_batch(writes, STRUCTURE)
}

fn pad<V: Clone, S: SingleAccessMachine<BalancedCell<V>>>(
    sam: &mut S,
    n: u64,
) -> Result<(), SamError> {
    for _ in 0..n {
        let a = alloc(sam);
        let _ = sam.read(a, "BalancedPointerPadding")?;
    }
    Ok(())
}

struct NoRoots;
impl CachedRoots<BalancedWriteback> for NoRoots {
    fn is_cached(&self, _root: Address) -> bool {
        false
    }
    fn writeback_mut(&mut self, _root: Address) -> Option<&mut BalancedWriteback> {
        None
    }
}

// ---------------------------------------------------------------------------
// The in-memory view used by copies and deletes.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Kid {
    Leaf(Address),
    Node(usize),
    Unknown(BalancedEntry),
}

#[derive(Clone, Debug)]
struct KNode {
    addr: Address,
    down: Address,
    kids: Vec<Kid>,
    level: usize,
    alive: bool,
}

struct View<V> {
    nodes: Vec<KNode>,
    root: usize,
    /// `None` when the root is live in the client cache.
    value: Option<V>,
    meta: BalancedMeta,
    /// Index of the alias's leaf parent and the alias's slot there.
    slot: (usize, usize),
    reads: u64,
}

impl<V: Clone> View<V> {
    /// Reads the rest of an alias's path (`H >= 1`), given its already-read
    /// cell, stopping at a cached root.
    fn load<S: SingleAccessMachine<BalancedCell<V>>>(
        sam: &mut S,
        first: BalancedCell<V>,
        roots: &mut dyn CachedRoots<BalancedWriteback>,
    ) -> Result<Self, SamError> {
        let BalancedCell::Node {
            parent,
            group,
            index,
        } = first
        else {
            return Err(err("expected an alias node"));
        };
        let mut reads = 1;
        let mut nodes: Vec<KNode> = Vec::new();
        let mut kids: Vec<Kid> = group.iter().map(|e| Kid::Leaf(e.node)).collect();
        let (mut level, mut cur) = (1usize, parent);
        loop {
            if roots.is_cached(cur) {
                let meta = roots
                    .writeback_mut(cur)
                    .ok_or(err("cached root without write-back"))?
                    .meta;
                nodes.push(KNode {
                    addr: cur,
                    down: meta.down.ok_or(err("root without child list"))?,
                    kids,
                    level,
                    alive: true,
                });
                let root = nodes.len() - 1;
                return Ok(Self {
                    nodes,
                    root,
                    value: None,
                    meta,
                    slot: (0, index),
                    reads,
                });
            }
            let cell = read_cell(sam, cur)?;
            reads += 1;
            match cell {
                BalancedCell::Root { value, meta } => {
                    if meta.height as usize != level {
                        return Err(err("tree height does not match its root"));
                    }
                    let down = meta.down.ok_or(err("root without child list"))?;
                    nodes.push(KNode {
                        addr: alloc(sam),
                        down,
                        kids,
                        level,
                        alive: true,
                    });
                    let root = nodes.len() - 1;
                    return Ok(Self {
                        nodes,
                        root,
                        value: Some(value),
                        meta,
                        slot: (0, index),
                        reads,
                    });
                }
                BalancedCell::Node {
                    parent: up,
                    group: pg,
                    index: pi,
                } => {
                    let down = pg[pi].down.ok_or(err("internal node without child list"))?;
                    nodes.push(KNode {
                        addr: alloc(sam),
                        down,
                        kids,
                        level,
                        alive: true,
                    });
                    let me = nodes.len() - 1;
                    kids = pg
                        .iter()
                        .enumerate()
                        .map(|(j, e)| {
                            if j == pi {
                                Kid::Node(me)
                            } else {
                                Kid::Unknown(*e)
                            }
                        })
                        .collect();
                    level += 1;
                    cur = up;
                }
                BalancedCell::Kids { .. } => return Err(err("child list on an upward path")),
            }
        }
    }

    fn height(&self) -> usize {
        self.nodes[self.root].level
    }

    /// Makes child `d` of node `v` a loaded node, reading its child list if needed.
    fn open_child<S: SingleAccessMachine<BalancedCell<V>>>(
        &mut self,
        sam: &mut S,
        v: usize,
        d: usize,
    ) -> Result<usize, SamError> {
        match self.nodes[v].kids[d] {
            Kid::Node(i) => Ok(i),
            Kid::Leaf(_) => Err(err("descended into an alias")),
            Kid::Unknown(e) => {
                let down = e.down.ok_or(err("internal entry without child list"))?;
                let BalancedCell::Kids { group } = read_cell(sam, down)? else {
                    return Err(err("expected a child list"));
                };
                self.reads += 1;
                let level = self.nodes[v].level - 1;
                let kids = group
                    .iter()
                    .map(|g| {
                        if g.down.is_none() {
                            Kid::Leaf(g.node)
                        } else {
                            Kid::Unknown(*g)
                        }
                    })
                    .collect();
                // The child list was read, so it moves; the node cell keeps its address.
                self.nodes.push(KNode {
                    addr: e.node,
                    down: alloc(sam),
                    kids,
                    level,
                    alive: true,
                });
                let i = self.nodes.len() - 1;
                self.nodes[v].kids[d] = Kid::Node(i);
                Ok(i)
            }
        }
    }

    fn entry(&self, k: Kid) -> BalancedEntry {
        match k {
            Kid::Leaf(a) => BalancedEntry {
                node: a,
                down: None,
            },
            Kid::Node(i) => BalancedEntry {
                node: self.nodes[i].addr,
                down: Some(self.nodes[i].down),
            },
            Kid::Unknown(e) => e,
        }
    }

    /// Rewrites every loaded node's group and child list.
    fn commit<S: SingleAccessMachine<BalancedCell<V>>>(&self, sam: &mut S) -> Result<(), SamError> {
        for v in self.nodes.iter().filter(|v| v.alive) {
            let group: Vec<BalancedEntry> = v.kids.iter().map(|&k| self.entry(k)).collect();
            if !group.is_empty() {
                write_group(sam, v.addr, v.down, &group)?;
            }
        }
        Ok(())
    }
}

fn digit(pos: u64, level: usize, b: usize) -> usize {
    ((pos / (b as u64).pow(level as u32)) % b as u64) as usize
}

// ---------------------------------------------------------------------------
// The backend.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct BalancedPointers {
    b: usize,
    new_calls: usize,
}

impl BalancedPointers {
    pub fn new(b: usize) -> Result<Self, SamError> {
        if !(2..=255).contains(&b) {
            return Err(SamError::InvalidParameter(
                "branching factor must be in 2..=255",
            ));
        }
        Ok(Self { b, new_calls: 0 })
    }

    pub fn branching_factor(&self) -> usize {
        self.b
    }

    /// Dereference; walks stop at cached roots.
    fn deref_inner<V: Clone, S: SingleAccessMachine<BalancedCell<V>>>(
        &mut self,
        sam: &mut S,
        ptr: &mut BalancedPointer,
        roots: &mut dyn CachedRoots<BalancedWriteback>,
    ) -> Result<CachedDeref<V, BalancedWriteback>, SamError> {
        let head = ptr.head.ok_or(err("deleted balanced pointer"))?;
        if roots.is_cached(head) {
            return Ok(CachedDeref::Hit { root: head });
        }
        let fresh = alloc(sam);
        ptr.head = Some(fresh);
        let (parent, mut group) = match read_cell(sam, head)? {
            BalancedCell::Root { value, meta } => {
                return Ok(CachedDeref::Miss {
                    root: fresh,
                    value,
                    writeback: BalancedWriteback { root: fresh, meta },
                });
            }
            BalancedCell::Node {
                parent,
                group,
                index,
            } => {
                let mut g = group.to_vec();
                g[index].node = fresh;
                (parent, g)
            }
            BalancedCell::Kids { .. } => return Err(err("pointer to a child list")),
        };
        let mut cur = parent;
        loop {
            if roots.is_cached(cur) {
                let meta = roots
                    .writeback_mut(cur)
                    .ok_or(err("cached root without write-back"))?
                    .meta;
                write_group(
                    sam,
                    cur,
                    meta.down.ok_or(err("root without child list"))?,
                    &group,
                )?;
                return Ok(CachedDeref::Hit { root: cur });
            }
            match read_cell(sam, cur)? {
                BalancedCell::Root { value, meta } => {
                    let root = alloc(sam);
                    write_group(
                        sam,
                        root,
                        meta.down.ok_or(err("root without child list"))?,
                        &group,
                    )?;
                    return Ok(CachedDeref::Miss {
                        root,
                        value,
                        writeback: BalancedWriteback { root, meta },
                    });
                }
                BalancedCell::Node {
                    parent: up,
                    group: pg,
                    index: pi,
                } => {
                    let node = alloc(sam);
                    write_group(
                        sam,
                        node,
                        pg[pi].down.ok_or(err("internal node without child list"))?,
                        &group,
                    )?;
                    let mut pg = pg.to_vec();
                    pg[pi].node = node;
                    group = pg;
                    cur = up;
                }
                BalancedCell::Kids { .. } => return Err(err("child list on an upward path")),
            }
        }
    }

    /// Uncached dereference that writes the (possibly updated) value back.
    fn access<V: Clone, S: SingleAccessMachine<BalancedCell<V>>, R>(
        &mut self,
        sam: &mut S,
        ptr: &mut BalancedPointer,
        op: impl FnOnce(&mut V, &BalancedMeta, &mut Self, &mut S) -> Result<R, SamError>,
    ) -> Result<R, SamError> {
        match self.deref_inner(sam, ptr, &mut NoRoots)? {
            CachedDeref::Miss {
                mut value,
                writeback,
                ..
            } => {
                let r = op(&mut value, &writeback.meta, self, sam)?;
                sam.write(
                    writeback.root,
                    BalancedCell::Root {
                        value,
                        meta: writeback.meta,
                    },
                    STRUCTURE,
                )?;
                Ok(r)
            }
            CachedDeref::Hit { .. } => Err(err("uncached walk reported a hit")),
        }
    }

    /// Adds one alias at position `f`; `ptr` is refreshed.
    fn copy_one<V: Clone, S: SingleAccessMachine<BalancedCell<V>>>(
        &mut self,
        sam: &mut S,
        ptr: &mut BalancedPointer,
        roots: &mut dyn CachedRoots<BalancedWriteback>,
    ) -> Result<BalancedPointer, SamError> {
        let head = ptr.head.ok_or(err("deleted balanced pointer"))?;
        let fresh = alloc(sam);
        let n = alloc(sam);
        // A single alias is the root itself.
        let single = if roots.is_cached(head) {
            let wb = roots
                .writeback_mut(head)
                .ok_or(err("cached root without write-back"))?;
            if wb.meta.height == 0 {
                Some((head, None, wb.meta))
            } else {
                None
            }
        } else {
            None
        };
        let single = match single {
            Some(s) => Some(s),
            None if !roots.is_cached(head) => {
                // Peek by reading: an H = 0 root, or an alias node.
                match read_cell::<V, S>(sam, head)? {
                    BalancedCell::Root { value, meta } => Some((alloc(sam), Some(value), meta)),
                    cell @ BalancedCell::Node { .. } => {
                        return self.copy_general(sam, ptr, cell, fresh, n, roots);
                    }
                    BalancedCell::Kids { .. } => return Err(err("pointer to a child list")),
                }
            }
            None => None,
        };
        let (root, value, meta) = single.ok_or(err("cached alias is not a root"))?;
        let down = alloc(sam);
        write_group::<V, S>(
            sam,
            root,
            down,
            &[
                BalancedEntry {
                    node: fresh,
                    down: None,
                },
                BalancedEntry {
                    node: n,
                    down: None,
                },
            ],
        )?;
        let meta = BalancedMeta {
            count: meta.count + 1,
            height: 1,
            down: Some(down),
        };
        match value {
            Some(value) => sam.write(root, BalancedCell::Root { value, meta }, STRUCTURE)?,
            None => {
                roots
                    .writeback_mut(root)
                    .ok_or(err("cached root vanished"))?
                    .meta = meta
            }
        }
        ptr.head = Some(fresh);
        Ok(BalancedPointer { head: Some(n) })
    }

    #[allow(clippy::too_many_arguments)]
    fn copy_general<V: Clone, S: SingleAccessMachine<BalancedCell<V>>>(
        &mut self,
        sam: &mut S,
        ptr: &mut BalancedPointer,
        first: BalancedCell<V>,
        fresh: Address,
        n: Address,
        roots: &mut dyn CachedRoots<BalancedWriteback>,
    ) -> Result<BalancedPointer, SamError> {
        let b = self.b;
        // `load` reads `head` itself; hand it the cell we already read.
        let mut view = View::load(sam, first, roots)?;
        let h0 = view.height();
        let (p1, s) = view.slot;
        view.nodes[p1].kids[s] = Kid::Leaf(fresh);
        let pos = view.meta.count as u64;
        let mut h = h0;
        if pos == (b as u64).pow(h as u32) {
            // Full: a new level below the root takes all of its children.
            let r = view.root;
            let kids = std::mem::take(&mut view.nodes[r].kids);
            view.nodes.push(KNode {
                addr: alloc(sam),
                down: alloc(sam),
                kids,
                level: h,
                alive: true,
            });
            let u = view.nodes.len() - 1;
            view.nodes[r].kids = vec![Kid::Node(u)];
            view.nodes[r].level = h + 1;
            h += 1;
        }
        let mut v = view.root;
        for level in (1..=h).rev() {
            let d = digit(pos, level - 1, b);
            let len = view.nodes[v].kids.len();
            if level == 1 {
                if d != len {
                    return Err(err("frontier slot is not the next free one"));
                }
                view.nodes[v].kids.push(Kid::Leaf(n));
            } else if d < len {
                v = view.open_child(sam, v, d)?;
            } else {
                if d != len {
                    return Err(err("frontier child is not the next free one"));
                }
                view.nodes.push(KNode {
                    addr: alloc(sam),
                    down: alloc(sam),
                    kids: Vec::new(),
                    level: level - 1,
                    alive: true,
                });
                let c = view.nodes.len() - 1;
                view.nodes[v].kids.push(Kid::Node(c));
                v = c;
            }
        }
        let used = view.reads;
        pad(sam, (2 * h0 as u64).saturating_sub(used))?;
        view.meta.count += 1;
        view.meta.height = h as u8;
        view.commit(sam)?;
        let root = view.nodes[view.root].addr;
        match view.value.take() {
            Some(value) => sam.write(
                root,
                BalancedCell::Root {
                    value,
                    meta: view.meta,
                },
                STRUCTURE,
            )?,
            None => {
                roots
                    .writeback_mut(root)
                    .ok_or(err("cached root vanished"))?
                    .meta = view.meta
            }
        }
        ptr.head = Some(fresh);
        Ok(BalancedPointer { head: Some(n) })
    }

    /// Removes one alias (only while nothing is cached).
    fn delete_one<V: Clone, S: SingleAccessMachine<BalancedCell<V>>>(
        &mut self,
        sam: &mut S,
        ptr: &mut BalancedPointer,
    ) -> Result<(), SamError> {
        let b = self.b;
        let head = ptr.head.take().ok_or(err("deleted balanced pointer"))?;
        let first = read_cell::<V, S>(sam, head)?;
        if let BalancedCell::Root { .. } = first {
            return Ok(()); // the last alias: the value is dropped
        }
        let mut view = View::load(sam, first, &mut NoRoots)?;
        let h0 = view.height();
        let f = view.meta.count as u64;
        let last = f - 1;
        let (p1, s) = view.slot;
        // Walk down to the last leaf, remembering the frontier path.
        let mut path = vec![view.root];
        let mut v = view.root;
        for level in (2..=h0).rev() {
            v = view.open_child(sam, v, digit(last, level - 1, b))?;
            path.push(v);
        }
        let dl = digit(last, 0, b);
        if dl + 1 != view.nodes[v].kids.len() {
            return Err(err("last leaf is not the last child"));
        }
        if (v, dl) != (p1, s) {
            let moved = view.nodes[v].kids[dl];
            view.nodes[p1].kids[s] = moved;
        }
        view.nodes[v].kids.pop();
        // Remove emptied frontier nodes (never the root).
        for i in (1..path.len()).rev() {
            let (child, parent) = (path[i], path[i - 1]);
            if view.nodes[child].kids.is_empty() {
                view.nodes[child].alive = false;
                view.nodes[parent].kids.pop();
            }
        }
        let count = view.meta.count - 1;
        if count == 0 {
            return Ok(());
        }
        // Contract while the root has a single internal child.
        let mut h = h0;
        while h > 1 && view.nodes[view.root].kids.len() == 1 {
            let r = view.root;
            let c = view.open_child(sam, r, 0)?;
            view.nodes[r].kids = std::mem::take(&mut view.nodes[c].kids);
            view.nodes[c].alive = false;
            h -= 1;
            view.nodes[r].level = h;
        }
        pad(sam, (2 * h0 as u64).saturating_sub(view.reads))?;
        let value = view.value.take().ok_or(err("delete met a cached root"))?;
        if count == 1 {
            // One alias left: its own cell becomes the root (height 0).
            let Kid::Leaf(x) = view.nodes[view.root].kids[0] else {
                return Err(err("single remaining child is not an alias"));
            };
            return sam.write(
                x,
                BalancedCell::Root {
                    value,
                    meta: BalancedMeta {
                        count: 1,
                        height: 0,
                        down: None,
                    },
                },
                STRUCTURE,
            );
        }
        view.meta.count = count;
        view.meta.height = h as u8;
        view.commit(sam)?;
        let root = view.nodes[view.root].addr;
        sam.write(
            root,
            BalancedCell::Root {
                value,
                meta: view.meta,
            },
            STRUCTURE,
        )
    }

    /// Bulk install: a complete tree with `f` aliases, in position order.
    pub fn install<V: Clone, S: SingleAccessMachine<BalancedCell<V>>>(
        &mut self,
        sam: &mut S,
        value: V,
        f: usize,
    ) -> Result<Vec<BalancedPointer>, SamError> {
        if f == 0 {
            return Err(SamError::InvalidParameter(
                "install needs at least one alias",
            ));
        }
        if f == 1 {
            return Ok(vec![self.new_pointer(sam, value)?]);
        }
        let b = self.b;
        let h = balanced_height(f as u32, b) as usize;
        let mut w = Bounded {
            sam,
            b,
            pending: 0,
            _v: std::marker::PhantomData,
        };
        let mut leaves = Vec::with_capacity(f);
        let root = alloc(w.sam);
        let down = alloc(w.sam);
        let kids = build_level(&mut w, 0, f as u64, h, b, &mut leaves)?;
        w.group(root, down, &kids)?;
        w.write(
            root,
            BalancedCell::Root {
                value,
                meta: BalancedMeta {
                    count: f as u32,
                    height: h as u8,
                    down: Some(down),
                },
            },
        )?;
        w.finish()?;
        Ok(leaves
            .into_iter()
            .map(|a| BalancedPointer { head: Some(a) })
            .collect())
    }
}

/// BlockOSAM-style bounded writer for bulk builds: a public flush after every
/// `b` writes, so that pending writes stay bounded.
struct Bounded<'a, V, S> {
    sam: &'a mut S,
    b: usize,
    pending: usize,
    _v: std::marker::PhantomData<V>,
}

impl<V: Clone, S: SingleAccessMachine<BalancedCell<V>>> Bounded<'_, V, S> {
    fn tick(&mut self, n: usize) -> Result<(), SamError> {
        self.pending += n;
        while self.pending >= self.b {
            self.pending -= self.b;
            self.sam.flush(self.b, STRUCTURE)?;
        }
        Ok(())
    }
    fn write(&mut self, a: Address, c: BalancedCell<V>) -> Result<(), SamError> {
        self.sam.write(a, c, STRUCTURE)?;
        self.tick(1)
    }
    fn group(
        &mut self,
        parent: Address,
        down: Address,
        g: &[BalancedEntry],
    ) -> Result<(), SamError> {
        write_group::<V, S>(self.sam, parent, down, g)?;
        self.tick(g.len() + 1)
    }
    fn finish(&mut self) -> Result<(), SamError> {
        if self.pending > 0 {
            self.pending = 0;
            self.sam.flush(self.b, STRUCTURE)?;
        }
        Ok(())
    }
}

/// Builds the children (at `level - 1`) of a node covering positions `[lo, hi)`.
fn build_level<V: Clone, S: SingleAccessMachine<BalancedCell<V>>>(
    w: &mut Bounded<'_, V, S>,
    lo: u64,
    hi: u64,
    level: usize,
    b: usize,
    leaves: &mut Vec<Address>,
) -> Result<Vec<BalancedEntry>, SamError> {
    let span = (b as u64).pow(level as u32 - 1);
    let mut kids = Vec::new();
    let mut s = lo;
    while s < hi {
        let e = hi.min(s + span);
        let node = alloc(w.sam);
        if level == 1 {
            leaves.push(node);
            kids.push(BalancedEntry { node, down: None });
        } else {
            let down = alloc(w.sam);
            let g = build_level(w, s, e, level - 1, b, leaves)?;
            w.group(node, down, &g)?;
            kids.push(BalancedEntry {
                node,
                down: Some(down),
            });
        }
        s = e;
    }
    Ok(kids)
}

impl<V: Clone> SmartPointerBackend<V> for BalancedPointers {
    type Pointer = BalancedPointer;
    type Cell = BalancedCell<V>;

    fn label(&self) -> &'static str {
        "balanced-rary"
    }

    fn new_pointer<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        value: V,
    ) -> Result<BalancedPointer, SamError> {
        let a = alloc(sam);
        sam.write(
            a,
            BalancedCell::Root {
                value,
                meta: BalancedMeta {
                    count: 1,
                    height: 0,
                    down: None,
                },
            },
            STRUCTURE,
        )?;
        self.new_calls += 1;
        if self.new_calls == self.b {
            self.new_calls = 0;
            sam.flush(self.b, STRUCTURE)?;
        }
        Ok(BalancedPointer { head: Some(a) })
    }

    fn copy_pointer<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        ptr: &mut BalancedPointer,
    ) -> Result<BalancedPointer, SamError> {
        self.copy_one::<V, S>(sam, ptr, &mut NoRoots)
    }

    fn get<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        ptr: &mut BalancedPointer,
    ) -> Result<Option<V>, SamError> {
        self.access(sam, ptr, |v: &mut V, _, _, _| Ok(Some(v.clone())))
    }

    fn put<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        ptr: &mut BalancedPointer,
        value: V,
    ) -> Result<(), SamError> {
        self.access(sam, ptr, |v: &mut V, _, _, _| {
            *v = value;
            Ok(())
        })
    }

    fn delete<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        ptr: &mut BalancedPointer,
    ) -> Result<(), SamError> {
        self.delete_one::<V, S>(sam, ptr)
    }

    fn is_single_reference<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        ptr: &mut BalancedPointer,
    ) -> Result<bool, SamError> {
        self.access(sam, ptr, |_: &mut V, meta, _, _| Ok(meta.count == 1))
    }

    fn with_value<S, R, F>(
        &mut self,
        sam: &mut S,
        ptr: &mut BalancedPointer,
        operation: F,
    ) -> Result<R, SamError>
    where
        S: SingleAccessMachine<Self::Cell>,
        F: FnOnce(&mut V, &mut Self, &mut S) -> Result<R, SamError>,
    {
        self.access(sam, ptr, |v, _, me, sam| operation(v, me, sam))
    }
}

impl<V: Clone> CacheablePointerBackend<V> for BalancedPointers {
    type Writeback = BalancedWriteback;

    fn deref_cached<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        ptr: &mut BalancedPointer,
        roots: &mut dyn CachedRoots<BalancedWriteback>,
    ) -> Result<Option<CachedDeref<V, BalancedWriteback>>, SamError> {
        self.deref_inner(sam, ptr, roots).map(Some)
    }

    fn finish_cached<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        wb: BalancedWriteback,
        value: V,
    ) -> Result<(), SamError> {
        sam.write(
            wb.root,
            BalancedCell::Root {
                value,
                meta: wb.meta,
            },
            STRUCTURE,
        )
    }

    fn copy_cached<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        ptr: &mut BalancedPointer,
        num_copies: usize,
        roots: &mut dyn CachedRoots<BalancedWriteback>,
    ) -> Result<Vec<BalancedPointer>, SamError> {
        (0..num_copies)
            .map(|_| self.copy_one::<V, S>(sam, ptr, roots))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Block codec.
// ---------------------------------------------------------------------------

/// Compact encoding: addresses as 4-byte identifiers (0 = none). A node
/// cell is `6 + 8b` bytes; its own index is recovered from the group.
#[derive(Clone, Debug)]
pub struct BalancedCellValueCodec<C> {
    payload: C,
}

impl<C> BalancedCellValueCodec<C> {
    pub fn new(payload: C) -> Self {
        Self { payload }
    }
}

fn put_addr(a: Option<Address>, out: &mut Vec<u8>) -> Result<(), SamError> {
    let id: u32 = match a {
        None => 0,
        Some(Address::Oblivious(id)) => {
            u32::try_from(id).map_err(|_| SamError::Backend("address exceeds 32 bits".into()))?
        }
        Some(Address::Plaintext(_)) => {
            return Err(SamError::Backend(
                "balanced pointers use oblivious addresses".into(),
            ))
        }
    };
    out.extend_from_slice(&id.to_le_bytes());
    Ok(())
}

fn get_addr(input: &mut &[u8]) -> Result<Option<Address>, SamError> {
    if input.len() < 4 {
        return Err(SamError::Backend("truncated balanced cell".into()));
    }
    let id = u32::from_le_bytes(input[..4].try_into().unwrap());
    *input = &input[4..];
    Ok((id != 0).then_some(Address::Oblivious(id as u64)))
}

fn get_byte(input: &mut &[u8]) -> Result<u8, SamError> {
    let (&x, rest) = input
        .split_first()
        .ok_or_else(|| SamError::Backend("truncated balanced cell".into()))?;
    *input = rest;
    Ok(x)
}

fn put_group(g: &[BalancedEntry], out: &mut Vec<u8>) -> Result<(), SamError> {
    out.push(u8::try_from(g.len()).map_err(|_| SamError::Backend("group exceeds 255".into()))?);
    for e in g {
        put_addr(Some(e.node), out)?;
        put_addr(e.down, out)?;
    }
    Ok(())
}

fn get_group(input: &mut &[u8]) -> Result<Arc<[BalancedEntry]>, SamError> {
    let n = get_byte(input)? as usize;
    let mut g = Vec::with_capacity(n);
    for _ in 0..n {
        let node = get_addr(input)?.ok_or_else(|| SamError::Backend("empty group entry".into()))?;
        let down = get_addr(input)?;
        g.push(BalancedEntry { node, down });
    }
    Ok(g.into())
}

impl<V, C: ValueCodec<V>> ValueCodec<BalancedCell<V>> for BalancedCellValueCodec<C> {
    fn encode_value(&self, value: &BalancedCell<V>, out: &mut Vec<u8>) -> Result<(), SamError> {
        match value {
            BalancedCell::Root { value, meta } => {
                out.push(0);
                out.extend_from_slice(&meta.count.to_le_bytes());
                out.push(meta.height);
                put_addr(meta.down, out)?;
                self.payload.encode_value(value, out)?;
            }
            BalancedCell::Node { parent, group, .. } => {
                out.push(1);
                put_addr(Some(*parent), out)?;
                put_group(group, out)?;
            }
            BalancedCell::Kids { group } => {
                out.push(2);
                put_group(group, out)?;
            }
        }
        Ok(())
    }

    fn decode_value(&self, input: &mut &[u8]) -> Result<BalancedCell<V>, SamError> {
        match get_byte(input)? {
            0 => {
                if input.len() < 5 {
                    return Err(SamError::Backend("truncated root".into()));
                }
                let count = u32::from_le_bytes(input[..4].try_into().unwrap());
                *input = &input[4..];
                let height = get_byte(input)?;
                let down = get_addr(input)?;
                let value = self.payload.decode_value(input)?;
                Ok(BalancedCell::Root {
                    value,
                    meta: BalancedMeta {
                        count,
                        height,
                        down,
                    },
                })
            }
            1 => {
                let parent = get_addr(input)?
                    .ok_or_else(|| SamError::Backend("node without parent".into()))?;
                let group = get_group(input)?;
                Ok(BalancedCell::Node {
                    parent,
                    group,
                    index: UNRESOLVED,
                })
            }
            2 => Ok(BalancedCell::Kids {
                group: get_group(input)?,
            }),
            _ => Err(SamError::Backend("unknown balanced cell tag".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pointer::CachedPointers;
    use crate::{AccessPolicy, DryRunSam};
    use rand::{rngs::StdRng, Rng, SeedableRng};

    fn sam() -> DryRunSam<BalancedCell<u64>> {
        DryRunSam::new(AccessPolicy::MULTI_WRITE)
    }
    fn reads(s: &DryRunSam<BalancedCell<u64>>) -> u64 {
        s.stats().operations.reads
    }

    #[test]
    fn deref_costs_exactly_height_plus_one() {
        for b in [2usize, 3, 4, 8, 30] {
            for f in [1usize, 2, 3, 7, 30, 31, 64, 100, 952] {
                let mut s = sam();
                let mut be = BalancedPointers::new(b).unwrap();
                let mut ps = be.install(&mut s, 7u64, f).unwrap();
                let mut rng = StdRng::seed_from_u64(1);
                for _ in 0..200 {
                    let i = rng.gen_range(0..f);
                    let r0 = reads(&s);
                    assert_eq!(
                        SmartPointerBackend::<u64>::get(&mut be, &mut s, &mut ps[i]).unwrap(),
                        Some(7)
                    );
                    assert_eq!(
                        reads(&s) - r0,
                        balanced_deref_reads(f as u32, b),
                        "b={b} f={f}"
                    );
                }
            }
        }
    }

    #[test]
    fn random_copies_deletes_and_puts_stay_complete() {
        for b in [2usize, 3, 4, 5] {
            let mut s = sam();
            let mut be = BalancedPointers::new(b).unwrap();
            let mut ps = vec![SmartPointerBackend::<u64>::new_pointer(&mut be, &mut s, 0).unwrap()];
            let mut rng = StdRng::seed_from_u64(b as u64);
            let mut value = 0u64;
            for step in 0..3000 {
                let f = ps.len() as u32;
                let op = rng.gen_range(0..10);
                let i = rng.gen_range(0..ps.len());
                let r0 = reads(&s);
                if op < 3 || ps.len() == 1 {
                    let c = SmartPointerBackend::<u64>::copy_pointer(&mut be, &mut s, &mut ps[i])
                        .unwrap();
                    ps.push(c);
                    assert_eq!(
                        reads(&s) - r0,
                        balanced_update_reads(f, b),
                        "copy b={b} f={f} step={step}"
                    );
                } else if op < 5 {
                    let mut p = ps.swap_remove(i);
                    SmartPointerBackend::<u64>::delete(&mut be, &mut s, &mut p).unwrap();
                    assert_eq!(
                        reads(&s) - r0,
                        balanced_update_reads(f, b),
                        "delete b={b} f={f} step={step}"
                    );
                } else if op < 7 {
                    value += 1;
                    SmartPointerBackend::<u64>::put(&mut be, &mut s, &mut ps[i], value).unwrap();
                    assert_eq!(reads(&s) - r0, balanced_deref_reads(f, b));
                } else {
                    let v = SmartPointerBackend::<u64>::get(&mut be, &mut s, &mut ps[i]).unwrap();
                    assert_eq!(v, Some(value), "b={b} step={step}");
                    assert_eq!(
                        reads(&s) - r0,
                        balanced_deref_reads(f, b),
                        "get b={b} f={f} step={step}"
                    );
                }
                // Every alias still reaches the value at the complete-tree cost.
                if step % 97 == 0 {
                    let f = ps.len() as u32;
                    for p in ps.iter_mut() {
                        let r0 = reads(&s);
                        assert_eq!(
                            SmartPointerBackend::<u64>::get(&mut be, &mut s, p).unwrap(),
                            Some(value)
                        );
                        assert_eq!(reads(&s) - r0, balanced_deref_reads(f, b));
                    }
                    let single = SmartPointerBackend::<u64>::is_single_reference(
                        &mut be, &mut s, &mut ps[0],
                    )
                    .unwrap();
                    assert_eq!(single, ps.len() == 1);
                }
            }
        }
    }

    #[test]
    fn cached_aliases_hit_the_open_root() {
        let b = 4;
        let mut s = sam();
        let mut be = BalancedPointers::new(b).unwrap();
        let raw = be.install(&mut s, 5u64, 20).unwrap();
        let mut cache: CachedPointers<u64, BalancedPointers> = CachedPointers::new(be);
        let mut ps: Vec<_> = raw
            .into_iter()
            .map(crate::pointer::CachedPointer::from_raw)
            .collect();
        let mut a = cache.get(&mut s, &mut ps[3]).unwrap().unwrap();
        *cache.value_mut(&a).unwrap() = 6;
        // Another alias meets the live root: one read fewer.
        let r0 = reads(&s);
        let mut o = cache.get(&mut s, &mut ps[17]).unwrap().unwrap();
        assert_eq!(reads(&s) - r0, balanced_deref_reads(20, b) - 1);
        assert_eq!(*cache.value(&o).unwrap(), 6);
        // Copy while cached, then release everything.
        let mut c = cache.copy_pointer(&mut s, &mut ps[5], false).unwrap();
        cache.release(&mut s, &mut o).unwrap();
        cache.release(&mut s, &mut a).unwrap();
        assert_eq!(cache.with_value(&mut s, &mut c, |v| *v).unwrap(), Some(6));
        for p in ps.iter_mut() {
            let r0 = reads(&s);
            assert_eq!(cache.with_value(&mut s, p, |v| *v).unwrap(), Some(6));
            assert_eq!(reads(&s) - r0, balanced_deref_reads(21, b));
        }
    }

    #[test]
    fn codec_round_trips_and_fits() {
        use crate::pointer::{FixedSizeCodec, U64ValueCodec};
        use crate::BlockCodec;
        let codec = FixedSizeCodec::new(BalancedCellValueCodec::new(U64ValueCodec));
        let g: Vec<BalancedEntry> = (1..=30)
            .map(|i| BalancedEntry {
                node: Address::Oblivious(i),
                down: Some(Address::Oblivious(100 + i)),
            })
            .collect();
        let cell = BalancedCell::<u64>::Node {
            parent: Address::Oblivious(7),
            group: g.into(),
            index: 3,
        };
        let block: [u8; 256] = codec.encode(&cell).unwrap();
        let BalancedCell::Node {
            parent,
            group,
            index,
        } = codec.decode(&block).unwrap()
        else {
            panic!()
        };
        assert_eq!(parent, Address::Oblivious(7));
        assert_eq!(group.len(), 30);
        assert_eq!(index, UNRESOLVED);
    }

    #[test]
    fn cached_copies_grow_the_tree() {
        for (b, f) in [(4usize, 16usize), (2, 8), (3, 1), (5, 25)] {
            let mut s = sam();
            let mut be = BalancedPointers::new(b).unwrap();
            let raw = be.install(&mut s, 1u64, f).unwrap();
            let mut cache: CachedPointers<u64, BalancedPointers> = CachedPointers::new(be);
            let mut ps: Vec<_> = raw
                .into_iter()
                .map(crate::pointer::CachedPointer::from_raw)
                .collect();
            let mut o = cache.get(&mut s, &mut ps[0]).unwrap().unwrap();
            *cache.value_mut(&o).unwrap() = 9;
            let mut extra = Vec::new();
            for k in 0..(2 * b + 1) {
                let src = k % ps.len();
                extra.push(cache.copy_pointer(&mut s, &mut ps[src], false).unwrap());
            }
            cache.release(&mut s, &mut o).unwrap();
            ps.extend(extra);
            let total = ps.len() as u32;
            for p in ps.iter_mut() {
                let r0 = reads(&s);
                assert_eq!(cache.with_value(&mut s, p, |v| *v).unwrap(), Some(9));
                assert_eq!(
                    reads(&s) - r0,
                    balanced_deref_reads(total, b),
                    "b={b} f={f}"
                );
            }
        }
    }

    #[test]
    fn random_mix_through_the_cache() {
        let b = 3;
        let mut s = sam();
        let be = BalancedPointers::new(b).unwrap();
        let mut cache: CachedPointers<u64, BalancedPointers> = CachedPointers::new(be);
        let mut ps = vec![cache.new_pointer(&mut s, 0u64).unwrap()];
        let mut rng = StdRng::seed_from_u64(11);
        let mut value = 0u64;
        for step in 0..4000 {
            let i = rng.gen_range(0..ps.len());
            match rng.gen_range(0..6) {
                0 | 1 => {
                    // Copy while another alias holds the value open.
                    let j = rng.gen_range(0..ps.len());
                    let mut o = cache.get(&mut s, &mut ps[j]).unwrap().unwrap();
                    let c = cache.copy_pointer(&mut s, &mut ps[i], false).unwrap();
                    cache.release(&mut s, &mut o).unwrap();
                    ps.push(c);
                }
                2 if ps.len() > 1 => {
                    let mut p = ps.swap_remove(i);
                    cache.delete_pointer(&mut s, &mut p).unwrap();
                }
                3 => {
                    value += 1;
                    cache.put(&mut s, &mut ps[i], value).unwrap();
                }
                _ => {
                    // Two aliases open at once.
                    let j = rng.gen_range(0..ps.len());
                    let mut a = cache.get(&mut s, &mut ps[i]).unwrap().unwrap();
                    let mut c = cache.get(&mut s, &mut ps[j]).unwrap().unwrap();
                    assert_eq!(*cache.value(&c).unwrap(), value, "step {step}");
                    cache.release(&mut s, &mut c).unwrap();
                    cache.release(&mut s, &mut a).unwrap();
                }
            }
        }
        let f = ps.len() as u32;
        for p in ps.iter_mut() {
            let r0 = reads(&s);
            assert_eq!(cache.with_value(&mut s, p, |v| *v).unwrap(), Some(value));
            assert_eq!(reads(&s) - r0, balanced_deref_reads(f, b));
        }
    }
}
