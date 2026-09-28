//! Port of Python's `smart_queue.py` (`SmartQueue`).
//!
//! A FIFO linked list stored in SAM. The queue keeps a pre-allocated, still
//! unwritten `tail` address; `enqueue` allocates the next tail and writes
//! `(new_tail, value)` to the old one, `dequeue` reads the head entry once.
//!
//! Python's class-level `SmartQueue.queue_osam` flag is read at every
//! allocation, so the port takes it as an explicit [`MemoryClass`] argument of
//! [`SmartQueue::init`] and [`SmartQueue::enqueue`] (`Oblivious` = `True`,
//! `Plaintext` = `False`).
//!
//! Entries are raw cells of a pointer backend `B` (see
//! [`RawValueCells`]); `B` cannot be inferred from the arguments, so name it
//! with a turbofish: `queue.enqueue::<MultiWritePointers, _, _>(sam, class, v)`.

use super::{Item, QueueEntry};
use crate::pointer::RawValueCells;
use crate::{Address, GraphObject, MemoryClass, SamError, SingleAccessMachine};

/// Structure label used for SAM statistics (Python's `SmartQueue.structure`).
pub const STRUCTURE: &str = "SmartQueue";

/// A SAM-built FIFO queue. `head == tail` means empty.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SmartQueue {
    /// Address of the next entry to dequeue (`None` once the queue is closed).
    pub head: Option<Address>,
    /// Pre-allocated, not yet written address of the next entry.
    pub tail: Option<Address>,
}

impl SmartQueue {
    /// Python's `SmartQueue(head, tail)`.
    pub fn new(head: Option<Address>, tail: Option<Address>) -> Self {
        Self { head, tail }
    }

    /// Python's `SmartQueue.init_queue()`: one allocation, no reads/writes.
    pub fn init<V: Clone, S: SingleAccessMachine<V>>(sam: &mut S, class: MemoryClass) -> Self {
        let head = sam.alloc(class, STRUCTURE);
        Self {
            head: Some(head),
            tail: Some(head),
        }
    }

    /// True when `head == tail`, i.e. a dequeue would find nothing (no read).
    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    /// Python's `SmartQueue.enqueue(queue, value)`: one allocation and one write.
    ///
    /// Errors if the queue was closed (Python asserts `queue.tail` is an int).
    pub fn enqueue<B, P, S>(
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
        let tail = self
            .tail
            .ok_or(SamError::InvalidParameter("enqueue on a closed SmartQueue"))?;
        let new_tail = sam.alloc(class, STRUCTURE);
        sam.write(
            tail,
            B::raw_cell(GraphObject::Queue(QueueEntry {
                next: new_tail,
                value,
            })),
            STRUCTURE,
        )?;
        self.tail = Some(new_tail);
        Ok(())
    }

    /// Python's `SmartQueue.dequeue(queue)`.
    ///
    /// * `head == tail` (empty, including an already closed queue): retires
    ///   the head, closes the queue (`head = tail = None`) and returns
    ///   `Item::None` without reading.
    /// * otherwise one read of `head` followed by retiring it.
    ///
    /// As in Python, an empty queue and a stored `None` value both yield
    /// `Item::None`.
    pub fn dequeue<B, P, S>(&mut self, sam: &mut S) -> Result<Item<P>, SamError>
    where
        P: Clone,
        B: RawValueCells<GraphObject<P>>,
        S: SingleAccessMachine<B::Cell>,
    {
        if self.head == self.tail {
            if let Some(head) = self.head {
                sam.retire(head);
            }
            self.head = None;
            self.tail = None;
            return Ok(Item::None);
        }
        let Some(head) = self.head else {
            return Ok(Item::None);
        };
        let entry = sam.read(head, STRUCTURE)?;
        sam.retire(head);
        match entry {
            // Python: a non-tuple entry (here: an unwritten cell) becomes the head.
            None => {
                self.head = None;
                Ok(Item::None)
            }
            Some(cell) => match B::raw_value(cell)? {
                GraphObject::Queue(QueueEntry { next, value }) => {
                    self.head = Some(next);
                    Ok(value)
                }
                _ => Err(SamError::InvalidPointerCell("expected a SmartQueue entry")),
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
    fn fifo_order_and_counts() {
        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut q = SmartQueue::init(&mut sam, MemoryClass::Oblivious);
        for v in 0..5 {
            q.enqueue::<MultiWritePointers, P, _>(&mut sam, MemoryClass::Oblivious, Item::Int(v))
                .unwrap();
        }
        assert!(!q.is_empty());
        for v in 0..5 {
            assert_eq!(
                q.dequeue::<MultiWritePointers, P, _>(&mut sam).unwrap(),
                Item::Int(v)
            );
        }
        assert!(q.is_empty());
        assert_eq!(
            q.dequeue::<MultiWritePointers, P, _>(&mut sam).unwrap(),
            Item::None
        );
        assert_eq!(q, SmartQueue::new(None, None));
        // Dequeue on a closed queue is free; enqueue on it is an error.
        assert_eq!(
            q.dequeue::<MultiWritePointers, P, _>(&mut sam).unwrap(),
            Item::None
        );
        assert!(q
            .enqueue::<MultiWritePointers, P, _>(&mut sam, MemoryClass::Oblivious, Item::Int(1))
            .is_err());
        assert_eq!(
            sam.stats().operations,
            OperationCounts {
                allocations: 6,
                reads: 5,
                writes: 5
            }
        );
        assert_eq!(sam.stats().by_structure[STRUCTURE].allocations, 6);
    }

    #[test]
    fn interleaved_fifo_matches_vecdeque() {
        let mut sam = DryRunSam::new(AccessPolicy::RECURSIVE);
        let mut q = SmartQueue::init(&mut sam, MemoryClass::Oblivious);
        let mut model = std::collections::VecDeque::new();
        let mut x: u64 = 5;
        for step in 0..500 {
            x = (x * 1103515245 + 12345) % (1 << 31);
            if !x.is_multiple_of(3) {
                q.enqueue::<RecursivePointers, P, _>(
                    &mut sam,
                    MemoryClass::Oblivious,
                    Item::Int(step),
                )
                .unwrap();
                model.push_back(step);
            } else if !q.is_empty() {
                let got = q.dequeue::<RecursivePointers, P, _>(&mut sam).unwrap();
                assert_eq!(got, Item::Int(model.pop_front().unwrap()));
            }
        }
        while !q.is_empty() {
            let got = q.dequeue::<RecursivePointers, P, _>(&mut sam).unwrap();
            assert_eq!(got, Item::Int(model.pop_front().unwrap()));
        }
        assert!(model.is_empty());
    }

    #[test]
    fn plaintext_queue_is_not_counted() {
        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut q = SmartQueue::init(&mut sam, MemoryClass::Plaintext);
        q.enqueue::<MultiWritePointers, P, _>(&mut sam, MemoryClass::Plaintext, Item::Int(9))
            .unwrap();
        assert_eq!(
            q.dequeue::<MultiWritePointers, P, _>(&mut sam).unwrap(),
            Item::Int(9)
        );
        assert_eq!(sam.stats().operations, OperationCounts::default());
    }
}
