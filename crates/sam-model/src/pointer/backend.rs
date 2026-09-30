use super::{
    MultiWriteCell, MultiWritePointer, OriginalCell, OriginalPointer, OriginalWriteback, RaryCell,
    RaryPointer, RecursivePointer, RecursivePointers,
};
use crate::{AccessPolicy, AccessStrategy, Address, SamError, SingleAccessMachine};

/// User-facing pointer selection used by benchmark command-line parsing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerKind {
    /// Original queue-based single-read/single-write pointer.
    Original,
    /// Binary single-read/multi-write pointer.
    MultiWrite,
    /// R-ary single-read/multi-write pointer.
    MultiWriteRary { branching_factor: usize },
    /// Reference-counted multi-read/multi-write pointer.
    Recursive,
    /// Complete b-ary multi-write pointer tree with downward pointers
    /// (`pointer::BalancedPointers`). With `buffered_writes`, writes are
    /// buffered and flushed in public batches and only reads are charged, as
    /// for the r-ary pointer; otherwise every write is a round trip.
    Balanced {
        branching_factor: usize,
        buffered_writes: bool,
    },
}

impl PointerKind {
    /// Parses the labels accepted by the Python benchmark.
    pub fn from_label(label: &str, branching_factor: usize) -> Result<Self, SamError> {
        match label {
            "original" => Ok(Self::Original),
            "multiwrite" => Ok(Self::MultiWrite),
            "multiwriterary" | "multiwrite_rary" => {
                RaryPointers::new(branching_factor)?;
                Ok(Self::MultiWriteRary { branching_factor })
            }
            "recursive" => Ok(Self::Recursive),
            "balanced" | "balancedrary" => {
                super::BalancedPointers::new(branching_factor)?;
                Ok(Self::Balanced {
                    branching_factor,
                    buffered_writes: label == "balancedrary",
                })
            }
            _ => Err(SamError::InvalidParameter("unknown smart-pointer label")),
        }
    }

    /// SAM access limits required by this pointer type.
    pub fn access_policy(self) -> AccessPolicy {
        match self {
            Self::Original => AccessPolicy::SINGLE_WRITE,
            Self::MultiWrite | Self::MultiWriteRary { .. } | Self::Balanced { .. } => {
                AccessPolicy::MULTI_WRITE
            }
            Self::Recursive => AccessPolicy::RECURSIVE,
        }
    }

    /// Cryptographic access strategy used after the dry-run handoff.
    pub fn access_strategy(self) -> AccessStrategy {
        match self {
            Self::MultiWriteRary { .. }
            | Self::Balanced {
                buffered_writes: true,
                ..
            } => AccessStrategy::MULTI_WRITE_RARY,
            _ => AccessStrategy::STANDARD,
        }
    }
}

/// Static metadata shared by all pointer handle types.
pub trait PointerHandle {
    /// Python-compatible backend label.
    const LABEL: &'static str;
}

impl PointerHandle for OriginalPointer {
    const LABEL: &'static str = "original";
}

impl PointerHandle for MultiWritePointer {
    const LABEL: &'static str = "multiwrite";
}

impl PointerHandle for RaryPointer {
    const LABEL: &'static str = "multiwriterary";
}

impl PointerHandle for RecursivePointer {
    const LABEL: &'static str = "recursive";
}

impl PointerHandle for super::BalancedPointer {
    const LABEL: &'static str = "balanced";
}

/// Backend-neutral operations used by native graph data structures.
///
/// The trait is intentionally generic rather than object-safe: each backend
/// has a different SAM cell format, while graph algorithms can still be
/// written once and monomorphized for the selected pointer implementation.
pub trait SmartPointerBackend<V: Clone> {
    /// Client-side pointer handle.
    type Pointer;
    /// Value stored at each SAM address for this backend.
    type Cell: Clone;

    /// Stable name matching the Python command-line labels.
    fn label(&self) -> &'static str;

