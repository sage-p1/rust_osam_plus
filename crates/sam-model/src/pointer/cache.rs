//! Client-side cache (paper Section "Client-Side Cache").
//!
//! The cache is a map from SAM⁺ write-back addresses to values. A value
//! enters the cache when a pointer to it is dereferenced: the backend's walk
//! rebuilds the pointer path, stages the value's root at a fresh write-back
//! address, and the value is kept client-side under that address instead of
//! being written. Each entry carries a reference count (live [`CacheObject`]
//! handles, plus scoped field accesses); when it drops to zero the value is
//! written to its root and leaves the cache. The cache therefore holds only
//! values some client variable still references: its size is bounded by the
//! program, not the data structure.
//!
//! When another alias is dereferenced while the value is cached, its walk
//! reaches the cached root's address and stops there without reading it
//! ([`CachedDeref::Hit`]): the alias's own path below the root is still
//! rebuilt, and the cached value is shared.
//!
//! Pointers stay real SAM⁺ pointers: [`CachedPointer`] wraps the backend's
//! raw pointer inline, so pointers stored inside values (e.g. graph edges)
//! are persisted as ordinary addresses and the client keeps no per-pointer
//! table. Deleting a pointer may need to walk through a live cached root, so
//! deletions are deferred until the cache is empty (as in the Python
//! implementation).

use super::{
    CacheablePointerBackend, CachedDeref, CachedRoots, RawValueCells, SmartPointerBackend,
};
use crate::{Address, SamError, SingleAccessMachine};
use std::collections::HashMap;

/// A smart pointer usable through [`CachedPointers`].
///
/// It is the backend's raw pointer, held inline so it can be stored inside
/// values and persisted as an ordinary SAM⁺ pointer.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CachedPointer<P> {
    raw: Option<P>,
    temp_copy: bool,
}

impl<P> CachedPointer<P> {
    /// Wraps a raw backend pointer (e.g. one decoded from a stored value).
    pub fn from_raw(raw: P) -> Self {
        Self {
            raw: Some(raw),
            temp_copy: false,
        }
    }

    /// The raw backend pointer, or `None` once deleted.
    pub fn raw(&self) -> Option<&P> {
        self.raw.as_ref()
    }

    /// Returns whether this pointer has been deleted.
    pub fn is_deleted(&self) -> bool {
        self.raw.is_none()
    }

    /// Marks this alias as temporary.
    pub fn set_temp_copy(&mut self, temp_copy: bool) {
        self.temp_copy = temp_copy;
    }

    /// Returns whether this alias is temporary.
    pub fn is_temp_copy(&self) -> bool {
        self.temp_copy
    }

    fn raw_mut(&mut self) -> Result<&mut P, SamError> {
        self.raw
            .as_mut()
            .ok_or(SamError::InvalidPointerCell("deleted cached pointer"))
    }
}

/// One explicit reference to a live cached value, identified by its
/// write-back root address.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CacheObject {
    root: Address,
    released: bool,
}

impl CacheObject {
    /// Returns whether this handle has already been released.
    pub fn is_released(self) -> bool {
        self.released
    }

    /// The write-back root address this value is cached under.
    pub fn root(self) -> Address {
        self.root
    }
}

#[derive(Clone, Debug)]
struct CacheEntry<V, W> {
    /// `None` while the value is lent to a scoped access (`with_value` or a
    /// pointer-field operation); the entry itself stays in the cache so that
    /// aliases reaching its root still hit.
    value: Option<V>,
    writeback: W,
    references: usize,
}

/// [`CachedRoots`] view of the live cache entries.
struct LiveRoots<'a, V, W>(&'a mut HashMap<Address, CacheEntry<V, W>>);

impl<V, W> CachedRoots<W> for LiveRoots<'_, V, W> {
    fn is_cached(&self, root: Address) -> bool {
        self.0.contains_key(&root)
    }

    fn writeback_mut(&mut self, root: Address) -> Option<&mut W> {
        self.0.get_mut(&root).map(|entry| &mut entry.writeback)
    }
}

/// Client-side cache over a [`CacheablePointerBackend`].
pub struct CachedPointers<V, B>
where
    V: Clone,
    B: CacheablePointerBackend<V>,
{
    backend: B,
    /// Live values keyed by write-back root address.
    entries: HashMap<Address, CacheEntry<V, B::Writeback>>,
    /// Raw pointers deleted while values were cached, deleted once the cache
    /// empties (a deletion walk may otherwise need to read a cached root).
    deferred_deletes: Vec<B::Pointer>,
    max_cached_values: usize,
}

