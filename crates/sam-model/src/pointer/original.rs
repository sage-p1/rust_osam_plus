use super::backend::{CachedRoots, NoCachedRoots};
use crate::{Address, MemoryClass, SamError, SingleAccessMachine};

const POINTER_STRUCTURE: &str = "SmartPointerOriginal";
const QUEUE_STRUCTURE: &str = "AddressQueue";

/// One node in the original single-read/single-write pointer tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OriginalNode<V> {
    pub(crate) left_tail: Option<Address>,
    pub(crate) right_tail: Option<Address>,
    pub(crate) parent_head: Option<Address>,
    pub(crate) root: bool,
    pub(crate) value: Option<V>,
    pub(crate) count: usize,
}

/// Storage used by both the original pointer tree and its SAM queues.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OriginalCell<V> {
    /// One queue link containing a pointer-tree node address.
    Queue { value: Address, next: Address },
    /// A pointer-tree node.
    Node(OriginalNode<V>),
    /// A raw value written straight to SAM by an oblivious data structure
    /// (queue, stack, AVL tree) sharing the pointers' SAM.
    Raw(V),
}

/// Smart pointer for a single-read, single-write SAM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OriginalPointer {
    head: Option<Address>,
}

/// Staged root metadata retained by the client cache until write-back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OriginalWriteback<V> {
    address: Address,
    node: OriginalNode<V>,
}

/// Where following one original-pointer queue led.
enum Chased<V> {
    /// The queue was empty.
    Empty,
    /// A node read from SAM, with the consumed tail cleared.
    Node(OriginalNode<V>),
    /// A live cached root at this write-back address (not read).
    Cached(Address),
}

impl OriginalPointer {
    pub(crate) fn from_persisted_head(head: Address) -> Self {
        Self { head: Some(head) }
    }