    /// Peak client-cache occupancy when a cache layer is active.
    fn max_cached_values(&self) -> Option<usize> {
        None
    }

    /// Empties the client-side cache at a phase boundary (e.g. between
    /// benchmark algorithms) and restarts its peak-occupancy counter.
    /// Returns how many cached values were dropped; errors if any cache
    /// handle is still live. Backends without a cache do nothing.
    fn clear_cache(&mut self) -> Result<usize, SamError> {
        Ok(0)
    }

    /// Allocates a pointer to a new value.
    fn new_pointer<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        value: V,
    ) -> Result<Self::Pointer, SamError>;

    /// Creates one alias, refreshing the original when required.
    fn copy_pointer<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<Self::Pointer, SamError>;

    /// Creates explicit multiple aliases. Scalar `copy` remains scalar.
    fn copy_many<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        num_copies: usize,
    ) -> Result<Vec<Self::Pointer>, SamError> {
        if num_copies == 0 {
            return Err(SamError::InvalidParameter("num_copies must be positive"));
        }
        (0..num_copies)
            .map(|_| self.copy_pointer(sam, pointer))
            .collect()
    }

    /// Reads the shared value.
    fn get<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<Option<V>, SamError>;

    /// Replaces the shared value.
    fn put<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        value: V,
    ) -> Result<(), SamError>;

    /// Removes one alias.
    fn delete<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<(), SamError>;

    /// Returns whether this is the only alias.
    fn is_single_reference<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<bool, SamError>;

    /// Operates on the live pointee while exposing backend state and the SAM.
    fn with_value<S, R, F>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        operation: F,
    ) -> Result<R, SamError>
    where
        S: SingleAccessMachine<Self::Cell>,
        F: FnOnce(&mut V, &mut Self, &mut S) -> Result<R, SamError>;

    /// [`Self::with_value`] for an operation that leaves the stored value's
    /// representation unchanged. Backends whose addresses can be read more
    /// than once (the recursive ORAM) skip the write-back; single-read
    /// backends must write back anyway (the read consumed the address).
    fn with_value_read<S, R, F>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        operation: F,
    ) -> Result<R, SamError>
    where
        S: SingleAccessMachine<Self::Cell>,
        F: FnOnce(&mut V, &mut Self, &mut S) -> Result<R, SamError>,
    {
        self.with_value(sam, pointer, operation)
    }
}

/// Backends whose SAM cells can also hold raw values written directly by an
/// oblivious data structure (queue, stack, AVL tree). Python keeps one global
/// SAM for pointers and those structures; this lets them share one here too.
pub trait RawValueCells<V: Clone>: SmartPointerBackend<V> {
    /// Wraps a raw value as this backend's SAM cell.
    fn raw_cell(value: V) -> Self::Cell;

    /// Unwraps a raw value read back from SAM.
    fn raw_value(cell: Self::Cell) -> Result<V, SamError>;
}

impl<V: Clone> RawValueCells<V> for MultiWritePointers {
    fn raw_cell(value: V) -> Self::Cell {
        MultiWriteCell::Root(value)
    }

    fn raw_value(cell: Self::Cell) -> Result<V, SamError> {
        match cell {
            MultiWriteCell::Root(value) => Ok(value),
            MultiWriteCell::Inner { .. } => Err(SamError::InvalidPointerCell(
                "expected a raw structure cell, found a pointer node",
            )),
        }
    }
}

impl<V: Clone> RawValueCells<V> for RaryPointers {
    fn raw_cell(value: V) -> Self::Cell {
        RaryCell::Root(value)
    }

    fn raw_value(cell: Self::Cell) -> Result<V, SamError> {
        match cell {
            RaryCell::Root(value) => Ok(value),
            RaryCell::Node { .. } => Err(SamError::InvalidPointerCell(
                "expected a raw structure cell, found a pointer node",
            )),
        }
    }
}