impl<V, B> CachedPointers<V, B>
where
    V: Clone,
    B: CacheablePointerBackend<V>,
{
    /// Creates an empty cache over `backend`.
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            entries: HashMap::new(),
            deferred_deletes: Vec::new(),
            max_cached_values: 0,
        }
    }

    /// Returns the underlying backend configuration.
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Returns mutable access to the underlying backend configuration.
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// Peak number of simultaneously cached values since the last reset.
    pub fn max_cached_values(&self) -> usize {
        self.max_cached_values
    }

    /// Restarts peak-occupancy tracking.
    pub fn reset_max_cached_values(&mut self) {
        self.max_cached_values = self.entries.len();
    }

    /// Current number of live cached values.
    pub fn cached_values(&self) -> usize {
        self.entries.len()
    }

    /// Deletions waiting for the cache to empty.
    pub fn deferred_deletes(&self) -> usize {
        self.deferred_deletes.len()
    }

    /// Allocates a new pointee.
    pub fn new_pointer<S: SingleAccessMachine<B::Cell>>(
        &mut self,
        sam: &mut S,
        value: V,
    ) -> Result<CachedPointer<B::Pointer>, SamError> {
        Ok(CachedPointer::from_raw(
            self.backend.new_pointer(sam, value)?,
        ))
    }

    /// Smart-copies a pointer (a new raw alias of the same pointee).
    pub fn copy_pointer<S: SingleAccessMachine<B::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut CachedPointer<B::Pointer>,
        temp_copy: bool,
    ) -> Result<CachedPointer<B::Pointer>, SamError> {
        Ok(self.copy_many(sam, pointer, 1, temp_copy)?.remove(0))
    }

    /// Smart-copies a pointer `num_copies` times using the backend's bulk path.
    pub fn copy_many<S: SingleAccessMachine<B::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut CachedPointer<B::Pointer>,
        num_copies: usize,
        temp_copy: bool,
    ) -> Result<Vec<CachedPointer<B::Pointer>>, SamError> {
        let raw = pointer.raw_mut()?;
        let copies =
            self.backend
                .copy_cached(sam, raw, num_copies, &mut LiveRoots(&mut self.entries))?;
        Ok(copies
            .into_iter()
            .map(|raw| CachedPointer {
                raw: Some(raw),
                temp_copy,
            })
            .collect())
    }

    /// Dereferences a pointer, caching its value (or joining the live entry
    /// its path leads to), and returns a handle to it.
    pub fn get<S: SingleAccessMachine<B::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut CachedPointer<B::Pointer>,
    ) -> Result<Option<CacheObject>, SamError> {
        let raw = pointer.raw_mut()?;
        let outcome = self
            .backend
            .deref_cached(sam, raw, &mut LiveRoots(&mut self.entries))?;
        let root = match outcome {
            None => return Ok(None),
            Some(CachedDeref::Hit { root }) => {
                self.entries
                    .get_mut(&root)
                    .ok_or(SamError::InvalidPointerCell("cache hit on a missing root"))?
                    .references += 1;
                root
            }
            Some(CachedDeref::Miss {
                root,
                value,
                writeback,
            }) => {
                let previous = self.entries.insert(
                    root,
                    CacheEntry {
                        value: Some(value),
                        writeback,
                        references: 1,
                    },
                );
                if previous.is_some() {
                    return Err(SamError::InvalidPointerCell(
                        "cache miss staged an already-cached root",
                    ));
                }
                self.max_cached_values = self.max_cached_values.max(self.entries.len());
                root
            }
        };
        Ok(Some(CacheObject {
            root,
            released: false,
        }))
    }

    /// Adds another reference to a live cached value.
    pub fn clone_object(&mut self, object: CacheObject) -> Result<CacheObject, SamError> {
        self.live_entry_mut(&object)?.references += 1;
        Ok(CacheObject {
            root: object.root,
            released: false,
        })
    }

    fn live_entry_mut(
        &mut self,
        object: &CacheObject,
    ) -> Result<&mut CacheEntry<V, B::Writeback>, SamError> {
        if object.released {
            return Err(SamError::InvalidPointerCell("cache object was released"));
        }
        self.entries
            .get_mut(&object.root)
            .ok_or(SamError::InvalidPointerCell("cache object is not live"))
    }

    /// Borrows the cached value immutably.
    pub fn value(&self, object: &CacheObject) -> Result<&V, SamError> {
        if object.released {
            return Err(SamError::InvalidPointerCell("cache object was released"));
        }
        self.entries
            .get(&object.root)
            .ok_or(SamError::InvalidPointerCell("cache object is not live"))?
            .value
            .as_ref()
            .ok_or(SamError::InvalidPointerCell(
                "cached value is in use by an enclosing access",
            ))
    }

    /// Borrows the cached value mutably.
    pub fn value_mut(&mut self, object: &CacheObject) -> Result<&mut V, SamError> {
        self.live_entry_mut(object)?
            .value
            .as_mut()
            .ok_or(SamError::InvalidPointerCell(
                "cached value is in use by an enclosing access",
            ))
    }

    /// Replaces the live cached value.
    pub fn replace(&mut self, object: &CacheObject, value: V) -> Result<(), SamError> {
        *self.value_mut(object)? = value;
        Ok(())
    }

    /// Lends the value of a live entry to `operation`, keeping the entry (and
    /// so its root) in the cache meanwhile.
    fn with_lent_value<R>(
        &mut self,
        object: &CacheObject,
        operation: impl FnOnce(&mut Self, &mut V) -> R,
    ) -> Result<R, SamError> {
        let mut value =
            self.live_entry_mut(object)?
                .value
                .take()
                .ok_or(SamError::InvalidPointerCell(
                    "cached value is in use by an enclosing access",
                ))?;
        let result = operation(self, &mut value);
        // The entry cannot have been released: `object` still references it.
        self.live_entry_mut(object)?.value = Some(value);
        Ok(result)
    }

    /// Dereferences a pointer field of a cached value (Python `CachePtr.deref`).
    /// The owner stays cached while the child is acquired.
    pub fn get_pointer_field<S, F>(
        &mut self,
        sam: &mut S,
        owner: &CacheObject,
        select: F,
    ) -> Result<Option<CacheObject>, SamError>
    where
        S: SingleAccessMachine<B::Cell>,
        F: for<'a> FnOnce(&'a mut V) -> Option<&'a mut CachedPointer<B::Pointer>>,
    {
        self.with_lent_value(owner, |cache, value| match select(value) {
            Some(pointer) => cache.get(sam, pointer),
            None => Ok(None),
        })?
    }

    /// Smart-copies a pointer field of a cached value.
    pub fn copy_pointer_field<S, F>(
        &mut self,
        sam: &mut S,
        owner: &CacheObject,
        select: F,
        temp_copy: bool,
    ) -> Result<Option<CachedPointer<B::Pointer>>, SamError>
    where
        S: SingleAccessMachine<B::Cell>,
        F: for<'a> FnOnce(&'a mut V) -> Option<&'a mut CachedPointer<B::Pointer>>,
    {
        self.with_lent_value(owner, |cache, value| match select(value) {
            Some(pointer) => cache.copy_pointer(sam, pointer, temp_copy).map(Some),
            None => Ok(None),
        })?
    }

    /// Deletes and clears a pointer field of a cached value.
    pub fn delete_pointer_field<S, F>(
        &mut self,
        sam: &mut S,
        owner: &CacheObject,
        select: F,
    ) -> Result<(), SamError>
    where
        S: SingleAccessMachine<B::Cell>,
        F: for<'a> FnOnce(&'a mut V) -> &'a mut Option<CachedPointer<B::Pointer>>,
    {
        self.with_lent_value(owner, |cache, value| match select(value).take() {
            Some(mut pointer) => cache.delete_pointer(sam, &mut pointer),
            None => Ok(()),
        })?
    }

    /// Releases one reference; the last release writes the value back to its
    /// root and evicts it from the cache.
    pub fn release<S: SingleAccessMachine<B::Cell>>(
        &mut self,
        sam: &mut S,
        object: &mut CacheObject,
    ) -> Result<(), SamError> {
        if object.released {
            return Ok(());
        }
        let entry = self.live_entry_mut(object)?;
        entry.references = entry
            .references
            .checked_sub(1)
            .ok_or(SamError::InvalidPointerCell(
                "negative cache reference count",
            ))?;
        object.released = true;
        if entry.references != 0 {
            return Ok(());
        }
        let entry = self.entries.remove(&object.root).unwrap();
        let value = entry.value.ok_or(SamError::InvalidPointerCell(
            "cached value released while lent to an enclosing access",
        ))?;
        self.backend.finish_cached(sam, entry.writeback, value)?;
        self.flush_deferred_deletes(sam)
    }

    /// Runs a scoped access to the pointee and always releases its handle.
    pub fn with_value<S, R, F>(
        &mut self,
        sam: &mut S,
        pointer: &mut CachedPointer<B::Pointer>,
        operation: F,
    ) -> Result<Option<R>, SamError>
    where
        S: SingleAccessMachine<B::Cell>,
        F: FnOnce(&mut V) -> R,
    {
        let Some(mut object) = self.get(sam, pointer)? else {
            return Ok(None);
        };
        let result = self.value_mut(&object).map(operation);
        let release = self.release(sam, &mut object);
        let result = result?;
        release?;
        Ok(Some(result))
    }

    /// Replaces the pointee through the cache.
    pub fn put<S: SingleAccessMachine<B::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut CachedPointer<B::Pointer>,
        value: V,
    ) -> Result<(), SamError> {
        let mut object = self.get(sam, pointer)?.ok_or(SamError::InvalidPointerCell(
            "cannot put through an empty pointer",
        ))?;
        let replaced = self.replace(&object, value);
        let release = self.release(sam, &mut object);
        replaced?;
        release
    }

    /// Returns whether this is the only alias. The backends answer this with
    /// their own traversal, so it is only available while nothing is cached.
    pub fn is_single_reference<S: SingleAccessMachine<B::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut CachedPointer<B::Pointer>,
    ) -> Result<bool, SamError> {
        if !self.entries.is_empty() {
            return Err(SamError::InvalidParameter(
                "is_single_reference is unavailable while cached values are live",
            ));
        }
        self.backend.is_single_reference(sam, pointer.raw_mut()?)
    }

    /// Deletes a pointer, deferring the backend deletion while values are
    /// cached.
    pub fn delete_pointer<S: SingleAccessMachine<B::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut CachedPointer<B::Pointer>,
    ) -> Result<(), SamError> {
        let Some(mut raw) = pointer.raw.take() else {
            return Ok(());
        };
        if self.entries.is_empty() {
            self.backend.delete(sam, &mut raw)
        } else {
            self.deferred_deletes.push(raw);
            Ok(())
        }
    }

    /// Deletes a pointer only if it is a temporary copy.
    pub fn delete_temp_copy<S: SingleAccessMachine<B::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut CachedPointer<B::Pointer>,
    ) -> Result<(), SamError> {
        if pointer.temp_copy {
            self.delete_pointer(sam, pointer)?;
        }
        Ok(())
    }

    fn flush_deferred_deletes<S: SingleAccessMachine<B::Cell>>(
        &mut self,
        sam: &mut S,
    ) -> Result<(), SamError> {
        if !self.entries.is_empty() {
            return Ok(());
        }
        for mut raw in std::mem::take(&mut self.deferred_deletes) {
            self.backend.delete(sam, &mut raw)?;
        }
        Ok(())
    }

    /// Ends a cache phase: errors if any value is still referenced.
    pub fn clear(&mut self) -> Result<(), SamError> {
        if !self.entries.is_empty() {
            return Err(SamError::InvalidPointerCell(
                "cannot clear cache while live cache handles remain",
            ));
        }
        debug_assert!(self.deferred_deletes.is_empty());
        Ok(())
    }
}

