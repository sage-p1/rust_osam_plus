use crate::{Address, MemoryClass, SamError, SingleAccessMachine};
use std::collections::HashSet;
use std::sync::Arc;

const STRUCTURE: &str = "SmartPointerMultiWriteRary";

/// Group index stored in a [`RaryCell::Node`] decoded from a block before it
/// is resolved. Encoded cells omit the index because it is recoverable from
/// where the node's own address sits in its group.
pub const UNRESOLVED_RARY_INDEX: usize = usize::MAX;

/// A cell in the inverted r-ary multi-write pointer tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RaryCell<V> {
    /// The unique cell holding the shared value.
    Root(V),
    /// A member of a sibling group. Every live member stores the same group.
    Node {
        parent: Address,
        /// The group's members, compact from slot 0. Slots past the end are
        /// empty; in memory the group is not padded to the fanout, and the
        /// members' cells share one allocation (cloning a cell is cheap).
        group: Arc<[Option<Address>]>,
        index: usize,
    },
}

/// BlockOSAM's bounded writer: counts the pointer-cell writes of a burst
/// without genuine reads (a bulk copy, or a run of `New` calls) and performs
/// a public flush of `batch` evictions after every `batch` of them, so the
/// pending writes stay bounded. [`Self::finish`] flushes a final partial batch.
pub(crate) struct BoundedWrites<'a, S> {
    sam: &'a mut S,
    batch: usize,
    pending: usize,
}

impl<'a, S> BoundedWrites<'a, S> {
    pub(crate) fn new(sam: &'a mut S, batch: usize) -> Self {
        Self {
            sam,
            batch,
            pending: 0,
        }
    }

    /// Flushes the writes of a final partial batch.
    pub(crate) fn finish<V: Clone>(self) -> Result<(), SamError>
    where
        S: SingleAccessMachine<V>,
    {
        if self.pending > 0 {
            self.sam.flush(self.batch, STRUCTURE)?;
        }
        Ok(())
    }
}

impl<V: Clone, S: SingleAccessMachine<V>> SingleAccessMachine<V> for BoundedWrites<'_, S> {
    fn alloc(&mut self, class: MemoryClass, structure: &'static str) -> Address {
        self.sam.alloc(class, structure)
    }

    fn write(
        &mut self,
        address: Address,
        value: V,
        structure: &'static str,
    ) -> Result<(), SamError> {
        self.sam.write(address, value, structure)?;
        if matches!(address, Address::Oblivious(_)) {
            self.pending += 1;
            if self.pending == self.batch {
                self.pending = 0;
                self.sam.flush(self.batch, STRUCTURE)?;
            }
        }
        Ok(())
    }

    fn read(&mut self, address: Address, structure: &'static str) -> Result<Option<V>, SamError> {
        self.sam.read(address, structure)
    }

    fn retire(&mut self, address: Address) {
        self.sam.retire(address)
    }

    fn flush(&mut self, paths: usize, structure: &'static str) -> Result<(), SamError> {
        self.sam.flush(paths, structure)
    }

    fn stats(&self) -> &crate::Stats {
        self.sam.stats()
    }

    fn reset_stash_maximum(&mut self) {
        self.sam.reset_stash_maximum()
    }
}

/// Multi-write pointer whose physical tree has configurable even fanout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RaryPointer {
    head: Option<Address>,
    branching_factor: usize,
    last_depth: usize,
}

impl RaryPointer {
    pub(crate) fn from_persisted_head(
        head: Address,
        branching_factor: usize,
    ) -> Result<Self, SamError> {
        Self::check_branching_factor(branching_factor)?;
        Ok(Self {
            head: Some(head),
            branching_factor,
            last_depth: 0,
        })
    }