impl<V: Clone> RawValueCells<V> for OriginalPointers {
    fn raw_cell(value: V) -> Self::Cell {
        OriginalCell::Raw(value)
    }

    fn raw_value(cell: Self::Cell) -> Result<V, SamError> {
        match cell {
            OriginalCell::Raw(value) => Ok(value),
            _ => Err(SamError::InvalidPointerCell(
                "expected a raw structure cell, found a pointer cell",
            )),
        }
    }
}

impl<V: Clone> RawValueCells<V> for RecursivePointers {
    fn raw_cell(value: V) -> Self::Cell {
        value
    }

    fn raw_value(cell: Self::Cell) -> Result<V, SamError> {
        Ok(cell)
    }
}

/// The live cached roots visible to a cache-aware pointer walk.
///
/// A root in this set has its value in the client cache and is not yet
/// written back, so walks must link to it without reading it.
pub trait CachedRoots<W> {
    /// Whether `root` is the write-back address of a live cached value.
    fn is_cached(&self, root: Address) -> bool;

    /// The staged write-back state of a live cached root, for backends whose
    /// cache hits must update the client-resident root (the original pointer).
    fn writeback_mut(&mut self, root: Address) -> Option<&mut W>;
}

/// An empty [`CachedRoots`], used when no value is cached.
pub(crate) struct NoCachedRoots;

impl<W> CachedRoots<W> for NoCachedRoots {
    fn is_cached(&self, _root: Address) -> bool {
        false
    }

    fn writeback_mut(&mut self, _root: Address) -> Option<&mut W> {
        None
    }
}

/// Outcome of a cache-aware dereference.
#[derive(Debug)]
pub enum CachedDeref<V, W> {
    /// The value was read from SAM; its root is staged at `root`, to be
    /// written with `writeback` when the cache entry is released.
    Miss {
        root: Address,
        value: V,
        writeback: W,
    },
    /// The path led to the live cached root `root`; nothing was read there.
    Hit { root: Address },
}

/// Cache-aware pointer operations used by [`crate::pointer::CachedPointers`]
/// to implement the paper's client-side cache: values are cached under their
/// write-back root address, and walks from other aliases stop at that root.
pub trait CacheablePointerBackend<V: Clone>: SmartPointerBackend<V> {
    /// Backend-specific staged write-back state.
    type Writeback;

    /// Dereferences `pointer`, rebuilding its path, without reading any root
    /// in `roots`. Returns `None` for a pointer with no value.
    fn deref_cached<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        roots: &mut dyn CachedRoots<Self::Writeback>,
    ) -> Result<Option<CachedDeref<V, Self::Writeback>>, SamError>;

    /// Writes a cached value back to its staged root.
    fn finish_cached<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        writeback: Self::Writeback,
        value: V,
    ) -> Result<(), SamError>;

    /// Copies `pointer` while roots in `roots` may be live. The default is an
    /// ordinary bulk copy, correct for backends whose copies never read.
    fn copy_cached<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        num_copies: usize,
        roots: &mut dyn CachedRoots<Self::Writeback>,
    ) -> Result<Vec<Self::Pointer>, SamError> {
        let _ = roots;
        self.copy_many(sam, pointer, num_copies)
    }
}

/// Stateless original single-write backend.
#[derive(Clone, Copy, Debug, Default)]
pub struct OriginalPointers;

impl<V: Clone> SmartPointerBackend<V> for OriginalPointers {
    type Pointer = OriginalPointer;
    type Cell = OriginalCell<V>;

    fn label(&self) -> &'static str {
        "original"
    }

    fn new_pointer<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        value: V,
    ) -> Result<Self::Pointer, SamError> {
        OriginalPointer::new(sam, value)
    }

    fn copy_pointer<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<Self::Pointer, SamError> {
        pointer.copy(sam)
    }

    fn get<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<Option<V>, SamError> {
        pointer.get(sam).map(Some)
    }

    fn put<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        value: V,
    ) -> Result<(), SamError> {
        pointer.put(sam, value)
    }

    fn delete<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<(), SamError> {
        pointer.delete(sam)
    }

    fn is_single_reference<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<bool, SamError> {
        pointer.is_single_reference(sam)
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
        pointer.with_value(sam, |value, sam| operation(value, self, sam))
    }
}

