use crate::{Address, MemoryClass, SamError, SingleAccessMachine};

const STRUCTURE: &str = "SmartPointerMultiWrite";

/// A binary inverted-tree cell used by [`MultiWritePointer`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MultiWriteCell<V> {
    /// Unique root containing the shared payload.
    Root(V),
    /// One tree member pointing upward and naming its sibling.
    Inner { parent: Address, sibling: Address },
}

/// A smart pointer for single-read, multi-write SAM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MultiWritePointer {
    head: Option<Address>,
}

impl MultiWritePointer {
    pub(crate) fn from_persisted_head(head: Address) -> Self {
        Self { head: Some(head) }
    }

    /// Allocates a new uniquely referenced value.
    pub fn new<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        sam: &mut S,
        value: V,
    ) -> Result<Self, SamError> {
        let head = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
        sam.write(head, MultiWriteCell::Root(value), STRUCTURE)?;
        Ok(Self { head: Some(head) })
    }

    /// Returns the pointer's current logical address.
    pub fn head(&self) -> Option<Address> {
        self.head
    }

    /// Creates one alias and refreshes the original pointer.
    pub fn copy<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<Self, SamError> {
        Ok(self.copy_many(sam, 1)?.remove(0))
    }

    /// Creates `num_copies` aliases as a balanced binary subtree.
    pub fn copy_many<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        &mut self,
        sam: &mut S,
        num_copies: usize,
    ) -> Result<Vec<Self>, SamError> {
        if num_copies == 0 {
            return Err(SamError::InvalidParameter("num_copies must be positive"));
        }
        let parent = self
            .head
            .ok_or(SamError::InvalidPointerCell("deleted multi-write pointer"))?;
        let mut leaves = Vec::with_capacity(num_copies + 1);
        Self::build_balanced_subtree(sam, parent, num_copies + 1, &mut leaves)?;
        self.head = Some(leaves[0]);
        Ok(leaves[1..]
            .iter()
            .copied()
            .map(|head| Self { head: Some(head) })
            .collect())
    }

    /// Creates `num_copies` raw aliases plus a replacement for `parent`.
    ///
    /// The returned vector contains the replacement original first followed by
    /// the requested copies. `copy` deliberately remains scalar, matching the
    /// Python interface for every backend except explicit bulk-copy calls.
    pub fn copy_raw_address<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        sam: &mut S,
        parent: Address,
        num_copies: usize,
    ) -> Result<Vec<Address>, SamError> {
        if num_copies == 0 {
            return Err(SamError::InvalidParameter("num_copies must be positive"));
        }
        let mut leaves = Vec::with_capacity(num_copies + 1);
        Self::build_balanced_subtree(sam, parent, num_copies + 1, &mut leaves)?;
        Ok(leaves)
    }

    fn build_balanced_subtree<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        sam: &mut S,
        parent: Address,
        leaves_needed: usize,
        leaves: &mut Vec<Address>,
    ) -> Result<(), SamError> {
        if leaves_needed < 2 {
            return Err(SamError::InvalidParameter(
                "balanced subtree needs two leaves",
            ));
        }
        let left_count = leaves_needed / 2;
        let right_count = leaves_needed - left_count;
        let left = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
        let right = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
        Self::link(sam, left, right, parent)?;

        if left_count == 1 {
            leaves.push(left);
        } else {
            Self::build_balanced_subtree(sam, left, left_count, leaves)?;
        }
        if right_count == 1 {
            leaves.push(right);
        } else {
            Self::build_balanced_subtree(sam, right, right_count, leaves)?;
        }
        Ok(())
    }

    fn link<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        sam: &mut S,
        left: Address,
        right: Address,
        parent: Address,
    ) -> Result<(), SamError> {
        sam.write(
            left,
            MultiWriteCell::Inner {
                parent,
                sibling: right,
            },
            STRUCTURE,
        )?;
        sam.write(
            right,
            MultiWriteCell::Inner {
                parent,
                sibling: left,
            },
            STRUCTURE,
        )
    }

    fn deref<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        sam: &mut S,
        head: Address,
    ) -> Result<(Address, Address, V), SamError> {
        let (new_head, root, value) = Self::deref_until(sam, head, &|_| false)?;
        let value = value.ok_or(SamError::InvalidPointerCell(
            "uncached walk reported a cache hit",
        ))?;
        Ok((new_head, root, value))
    }

    /// Dereferences `head`, rebuilding its path, but never reads an address
    /// for which `cached` holds: such an address is a live cached root whose
    /// value is client-resident and not yet written back. Returns the new
    /// head, the write-back root, and the value on a miss (`None` on a hit).
    fn deref_until<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        sam: &mut S,
        head: Address,
        cached: &dyn Fn(Address) -> bool,
    ) -> Result<(Address, Address, Option<V>), SamError> {
        if cached(head) {
            // This pointer's head is itself the live cached root.
            return Ok((head, head, None));
        }
        let cell = sam
            .read(head, STRUCTURE)?
            .ok_or(SamError::InvalidPointerCell("unwritten pointer head"))?;
        sam.retire(head);
        let fresh_head = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
        match cell {
            MultiWriteCell::Root(value) => Ok((fresh_head, fresh_head, Some(value))),
            MultiWriteCell::Inner { parent, sibling } => {
                let (root, value) = Self::splay(sam, fresh_head, sibling, parent, cached)?;
                Ok((fresh_head, root, value))
            }
        }
    }

    /// Cache-aware dereference used by [`crate::pointer::CachedPointers`].
    pub(crate) fn deref_cached<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        &mut self,
        sam: &mut S,
        cached: &dyn Fn(Address) -> bool,
    ) -> Result<(Address, Option<V>), SamError> {
        let head = self
            .head
            .ok_or(SamError::InvalidPointerCell("deleted multi-write pointer"))?;
        let (new_head, root, value) = Self::deref_until(sam, head, cached)?;
        self.head = Some(new_head);
        Ok((root, value))
    }

    pub(crate) fn finish_cached<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        sam: &mut S,
        root: Address,
        value: V,
    ) -> Result<(), SamError> {
        sam.write(root, MultiWriteCell::Root(value), STRUCTURE)
    }

    /// Zig-zag splay towards the root. A live cached root is linked to but
    /// never read, exactly as a read root would be except that it keeps its
    /// address (the cache's write-back address) instead of getting a new one.
    fn splay<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        sam: &mut S,
        mut left: Address,
        mut right: Address,
        mut parent: Address,
        cached: &dyn Fn(Address) -> bool,
    ) -> Result<(Address, Option<V>), SamError> {
        loop {
            if cached(parent) {
                Self::link(sam, left, right, parent)?;
                return Ok((parent, None));
            }
            let cell = sam
                .read(parent, STRUCTURE)?
                .ok_or(SamError::InvalidPointerCell("unwritten parent"))?;
            sam.retire(parent);
            match cell {
                MultiWriteCell::Root(value) => {
                    let new_root = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
                    Self::link(sam, left, right, new_root)?;
                    return Ok((new_root, Some(value)));
                }
                MultiWriteCell::Inner {
                    parent: grandparent,
                    sibling,
                } => {
                    if cached(grandparent) {
                        let lower_parent = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
                        Self::link(sam, right, sibling, lower_parent)?;
                        Self::link(sam, left, lower_parent, grandparent)?;
                        return Ok((grandparent, None));
                    }
                    let grandparent_cell = sam
                        .read(grandparent, STRUCTURE)?
                        .ok_or(SamError::InvalidPointerCell("unwritten grandparent"))?;
                    sam.retire(grandparent);
                    match grandparent_cell {
                        MultiWriteCell::Root(value) => {
                            let lower_parent = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
                            let new_root = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
                            Self::link(sam, right, sibling, lower_parent)?;
                            Self::link(sam, left, lower_parent, new_root)?;
                            return Ok((new_root, Some(value)));
                        }
                        MultiWriteCell::Inner {
                            parent: next_parent,
                            sibling: grandparent_sibling,
                        } => {
                            let left_parent = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
                            let right_parent = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
                            Self::link(sam, left, right, left_parent)?;
                            Self::link(sam, sibling, grandparent_sibling, right_parent)?;
                            left = left_parent;
                            right = right_parent;
                            parent = next_parent;
                        }
                    }
                }
            }
        }
    }

    /// Reads the shared payload and rebuilds the consumed path.
    pub fn get<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<V, SamError> {
        let head = self
            .head
            .ok_or(SamError::InvalidPointerCell("deleted multi-write pointer"))?;
        let (new_head, root, value) = Self::deref(sam, head)?;
        sam.write(root, MultiWriteCell::Root(value.clone()), STRUCTURE)?;
        self.head = Some(new_head);
        Ok(value)
    }

    /// Reads and independently clones the shared value.
    pub fn get_and_copy<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<V, SamError> {
        self.get(sam)
    }

    /// Reads a value and lets the caller smart-copy nested pointer fields.
    pub fn get_and_copy_with<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        &mut self,
        sam: &mut S,
        copy_nested: impl FnOnce(&mut V, &mut S) -> Result<V, SamError>,
    ) -> Result<V, SamError> {
        self.with_value(sam, copy_nested)
    }

    /// Reads one field selected by a Rust closure.
    pub fn get_attr<V: Clone, T: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        &mut self,
        sam: &mut S,
        select: impl FnOnce(&V) -> &T,
    ) -> Result<T, SamError> {
        self.get(sam).map(|value| select(&value).clone())
    }

    /// Replaces the shared payload.
    pub fn put<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        &mut self,
        sam: &mut S,
        value: V,
    ) -> Result<(), SamError> {
        let head = self
            .head
            .ok_or(SamError::InvalidPointerCell("deleted multi-write pointer"))?;
        let (new_head, root, _) = Self::deref(sam, head)?;
        sam.write(root, MultiWriteCell::Root(value), STRUCTURE)?;
        self.head = Some(new_head);
        Ok(())
    }

    /// Mutates the shared value with one traversal and one root write-back.
    pub fn modify<V: Clone, R, S: SingleAccessMachine<MultiWriteCell<V>>>(
        &mut self,
        sam: &mut S,
        update: impl FnOnce(&mut V) -> R,
    ) -> Result<R, SamError> {
        let head = self
            .head
            .ok_or(SamError::InvalidPointerCell("deleted multi-write pointer"))?;
        let (new_head, root, mut value) = Self::deref(sam, head)?;
        let result = update(&mut value);
        sam.write(root, MultiWriteCell::Root(value), STRUCTURE)?;
        self.head = Some(new_head);
        Ok(result)
    }

    /// Operates on a live value and exposes the SAM for nested pointer work.
    ///
    /// This is the Rust counterpart of Python's ownership-aware `move`: the
    /// enclosing pointer is traversed once, nested pointers may refresh their
    /// own heads through `sam`, and the containing value is then written back.
    pub fn with_value<V: Clone, R, S: SingleAccessMachine<MultiWriteCell<V>>>(
        &mut self,
        sam: &mut S,
        operation: impl FnOnce(&mut V, &mut S) -> Result<R, SamError>,
    ) -> Result<R, SamError> {
        let head = self
            .head
            .ok_or(SamError::InvalidPointerCell("deleted multi-write pointer"))?;
        let (new_head, root, mut value) = Self::deref(sam, head)?;
        let result = operation(&mut value, sam);
        sam.write(root, MultiWriteCell::Root(value), STRUCTURE)?;
        self.head = Some(new_head);
        result
    }

    /// Rust equivalent of Python's dynamic `put_attr` operation.
    pub fn put_attr<V: Clone, R, S: SingleAccessMachine<MultiWriteCell<V>>>(
        &mut self,
        sam: &mut S,
        update: impl FnOnce(&mut V) -> R,
    ) -> Result<R, SamError> {
        self.modify(sam, update)
    }

    /// Returns whether this is the only alias.
    pub fn is_single_reference<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<bool, SamError> {
        let head = self
            .head
            .ok_or(SamError::InvalidPointerCell("deleted multi-write pointer"))?;
        let (new_head, root, value) = Self::deref(sam, head)?;
        sam.write(root, MultiWriteCell::Root(value), STRUCTURE)?;
        self.head = Some(new_head);
        Ok(new_head == root)
    }

    /// Removes this alias and repairs its sibling path.
    pub fn delete<V: Clone, S: SingleAccessMachine<MultiWriteCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<(), SamError> {
        let Some(head) = self.head.take() else {
            return Ok(());
        };
        let cell = sam
            .read(head, STRUCTURE)?
            .ok_or(SamError::InvalidPointerCell("unwritten deleted head"))?;
        sam.retire(head);
        if let MultiWriteCell::Inner { parent, sibling } = cell {
            let parent_cell = sam
                .read(parent, STRUCTURE)?
                .ok_or(SamError::InvalidPointerCell("unwritten deleted parent"))?;
            sam.retire(parent);
            match parent_cell {
                MultiWriteCell::Root(value) => {
                    sam.write(sibling, MultiWriteCell::Root(value), STRUCTURE)?;
                }
                MultiWriteCell::Inner {
                    parent: grandparent,
                    sibling: parent_sibling,
                } => {
                    Self::link(sam, sibling, parent_sibling, grandparent)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccessPolicy, DryRunSam};
    use rand::{rngs::StdRng, Rng, SeedableRng};

    #[test]
    fn balanced_bulk_copies_share_updates() {
        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut original = MultiWritePointer::new(&mut sam, 7_u64).unwrap();
        let before = sam.stats().operations;
        let mut copies = original.copy_many(&mut sam, 100).unwrap();
        let after = sam.stats().operations;
        assert_eq!(after.reads - before.reads, 0);
        assert_eq!(copies.len(), 100);

        copies[37].put(&mut sam, 11).unwrap();
        assert_eq!(original.get(&mut sam).unwrap(), 11);
        for copy in &mut copies {
            assert_eq!(copy.get(&mut sam).unwrap(), 11);
        }
    }

    #[test]
    fn deletion_contracts_the_last_sibling() {
        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut original = MultiWritePointer::new(&mut sam, 3_u64).unwrap();
        let mut copy = original.copy(&mut sam).unwrap();
        copy.delete(&mut sam).unwrap();
        assert_eq!(original.get(&mut sam).unwrap(), 3);
        assert!(original.is_single_reference(&mut sam).unwrap());
    }

    #[test]
    fn python_shared_update_sequence() {
        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut first = MultiWritePointer::new(&mut sam, "cat").unwrap();
        assert!(first.is_single_reference(&mut sam).unwrap());
        let mut second = first.copy(&mut sam).unwrap();
        assert!(!first.is_single_reference(&mut sam).unwrap());
        assert_eq!(first.get(&mut sam).unwrap(), "cat");
        assert_eq!(first.get(&mut sam).unwrap(), "cat");
        let mut third = first.copy(&mut sam).unwrap();
        first.put(&mut sam, "dog").unwrap();
        for _ in 0..10 {
            assert_eq!(first.get(&mut sam).unwrap(), "dog");
            assert_eq!(second.get(&mut sam).unwrap(), "dog");
        }

        let independent_value = second.get(&mut sam).unwrap();
        let mut independent = MultiWritePointer::new(&mut sam, independent_value).unwrap();
        first.put(&mut sam, "horse").unwrap();
        assert_eq!(second.get(&mut sam).unwrap(), "horse");
        assert_eq!(independent.get(&mut sam).unwrap(), "dog");
        third.delete(&mut sam).unwrap();
        second.delete(&mut sam).unwrap();
        assert!(first.is_single_reference(&mut sam).unwrap());
    }

    #[test]
    fn raw_bulk_copy_builds_balanced_leaves_without_reads() {
        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let root = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
        sam.write(root, MultiWriteCell::Root(5_u64), STRUCTURE)
            .unwrap();
        let before = sam.stats().operations;
        let leaves = MultiWritePointer::copy_raw_address(&mut sam, root, 63).unwrap();
        let after = sam.stats().operations;
        assert_eq!(leaves.len(), 64);
        assert_eq!(after.reads - before.reads, 0);
    }

    #[test]
    fn closure_attributes_and_modify_match_python_interface() {
        #[derive(Clone, Debug, Eq, PartialEq)]
        struct Record {
            label: String,
            values: Vec<u64>,
        }

        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut pointer = MultiWritePointer::new(
            &mut sam,
            Record {
                label: "old".into(),
                values: vec![1, 2, 3],
            },
        )
        .unwrap();
        assert_eq!(
            pointer.get_attr(&mut sam, |record| &record.label).unwrap(),
            "old"
        );
        pointer
            .put_attr(&mut sam, |record| {
                record.label = "new".into();
                record.values[1] = 9;
            })
            .unwrap();
        assert_eq!(pointer.get(&mut sam).unwrap().values, vec![1, 9, 3]);
    }

    #[test]
    fn nested_pointer_list_can_be_smart_copied() {
        #[derive(Clone, Debug, Eq, PartialEq)]
        enum Payload {
            Animal(&'static str),
            Pointers(Vec<MultiWritePointer>),
        }

        let animals = ["cow", "cat", "dog", "horse"];
        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut children = animals
            .iter()
            .map(|animal| MultiWritePointer::new(&mut sam, Payload::Animal(animal)).unwrap())
            .collect::<Vec<_>>();
        let mut parent =
            MultiWritePointer::new(&mut sam, Payload::Pointers(std::mem::take(&mut children)))
                .unwrap();

        let copied = parent
            .get_and_copy_with(&mut sam, |value, sam| {
                let Payload::Pointers(children) = value else {
                    return Err(SamError::InvalidParameter("expected pointer list"));
                };
                let aliases = children
                    .iter_mut()
                    .map(|pointer| pointer.copy(sam))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Payload::Pointers(aliases))
            })
            .unwrap();
        let Payload::Pointers(mut aliases) = copied else {
            panic!("expected pointer list")
        };
        for (pointer, animal) in aliases.iter_mut().zip(animals) {
            assert_eq!(pointer.get(&mut sam).unwrap(), Payload::Animal(animal));
        }
    }

    #[test]
    fn hundred_copy_indexed_and_random_sweeps_match_python_benchmark_test() {
        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut original = MultiWritePointer::new(&mut sam, "cat").unwrap();
        let mut copies = (0..100)
            .map(|_| original.copy(&mut sam).unwrap())
            .collect::<Vec<_>>();
        for _ in 0..3 {
            for pointer in &mut copies {
                assert_eq!(pointer.get(&mut sam).unwrap(), "cat");
            }
        }
        let mut rng = StdRng::seed_from_u64(0);
        for _ in 0..300 {
            let index = rng.gen_range(0..copies.len());
            assert_eq!(copies[index].get(&mut sam).unwrap(), "cat");
        }
    }
}
