use crate::{Address, MemoryClass, SamError, SingleAccessMachine};
use std::collections::HashMap;

const STRUCTURE: &str = "RecursivePointer";

/// One client-side alias of a multi-read SAM value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecursivePointer {
    head: Option<Address>,
}

impl RecursivePointer {
    pub(crate) fn from_persisted_head(head: Address) -> Self {
        Self { head: Some(head) }
    }

    /// Returns the pointer's current logical address.
    pub fn head(&self) -> Option<Address> {
        self.head
    }
}

/// Owns recursive-pointer reference counts while values remain in SAM.
#[derive(Clone, Debug, Default)]
pub struct RecursivePointers {
    references: HashMap<Address, usize>,
}

impl RecursivePointers {
    /// Allocates a new recursive pointer.
    pub fn new_pointer<V: Clone, S: SingleAccessMachine<V>>(
        &mut self,
        sam: &mut S,
        value: V,
    ) -> Result<RecursivePointer, SamError> {
        let head = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
        sam.write(head, value, STRUCTURE)?;
        self.references.insert(head, 1);
        Ok(RecursivePointer { head: Some(head) })
    }

    /// Creates another alias without a SAM access.
    pub fn copy(&mut self, pointer: RecursivePointer) -> Result<RecursivePointer, SamError> {
        let Some(head) = pointer.head else {
            return Ok(pointer);
        };
        let count = self
            .references
            .get_mut(&head)
            .ok_or(SamError::InvalidAddress(head))?;
        *count += 1;
        Ok(RecursivePointer { head: Some(head) })
    }

    /// Creates `num_copies` aliases without a SAM access.
    pub fn copy_many(
        &mut self,
        pointer: RecursivePointer,
        num_copies: usize,
    ) -> Result<Vec<RecursivePointer>, SamError> {
        if num_copies == 0 {
            return Err(SamError::InvalidParameter("num_copies must be positive"));
        }
        (0..num_copies).map(|_| self.copy(pointer)).collect()
    }

    /// Reads the shared value.
    pub fn get<V: Clone, S: SingleAccessMachine<V>>(
        &self,
        sam: &mut S,
        pointer: RecursivePointer,
    ) -> Result<Option<V>, SamError> {
        pointer
            .head
            .map_or(Ok(None), |head| sam.read(head, STRUCTURE))
    }

    /// Reads and independently clones the shared value.
    pub fn get_and_copy<V: Clone, S: SingleAccessMachine<V>>(
        &self,
        sam: &mut S,
        pointer: RecursivePointer,
    ) -> Result<Option<V>, SamError> {
        self.get(sam, pointer)
    }

    /// Reads a value and lets the caller smart-copy nested pointer fields.
    pub fn get_and_copy_with<V: Clone, S: SingleAccessMachine<V>>(
        &mut self,
        sam: &mut S,
        pointer: RecursivePointer,
        copy_nested: impl FnOnce(&mut V, &mut Self, &mut S) -> Result<V, SamError>,
    ) -> Result<Option<V>, SamError> {
        let Some(head) = pointer.head else {
            return Ok(None);
        };
        let mut value = sam
            .read(head, STRUCTURE)?
            .ok_or(SamError::InvalidPointerCell("unwritten recursive pointer"))?;
        let copied = copy_nested(&mut value, self, sam);
        sam.write(head, value, STRUCTURE)?;
        copied.map(Some)
    }

    /// Reads one field selected by a Rust closure.
    pub fn get_attr<V: Clone, T: Clone, S: SingleAccessMachine<V>>(
        &self,
        sam: &mut S,
        pointer: RecursivePointer,
        select: impl FnOnce(&V) -> &T,
    ) -> Result<Option<T>, SamError> {
        self.get(sam, pointer)
            .map(|value| value.map(|value| select(&value).clone()))
    }