impl<V: Clone> CacheablePointerBackend<V> for OriginalPointers {
    type Writeback = OriginalWriteback<V>;

    fn deref_cached<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        roots: &mut dyn CachedRoots<Self::Writeback>,
    ) -> Result<Option<CachedDeref<V, Self::Writeback>>, SamError> {
        let (root, miss) = pointer.deref_cached(sam, roots)?;
        Ok(Some(match miss {
            Some((value, writeback)) => CachedDeref::Miss {
                root,
                value,
                writeback,
            },
            None => CachedDeref::Hit { root },
        }))
    }

    fn finish_cached<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        writeback: Self::Writeback,
        value: V,
    ) -> Result<(), SamError> {
        OriginalPointer::finish_cached(sam, writeback, value)
    }

    fn copy_cached<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        num_copies: usize,
        roots: &mut dyn CachedRoots<Self::Writeback>,
    ) -> Result<Vec<Self::Pointer>, SamError> {
        if num_copies == 0 {
            return Err(SamError::InvalidParameter("num_copies must be positive"));
        }
        (0..num_copies)
            .map(|_| pointer.copy_cached(sam, roots))
            .collect()
    }
}

/// Stateless balanced binary multi-write backend.
#[derive(Clone, Copy, Debug, Default)]
pub struct MultiWritePointers;

impl<V: Clone> SmartPointerBackend<V> for MultiWritePointers {
    type Pointer = MultiWritePointer;
    type Cell = MultiWriteCell<V>;

    fn label(&self) -> &'static str {
        "multiwrite"
    }

    fn new_pointer<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        value: V,
    ) -> Result<Self::Pointer, SamError> {
        MultiWritePointer::new(sam, value)
    }

    fn copy_pointer<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<Self::Pointer, SamError> {
        pointer.copy(sam)
    }

    fn copy_many<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        num_copies: usize,
    ) -> Result<Vec<Self::Pointer>, SamError> {
        pointer.copy_many(sam, num_copies)
    }

    fn get<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<Option<V>, SamError> {
        pointer.get(sam).map(Some)
    }

    fn put<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        value: V,
    ) -> Result<(), SamError> {
        pointer.put(sam, value)
    }

    fn delete<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<(), SamError> {
        pointer.delete(sam)
    }

    fn is_single_reference<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<bool, SamError> {
        pointer.is_single_reference(sam)
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
        pointer.with_value(sam, |value, sam| operation(value, self, sam))
    }
}

impl<V: Clone> CacheablePointerBackend<V> for MultiWritePointers {
    type Writeback = Address;

    fn deref_cached<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        roots: &mut dyn CachedRoots<Self::Writeback>,
    ) -> Result<Option<CachedDeref<V, Self::Writeback>>, SamError> {
        let (root, value) = pointer.deref_cached(sam, &|address| roots.is_cached(address))?;
        Ok(Some(tree_deref(root, value)))
    }

    fn finish_cached<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        writeback: Self::Writeback,
        value: V,
    ) -> Result<(), SamError> {
        MultiWritePointer::finish_cached(sam, writeback, value)
    }

    // Binary multi-write copies never read (they only link new leaves under
    // the current head), so the default ordinary copy is already cache-safe.
}

/// Configured r-ary multi-write backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RaryPointers {
    branching_factor: usize,
    /// `New` calls since the last public flush: BlockOSAM flushes after
    /// every `branching_factor` of them (each writes one root and reads
    /// nothing).
    new_calls: usize,
}

