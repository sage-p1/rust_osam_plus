//! Port of Python's `smart_stack.py` (`SmartStack`).
//!
//! A LIFO linked list stored in SAM. Python represents the stack by a bare
//! `top: Optional[int]`; [`SmartStack`] wraps that value (`top` is public).
//! `push` allocates a new top and writes `(value, previous_top)` to it; `pop`
//! reads the top once and retires it.
//!
//! Python's class-level `SmartStack.stack_osam` flag is taken as an explicit
//! [`MemoryClass`] argument of [`SmartStack::push`]. The pointer backend `B`
//! is named with a turbofish: `stack.push::<MultiWritePointers, _, _>(..)`.

use super::{Item, StackEntry};
use crate::pointer::RawValueCells;
use crate::{Address, GraphObject, MemoryClass, SamError, SingleAccessMachine};

/// Structure label used for SAM statistics (Python's `SmartStack.structure`).
pub const STRUCTURE: &str = "SmartStack";

/// A SAM-built LIFO stack (Python's `top`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SmartStack {
    /// Address of the most recently pushed entry; `None` when empty.
    pub top: Option<Address>,
}

impl SmartStack {
    /// Python's `SmartStack.init_stack()`: an empty stack, no SAM operations.
    pub fn init() -> Self {
        Self { top: None }
    }

    /// True when the stack is empty.
    pub fn is_empty(&self) -> bool {
        self.top.is_none()
    }

    /// Python's `SmartStack.push(top, value)`: one allocation and one write.
    pub fn push<B, P, S>(
        &mut self,
        sam: &mut S,
        class: MemoryClass,
        value: Item<P>,
    ) -> Result<(), SamError>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
    {
        let new_top = sam.alloc(class, STRUCTURE);
        sam.write(
            new_top,
            B::raw_cell(GraphObject::Stack(StackEntry {
                value,
                next: self.top,
            })),
            STRUCTURE,
        )?;
        self.top = Some(new_top);
        Ok(())
    }

    /// Python's `SmartStack.pop(top)`: `Item::None` without SAM operations on
    /// an empty stack, otherwise one read of `top` followed by retiring it.
    pub fn pop<B, P, S>(&mut self, sam: &mut S) -> Result<Item<P>, SamError>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
    {
        let Some(top) = self.top else {
            return Ok(Item::None);
        };
        let entry = sam.read(top, STRUCTURE)?;
        sam.retire(top);
        match entry {
            // Python: a non-tuple entry yields (None, None).
            None => {
                self.top = None;
                Ok(Item::None)
            }
            Some(cell) => match B::raw_value(cell)? {
                GraphObject::Stack(StackEntry { value, next }) => {
                    self.top = next;
                    Ok(value)
                }
                _ => Err(SamError::InvalidPointerCell("expected a SmartStack entry")),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pointer::{MultiWritePointer, MultiWritePointers, RecursivePointers};
    use crate::{AccessPolicy, DryRunSam, OperationCounts};

    type P = MultiWritePointer;

    #[test]
    fn lifo_order_and_counts() {
        let mut sam = DryRunSam::new(AccessPolicy::SINGLE_WRITE);
        let mut s = SmartStack::init();
        assert_eq!(
            s.pop::<MultiWritePointers, P, _>(&mut sam).unwrap(),
            Item::None
        );
        for v in 0..4 {
            s.push::<MultiWritePointers, P, _>(&mut sam, MemoryClass::Oblivious, Item::Int(v))
                .unwrap();
        }
        for v in (0..4).rev() {
            assert_eq!(
                s.pop::<MultiWritePointers, P, _>(&mut sam).unwrap(),
                Item::Int(v)
            );
        }
        assert!(s.is_empty());
        assert_eq!(
            s.pop::<MultiWritePointers, P, _>(&mut sam).unwrap(),
            Item::None
        );
        assert_eq!(
            sam.stats().operations,
            OperationCounts {
                allocations: 4,
                reads: 4,
                writes: 4
            }
        );
    }

    #[test]
    fn interleaved_lifo_matches_vec() {
        let mut sam = DryRunSam::new(AccessPolicy::RECURSIVE);
        let mut s = SmartStack::init();
        let mut model = Vec::new();
        let mut x: u64 = 9;
        for step in 0..500 {
            x = (x * 1103515245 + 12345) % (1 << 31);
            if x % 5 < 3 {
                s.push::<RecursivePointers, P, _>(
                    &mut sam,
                    MemoryClass::Oblivious,
                    Item::Int(step),
                )
                .unwrap();
                model.push(step);
            } else {
                let got = s.pop::<RecursivePointers, P, _>(&mut sam).unwrap();
                assert_eq!(got, model.pop().map_or(Item::None, Item::Int));
            }
        }
        // Recycling keeps fresh allocations at the peak stack depth.
        assert!(sam.stats().operations.allocations < 500);
    }
}
