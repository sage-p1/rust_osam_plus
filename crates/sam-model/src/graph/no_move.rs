//! The Python benchmarks' "no-move" access pattern (`move-false`).
//!
//! The native graph reads and writes each object once per operation, holding
//! it while the operation runs: that is the move pattern (Python's cached
//! `CachedPointer`), and caching adds nothing to it. Python's uncached
//! `SmartPointer` works differently, and its costs are the `OSAM` and `ORAM`
//! series of the paper. [`NoMovePointers`] reproduces that cost model around
//! any backend, so the graph algorithms stay a single implementation:
//!
//! * A read (Python's `get_attr` with `UNCACHED_FULL_OWNERSHIP`) takes the
//!   whole object: one access copies every nested pointer (child slots and
//!   the edge) and ends, and the operation then runs on that detached copy.
//!   Afterwards every copy is deleted (Python returns the requested ones and
//!   deletes the rest; the caller deletes the requested ones once used).
//! * An operation that changes the object (`put_attr`, i.e. `modify` and
//!   `move`) also stores it: the stored object is read, its nested pointers
//!   are deleted, a copy of the new value is written (copying every nested
//!   pointer again), and the detached copy is discarded.
//! * `put` is Python's `put(delete_old=True)`, the same store.
//!
//! No object is held while another one is accessed, exactly as in Python.
//! A read on a backend whose addresses can be read more than once (the
//! recursive ORAM) is a plain read with no write-back, as in Python.
//!
//! To tell reads from changes, every pointer handle carries the identity of
//! the alias it is ([`Tagged`]): copies and dereferences through a slot keep
//! its identity, so an operation changed the object exactly when a field
//! differs or a slot holds a different alias (or none).

use super::{GraphObject, GraphPointerCodec};
use crate::pointer::{RawValueCells, SmartPointerBackend};
use crate::{SamError, SingleAccessMachine};
use std::sync::atomic::{AtomicU64, Ordering};

type Result<T> = std::result::Result<T, SamError>;

static NEXT_TAG: AtomicU64 = AtomicU64::new(1);

/// A pointer handle together with the identity of the alias it is. Equality
/// compares identities only.
#[derive(Clone, Debug)]
pub struct Tagged<P> {
    raw: P,
    tag: u64,
}