impl RaryPointers {
    /// Creates an r-ary backend after validating its even fanout.
    pub fn new(branching_factor: usize) -> Result<Self, SamError> {
        if branching_factor < 2 || !branching_factor.is_multiple_of(2) {
            return Err(SamError::InvalidParameter(
                "branching factor must be an even integer of at least two",
            ));
        }
        Ok(Self {
            branching_factor,
            new_calls: 0,
        })
    }

    /// Returns the configured fanout.
    pub fn branching_factor(&self) -> usize {
        self.branching_factor
    }
}

impl<V: Clone> SmartPointerBackend<V> for RaryPointers {
    type Pointer = RaryPointer;
    type Cell = RaryCell<V>;

    fn label(&self) -> &'static str {
        "multiwriterary"
    }

    fn new_pointer<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        value: V,
    ) -> Result<Self::Pointer, SamError> {
        let pointer = RaryPointer::new(sam, value, self.branching_factor)?;
        self.new_calls += 1;
        if self.new_calls == self.branching_factor {
            self.new_calls = 0;
            sam.flush(self.branching_factor, "SmartPointerMultiWriteRary")?;
        }
        Ok(pointer)
    }

    fn copy_pointer<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<Self::Pointer, SamError> {
        pointer.copy(sam)
    }

    fn copy_many<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        num_copies: usize,
    ) -> Result<Vec<Self::Pointer>, SamError> {
        pointer.copy_many(sam, num_copies)
    }

    fn get<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<Option<V>, SamError> {
        pointer.get(sam).map(Some)
    }

    fn put<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        value: V,
    ) -> Result<(), SamError> {
        pointer.put(sam, value)
    }

    fn delete<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<(), SamError> {
        pointer.delete(sam)
    }

    fn is_single_reference<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<bool, SamError> {
        pointer.is_single_reference(sam)
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
        pointer.with_value(sam, |value, sam| operation(value, self, sam))
    }
}

impl<V: Clone> CacheablePointerBackend<V> for RaryPointers {
    type Writeback = Address;

    fn deref_cached<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        roots: &mut dyn CachedRoots<Self::Writeback>,
    ) -> Result<Option<CachedDeref<V, Self::Writeback>>, SamError> {
        let (root, value) = pointer.deref_cached(sam, &|address| roots.is_cached(address))?;
        Ok(Some(tree_deref(root, value)))
    }

    fn finish_cached<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        writeback: Self::Writeback,
        value: V,
    ) -> Result<(), SamError> {
        RaryPointer::finish_cached(sam, writeback, value)
    }

    fn copy_cached<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        num_copies: usize,
        roots: &mut dyn CachedRoots<Self::Writeback>,
    ) -> Result<Vec<Self::Pointer>, SamError> {
        if pointer.branching_factor() != self.branching_factor {
            return Err(SamError::InvalidParameter(
                "r-ary pointer and backend fanouts differ",
            ));
        }
        pointer.copy_cached(sam, num_copies, &|address| roots.is_cached(address))
    }
}

/// Miss/hit for tree backends whose write-back state is just the root.
fn tree_deref<V>(root: Address, value: Option<V>) -> CachedDeref<V, Address> {
    match value {
        Some(value) => CachedDeref::Miss {
            root,
            value,
            writeback: root,
        },
        None => CachedDeref::Hit { root },
    }
}

impl<V: Clone> SmartPointerBackend<V> for RecursivePointers {
    type Pointer = RecursivePointer;
    type Cell = V;