    /// Overwrites the shared value.
    pub fn put<V: Clone, S: SingleAccessMachine<V>>(
        &self,
        sam: &mut S,
        pointer: RecursivePointer,
        value: V,
    ) -> Result<(), SamError> {
        let head = pointer
            .head
            .ok_or(SamError::InvalidPointerCell("deleted recursive pointer"))?;
        sam.write(head, value, STRUCTURE)
    }

    /// Mutates a value using one read and one write.
    pub fn modify<V: Clone, R, S: SingleAccessMachine<V>>(
        &self,
        sam: &mut S,
        pointer: RecursivePointer,
        update: impl FnOnce(&mut V) -> R,
    ) -> Result<R, SamError> {
        let head = pointer
            .head
            .ok_or(SamError::InvalidPointerCell("deleted recursive pointer"))?;
        let mut value = sam
            .read(head, STRUCTURE)?
            .ok_or(SamError::InvalidPointerCell("unwritten recursive pointer"))?;
        let result = update(&mut value);
        sam.write(head, value, STRUCTURE)?;
        Ok(result)
    }

    /// Operates on a live value and exposes recursive state and SAM for nested pointers.
    pub fn with_value<V: Clone, R, S: SingleAccessMachine<V>>(
        &mut self,
        sam: &mut S,
        pointer: RecursivePointer,
        operation: impl FnOnce(&mut V, &mut Self, &mut S) -> Result<R, SamError>,
    ) -> Result<R, SamError> {
        let head = pointer
            .head
            .ok_or(SamError::InvalidPointerCell("deleted recursive pointer"))?;
        let mut value = sam
            .read(head, STRUCTURE)?
            .ok_or(SamError::InvalidPointerCell("unwritten recursive pointer"))?;
        let result = operation(&mut value, self, sam);
        sam.write(head, value, STRUCTURE)?;
        result
    }

    /// Rust equivalent of Python's dynamic `put_attr` operation.
    pub fn put_attr<V: Clone, R, S: SingleAccessMachine<V>>(
        &self,
        sam: &mut S,
        pointer: RecursivePointer,
        update: impl FnOnce(&mut V) -> R,
    ) -> Result<R, SamError> {
        self.modify(sam, pointer, update)
    }

    /// Returns whether this pointer is the only alias.
    pub fn is_single_reference(&self, pointer: RecursivePointer) -> bool {
        self.reference_count(pointer) == 1
    }