impl<V, B> SmartPointerBackend<V> for CachedPointers<V, B>
where
    V: Clone,
    B: CacheablePointerBackend<V>,
{
    type Pointer = CachedPointer<B::Pointer>;
    type Cell = B::Cell;

    fn label(&self) -> &'static str {
        self.backend.label()
    }

    fn clear_cache(&mut self) -> Result<usize, SamError> {
        // Entries exist only while referenced, so an idle cache is already
        // empty; this checks that and restarts the per-phase peak.
        let dropped = self.entries.len();
        self.clear()?;
        self.reset_max_cached_values();
        Ok(dropped)
    }

    fn max_cached_values(&self) -> Option<usize> {
        Some(self.max_cached_values)
    }

    fn new_pointer<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        value: V,
    ) -> Result<Self::Pointer, SamError> {
        CachedPointers::new_pointer(self, sam, value)
    }

    fn copy_pointer<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<Self::Pointer, SamError> {
        CachedPointers::copy_pointer(self, sam, pointer, false)
    }

    fn copy_many<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        num_copies: usize,
    ) -> Result<Vec<Self::Pointer>, SamError> {
        CachedPointers::copy_many(self, sam, pointer, num_copies, false)
    }

    fn get<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<Option<V>, SamError> {
        CachedPointers::with_value(self, sam, pointer, |value| value.clone())
    }

    fn put<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        value: V,
    ) -> Result<(), SamError> {
        CachedPointers::put(self, sam, pointer, value)
    }

    fn delete<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<(), SamError> {
        self.delete_pointer(sam, pointer)
    }

    fn is_single_reference<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<bool, SamError> {
        CachedPointers::is_single_reference(self, sam, pointer)
    }

    fn with_value<S, R, F>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        operation: F,
    ) -> Result<R, SamError>
    where
        S: SingleAccessMachine<Self::Cell>,
        F: FnOnce(&mut V, &mut Self, &mut S) -> Result<R, SamError>,
    {
        let mut object = CachedPointers::get(self, sam, pointer)?
            .ok_or(SamError::InvalidPointerCell("deleted cached pointer"))?;
        let result = self.with_lent_value(&object, |cache, value| operation(value, cache, sam));
        let release = self.release(sam, &mut object);
        let result = result?;
        release?;
        result
    }
}