    fn label(&self) -> &'static str {
        "recursive"
    }

    fn new_pointer<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        value: V,
    ) -> Result<Self::Pointer, SamError> {
        RecursivePointers::new_pointer(self, sam, value)
    }

    fn copy_pointer<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        _sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<Self::Pointer, SamError> {
        self.copy(*pointer)
    }

    fn copy_many<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        _sam: &mut S,
        pointer: &mut Self::Pointer,
        num_copies: usize,
    ) -> Result<Vec<Self::Pointer>, SamError> {
        RecursivePointers::copy_many(self, *pointer, num_copies)
    }

    fn get<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<Option<V>, SamError> {
        RecursivePointers::get(self, sam, *pointer)
    }

    fn put<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        value: V,
    ) -> Result<(), SamError> {
        RecursivePointers::put(self, sam, *pointer, value)
    }

    fn delete<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<(), SamError> {
        let head = pointer.head();
        if RecursivePointers::delete(self, pointer)? {
            // The last alias is gone: the value's address can be reused.
            if let Some(head) = head {
                sam.retire(head);
            }
        }
        Ok(())
    }

    fn is_single_reference<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        _sam: &mut S,
        pointer: &mut Self::Pointer,
    ) -> Result<bool, SamError> {
        Ok(RecursivePointers::is_single_reference(self, *pointer))
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
        RecursivePointers::with_value(self, sam, *pointer, operation)
    }

    fn with_value_read<S, R, F>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        operation: F,
    ) -> Result<R, SamError>
    where
        S: SingleAccessMachine<Self::Cell>,
        F: FnOnce(&mut V, &mut Self, &mut S) -> Result<R, SamError>,
    {
        // A multi-read address: read it and leave it in place. Copying the
        // value's nested recursive pointers only touches client reference
        // counts, so the stored value is unchanged.
        let mut value = RecursivePointers::get(self, sam, *pointer)?.ok_or(
            SamError::InvalidPointerCell("recursive pointer has no value"),
        )?;
        operation(&mut value, self, sam)
    }
}

impl<V: Clone> CacheablePointerBackend<V> for RecursivePointers {
    type Writeback = Address;

    fn deref_cached<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        pointer: &mut Self::Pointer,
        roots: &mut dyn CachedRoots<Self::Writeback>,
    ) -> Result<Option<CachedDeref<V, Self::Writeback>>, SamError> {
        // A recursive pointer's value lives at one multi-read address, which
        // doubles as the cache key.
        let Some(address) = pointer.head() else {
            return Ok(None);
        };
        if roots.is_cached(address) {
            return Ok(Some(CachedDeref::Hit { root: address }));
        }
        Ok(sam
            .read(address, "RecursivePointer")?
            .map(|value| CachedDeref::Miss {
                root: address,
                value,
                writeback: address,
            }))
    }

    fn finish_cached<S: SingleAccessMachine<Self::Cell>>(
        &mut self,
        sam: &mut S,
        writeback: Self::Writeback,
        value: V,
    ) -> Result<(), SamError> {
        sam.write(writeback, value, "RecursivePointer")
    }
}

/// Generic wrapper carrying Python's temporary-copy ownership marker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmartPointer<P> {
    pointer: P,
    temp_copy: bool,
}

impl<P> SmartPointer<P> {
    /// Wraps an already-created backend pointer.
    pub fn from_pointer(pointer: P) -> Self {
        Self {
            pointer,
            temp_copy: false,
        }
    }

    /// Returns the backend pointer handle.
    pub fn pointer(&self) -> &P {
        &self.pointer
    }

    /// Returns the mutable backend pointer handle.
    pub fn pointer_mut(&mut self) -> &mut P {
        &mut self.pointer
    }

    /// Marks or clears temporary ownership.
    pub fn set_temp_copy(&mut self, temp_copy: bool) {
        self.temp_copy = temp_copy;
    }

    /// Returns whether this alias is temporary.
    pub fn is_temp_copy(&self) -> bool {
        self.temp_copy
    }

    /// Returns the active backend label.
    pub fn get_label(&self) -> &'static str
    where
        P: PointerHandle,
    {
        P::LABEL
    }

    /// Allocates and wraps a new pointer.
    pub fn new<V, B, S>(backend: &mut B, sam: &mut S, value: V) -> Result<Self, SamError>
    where
        V: Clone,
        B: SmartPointerBackend<V, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
    {
        Ok(Self::from_pointer(backend.new_pointer(sam, value)?))
    }