    /// Allocates a uniquely referenced value.
    pub fn new<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        sam: &mut S,
        value: V,
        branching_factor: usize,
    ) -> Result<Self, SamError> {
        Self::check_branching_factor(branching_factor)?;
        let root = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
        sam.write(root, RaryCell::Root(value), STRUCTURE)?;
        Ok(Self {
            head: Some(root),
            branching_factor,
            last_depth: 0,
        })
    }

    /// Returns the current logical address.
    pub fn head(&self) -> Option<Address> {
        self.head
    }

    /// Returns this tree's configured fanout.
    pub fn branching_factor(&self) -> usize {
        self.branching_factor
    }

    /// Returns the NODE depth traversed by the most recent dereference.
    pub fn last_depth(&self) -> usize {
        self.last_depth
    }

    fn check_branching_factor(branching_factor: usize) -> Result<(), SamError> {
        if branching_factor < 2 || !branching_factor.is_multiple_of(2) {
            return Err(SamError::InvalidParameter(
                "branching factor must be an even integer of at least two",
            ));
        }
        Ok(())
    }

    /// Returns virtual-binary sibling blocks, ordered from the leaf upward.
    pub fn sibling_blocks(
        index: usize,
        group_len: usize,
        branching_factor: usize,
    ) -> Result<Vec<Vec<usize>>, SamError> {
        Self::check_branching_factor(branching_factor)?;
        if group_len == 0 {
            return Ok(Vec::new());
        }
        if index >= group_len {
            return Err(SamError::InvalidParameter("group index is out of range"));
        }

        let mut top_down = Vec::new();
        let (mut lo, mut hi) = (0, group_len);
        while hi - lo > 1 {
            let midpoint = (lo + hi).div_ceil(2);
            if index < midpoint {
                top_down.push((midpoint..hi).collect());
                hi = midpoint;
            } else {
                top_down.push((lo..midpoint).collect());
                lo = midpoint;
            }
        }
        top_down.reverse();
        Ok(top_down)
    }

    fn normalize_group<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        sam: &mut S,
        members: &[Address],
        parent: Address,
        branching_factor: usize,
    ) -> Result<(), SamError> {
        if members.is_empty() || members.len() > branching_factor {
            return Err(SamError::InvalidPointerCell("invalid r-ary group size"));
        }
        if members.iter().copied().collect::<HashSet<_>>().len() != members.len() {
            return Err(SamError::InvalidPointerCell(
                "duplicate address in r-ary group",
            ));
        }
        // Compact, unpadded (slots past `members.len()` are empty), and
        // shared by every member's cell.
        let group: Arc<[Option<Address>]> = members.iter().copied().map(Some).collect();
        sam.write_batch(
            members
                .iter()
                .copied()
                .enumerate()
                .map(|(index, address)| {
                    (
                        address,
                        RaryCell::Node {
                            parent,
                            group: group.clone(),
                            index,
                        },
                    )
                })
                .collect(),
            STRUCTURE,
        )
    }

    /// Validates a node read from `address` and fills in its group index.
    ///
    /// A node's slot in its sibling group is exactly where its own address
    /// appears in `group`, so encoded cells omit the index (see
    /// [`UNRESOLVED_RARY_INDEX`]) and it is recovered here after each read.
    fn resolve_node(
        address: Address,
        cell: &mut RaryCell<impl Clone>,
        branching_factor: usize,
    ) -> Result<(), SamError> {
        let RaryCell::Node { group, index, .. } = cell else {
            return Ok(());
        };
        if group.is_empty() || group.len() > branching_factor {
            return Err(SamError::InvalidPointerCell("malformed r-ary group"));
        }
        let slot = group
            .iter()
            .position(|member| *member == Some(address))
            .ok_or(SamError::InvalidPointerCell("r-ary group/index mismatch"))?;
        if *index != UNRESOLVED_RARY_INDEX && *index != slot {
            return Err(SamError::InvalidPointerCell("r-ary group/index mismatch"));
        }
        *index = slot;
        let live = group.iter().flatten().copied().collect::<Vec<_>>();
        if live.iter().copied().collect::<HashSet<_>>().len() != live.len() {
            return Err(SamError::InvalidPointerCell(
                "duplicate live r-ary group member",
            ));
        }
        Ok(())
    }

    fn sibling_sets(
        group: &[Option<Address>],
        index: usize,
        branching_factor: usize,
    ) -> Result<Vec<Vec<Address>>, SamError> {
        // The virtual-binary split is over all `branching_factor` slots
        // (whether or not the group is padded), so read patterns do not
        // depend on how the group is stored.
        Ok(
            Self::sibling_blocks(index, branching_factor, branching_factor)?
                .into_iter()
                .filter_map(|block| {
                    let members = block
                        .into_iter()
                        .filter_map(|slot| group.get(slot).copied().flatten())
                        .collect::<Vec<_>>();
                    (!members.is_empty()).then_some(members)
                })
                .collect(),
        )
    }

    fn live_members(group: &[Option<Address>]) -> Vec<Address> {
        group.iter().flatten().copied().collect()
    }

    fn live_without(group: &[Option<Address>], remove: usize) -> Vec<Address> {
        group
            .iter()
            .enumerate()
            .filter_map(|(index, member)| (index != remove).then_some(*member).flatten())
            .collect()
    }

    /// Creates one alias while refreshing the original pointer.
    pub fn copy<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<Self, SamError> {
        Ok(self.copy_many(sam, 1)?.remove(0))
    }

    /// Cache-aware bulk copy. Ordinary copies read only the head, which is
    /// safe below a live cached root; the exception is a pointer whose head
    /// *is* that root (a sole alias just dereferenced). Its new leaves are
    /// built directly under the root, whose value stays in the client cache.
    pub(crate) fn copy_cached<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
        num_copies: usize,
        cached: &dyn Fn(Address) -> bool,
    ) -> Result<Vec<Self>, SamError> {
        let head = self
            .head
            .ok_or(SamError::InvalidPointerCell("deleted r-ary pointer"))?;
        if !cached(head) {
            return self.copy_many(sam, num_copies);
        }
        if num_copies == 0 {
            return Err(SamError::InvalidParameter("num_copies must be positive"));
        }
        let mut leaves = Vec::with_capacity(num_copies + 1);
        if num_copies > 1 {
            let mut bounded = BoundedWrites::new(sam, self.branching_factor);
            Self::build_subtree(
                &mut bounded,
                head,
                num_copies + 1,
                self.branching_factor,
                &mut leaves,
            )?;
            bounded.finish()?;
        } else {
            Self::build_subtree(
                sam,
                head,
                num_copies + 1,
                self.branching_factor,
                &mut leaves,
            )?;
        }
        self.head = Some(leaves[0]);
        self.last_depth = 0;
        Ok(leaves[1..]
            .iter()
            .map(|&leaf| Self {
                head: Some(leaf),
                branching_factor: self.branching_factor,
                last_depth: 0,
            })
            .collect())
    }

    /// Creates many aliases after reading the old head only once. A bulk copy
    /// (more than one alias) is a write burst without reads, so its writes go
    /// through the bounded writer ([`BoundedWrites`]); a single copy is
    /// already bounded by the reads of the copied path.
    pub fn copy_many<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
        num_copies: usize,
    ) -> Result<Vec<Self>, SamError> {
        if num_copies <= 1 {
            return self.copy_many_unbounded(sam, num_copies);
        }
        let mut bounded = BoundedWrites::new(sam, self.branching_factor);
        let copies = self.copy_many_unbounded(&mut bounded, num_copies)?;
        bounded.finish()?;
        Ok(copies)
    }

    fn copy_many_unbounded<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
        num_copies: usize,
    ) -> Result<Vec<Self>, SamError> {
        if num_copies == 0 {
            return Err(SamError::InvalidParameter("num_copies must be positive"));
        }
        let old_head = self
            .head
            .ok_or(SamError::InvalidPointerCell("deleted r-ary pointer"))?;
        let mut cell = sam
            .read(old_head, STRUCTURE)?
            .ok_or(SamError::InvalidPointerCell("unwritten r-ary head"))?;
        sam.retire(old_head);
        Self::resolve_node(old_head, &mut cell, self.branching_factor)?;

        let count = num_copies + 1;
        let leaves = match cell {
            RaryCell::Root(value) => {
                let root = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
                sam.write(root, RaryCell::Root(value), STRUCTURE)?;
                let mut leaves = Vec::with_capacity(count);
                Self::build_subtree(sam, root, count, self.branching_factor, &mut leaves)?;
                leaves
            }
            RaryCell::Node {
                parent,
                group,
                index,
            } => {
                let live = Self::live_members(&group);
                if live.len() - 1 + count <= self.branching_factor {
                    let replacements = (0..count)
                        .map(|_| sam.alloc(MemoryClass::Oblivious, STRUCTURE))
                        .collect::<Vec<_>>();
                    let mut expanded = Vec::with_capacity(live.len() - 1 + count);
                    for (slot, member) in group.iter().copied().enumerate() {
                        if slot == index {
                            expanded.extend(replacements.iter().copied());
                        } else if let Some(member) = member {
                            expanded.push(member);
                        }
                    }
                    Self::normalize_group(sam, &expanded, parent, self.branching_factor)?;
                    replacements
                } else {
                    let internal = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
                    let upper = group
                        .iter()
                        .copied()
                        .enumerate()
                        .filter_map(|(slot, member)| {
                            if slot == index {
                                Some(internal)
                            } else {
                                member
                            }
                        })
                        .collect::<Vec<_>>();
                    Self::normalize_group(sam, &upper, parent, self.branching_factor)?;
                    let mut leaves = Vec::with_capacity(count);
                    Self::build_subtree(sam, internal, count, self.branching_factor, &mut leaves)?;
                    leaves
                }
            }
        };

        self.head = Some(leaves[0]);
        self.last_depth = 0;
        Ok(leaves[1..]
            .iter()
            .copied()
            .map(|head| Self {
                head: Some(head),
                branching_factor: self.branching_factor,
                last_depth: 0,
            })
            .collect())
    }

    /// Builds `count` pointers sharing one value in a balanced r-ary tree.
    pub fn install<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        sam: &mut S,
        value: V,
        count: usize,
        branching_factor: usize,
    ) -> Result<Vec<Self>, SamError> {
        Self::check_branching_factor(branching_factor)?;
        if count == 0 {
            return Err(SamError::InvalidParameter("pointer count must be positive"));
        }
        if count == 1 {
            return Ok(vec![Self::new(sam, value, branching_factor)?]);
        }
        let mut bounded = BoundedWrites::new(sam, branching_factor);
        let root = bounded.alloc(MemoryClass::Oblivious, STRUCTURE);
        bounded.write(root, RaryCell::Root(value), STRUCTURE)?;
        let mut leaves = Vec::with_capacity(count);
        Self::build_subtree(&mut bounded, root, count, branching_factor, &mut leaves)?;
        bounded.finish()?;
        Ok(leaves
            .into_iter()
            .map(|head| Self {
                head: Some(head),
                branching_factor,
                last_depth: 0,
            })
            .collect())
    }

    fn build_subtree<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        sam: &mut S,
        parent: Address,
        count: usize,
        branching_factor: usize,
        leaves: &mut Vec<Address>,
    ) -> Result<(), SamError> {
        if count <= branching_factor {
            let members = (0..count)
                .map(|_| sam.alloc(MemoryClass::Oblivious, STRUCTURE))
                .collect::<Vec<_>>();
            Self::normalize_group(sam, &members, parent, branching_factor)?;
            leaves.extend(members);
            return Ok(());
        }

        let base = count / branching_factor;
        let remainder = count % branching_factor;
        let quotas = (0..branching_factor)
            .map(|index| base + usize::from(index < remainder))
            .collect::<Vec<_>>();
        let members = (0..branching_factor)
            .map(|_| sam.alloc(MemoryClass::Oblivious, STRUCTURE))
            .collect::<Vec<_>>();
        Self::normalize_group(sam, &members, parent, branching_factor)?;
        for (member, quota) in members.into_iter().zip(quotas) {
            if quota == 1 {
                leaves.push(member);
            } else {
                Self::build_subtree(sam, member, quota, branching_factor, leaves)?;
            }
        }
        Ok(())
    }

    fn flatten_pending(pending: &[Vec<Address>]) -> Result<Vec<Address>, SamError> {
        let mut seen = HashSet::new();
        let mut members = Vec::new();
        for address in pending.iter().flatten().copied() {
            if !seen.insert(address) {
                return Err(SamError::InvalidPointerCell("duplicate pending r-ary root"));
            }
            members.push(address);
        }
        Ok(members)
    }

    fn dense_chunks(members: Vec<Address>, branching_factor: usize) -> Vec<Vec<Address>> {
        if members.len() <= branching_factor {
            return vec![members];
        }
        let chunks = members.len().div_ceil(branching_factor);
        let base = members.len() / chunks;
        let remainder = members.len() % chunks;
        let mut sizes = (0..chunks)
            .map(|index| base + usize::from(index < remainder))
            .collect::<Vec<_>>();
        let mut source = members.into_iter();
        sizes
            .drain(..)
            .map(|size| source.by_ref().take(size).collect())
            .collect()
    }

    fn pack_members<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        sam: &mut S,
        members: Vec<Address>,
        branching_factor: usize,
    ) -> Result<Vec<Vec<Address>>, SamError> {
        if members.len() <= branching_factor {
            return Ok(vec![members]);
        }
        let mut packed = Vec::new();
        for group in Self::dense_chunks(members, branching_factor) {
            if group.len() == 1 {
                // Only possible with fanout 2 (three members chunk as 2 + 1).
                // Wrapping the lone member in a fresh parent would add a
                // unary node on every access, so pass it up unchanged; the
                // next level's normalize_group re-parents it.
                packed.push(group);
                continue;
            }
            let parent = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
            Self::normalize_group(sam, &group, parent, branching_factor)?;
            packed.push(vec![parent]);
        }
        Ok(packed)
    }

    fn collapse_for_root<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        sam: &mut S,
        mut pending: Vec<Vec<Address>>,
        branching_factor: usize,
    ) -> Result<Vec<Address>, SamError> {
        loop {
            let members = Self::flatten_pending(&pending)?;
            if members.len() <= branching_factor {
                return Ok(members);
            }
            pending = Self::pack_members(sam, members, branching_factor)?;
        }
    }

    fn deref<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<(Address, Address, V), SamError> {
        let (head, root, value) = self.deref_until(sam, &|_| false)?;
        let value = value.ok_or(SamError::InvalidPointerCell(
            "uncached walk reported a cache hit",
        ))?;
        Ok((head, root, value))
    }

    /// Cache-aware dereference used by [`crate::pointer::CachedPointers`].
    /// Returns the write-back root and, on a miss, the value.
    pub(crate) fn deref_cached<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
        cached: &dyn Fn(Address) -> bool,
    ) -> Result<(Address, Option<V>), SamError> {
        let (head, root, value) = self.deref_until(sam, cached)?;
        self.head = Some(head);
        Ok((root, value))
    }

    /// Dereferences and rebuilds the path, never reading an address for
    /// which `cached` holds (a live cached root, absent from SAM until its
    /// write-back). Returns the new head, the write-back root, and the value
    /// on a miss (`None` on a hit).
    fn deref_until<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
        cached: &dyn Fn(Address) -> bool,
    ) -> Result<(Address, Address, Option<V>), SamError> {
        let head = self
            .head
            .ok_or(SamError::InvalidPointerCell("deleted r-ary pointer"))?;
        if cached(head) {
            // This pointer's head is itself the live cached root.
            self.last_depth = 0;
            return Ok((head, head, None));
        }
        let mut cell = sam
            .read(head, STRUCTURE)?
            .ok_or(SamError::InvalidPointerCell("unwritten r-ary head"))?;
        sam.retire(head);
        Self::resolve_node(head, &mut cell, self.branching_factor)?;
        let fresh_head = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
        match cell {
            RaryCell::Root(value) => {
                self.last_depth = 0;
                Ok((fresh_head, fresh_head, Some(value)))
            }
            RaryCell::Node {
                parent,
                group,
                index,
            } => {
                let mut pending = vec![vec![fresh_head]];
                pending.extend(Self::sibling_sets(&group, index, self.branching_factor)?);
                let (root, value, depth) =
                    Self::splay(sam, pending, parent, self.branching_factor, cached)?;
                self.last_depth = depth;
                Ok((fresh_head, root, value))
            }
        }
    }

    pub(crate) fn finish_cached<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        sam: &mut S,
        root: Address,
        value: V,
    ) -> Result<(), SamError> {
        sam.write(root, RaryCell::Root(value), STRUCTURE)
    }

    /// Rebuilds the path up to the root. A live cached root is not read:
    /// the rebuilt children are grouped under its (unchanged) write-back
    /// address, just as they would be under a freshly allocated root.
    fn splay<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        sam: &mut S,
        mut pending: Vec<Vec<Address>>,
        mut current: Address,
        branching_factor: usize,
        cached: &dyn Fn(Address) -> bool,
    ) -> Result<(Address, Option<V>, usize), SamError> {
        let mut depth = 1;
        loop {
            if cached(current) {
                let children = Self::collapse_for_root(sam, pending, branching_factor)?;
                Self::normalize_group(sam, &children, current, branching_factor)?;
                return Ok((current, None, depth));
            }
            let mut cell = sam
                .read(current, STRUCTURE)?
                .ok_or(SamError::InvalidPointerCell("unwritten r-ary path"))?;
            sam.retire(current);
            Self::resolve_node(current, &mut cell, branching_factor)?;
            match cell {
                RaryCell::Root(value) => {
                    let root = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
                    let children = Self::collapse_for_root(sam, pending, branching_factor)?;
                    Self::normalize_group(sam, &children, root, branching_factor)?;
                    return Ok((root, Some(value), depth));
                }
                RaryCell::Node {
                    parent,
                    group,
                    index,
                } => {
                    pending.extend(Self::sibling_sets(&group, index, branching_factor)?);
                    let members = Self::flatten_pending(&pending)?;
                    pending = Self::pack_members(sam, members, branching_factor)?;
                    current = parent;
                    depth += 1;
                }
            }
        }
    }

    /// Reads the shared value and rebuilds the consumed path.
    pub fn get<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<V, SamError> {
        let (head, root, value) = self.deref(sam)?;
        sam.write(root, RaryCell::Root(value.clone()), STRUCTURE)?;
        self.head = Some(head);
        Ok(value)
    }

    /// Reads and clones the shared value.
    pub fn get_and_copy<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<V, SamError> {
        self.get(sam)
    }

    /// Reads a value and lets the caller smart-copy nested pointer fields.
    pub fn get_and_copy_with<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
        copy_nested: impl FnOnce(&mut V, &mut S) -> Result<V, SamError>,
    ) -> Result<V, SamError> {
        self.with_value(sam, copy_nested)
    }

    /// Reads one field selected by a Rust closure.
    pub fn get_attr<V: Clone, T: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
        select: impl FnOnce(&V) -> &T,
    ) -> Result<T, SamError> {
        self.get(sam).map(|value| select(&value).clone())
    }

    /// Replaces the shared value.
    pub fn put<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
        value: V,
    ) -> Result<(), SamError> {
        let (head, root, _) = self.deref(sam)?;
        sam.write(root, RaryCell::Root(value), STRUCTURE)?;
        self.head = Some(head);
        Ok(())
    }

    /// Mutates the shared value with one dereference and one root write-back.
    pub fn modify<V: Clone, R, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
        update: impl FnOnce(&mut V) -> R,
    ) -> Result<R, SamError> {
        let (head, root, mut value) = self.deref(sam)?;
        let result = update(&mut value);
        sam.write(root, RaryCell::Root(value), STRUCTURE)?;
        self.head = Some(head);
        Ok(result)
    }

    /// Operates on a live value and exposes the SAM for nested pointer work.
    pub fn with_value<V: Clone, R, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
        operation: impl FnOnce(&mut V, &mut S) -> Result<R, SamError>,
    ) -> Result<R, SamError> {
        let (head, root, mut value) = self.deref(sam)?;
        let result = operation(&mut value, sam);
        sam.write(root, RaryCell::Root(value), STRUCTURE)?;
        self.head = Some(head);
        result
    }

    /// Rust equivalent of Python's `put_attr`: update through a closure.
    pub fn put_attr<V: Clone, R, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
        update: impl FnOnce(&mut V) -> R,
    ) -> Result<R, SamError> {
        self.modify(sam, update)
    }

    /// Returns whether this is the only pointer to the shared value.
    pub fn is_single_reference<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<bool, SamError> {
        let (head, root, value) = self.deref(sam)?;
        sam.write(root, RaryCell::Root(value), STRUCTURE)?;
        self.head = Some(head);
        Ok(head == root)
    }

    /// Deletes this alias and repairs or contracts underfull groups.
    pub fn delete<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<(), SamError> {
        let Some(head) = self.head.take() else {
            return Ok(());
        };
        Self::delete_address(sam, head, self.branching_factor)
    }

    fn delete_address<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        sam: &mut S,
        address: Address,
        branching_factor: usize,
    ) -> Result<(), SamError> {
        let mut cell = sam
            .read(address, STRUCTURE)?
            .ok_or(SamError::InvalidPointerCell("unwritten deleted r-ary cell"))?;
        sam.retire(address);
        Self::resolve_node(address, &mut cell, branching_factor)?;
        match cell {
            RaryCell::Root(_) => Ok(()),
            RaryCell::Node {
                parent,
                group,
                index,
            } => Self::remove_from_group(sam, parent, group, index, branching_factor),
        }
    }

    fn remove_from_group<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        sam: &mut S,
        parent: Address,
        group: Arc<[Option<Address>]>,
        index: usize,
        branching_factor: usize,
    ) -> Result<(), SamError> {
        let minimum = branching_factor / 2;
        let old_live = Self::live_members(&group);
        let survivors = Self::live_without(&group, index);
        if survivors.is_empty() {
            return Self::delete_address(sam, parent, branching_factor);
        }
        if survivors.len() == 1 && minimum < 2 {
            // With fanout 2 the minimum occupancy (b / 2) is one, so a lone
            // survivor would otherwise stay behind as a unary node. Those
            // accumulate on every delete, deepening the tree without bound;
            // contract the survivor into its parent's place instead.
            return Self::contract_single(sam, survivors[0], parent, branching_factor);
        }
        if old_live.len() < minimum || survivors.len() >= minimum {
            return Self::normalize_group(sam, &survivors, parent, branching_factor);
        }
        Self::repair_underfull(sam, parent, survivors, branching_factor)
    }

    fn repair_underfull<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        sam: &mut S,
        parent: Address,
        survivors: Vec<Address>,
        branching_factor: usize,
    ) -> Result<(), SamError> {
        let minimum = branching_factor / 2;
        if survivors.len() >= minimum {
            return Self::normalize_group(sam, &survivors, parent, branching_factor);
        }
        let mut cell = sam
            .read(parent, STRUCTURE)?
            .ok_or(SamError::InvalidPointerCell("unwritten r-ary parent"))?;
        sam.retire(parent);
        Self::resolve_node(parent, &mut cell, branching_factor)?;
        match cell {
            RaryCell::Root(value) => {
                if survivors.len() == 1 {
                    sam.write(survivors[0], RaryCell::Root(value), STRUCTURE)
                } else {
                    let root = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
                    sam.write(root, RaryCell::Root(value), STRUCTURE)?;
                    Self::normalize_group(sam, &survivors, root, branching_factor)
                }
            }
            RaryCell::Node {
                parent: grandparent,
                group,
                index,
            } => {
                let upper = Self::live_members(&group);
                // Only the root's child group may hold fewer than `minimum`
                // members (like a B-tree root). Borrowing needs a spare
                // member; otherwise merging is valid for any upper group of
                // at most `minimum` members, including such a root group.
                if upper.len() > minimum {
                    let borrowed = group
                        .iter()
                        .enumerate()
                        .find_map(|(slot, member)| (slot != index).then_some(*member).flatten())
                        .ok_or(SamError::InvalidPointerCell(
                            "missing borrowable r-ary sibling",
                        ))?;
                    let replacement = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
                    let mut lower = survivors;
                    lower.push(borrowed);
                    let upper = group
                        .iter()
                        .copied()
                        .enumerate()
                        .filter_map(|(slot, member)| {
                            if slot == index {
                                Some(replacement)
                            } else if member == Some(borrowed) {
                                None
                            } else {
                                member
                            }
                        })
                        .collect::<Vec<_>>();
                    Self::normalize_group(sam, &lower, replacement, branching_factor)?;
                    Self::normalize_group(sam, &upper, grandparent, branching_factor)
                } else {
                    let mut merged = Vec::new();
                    for (slot, member) in group.iter().copied().enumerate() {
                        if slot == index {
                            merged.extend(survivors.iter().copied());
                        } else if let Some(member) = member {
                            merged.push(member);
                        }
                    }
                    match merged.len() {
                        0 => Self::delete_address(sam, grandparent, branching_factor),
                        1 => Self::contract_single(sam, merged[0], grandparent, branching_factor),
                        _ => Self::normalize_group(sam, &merged, grandparent, branching_factor),
                    }
                }
            }
        }
    }

    fn contract_single<V: Clone, S: SingleAccessMachine<RaryCell<V>>>(
        sam: &mut S,
        child: Address,
        parent: Address,
        branching_factor: usize,
    ) -> Result<(), SamError> {
        let mut cell = sam
            .read(parent, STRUCTURE)?
            .ok_or(SamError::InvalidPointerCell("unwritten contracted parent"))?;
        sam.retire(parent);
        Self::resolve_node(parent, &mut cell, branching_factor)?;
        match cell {
            RaryCell::Root(value) => sam.write(child, RaryCell::Root(value), STRUCTURE),
            RaryCell::Node {
                parent: grandparent,
                group,
                index,
            } => {
                let replacement = group
                    .iter()
                    .copied()
                    .enumerate()
                    .filter_map(
                        |(slot, member)| {
                            if slot == index {
                                Some(child)
                            } else {
                                member
                            }
                        },
                    )
                    .collect::<Vec<_>>();
                if replacement.len() == 1 {
                    Self::contract_single(sam, replacement[0], grandparent, branching_factor)
                } else {
                    Self::normalize_group(sam, &replacement, grandparent, branching_factor)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccessPolicy, DryRunSam};

    fn sam<V: Clone>() -> DryRunSam<RaryCell<V>> {
        DryRunSam::new(AccessPolicy::MULTI_WRITE)
    }

    #[test]
    fn sibling_block_examples_match_python() {
        assert_eq!(
            RaryPointer::sibling_blocks(0, 4, 4).unwrap(),
            vec![vec![1], vec![2, 3]]
        );
        assert_eq!(
            RaryPointer::sibling_blocks(2, 4, 4).unwrap(),
            vec![vec![3], vec![0, 1]]
        );
        assert_eq!(
            RaryPointer::sibling_blocks(5, 8, 8).unwrap(),
            vec![vec![4], vec![6, 7], vec![0, 1, 2, 3]]
        );
        assert_eq!(
            RaryPointer::sibling_blocks(1, 3, 4).unwrap(),
            vec![vec![0], vec![2]]
        );
    }

    #[test]
    fn bulk_copy_reads_the_old_head_once_and_flushes_every_b_writes() {
        let mut sam = sam();
        let mut pointer = RaryPointer::new(&mut sam, "cat", 4).unwrap();
        let before = sam.stats().clone();
        let copies = pointer.copy_many(&mut sam, 100).unwrap();
        let after = sam.stats().clone();
        assert_eq!(copies.len(), 100);
        let writes = after.operations.writes - before.operations.writes;
        let flushes = after.flushes - before.flushes;
        // One genuine read (the old head), then one public flush per full
        // batch of 4 writes and one for the final partial batch.
        assert_eq!(flushes, writes.div_ceil(4));
        assert_eq!(
            after.operations.reads - before.operations.reads,
            1 + flushes
        );
        // A single copy is bounded by its own reads: no flush.
        let mut single = RaryPointer::new(&mut sam, "dog", 4).unwrap();
        let before = sam.stats().flushes;
        single.copy(&mut sam).unwrap();
        assert_eq!(sam.stats().flushes, before);
    }

    #[test]
    fn bulk_copy_keeps_pending_writes_below_one_batch() {
        // Pending writes (writes not yet offset by a read or a flush) never
        // reach b + 1 during a bulk copy, whatever its size.
        for branching_factor in [2, 6, 64] {
            let mut sam = sam();
            let mut pointer = RaryPointer::new(&mut sam, 7_u64, branching_factor).unwrap();
            pointer.copy(&mut sam).unwrap();
            sam.reset_write_batches();
            pointer.copy_many(&mut sam, 1000).unwrap();
            assert!(
                sam.stats().max_write_batches <= branching_factor as u64,
                "b={branching_factor}: {} pending writes",
                sam.stats().max_write_batches
            );
        }
    }

    #[test]
    fn installed_non_power_counts_share_puts() {
        for count in [2, 3, 5, 10, 17, 50] {
            let mut sam = sam();
            let mut pointers = RaryPointer::install(&mut sam, count, count, 4).unwrap();
            for pointer in &mut pointers {
                assert_eq!(pointer.get(&mut sam).unwrap(), count);
            }
            pointers[count / 2].put(&mut sam, count + 100).unwrap();
            for pointer in &mut pointers {
                assert_eq!(pointer.get(&mut sam).unwrap(), count + 100);
            }
        }
    }

    #[test]
    fn arbitrary_even_branching_factors_work() {
        for branching_factor in [6, 10, 14] {
            let mut sam = sam();
            let count = 3 * branching_factor + 2;
            let mut pointers =
                RaryPointer::install(&mut sam, branching_factor, count, branching_factor).unwrap();
            for pointer in &mut pointers {
                assert_eq!(pointer.get(&mut sam).unwrap(), branching_factor);
            }
        }
    }

    #[test]
    fn binary_fanout_copy_delete_cycles_stay_bounded() {
        // Regression: with fanout 2, deleting temporary copies used to leave
        // unary nodes behind (4,199 cells for 100 aliases after 2,000 rounds).
        let mut sam = sam::<u64>();
        let mut pointers = vec![RaryPointer::new(&mut sam, 7, 2).unwrap()];
        let aliases = pointers[0].copy_many(&mut sam, 99).unwrap();
        pointers.extend(aliases);
        let live_after_copy = sam.snapshot().blocks.len();
        let mut state: u64 = 1;
        for _ in 0..2_000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let pointer = &mut pointers[(state >> 33) as usize % 100];
            for mut temporary in pointer.copy_many(&mut sam, 3).unwrap() {
                temporary.delete(&mut sam).unwrap();
            }
        }
        let live = sam.snapshot().blocks.len();
        assert!(
            live <= 2 * live_after_copy,
            "binary r-ary tree grew from {live_after_copy} to {live} cells"
        );
        for pointer in &mut pointers {
            assert_eq!(pointer.get(&mut sam).unwrap(), 7);
        }
    }

    #[test]
    fn binary_fanout_repeated_reads_of_one_alias_stay_bounded() {
        // Regression: with fanout 2, every read of the same alias used to add
        // unary nodes and one level of depth (12,199 cells after 2,000 reads).
        let mut sam = sam::<u64>();
        let mut pointers = vec![RaryPointer::new(&mut sam, 7, 2).unwrap()];
        let aliases = pointers[0].copy_many(&mut sam, 99).unwrap();
        pointers.extend(aliases);
        let live_after_copy = sam.snapshot().blocks.len();
        for _ in 0..2_000 {
            assert_eq!(pointers[0].get(&mut sam).unwrap(), 7);
        }
        let live = sam.snapshot().blocks.len();
        assert!(
            live <= 2 * live_after_copy,
            "binary r-ary tree grew from {live_after_copy} to {live} cells"
        );
        for pointer in &mut pointers {
            assert_eq!(pointer.get(&mut sam).unwrap(), 7);
        }
    }

    #[test]
    fn random_copies_deletes_and_reads_keep_the_tree_valid() {
        // Regression: deleting under a root whose child group is below the
        // minimum occupancy used to fail with "underfull non-leaf r-ary
        // parent group" (fanout 6, seed 0, step 380).
        use rand::{rngs::StdRng, Rng, SeedableRng};
        for fanout in [2, 4, 6, 8] {
            for seed in 0..10 {
                let mut sam = sam::<u64>();
                let mut rng = StdRng::seed_from_u64(seed);
                let mut pointers = vec![RaryPointer::new(&mut sam, 1, fanout).unwrap()];
                let copies = pointers[0].copy_many(&mut sam, 2).unwrap();
                pointers.extend(copies);
                for _ in 0..1_000 {
                    let index = rng.gen_range(0..pointers.len());
                    match rng.gen_range(0..3) {
                        0 => assert_eq!(pointers[index].get(&mut sam).unwrap(), 1),
                        1 => {
                            let count = rng.gen_range(1..3);
                            let copies = pointers[index].copy_many(&mut sam, count).unwrap();
                            pointers.extend(copies);
                        }
                        _ if pointers.len() > 2 => {
                            pointers.swap_remove(index).delete(&mut sam).unwrap();
                        }
                        _ => {}
                    }
                }
                for pointer in &mut pointers {
                    assert_eq!(pointer.get(&mut sam).unwrap(), 1);
                }
            }
        }
    }

    #[test]
    fn deletion_compacts_and_contracts() {
        let mut sam = sam();
        let mut pointers = RaryPointer::install(&mut sam, 7, 4, 4).unwrap();
        pointers[1].delete(&mut sam).unwrap();
        for index in [0, 2, 3] {
            assert_eq!(pointers[index].get(&mut sam).unwrap(), 7);
        }

        let mut only = RaryPointer::new(&mut sam, 9, 4).unwrap();
        let mut alias = only.copy(&mut sam).unwrap();
        alias.delete(&mut sam).unwrap();
        assert!(only.is_single_reference(&mut sam).unwrap());
        assert_eq!(only.get(&mut sam).unwrap(), 9);
    }

    #[test]
    fn mixed_copy_get_put_stress() {
        let mut sam = sam();
        let mut pointers = RaryPointer::install(&mut sam, 0, 20, 4).unwrap();
        let alias = pointers[3].copy(&mut sam).unwrap();
        pointers.push(alias);
        let alias = pointers.last_mut().unwrap().copy(&mut sam).unwrap();
        pointers.push(alias);
        for round in 1..4 {
            let index = round * 5 % pointers.len();
            pointers[index].put(&mut sam, round).unwrap();
            for pointer in &mut pointers {
                assert_eq!(pointer.get(&mut sam).unwrap(), round);
            }
        }
    }

    #[test]
    fn nonfull_and_full_group_copy_paths_work() {
        let mut sam = sam();
        let mut first = RaryPointer::new(&mut sam, "x", 4).unwrap();
        let mut second = first.copy(&mut sam).unwrap();
        let mut third = first.copy(&mut sam).unwrap();
        assert_eq!(first.get(&mut sam).unwrap(), "x");
        assert_eq!(second.get(&mut sam).unwrap(), "x");
        assert_eq!(third.get(&mut sam).unwrap(), "x");

        let mut fourth = first.copy(&mut sam).unwrap();
        let mut fifth = first.copy(&mut sam).unwrap();
        for pointer in [&mut first, &mut second, &mut third, &mut fourth, &mut fifth] {
            assert_eq!(pointer.get(&mut sam).unwrap(), "x");
        }
    }

    #[test]
    fn bf4_n64_repeated_gets_and_shared_put() {
        let mut sam = sam();
        let mut pointers = RaryPointer::install(&mut sam, "shared", 64, 4).unwrap();
        for _ in 0..2 {
            for pointer in &mut pointers {
                assert_eq!(pointer.get(&mut sam).unwrap(), "shared");
            }
        }
        pointers[5].put(&mut sam, "updated").unwrap();
        for pointer in &mut pointers {
            assert_eq!(pointer.get(&mut sam).unwrap(), "updated");
        }
    }

    #[test]
    fn copy_chain_and_binary_parity_share_updates() {
        for branching_factor in [2, 4] {
            let mut sam = sam();
            let mut pointers = vec![RaryPointer::new(&mut sam, "dog", branching_factor).unwrap()];
            for _ in 0..15 {
                let alias = pointers.last_mut().unwrap().copy(&mut sam).unwrap();
                pointers.push(alias);
            }
            for pointer in &mut pointers {
                assert_eq!(pointer.get(&mut sam).unwrap(), "dog");
            }
            pointers[7].put(&mut sam, "wolf").unwrap();
            for pointer in &mut pointers {
                assert_eq!(pointer.get(&mut sam).unwrap(), "wolf");
            }
        }
    }

    #[test]
    fn single_reference_transitions_and_field_updates() {
        #[derive(Clone, Debug, Eq, PartialEq)]
        struct Record {
            label: String,
            values: Vec<u64>,
        }

        let mut sam = sam();
        let mut pointer = RaryPointer::new(
            &mut sam,
            Record {
                label: "old".into(),
                values: vec![1, 2, 3],
            },
            4,
        )
        .unwrap();
        assert!(pointer.is_single_reference(&mut sam).unwrap());
        let mut alias = pointer.copy(&mut sam).unwrap();
        assert!(!pointer.is_single_reference(&mut sam).unwrap());
        pointer
            .put_attr(&mut sam, |record| {
                record.label = "new".into();
                record.values[1] = 9;
            })
            .unwrap();
        assert_eq!(alias.get(&mut sam).unwrap().values, vec![1, 9, 3]);
        alias.delete(&mut sam).unwrap();
        assert!(pointer.is_single_reference(&mut sam).unwrap());
    }
}