impl<P> Tagged<P> {
    fn fresh(raw: P) -> Self {
        Self {
            raw,
            tag: NEXT_TAG.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Wraps a raw pointer as a new alias identity (e.g. one decoded from a
    /// block; identities are not persisted).
    pub fn from_raw(raw: P) -> Self {
        Self::fresh(raw)
    }

    /// The wrapped backend pointer.
    pub fn raw(&self) -> &P {
        &self.raw
    }
}

impl<P> PartialEq for Tagged<P> {
    fn eq(&self, other: &Self) -> bool {
        self.tag == other.tag
    }
}

impl<P> Eq for Tagged<P> {}

/// Encodes a [`Tagged`] pointer as its raw pointer.
#[derive(Clone, Copy, Debug, Default)]
pub struct TaggedGraphPointerCodec<C> {
    raw: C,
}

impl<C> TaggedGraphPointerCodec<C> {
    pub fn new(raw: C) -> Self {
        Self { raw }
    }
}

impl<P, C: GraphPointerCodec<P>> GraphPointerCodec<Tagged<P>> for TaggedGraphPointerCodec<C> {
    fn identifier(&self, pointer: &Tagged<P>) -> Result<u64> {
        self.raw.identifier(&pointer.raw)
    }

    fn decode_identifier(&self, identifier: u64) -> Result<Tagged<P>> {
        Ok(Tagged::from_raw(self.raw.decode_identifier(identifier)?))
    }
}

/// The nested pointers of a graph object (child slots, then the edge).
fn slots<P>(object: &mut GraphObject<P>) -> Vec<&mut P> {
    match object {
        GraphObject::Vertex(vertex) => vertex.out_children.iter_mut().flatten().collect(),
        GraphObject::FanOut(fan_out) => fan_out
            .children
            .iter_mut()
            .flatten()
            .chain(fan_out.edge.as_mut())
            .collect(),
        // Tombstones hold no pointers; queue, stack and AVL entries are raw
        // cells, never reached through a pointer.
        _ => Vec::new(),
    }
}

/// Removes and returns the nested pointers of a graph object.
fn take_all<P>(object: &mut GraphObject<P>) -> Vec<P> {
    match object {
        GraphObject::Vertex(vertex) => vertex
            .out_children
            .iter_mut()
            .filter_map(Option::take)
            .collect(),
        GraphObject::FanOut(fan_out) => fan_out
            .children
            .iter_mut()
            .filter_map(Option::take)
            .chain(fan_out.edge.take())
            .collect(),
        _ => Vec::new(),
    }
}

/// Python's smart copy of an object: the same fields, with a new alias of
/// every nested pointer.
fn smart_copy<P, B, S>(
    inner: &mut B,
    sam: &mut S,
    object: &mut GraphObject<Tagged<P>>,
) -> Result<GraphObject<Tagged<P>>>
where
    P: Clone,
    B: SmartPointerBackend<GraphObject<Tagged<P>>, Pointer = P>,
    S: SingleAccessMachine<B::Cell>,
{
    let mut copy = object.clone();
    let aliases = slots(object)
        .into_iter()
        .map(|pointer| inner.copy_pointer(sam, &mut pointer.raw).map(Tagged::fresh))
        .collect::<Result<Vec<_>>>()?;
    for (slot, alias) in slots(&mut copy).into_iter().zip(aliases) {
        *slot = alias;
    }
    Ok(copy)
}

/// Any pointer backend with Python's no-move (uncached) access pattern; see
/// the module documentation.
#[derive(Clone, Debug, Default)]
pub struct NoMovePointers<B> {
    inner: B,
}

impl<B> NoMovePointers<B> {
    pub fn new(inner: B) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &B {
        &self.inner
    }

    /// Python's `put(delete_old=True)` of `value` (also the store that ends
    /// a `put_attr`): delete the stored object's nested pointers, write a
    /// smart copy of `value`, then delete `value`'s own pointers.
    fn store<P, S>(
        &mut self,
        sam: &mut S,
        pointer: &mut Tagged<P>,
        mut value: GraphObject<Tagged<P>>,
    ) -> Result<()>
    where
        P: Clone,
        B: SmartPointerBackend<GraphObject<Tagged<P>>, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
    {
        self.inner
            .with_value(sam, &mut pointer.raw, |object, inner, sam| {
                for mut old in take_all(object) {
                    inner.delete(sam, &mut old.raw)?;
                }
                *object = smart_copy(inner, sam, &mut value)?;
                Ok(())
            })?;
        for mut temporary in take_all(&mut value) {
            self.inner.delete(sam, &mut temporary.raw)?;
        }
        Ok(())
    }
}

impl<P, B> SmartPointerBackend<GraphObject<Tagged<P>>> for NoMovePointers<B>
where
    P: Clone,
    B: SmartPointerBackend<GraphObject<Tagged<P>>, Pointer = P>,
{
    type Pointer = Tagged<P>;
    type Cell = B::Cell;

    fn label(&self) -> &'static str {
        self.inner.label()
    }

    fn max_cached_values(&self) -> Option<usize> {
        self.inner.max_cached_values()
    }

    fn clear_cache(&mut self) -> Result<usize> {
        self.inner.clear_cache()
    }

    fn new_pointer<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        value: GraphObject<Tagged<P>>,
    ) -> Result<Self::Pointer> {
        self.inner.new_pointer(sam, value).map(Tagged::fresh)
    }

    fn copy_pointer<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<Self::Pointer> {
        self.inner
            .copy_pointer(sam, &mut pointer.raw)
            .map(Tagged::fresh)
    }

    fn copy_many<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        num_copies: usize,
    ) -> Result<Vec<Self::Pointer>> {
        Ok(self
            .inner
            .copy_many(sam, &mut pointer.raw, num_copies)?
            .into_iter()
            .map(Tagged::fresh)
            .collect())
    }

    fn get<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<Option<GraphObject<Tagged<P>>>> {
        self.inner.get(sam, &mut pointer.raw)
    }

    fn put<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        value: GraphObject<Tagged<P>>,
    ) -> Result<()> {
        self.store(sam, pointer, value)
    }

    fn delete<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<()> {
        self.inner.delete(sam, &mut pointer.raw)
    }

    fn is_single_reference<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<bool> {
        self.inner.is_single_reference(sam, &mut pointer.raw)
    }

    fn with_value<S, R, F>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        operation: F,
    ) -> Result<R>
    where
        S: SingleAccessMachine<Self::Cell>,
        F: FnOnce(&mut GraphObject<Tagged<P>>, &mut Self, &mut S) -> Result<R>,
    {
        // Full ownership: one access that copies every nested pointer.
        let mut value =
            self.inner
                .with_value_read(sam, &mut pointer.raw, |object, inner, sam| {
                    smart_copy(inner, sam, object)
                })?;
        let before = value.clone();
        let result = operation(&mut value, self, sam)?;
        if value == before {
            // A read: the copies the operation left in place are deleted.
            for mut copy in take_all(&mut value) {
                self.inner.delete(sam, &mut copy.raw)?;
            }
        } else {
            self.store(sam, pointer, value)?;
        }
        Ok(result)
    }
}

impl<P, B> RawValueCells<GraphObject<Tagged<P>>> for NoMovePointers<B>
where
    P: Clone,
    B: RawValueCells<GraphObject<Tagged<P>>, Pointer = P>,
{
    fn raw_cell(value: GraphObject<Tagged<P>>) -> Self::Cell {
        B::raw_cell(value)
    }

    fn raw_value(cell: Self::Cell) -> Result<GraphObject<Tagged<P>>> {
        B::raw_value(cell)
    }
}