    /// Creates one alias. This method always returns one pointer.
    pub fn copy<V, B, S>(
        &mut self,
        backend: &mut B,
        sam: &mut S,
        temp_copy: bool,
    ) -> Result<Self, SamError>
    where
        V: Clone,
        B: SmartPointerBackend<V, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
    {
        Ok(Self {
            pointer: backend.copy_pointer(sam, &mut self.pointer)?,
            temp_copy,
        })
    }

    /// Alias for `copy`, matching Python's `smart_copy` spelling.
    pub fn smart_copy<V, B, S>(
        &mut self,
        backend: &mut B,
        sam: &mut S,
        temp_copy: bool,
    ) -> Result<Self, SamError>
    where
        V: Clone,
        B: SmartPointerBackend<V, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
    {
        self.copy(backend, sam, temp_copy)
    }

    /// Explicit bulk-copy interface used by multi-write graph construction.
    pub fn copy_many<V, B, S>(
        &mut self,
        backend: &mut B,
        sam: &mut S,
        num_copies: usize,
        temp_copy: bool,
    ) -> Result<Vec<Self>, SamError>
    where
        V: Clone,
        B: SmartPointerBackend<V, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
    {
        Ok(backend
            .copy_many(sam, &mut self.pointer, num_copies)?
            .into_iter()
            .map(|pointer| Self { pointer, temp_copy })
            .collect())
    }

    /// Reads the pointee.
    pub fn get<V, B, S>(&mut self, backend: &mut B, sam: &mut S) -> Result<Option<V>, SamError>
    where
        V: Clone,
        B: SmartPointerBackend<V, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
    {
        backend.get(sam, &mut self.pointer)
    }