    /// Drops one alias and returns whether it was the final reference.
    pub fn delete(&mut self, pointer: &mut RecursivePointer) -> Result<bool, SamError> {
        let Some(head) = pointer.head.take() else {
            return Ok(false);
        };
        let count = self
            .references
            .get_mut(&head)
            .ok_or(SamError::InvalidAddress(head))?;
        *count -= 1;
        if *count == 0 {
            self.references.remove(&head);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Total aliases of all live values (a leak check: it returns to its
    /// previous value once every temporary copy is deleted).
    pub fn live_references(&self) -> usize {
        self.references.values().sum()
    }

    /// Returns the current alias count.
    pub fn reference_count(&self, pointer: RecursivePointer) -> usize {
        pointer
            .head
            .and_then(|head| self.references.get(&head).copied())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccessPolicy, DryRunSam};

    #[test]
    fn copies_share_updates_without_copy_accesses() {
        let mut sam = DryRunSam::new(AccessPolicy::RECURSIVE);
        let mut pointers = RecursivePointers::default();
        let original = pointers.new_pointer(&mut sam, 4_u64).unwrap();
        let copy = pointers.copy(original).unwrap();
        assert_eq!(pointers.reference_count(original), 2);
        assert_eq!(sam.stats().operations.reads, 0);
        pointers.put(&mut sam, copy, 9).unwrap();
        assert_eq!(pointers.get(&mut sam, original).unwrap(), Some(9));
    }

    #[test]
    fn python_recursive_pointer_sequence() {
        let mut sam = DryRunSam::new(AccessPolicy::RECURSIVE);
        let mut pointers = RecursivePointers::default();
        let mut first = pointers.new_pointer(&mut sam, "cat").unwrap();
        let mut second = pointers.copy(first).unwrap();
        assert_eq!(pointers.reference_count(first), 2);
        assert_eq!(pointers.get(&mut sam, first).unwrap(), Some("cat"));
        assert_eq!(pointers.get(&mut sam, first).unwrap(), Some("cat"));
        let mut third = pointers.copy(first).unwrap();
        pointers.put(&mut sam, first, "dog").unwrap();
        for _ in 0..10 {
            assert_eq!(pointers.get(&mut sam, first).unwrap(), Some("dog"));
            assert_eq!(pointers.get(&mut sam, second).unwrap(), Some("dog"));
        }

        let independent_value = pointers.get(&mut sam, second).unwrap().unwrap();
        let mut independent = pointers.new_pointer(&mut sam, independent_value).unwrap();
        pointers.put(&mut sam, first, "horse").unwrap();
        assert_eq!(pointers.get(&mut sam, second).unwrap(), Some("horse"));
        assert_eq!(pointers.get(&mut sam, independent).unwrap(), Some("dog"));
        pointers.delete(&mut third).unwrap();
        pointers.delete(&mut second).unwrap();
        assert!(pointers.is_single_reference(first));
        pointers.delete(&mut first).unwrap();
        pointers.delete(&mut independent).unwrap();
    }

    #[test]
    fn closure_attributes_and_modify_match_python_interface() {
        #[derive(Clone, Debug, Eq, PartialEq)]
        struct Record {
            label: String,
            values: Vec<u64>,
        }

        let mut sam = DryRunSam::new(AccessPolicy::RECURSIVE);
        let mut pointers = RecursivePointers::default();
        let pointer = pointers
            .new_pointer(
                &mut sam,
                Record {
                    label: "old".into(),
                    values: vec![1, 2, 3],
                },
            )
            .unwrap();
        assert_eq!(
            pointers
                .get_attr(&mut sam, pointer, |record| &record.label)
                .unwrap(),
            Some("old".into())
        );
        pointers
            .put_attr(&mut sam, pointer, |record| {
                record.label = "new".into();
                record.values[1] = 9;
            })
            .unwrap();
        assert_eq!(
            pointers.get(&mut sam, pointer).unwrap().unwrap().values,
            vec![1, 9, 3]
        );
    }

    #[test]
    fn nested_pointer_list_can_be_smart_copied() {
        #[derive(Clone, Debug, Eq, PartialEq)]
        enum Payload {
            Animal(&'static str),
            Pointers(Vec<RecursivePointer>),
        }

        let animals = ["cow", "cat", "dog", "horse"];
        let mut sam = DryRunSam::new(AccessPolicy::RECURSIVE);
        let mut pointers = RecursivePointers::default();
        let children = animals
            .iter()
            .map(|animal| {
                pointers
                    .new_pointer(&mut sam, Payload::Animal(animal))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let parent = pointers
            .new_pointer(&mut sam, Payload::Pointers(children))
            .unwrap();
        let copied = pointers
            .get_and_copy_with(&mut sam, parent, |value, pointers, _sam| {
                let Payload::Pointers(children) = value else {
                    return Err(SamError::InvalidParameter("expected pointer list"));
                };
                Ok(Payload::Pointers(
                    children
                        .iter()
                        .copied()
                        .map(|pointer| pointers.copy(pointer))
                        .collect::<Result<Vec<_>, _>>()?,
                ))
            })
            .unwrap()
            .unwrap();
        let Payload::Pointers(aliases) = copied else {
            panic!("expected pointer list")
        };
        for (pointer, animal) in aliases.into_iter().zip(animals) {
            assert_eq!(
                pointers.get(&mut sam, pointer).unwrap(),
                Some(Payload::Animal(animal))
            );
        }
    }
}