/// Structure cells (AVL nodes, queue and stack entries) bypass the cache:
/// they are written to and read from the SAM directly, in the wrapped
/// backend's cell format.
impl<V, B> RawValueCells<V> for CachedPointers<V, B>
where
    V: Clone,
    B: CacheablePointerBackend<V> + RawValueCells<V>,
{
    fn raw_cell(value: V) -> Self::Cell {
        B::raw_cell(value)
    }

    fn raw_value(cell: Self::Cell) -> Result<V, SamError> {
        B::raw_value(cell)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        pointer::{
            MultiWriteCell, MultiWritePointers, OriginalCell, OriginalPointers, RaryCell,
            RaryPointers, RecursivePointers,
        },
        AccessPolicy, DryRunSam,
    };
    use rand::{rngs::StdRng, Rng, SeedableRng};

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct Record {
        value: u64,
    }

    fn reads<S: SingleAccessMachine<C>, C: Clone>(sam: &S) -> u64 {
        sam.stats().operations.reads
    }

    #[test]
    fn aliases_share_one_entry_and_a_hit_skips_only_the_root() {
        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut cache = CachedPointers::new(MultiWritePointers);
        let mut pointer = cache.new_pointer(&mut sam, Record { value: 1 }).unwrap();
        let mut alias = cache.copy_pointer(&mut sam, &mut pointer, false).unwrap();

        let before = reads(&sam);
        let mut first = cache.get(&mut sam, &mut pointer).unwrap().unwrap();
        let miss_reads = reads(&sam) - before;
        let before = reads(&sam);
        let mut second = cache.get(&mut sam, &mut alias).unwrap().unwrap();
        let hit_reads = reads(&sam) - before;
        // The miss reads the leaf and the root; the hit reads only the
        // alias's own leaf and stops at the cached (unwritten) root.
        assert_eq!((miss_reads, hit_reads), (2, 1));
        assert_eq!(first.root(), second.root());
        assert_eq!(cache.cached_values(), 1);

        cache.value_mut(&first).unwrap().value = 9;
        assert_eq!(cache.value(&second).unwrap().value, 9);
        cache.release(&mut sam, &mut first).unwrap();
        assert_eq!(cache.cached_values(), 1);
        let writes = sam.stats().operations.writes;
        cache.release(&mut sam, &mut second).unwrap();
        assert_eq!(cache.cached_values(), 0);
        assert_eq!(sam.stats().operations.writes, writes + 1);

        let mut loaded = cache.get(&mut sam, &mut alias).unwrap().unwrap();
        assert_eq!(cache.value(&loaded).unwrap().value, 9);
        cache.release(&mut sam, &mut loaded).unwrap();
    }

    #[test]
    fn deletion_is_deferred_until_the_cache_is_empty() {
        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut cache = CachedPointers::new(MultiWritePointers);
        let mut pointer = cache.new_pointer(&mut sam, Record { value: 1 }).unwrap();
        let mut alias = cache.copy_pointer(&mut sam, &mut pointer, false).unwrap();
        let mut object = cache.get(&mut sam, &mut pointer).unwrap().unwrap();
        cache.value_mut(&object).unwrap().value = 7;
        cache.delete_pointer(&mut sam, &mut alias).unwrap();
        assert!(alias.is_deleted());
        assert_eq!(cache.deferred_deletes(), 1);
        cache.release(&mut sam, &mut object).unwrap();
        assert_eq!(cache.deferred_deletes(), 0);
        assert!(cache.is_single_reference(&mut sam, &mut pointer).unwrap());
        let mut loaded = cache.get(&mut sam, &mut pointer).unwrap().unwrap();
        assert_eq!(cache.value(&loaded).unwrap().value, 7);
        cache.release(&mut sam, &mut loaded).unwrap();
    }

    #[test]
    fn clear_rejects_live_handles_and_tracks_peak_occupancy() {
        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut cache = CachedPointers::new(MultiWritePointers);
        let mut first = cache.new_pointer(&mut sam, Record { value: 1 }).unwrap();
        let mut second = cache.new_pointer(&mut sam, Record { value: 2 }).unwrap();
        let mut first_object = cache.get(&mut sam, &mut first).unwrap().unwrap();
        let mut second_object = cache.get(&mut sam, &mut second).unwrap().unwrap();
        assert_eq!(cache.max_cached_values(), 2);
        assert!(cache.clear().is_err());
        cache.release(&mut sam, &mut first_object).unwrap();
        cache.release(&mut sam, &mut second_object).unwrap();
        assert_eq!(SmartPointerBackend::clear_cache(&mut cache).unwrap(), 0);
        assert_eq!(cache.max_cached_values(), 0);
    }

    fn exercise_basic<B, S>(cache: &mut CachedPointers<Record, B>, sam: &mut S)
    where
        B: CacheablePointerBackend<Record>,
        S: SingleAccessMachine<B::Cell>,
    {
        let mut pointer = cache.new_pointer(sam, Record { value: 1 }).unwrap();
        cache
            .with_value(sam, &mut pointer, |value| value.value = 2)
            .unwrap();
        let mut object = cache.get(sam, &mut pointer).unwrap().unwrap();
        assert_eq!(cache.value(&object).unwrap().value, 2);
        cache.release(sam, &mut object).unwrap();
    }

    fn exercise_copy_while_live<B, S>(cache: &mut CachedPointers<Record, B>, sam: &mut S)
    where
        B: CacheablePointerBackend<Record>,
        S: SingleAccessMachine<B::Cell>,
    {
        // The dereferenced sole pointer's head is (or leads straight to) the
        // cached root, the case copy_cached must handle without reading it.
        let mut pointer = cache.new_pointer(sam, Record { value: 1 }).unwrap();
        let mut first = cache.get(sam, &mut pointer).unwrap().unwrap();
        let mut alias = cache.copy_pointer(sam, &mut pointer, false).unwrap();
        let mut more = cache.copy_many(sam, &mut alias, 3, false).unwrap();
        let mut second = cache.get(sam, &mut alias).unwrap().unwrap();
        let mut third = cache.get(sam, &mut more[1]).unwrap().unwrap();
        assert_eq!(first.root(), second.root());
        assert_eq!(first.root(), third.root());
        cache.value_mut(&first).unwrap().value = 13;
        cache.release(sam, &mut second).unwrap();
        cache.release(sam, &mut third).unwrap();
        cache.release(sam, &mut first).unwrap();
        let [first_copy, _, third_copy] = &mut more[..] else {
            panic!("expected three copies");
        };
        for handle in [&mut pointer, &mut alias, first_copy, third_copy] {
            let mut loaded = cache.get(sam, handle).unwrap().unwrap();
            assert_eq!(cache.value(&loaded).unwrap().value, 13);
            cache.release(sam, &mut loaded).unwrap();
        }
    }

    #[test]
    fn every_backend_supports_scoped_access_and_copy_while_live() {
        exercise_basic(
            &mut CachedPointers::new(OriginalPointers),
            &mut DryRunSam::<OriginalCell<Record>>::new(AccessPolicy::SINGLE_WRITE),
        );
        exercise_basic(
            &mut CachedPointers::new(RaryPointers::new(4).unwrap()),
            &mut DryRunSam::<RaryCell<Record>>::new(AccessPolicy::MULTI_WRITE),
        );
        exercise_basic(
            &mut CachedPointers::new(RecursivePointers::default()),
            &mut DryRunSam::new(AccessPolicy::RECURSIVE),
        );
        exercise_copy_while_live(
            &mut CachedPointers::new(MultiWritePointers),
            &mut DryRunSam::new(AccessPolicy::MULTI_WRITE),
        );
        exercise_copy_while_live(
            &mut CachedPointers::new(OriginalPointers),
            &mut DryRunSam::new(AccessPolicy::SINGLE_WRITE),
        );
        for fanout in [2, 4, 6] {
            exercise_copy_while_live(
                &mut CachedPointers::new(RaryPointers::new(fanout).unwrap()),
                &mut DryRunSam::new(AccessPolicy::MULTI_WRITE),
            );
        }
    }

    #[test]
    fn pointer_fields_stay_real_pointers_and_share_child_entries() {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        struct Node {
            value: u64,
            child: Option<CachedPointer<crate::pointer::MultiWritePointer>>,
        }

        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut cache = CachedPointers::new(MultiWritePointers);
        let mut child = cache
            .new_pointer(
                &mut sam,
                Node {
                    value: 9,
                    child: None,
                },
            )
            .unwrap();
        let child_field = cache.copy_pointer(&mut sam, &mut child, false).unwrap();
        let mut parent = cache
            .new_pointer(
                &mut sam,
                Node {
                    value: 1,
                    child: Some(child_field),
                },
            )
            .unwrap();
        let mut parent_object = cache.get(&mut sam, &mut parent).unwrap().unwrap();
        let mut child_object = cache
            .get_pointer_field(&mut sam, &parent_object, |node| node.child.as_mut())
            .unwrap()
            .unwrap();
        // The child is also reachable through the client-held alias: a hit.
        let mut via_alias = cache.get(&mut sam, &mut child).unwrap().unwrap();
        assert_eq!(child_object.root(), via_alias.root());
        cache.value_mut(&child_object).unwrap().value = 11;
        cache.release(&mut sam, &mut via_alias).unwrap();
        cache.release(&mut sam, &mut child_object).unwrap();
        // The field's refreshed raw head lives in the parent value itself,
        // written back with the parent: no client-side pointer table.
        let stored_head = cache.value(&parent_object).unwrap().child.unwrap();
        cache.release(&mut sam, &mut parent_object).unwrap();
        assert!(stored_head.raw().unwrap().head().is_some());

        let mut loaded = cache.get(&mut sam, &mut child).unwrap().unwrap();
        assert_eq!(cache.value(&loaded).unwrap().value, 11);
        cache.release(&mut sam, &mut loaded).unwrap();
        let mut reloaded_parent = cache.get(&mut sam, &mut parent).unwrap().unwrap();
        let mut through_field = cache
            .get_pointer_field(&mut sam, &reloaded_parent, |node| node.child.as_mut())
            .unwrap()
            .unwrap();
        assert_eq!(cache.value(&through_field).unwrap().value, 11);
        cache.release(&mut sam, &mut through_field).unwrap();
        cache.release(&mut sam, &mut reloaded_parent).unwrap();
    }

    #[test]
    fn scoped_access_keeps_the_root_cached_for_nested_aliases() {
        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut cache = CachedPointers::new(RaryPointers::new(4).unwrap());
        let mut pointer = cache.new_pointer(&mut sam, Record { value: 5 }).unwrap();
        let mut alias = cache.copy_pointer(&mut sam, &mut pointer, false).unwrap();
        let nested = SmartPointerBackend::with_value(
            &mut cache,
            &mut sam,
            &mut pointer,
            |value, cache, sam| {
                value.value = 6;
                // The alias reaches the live root; its value is lent out, so a
                // nested access to the same value is rejected, not duplicated.
                let mut hit = cache.get(sam, &mut alias)?.unwrap();
                let nested = cache.value(&hit).is_err();
                cache.release(sam, &mut hit)?;
                Ok(nested)
            },
        )
        .unwrap();
        assert!(nested);
        assert_eq!(cache.cached_values(), 0);
        assert_eq!(
            SmartPointerBackend::get(&mut cache, &mut sam, &mut alias).unwrap(),
            Some(Record { value: 6 })
        );
    }

    /// Random nested acquisitions, copies and deletions across aliases of a
    /// few objects, checked against a model of each object's value.
    fn stress<B, S>(mut cache: CachedPointers<Record, B>, mut sam: S, seed: u64)
    where
        B: CacheablePointerBackend<Record>,
        S: SingleAccessMachine<B::Cell>,
    {
        let mut rng = StdRng::seed_from_u64(seed);
        let objects = 4;
        let mut model: Vec<u64> = (0..objects).map(|object| object as u64).collect();
        let mut aliases: Vec<(usize, CachedPointer<B::Pointer>)> = Vec::new();
        for (object, value) in model.iter().enumerate() {
            let mut pointer = cache
                .new_pointer(&mut sam, Record { value: *value })
                .unwrap();
            let copies = cache.copy_many(&mut sam, &mut pointer, 2, false).unwrap();
            aliases.push((object, pointer));
            aliases.extend(copies.into_iter().map(|copy| (object, copy)));
        }
        let mut live: Vec<(usize, CacheObject)> = Vec::new();
        for _ in 0..3_000 {
            match rng.gen_range(0..10) {
                0..=3 if live.len() < 6 => {
                    let index = rng.gen_range(0..aliases.len());
                    let (object, pointer) = &mut aliases[index];
                    let handle = cache.get(&mut sam, pointer).unwrap().unwrap();
                    assert_eq!(cache.value(&handle).unwrap().value, model[*object]);
                    live.push((*object, handle));
                }
                4 if !live.is_empty() => {
                    let (object, handle) = live[rng.gen_range(0..live.len())];
                    model[object] = rng.gen();
                    cache.value_mut(&handle).unwrap().value = model[object];
                }
                5 => {
                    let index = rng.gen_range(0..aliases.len());
                    let count = rng.gen_range(1..3);
                    let object = aliases[index].0;
                    let copies = cache
                        .copy_many(&mut sam, &mut aliases[index].1, count, false)
                        .unwrap();
                    aliases.extend(copies.into_iter().map(|copy| (object, copy)));
                }
                6 => {
                    // Keep at least two aliases of every object.
                    let index = rng.gen_range(0..aliases.len());
                    let object = aliases[index].0;
                    if aliases.iter().filter(|(owner, _)| *owner == object).count() > 2 {
                        let (_, mut pointer) = aliases.swap_remove(index);
                        cache.delete_pointer(&mut sam, &mut pointer).unwrap();
                    }
                }
                _ if !live.is_empty() => {
                    let (_, mut handle) = live.swap_remove(rng.gen_range(0..live.len()));
                    cache.release(&mut sam, &mut handle).unwrap();
                }
                _ => {}
            }
            assert!(cache.cached_values() <= objects);
        }
        for (_, mut handle) in live.drain(..) {
            cache.release(&mut sam, &mut handle).unwrap();
        }
        assert_eq!(cache.cached_values(), 0);
        assert_eq!(cache.deferred_deletes(), 0);
        for (object, pointer) in &mut aliases {
            assert_eq!(
                SmartPointerBackend::get(&mut cache, &mut sam, pointer).unwrap(),
                Some(Record {
                    value: model[*object]
                })
            );
        }
    }

    #[test]
    fn randomized_nested_cache_use_matches_a_model() {
        for seed in 0..5 {
            stress(
                CachedPointers::new(MultiWritePointers),
                DryRunSam::<MultiWriteCell<Record>>::new(AccessPolicy::MULTI_WRITE),
                seed,
            );
            stress(
                CachedPointers::new(OriginalPointers),
                DryRunSam::<OriginalCell<Record>>::new(AccessPolicy::SINGLE_WRITE),
                seed,
            );
            for fanout in [2, 4, 6] {
                stress(
                    CachedPointers::new(RaryPointers::new(fanout).unwrap()),
                    DryRunSam::<RaryCell<Record>>::new(AccessPolicy::MULTI_WRITE),
                    seed,
                );
            }
            stress(
                CachedPointers::new(RecursivePointers::default()),
                DryRunSam::new(AccessPolicy::RECURSIVE),
                seed,
            );
        }
    }
}