    /// Allocates a new uniquely referenced value.
    pub fn new<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        sam: &mut S,
        value: V,
    ) -> Result<Self, SamError> {
        let mut node = OriginalNode {
            left_tail: None,
            right_tail: None,
            parent_head: None,
            root: true,
            value: Some(value),
            count: 1,
        };
        let head = Self::add_tail(sam, &mut node)?;
        Self::save_node(sam, node)?;
        Ok(Self { head: Some(head) })
    }

    /// Returns the pointer's current queue head.
    pub fn head(&self) -> Option<Address> {
        self.head
    }

    fn init_queue<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        sam: &mut S,
    ) -> (Address, Address) {
        let head = sam.alloc(MemoryClass::Oblivious, QUEUE_STRUCTURE);
        (head, head)
    }

    fn enqueue<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        sam: &mut S,
        tail: Address,
        value: Address,
    ) -> Result<Address, SamError> {
        let next = sam.alloc(MemoryClass::Oblivious, QUEUE_STRUCTURE);
        sam.write(tail, OriginalCell::Queue { value, next }, QUEUE_STRUCTURE)?;
        Ok(next)
    }

    fn dequeue<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        sam: &mut S,
        head: Address,
    ) -> Result<(Option<Address>, Option<Address>), SamError> {
        let cell = sam.read(head, QUEUE_STRUCTURE)?;
        sam.retire(head);
        match cell {
            Some(OriginalCell::Queue { value, next }) => Ok((Some(value), Some(next))),
            None => Ok((None, None)),
            Some(OriginalCell::Node(_) | OriginalCell::Raw(_)) => Err(
                SamError::InvalidPointerCell("address queue contained a non-queue cell"),
            ),
        }
    }

    fn chase<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        sam: &mut S,
        head: Option<Address>,
    ) -> Result<Option<OriginalNode<V>>, SamError> {
        match Self::chase_cached(sam, head, &mut NoCachedRoots)? {
            Chased::Empty => Ok(None),
            Chased::Node(node) => Ok(Some(node)),
            Chased::Cached(_) => Err(SamError::InvalidPointerCell(
                "uncached original chase reached a cached root",
            )),
        }
    }

    /// Follows one queue to its latest node. A live cached root is never
    /// read: the consumed tail is cleared in its client-resident node instead.
    fn chase_cached<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        sam: &mut S,
        head: Option<Address>,
        roots: &mut dyn CachedRoots<OriginalWriteback<V>>,
    ) -> Result<Chased<V>, SamError> {
        let mut current = head;
        let mut target = None;
        let mut latest = None;
        let mut tail = None;
        while let Some(address) = current {
            latest = target;
            tail = Some(address);
            (target, current) = Self::dequeue(sam, address)?;
        }
        let Some(node_address) = latest else {
            return Ok(Chased::Empty);
        };
        if let Some(writeback) = roots.writeback_mut(node_address) {
            Self::clear_tail(&mut writeback.node, tail)?;
            return Ok(Chased::Cached(node_address));
        }
        let cell =
            sam.read(node_address, POINTER_STRUCTURE)?
                .ok_or(SamError::InvalidPointerCell(
                    "unwritten original pointer node",
                ))?;
        sam.retire(node_address);
        let OriginalCell::Node(mut node) = cell else {
            return Err(SamError::InvalidPointerCell(
                "original pointer path reached a queue cell",
            ));
        };
        Self::clear_tail(&mut node, tail)?;
        Ok(Chased::Node(node))
    }

    fn clear_tail<V>(node: &mut OriginalNode<V>, tail: Option<Address>) -> Result<(), SamError> {
        if node.left_tail == tail {
            node.left_tail = None;
        } else if node.right_tail == tail {
            node.right_tail = None;
        } else {
            return Err(SamError::InvalidPointerCell(
                "original pointer tail does not belong to node",
            ));
        }
        Ok(())
    }

    /// Starts a new queue on a live cached root whose first entry is the
    /// root's reserved write-back address, so the returned head becomes a
    /// legal pointer once the root is written back (Python's
    /// `__add_cached_tail`).
    fn add_cached_tail<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        sam: &mut S,
        node: &mut OriginalNode<V>,
        address: Address,
    ) -> Result<Address, SamError> {
        let (head, tail) = Self::init_queue(sam);
        let tail = Self::enqueue(sam, tail, address)?;
        if node.left_tail.is_none() {
            node.left_tail = Some(tail);
        } else if node.right_tail.is_none() {
            node.right_tail = Some(tail);
        } else {
            return Err(SamError::InvalidPointerCell(
                "original pointer node already has two tails",
            ));
        }
        Ok(head)
    }

    fn add_tail<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        sam: &mut S,
        node: &mut OriginalNode<V>,
    ) -> Result<Address, SamError> {
        let (head, tail) = Self::init_queue(sam);
        if node.left_tail.is_none() {
            node.left_tail = Some(tail);
        } else if node.right_tail.is_none() {
            node.right_tail = Some(tail);
        } else {
            return Err(SamError::InvalidPointerCell(
                "original pointer node already has two tails",
            ));
        }
        Ok(head)
    }

    fn stage_node<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        sam: &mut S,
        node: &mut OriginalNode<V>,
    ) -> Result<Address, SamError> {
        let address = sam.alloc(MemoryClass::Oblivious, POINTER_STRUCTURE);
        if let Some(tail) = node.left_tail {
            node.left_tail = Some(Self::enqueue(sam, tail, address)?);
        }
        if let Some(tail) = node.right_tail {
            node.right_tail = Some(Self::enqueue(sam, tail, address)?);
        }
        Ok(address)
    }

    fn save_node<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        sam: &mut S,
        mut node: OriginalNode<V>,
    ) -> Result<Address, SamError> {
        let address = Self::stage_node(sam, &mut node)?;
        sam.write(address, OriginalCell::Node(node), POINTER_STRUCTURE)?;
        Ok(address)
    }

    fn deref<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<OriginalNode<V>, SamError> {
        let mut node = Self::chase(sam, self.head)?
            .ok_or(SamError::InvalidPointerCell("empty original pointer"))?;
        self.head = Some(Self::add_tail(sam, &mut node)?);
        while !node.root {
            let mut parent = Self::chase(sam, node.parent_head)?.ok_or(
                SamError::InvalidPointerCell("missing original pointer parent"),
            )?;
            node.parent_head = Some(Self::add_tail(sam, &mut parent)?);
            Self::save_node(sam, node)?;
            node = parent;
        }
        Ok(node)
    }

    pub(crate) fn finish_cached<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        sam: &mut S,
        mut writeback: OriginalWriteback<V>,
        value: V,
    ) -> Result<(), SamError> {
        writeback.node.value = Some(value);
        sam.write(
            writeback.address,
            OriginalCell::Node(writeback.node),
            POINTER_STRUCTURE,
        )
    }

    /// Cache-aware dereference (Python's `deref_cached`). On a miss the path
    /// below the root is rebuilt as in `get` and the root is only staged; on a
    /// hit the rebuilt path is linked to the live cached root without reading
    /// it. Returns the write-back root and, on a miss, the value and staging.
    #[allow(clippy::type_complexity)]
    pub(crate) fn deref_cached<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        &mut self,
        sam: &mut S,
        roots: &mut dyn CachedRoots<OriginalWriteback<V>>,
    ) -> Result<(Address, Option<(V, OriginalWriteback<V>)>), SamError> {
        let mut node = match Self::chase_cached(sam, self.head, roots)? {
            Chased::Empty => return Err(SamError::InvalidPointerCell("empty original pointer")),
            Chased::Cached(root) => {
                let writeback = roots
                    .writeback_mut(root)
                    .ok_or(SamError::InvalidPointerCell(
                        "cached original root vanished",
                    ))?;
                self.head = Some(Self::add_cached_tail(sam, &mut writeback.node, root)?);
                return Ok((root, None));
            }
            Chased::Node(node) => node,
        };
        self.head = Some(Self::add_tail(sam, &mut node)?);
        while !node.root {
            match Self::chase_cached(sam, node.parent_head, roots)? {
                Chased::Empty => {
                    return Err(SamError::InvalidPointerCell(
                        "missing original pointer parent",
                    ))
                }
                Chased::Cached(root) => {
                    let writeback =
                        roots
                            .writeback_mut(root)
                            .ok_or(SamError::InvalidPointerCell(
                                "cached original root vanished",
                            ))?;
                    node.parent_head = Some(Self::add_cached_tail(sam, &mut writeback.node, root)?);
                    Self::save_node(sam, node)?;
                    return Ok((root, None));
                }
                Chased::Node(mut parent) => {
                    node.parent_head = Some(Self::add_tail(sam, &mut parent)?);
                    Self::save_node(sam, node)?;
                    node = parent;
                }
            }
        }
        let value = node
            .value
            .take()
            .ok_or(SamError::InvalidPointerCell("original root has no value"))?;
        let address = Self::stage_node(sam, &mut node)?;
        Ok((address, Some((value, OriginalWriteback { address, node }))))
    }

    /// Cache-aware smart copy (Python's `copy_cached`). Below every live root
    /// this is the ordinary copy; when this pointer's queue leads straight to
    /// a live cached root, the new aliases are attached to its client-resident
    /// node (through a fresh branch node if another alias still reaches it).
    pub(crate) fn copy_cached<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        &mut self,
        sam: &mut S,
        roots: &mut dyn CachedRoots<OriginalWriteback<V>>,
    ) -> Result<Self, SamError> {
        let mut node = match Self::chase_cached(sam, self.head, roots)? {
            Chased::Empty => return Ok(Self { head: None }),
            Chased::Cached(root) => {
                let writeback = roots
                    .writeback_mut(root)
                    .ok_or(SamError::InvalidPointerCell(
                        "cached original root vanished",
                    ))?;
                let root_node = &mut writeback.node;
                root_node.count += 1;
                if root_node.left_tail.is_some() || root_node.right_tail.is_some() {
                    let mut branch = OriginalNode {
                        left_tail: None,
                        right_tail: None,
                        parent_head: Some(Self::add_cached_tail(sam, root_node, root)?),
                        root: false,
                        value: None,
                        count: 1,
                    };
                    let copy_head = Self::add_tail(sam, &mut branch)?;
                    let original_head = Self::add_tail(sam, &mut branch)?;
                    Self::save_node(sam, branch)?;
                    self.head = Some(original_head);
                    return Ok(Self {
                        head: Some(copy_head),
                    });
                }
                let copy_head = Self::add_cached_tail(sam, root_node, root)?;
                let original_head = Self::add_cached_tail(sam, root_node, root)?;
                self.head = Some(original_head);
                return Ok(Self {
                    head: Some(copy_head),
                });
            }
            Chased::Node(node) => node,
        };
        node.count += 1;
        if node.left_tail.is_some() || node.right_tail.is_some() {
            let parent_head = Self::add_tail(sam, &mut node)?;
            Self::save_node(sam, node)?;
            node = OriginalNode {
                left_tail: None,
                right_tail: None,
                parent_head: Some(parent_head),
                root: false,
                value: None,
                count: 1,
            };
        }
        let copy_head = Self::add_tail(sam, &mut node)?;
        let original_head = Self::add_tail(sam, &mut node)?;
        Self::save_node(sam, node)?;
        self.head = Some(original_head);
        Ok(Self {
            head: Some(copy_head),
        })
    }

    /// Creates one smart alias and refreshes the original's queue path.
    pub fn copy<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<Self, SamError> {
        let Some(mut node) = Self::chase(sam, self.head)? else {
            return Ok(Self { head: None });
        };
        node.count += 1;
        if node.left_tail.is_some() || node.right_tail.is_some() {
            let parent_head = Self::add_tail(sam, &mut node)?;
            Self::save_node(sam, node)?;
            node = OriginalNode {
                left_tail: None,
                right_tail: None,
                parent_head: Some(parent_head),
                root: false,
                value: None,
                count: 1,
            };
        }
        let copy_head = Self::add_tail(sam, &mut node)?;
        let original_head = Self::add_tail(sam, &mut node)?;
        Self::save_node(sam, node)?;
        self.head = Some(original_head);
        Ok(Self {
            head: Some(copy_head),
        })
    }

    /// Reads the shared value and rebuilds every consumed queue/node address.
    pub fn get<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<V, SamError> {
        let node = self.deref(sam)?;
        let value = node
            .value
            .clone()
            .ok_or(SamError::InvalidPointerCell("original root has no value"))?;
        Self::save_node(sam, node)?;
        Ok(value)
    }

    /// Reads and independently clones the shared value.
    pub fn get_and_copy<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<V, SamError> {
        self.get(sam)
    }

    /// Reads a value and lets the caller smart-copy nested pointer fields.
    pub fn get_and_copy_with<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        &mut self,
        sam: &mut S,
        copy_nested: impl FnOnce(&mut V, &mut S) -> Result<V, SamError>,
    ) -> Result<V, SamError> {
        self.with_value(sam, copy_nested)
    }

    /// Reads one field selected by a Rust closure.
    pub fn get_attr<V: Clone, T: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        &mut self,
        sam: &mut S,
        select: impl FnOnce(&V) -> &T,
    ) -> Result<T, SamError> {
        self.get(sam).map(|value| select(&value).clone())
    }

    /// Replaces the root value, making the update visible to every alias.
    pub fn put<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        &mut self,
        sam: &mut S,
        value: V,
    ) -> Result<(), SamError> {
        let mut node = self.deref(sam)?;
        node.value = Some(value);
        Self::save_node(sam, node)?;
        Ok(())
    }

    /// Mutates the root with one traversal and write-back.
    pub fn modify<V: Clone, R, S: SingleAccessMachine<OriginalCell<V>>>(
        &mut self,
        sam: &mut S,
        update: impl FnOnce(&mut V) -> R,
    ) -> Result<R, SamError> {
        let mut node = self.deref(sam)?;
        let value = node
            .value
            .as_mut()
            .ok_or(SamError::InvalidPointerCell("original root has no value"))?;
        let result = update(value);
        Self::save_node(sam, node)?;
        Ok(result)
    }

    /// Operates on a live value and exposes the SAM for nested pointer work.
    pub fn with_value<V: Clone, R, S: SingleAccessMachine<OriginalCell<V>>>(
        &mut self,
        sam: &mut S,
        operation: impl FnOnce(&mut V, &mut S) -> Result<R, SamError>,
    ) -> Result<R, SamError> {
        let mut node = self.deref(sam)?;
        let value = node
            .value
            .as_mut()
            .ok_or(SamError::InvalidPointerCell("original root has no value"))?;
        let result = operation(value, sam);
        Self::save_node(sam, node)?;
        result
    }

    /// Rust equivalent of Python's dynamic `put_attr` interface.
    pub fn put_attr<V: Clone, R, S: SingleAccessMachine<OriginalCell<V>>>(
        &mut self,
        sam: &mut S,
        update: impl FnOnce(&mut V) -> R,
    ) -> Result<R, SamError> {
        self.modify(sam, update)
    }

    /// Returns the number of live aliases recorded at the root.
    pub fn reference_count<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<usize, SamError> {
        let node = self.deref(sam)?;
        let count = node.count;
        Self::save_node(sam, node)?;
        Ok(count)
    }

    /// Returns whether this is the only path to the root.
    pub fn is_single_reference<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<bool, SamError> {
        let mut node = Self::chase(sam, self.head)?
            .ok_or(SamError::InvalidPointerCell("empty original pointer"))?;
        self.head = Some(Self::add_tail(sam, &mut node)?);
        let mut another = node.left_tail.is_some() && node.right_tail.is_some();
        while !node.root {
            let mut parent = Self::chase(sam, node.parent_head)?.ok_or(
                SamError::InvalidPointerCell("missing original pointer parent"),
            )?;
            node.parent_head = Some(Self::add_tail(sam, &mut parent)?);
            Self::save_node(sam, node)?;
            node = parent;
            another |= node.left_tail.is_some() && node.right_tail.is_some();
        }
        Self::save_node(sam, node)?;
        Ok(!another)
    }

    /// Removes this alias and reconnects the surviving queue path.
    pub fn delete<V: Clone, S: SingleAccessMachine<OriginalCell<V>>>(
        &mut self,
        sam: &mut S,
    ) -> Result<(), SamError> {
        let Some(head) = self.head.take() else {
            return Ok(());
        };
        let Some(mut node) = Self::chase(sam, Some(head))? else {
            return Ok(());
        };
        if node.root {
            if !(node.left_tail.is_some() && node.right_tail.is_some()) {
                node.count = node.count.saturating_sub(1);
                Self::save_node(sam, node)?;
            }
            return Ok(());
        }

        let surviving_tail =
            node.left_tail
                .or(node.right_tail)
                .ok_or(SamError::InvalidPointerCell(
                    "deleted original branch has no surviving tail",
                ))?;
        let mut parent = Self::chase(sam, node.parent_head)?.ok_or(
            SamError::InvalidPointerCell("missing original delete parent"),
        )?;
        parent.count = parent.count.saturating_sub(1);
        if parent.left_tail.is_none() {
            parent.left_tail = Some(surviving_tail);
        } else if parent.right_tail.is_none() {
            parent.right_tail = Some(surviving_tail);
        } else {
            return Err(SamError::InvalidPointerCell(
                "original delete parent has no tail slot",
            ));
        }
        Self::save_node(sam, parent)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccessPolicy, DryRunSam};

    #[test]
    fn python_original_pointer_sequence() {
        let mut sam = DryRunSam::new(AccessPolicy::SINGLE_WRITE);
        let mut first = OriginalPointer::new(&mut sam, "cat").unwrap();
        assert_eq!(first.reference_count(&mut sam).unwrap(), 1);
        assert!(first.is_single_reference(&mut sam).unwrap());

        let mut second = first.copy(&mut sam).unwrap();
        assert_eq!(second.reference_count(&mut sam).unwrap(), 2);
        assert!(!first.is_single_reference(&mut sam).unwrap());
        assert_eq!(first.get(&mut sam).unwrap(), "cat");
        assert_eq!(first.get(&mut sam).unwrap(), "cat");

        let mut third = first.copy(&mut sam).unwrap();
        assert_eq!(third.reference_count(&mut sam).unwrap(), 3);
        first.put(&mut sam, "dog").unwrap();
        for _ in 0..10 {
            assert_eq!(first.get(&mut sam).unwrap(), "dog");
            assert_eq!(second.get(&mut sam).unwrap(), "dog");
        }

        let independent_value = second.get(&mut sam).unwrap();
        let mut independent = OriginalPointer::new(&mut sam, independent_value).unwrap();
        first.put(&mut sam, "horse").unwrap();
        assert_eq!(second.get(&mut sam).unwrap(), "horse");
        assert_eq!(independent.get(&mut sam).unwrap(), "dog");

        third.delete(&mut sam).unwrap();
        assert_eq!(second.reference_count(&mut sam).unwrap(), 2);
        second.delete(&mut sam).unwrap();
        assert!(first.is_single_reference(&mut sam).unwrap());
        assert_eq!(first.reference_count(&mut sam).unwrap(), 1);
        assert_eq!(first.get(&mut sam).unwrap(), "horse");
    }

    #[test]
    fn closure_attributes_and_modify_match_dynamic_python_operations() {
        #[derive(Clone, Debug, Eq, PartialEq)]
        struct Record {
            label: String,
            values: Vec<u64>,
        }

        let mut sam = DryRunSam::new(AccessPolicy::SINGLE_WRITE);
        let mut pointer = OriginalPointer::new(
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
        assert_eq!(
            pointer.get(&mut sam).unwrap(),
            Record {
                label: "new".into(),
                values: vec![1, 9, 3]
            }
        );
    }
}