    /// Reads and clones a pointee that has no nested smart-pointer ownership.
    pub fn get_and_copy<V, B, S>(
        &mut self,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<Option<V>, SamError>
    where
        V: Clone,
        B: SmartPointerBackend<V, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
    {
        self.get(backend, sam)
    }

    /// Reads one field selected by a closure and returns an owned clone.
    pub fn get_attr<V, T, B, S, F>(
        &mut self,
        backend: &mut B,
        sam: &mut S,
        select: F,
    ) -> Result<T, SamError>
    where
        V: Clone,
        T: Clone,
        B: SmartPointerBackend<V, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
        F: FnOnce(&V) -> &T,
    {
        self.with_value(backend, sam, |value, _backend, _sam| {
            Ok(select(value).clone())
        })
    }

    /// Replaces the pointee.
    pub fn put<V, B, S>(&mut self, backend: &mut B, sam: &mut S, value: V) -> Result<(), SamError>
    where
        V: Clone,
        B: SmartPointerBackend<V, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
    {
        backend.put(sam, &mut self.pointer, value)
    }

    /// Operates on the live value, including smart copies of nested pointers.
    pub fn with_value<V, B, S, R, F>(
        &mut self,
        backend: &mut B,
        sam: &mut S,
        operation: F,
    ) -> Result<R, SamError>
    where
        V: Clone,
        B: SmartPointerBackend<V, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
        F: FnOnce(&mut V, &mut B, &mut S) -> Result<R, SamError>,
    {
        backend.with_value(sam, &mut self.pointer, operation)
    }

    /// Ownership-aware move operation; Rust scopes the live value to `operation`.
    pub fn move_value<V, B, S, R, F>(
        &mut self,
        backend: &mut B,
        sam: &mut S,
        operation: F,
    ) -> Result<R, SamError>
    where
        V: Clone,
        B: SmartPointerBackend<V, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
        F: FnOnce(&mut V, &mut B, &mut S) -> Result<R, SamError>,
    {
        self.with_value(backend, sam, operation)
    }

    /// Mutates a pointee in one ownership-aware access.
    pub fn modify<V, B, S, R, F>(
        &mut self,
        backend: &mut B,
        sam: &mut S,
        update: F,
    ) -> Result<R, SamError>
    where
        V: Clone,
        B: SmartPointerBackend<V, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
        F: FnOnce(&mut V) -> R,
    {
        self.with_value(backend, sam, |value, _backend, _sam| Ok(update(value)))
    }

    /// Rust equivalent of Python's `put_attr`, expressed as a typed closure.
    pub fn put_attr<V, B, S, R, F>(
        &mut self,
        backend: &mut B,
        sam: &mut S,
        update: F,
    ) -> Result<R, SamError>
    where
        V: Clone,
        B: SmartPointerBackend<V, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
        F: FnOnce(&mut V) -> R,
    {
        self.modify(backend, sam, update)
    }

    /// Returns whether this is the only alias.
    pub fn is_single_reference<V, B, S>(
        &mut self,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<bool, SamError>
    where
        V: Clone,
        B: SmartPointerBackend<V, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
    {
        backend.is_single_reference(sam, &mut self.pointer)
    }

    /// Deletes this alias.
    pub fn delete<V, B, S>(&mut self, backend: &mut B, sam: &mut S) -> Result<(), SamError>
    where
        V: Clone,
        B: SmartPointerBackend<V, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
    {
        backend.delete(sam, &mut self.pointer)
    }

    /// Alias for `delete`, matching Python's `smart_delete` spelling.
    pub fn smart_delete<V, B, S>(&mut self, backend: &mut B, sam: &mut S) -> Result<(), SamError>
    where
        V: Clone,
        B: SmartPointerBackend<V, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
    {
        self.delete(backend, sam)
    }

    /// Deletes the alias only when it carries temporary ownership.
    pub fn delete_temp_copy<V, B, S>(
        &mut self,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<(), SamError>
    where
        V: Clone,
        B: SmartPointerBackend<V, Pointer = P>,
        S: SingleAccessMachine<B::Cell>,
    {
        if self.temp_copy {
            self.delete(backend, sam)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccessPolicy, DryRunSam};

    #[test]
    fn facade_exposes_scalar_and_bulk_copy_without_changing_scalar_return_type() {
        let mut backend = RaryPointers::new(4).unwrap();
        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut original = SmartPointer::new(&mut backend, &mut sam, 7_u64).unwrap();
        let mut scalar = original.smart_copy(&mut backend, &mut sam, true).unwrap();
        let mut bulk = original
            .copy_many(&mut backend, &mut sam, 8, false)
            .unwrap();
        assert!(scalar.is_temp_copy());
        assert_eq!(bulk.len(), 8);
        scalar.put(&mut backend, &mut sam, 11).unwrap();
        assert_eq!(original.get(&mut backend, &mut sam).unwrap(), Some(11));
        for pointer in &mut bulk {
            assert_eq!(pointer.get(&mut backend, &mut sam).unwrap(), Some(11));
        }
        scalar.delete_temp_copy(&mut backend, &mut sam).unwrap();
    }

    #[test]
    fn python_labels_select_the_expected_policy_and_strategy() {
        assert_eq!(
            PointerKind::from_label("original", 4)
                .unwrap()
                .access_policy(),
            AccessPolicy::SINGLE_WRITE
        );
        assert_eq!(
            PointerKind::from_label("multiwrite", 4)
                .unwrap()
                .access_policy(),
            AccessPolicy::MULTI_WRITE
        );
        let rary = PointerKind::from_label("multiwrite_rary", 6).unwrap();
        assert_eq!(rary.access_strategy(), AccessStrategy::MULTI_WRITE_RARY);
        assert_eq!(
            PointerKind::from_label("recursive", 4)
                .unwrap()
                .access_policy(),
            AccessPolicy::RECURSIVE
        );
        assert!(PointerKind::from_label("multiwriterary", 3).is_err());
    }
}
