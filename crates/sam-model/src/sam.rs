use crate::SamError;
use std::collections::BTreeMap;

/// A stable logical address used by pointer cells.
///
/// Oblivious identifiers begin at one to match `PathOsamPlus::alloc`. The
/// cryptographic adapter will keep the randomly assigned tree position out of
/// pointer payloads and map this identifier to its current position locally.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Address {
    /// A block that will eventually reside in OSAM.
    Oblivious(u64),
    /// Client-local storage used by simulations such as recursive pointers.
    Plaintext(u64),
}

/// Selects whether an allocation is modeled as oblivious or client-local.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryClass {
    /// Count and snapshot this allocation as a SAM operation.
    Oblivious,
    /// Keep this allocation client-local and exclude it from SAM counters.
    Plaintext,
}

/// Selects the OSAM+ operation used to satisfy a logical read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadStrategy {
    /// Read the requested path and perform the ordinary single-path eviction.
    SinglePath,
    /// Use `PathOsamPlus::read_multi_paths` to evict the configured path count.
    MultiPath,
}

/// Selects the OSAM+ operation used to satisfy a logical write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteStrategy {
    /// Write while performing an immediate server eviction.
    EvictPath,
    /// Use `PathOsamPlus::local_write` and keep the block in the stash.
    Local,
}

/// Cryptographic access behavior associated with a pointer implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccessStrategy {
    /// Read operation used by the cryptographic adapter.
    pub reads: ReadStrategy,
    /// Write operation used by the cryptographic adapter.
    pub writes: WriteStrategy,
}

impl AccessStrategy {
    /// Ordinary OSAM reads and writes.
    pub const STANDARD: Self = Self {
        reads: ReadStrategy::SinglePath,
        writes: WriteStrategy::EvictPath,
    };

    /// R-ary pointer policy: multi-path reads and client-local writes.
    pub const MULTI_WRITE_RARY: Self = Self {
        reads: ReadStrategy::MultiPath,
        writes: WriteStrategy::Local,
    };
}

/// Per-address access limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccessPolicy {
    /// Maximum reads permitted for one address.
    pub max_reads: usize,
    /// Maximum writes permitted for one address.
    pub max_writes: usize,
}

impl AccessPolicy {
    /// Single-read, single-write SAM.
    pub const SINGLE_WRITE: Self = Self {
        max_reads: 1,
        max_writes: 1,
    };

    /// Single-read, multi-write SAM.
    pub const MULTI_WRITE: Self = Self {
        max_reads: 1,
        max_writes: usize::MAX,
    };

    /// Multi-read, multi-write simulation.
    pub const RECURSIVE: Self = Self {
        max_reads: usize::MAX,
        max_writes: usize::MAX,
    };

    /// Constructs a checked policy.
    pub fn new(max_reads: usize, max_writes: usize) -> Result<Self, SamError> {
        if max_reads == 0 {
            return Err(SamError::InvalidParameter("max_reads must be positive"));
        }
        if max_writes == 0 {
            return Err(SamError::InvalidParameter("max_writes must be positive"));
        }
        Ok(Self {
            max_reads,
            max_writes,
        })
    }
}

/// Allocation/read/write counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OperationCounts {
    /// Fresh oblivious allocations. Recycled addresses are not counted.
    pub allocations: u64,
    /// Oblivious reads.
    pub reads: u64,
    /// Oblivious writes.
    pub writes: u64,
}

/// Operation counts attributed to one high-level structure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StructureStats {
    /// Fresh allocations.
    pub allocations: u64,
    /// Reads.
    pub reads: u64,
    /// Writes.
    pub writes: u64,
}

/// Stash occupancy observed by a cryptographic SAM backend.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StashStats {
    /// Occupancy after the most recent OSAM+ operation.
    pub current: u64,
    /// Maximum occupancy observed since backend construction or reset.
    pub maximum: u64,
    /// Number of occupancy observations.
    pub samples: u64,
}

impl StashStats {
    /// Records one occupancy sample and updates the high-water mark.
    pub fn observe(&mut self, occupancy: u64) {
        self.current = occupancy;
        self.maximum = self.maximum.max(occupancy);
        self.samples += 1;
    }
}

/// Complete dry-run statistics.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Stats {
    /// Global SAM operation counts.
    pub operations: OperationCounts,
    /// Current number of writes not yet offset by reads.
    pub write_batches: u64,
    /// High-water mark of `write_batches`.
    pub max_write_batches: u64,
    /// Public write-burst flushes (see [`SingleAccessMachine::flush`]). Each
    /// is also counted as one read, since it costs one round trip.
    pub flushes: u64,
    /// Counts grouped by caller-provided structure label.
    pub by_structure: BTreeMap<&'static str, StructureStats>,
    /// Real stash occupancy, present only for a cryptographic backend.
    pub stash: Option<StashStats>,
}

/// One live block at the dry-run/cryptographic transition boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotBlock<V> {
    /// Stable logical identifier.
    pub identifier: u64,
    /// Value to encode into a fixed-size OSAM block.
    pub value: V,
    /// Reads already consumed during dry-run construction.
    pub reads: usize,
    /// Writes already consumed during dry-run construction.
    pub writes: usize,
}

/// Live oblivious state produced after graph construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SamSnapshot<V> {
    /// Live blocks, sorted by identifier.
    pub blocks: Vec<SnapshotBlock<V>>,
    /// Identifier expected from the next allocation.
    pub next_identifier: u64,
    /// Build-phase operation statistics.
    pub stats: Stats,
}

/// Backend-neutral interface consumed by native smart-pointer algorithms.
pub trait SingleAccessMachine<V: Clone> {
    /// Allocates an address and attributes it to `structure`.
    fn alloc(&mut self, class: MemoryClass, structure: &'static str) -> Address;

    /// Writes a value to an allocated address.
    fn write(
        &mut self,
        address: Address,
        value: V,
        structure: &'static str,
    ) -> Result<(), SamError>;

    /// Writes several values. Backends may override this to use a native batch.
    fn write_batch(
        &mut self,
        writes: Vec<(Address, V)>,
        structure: &'static str,
    ) -> Result<(), SamError> {
        for (address, value) in writes {
            self.write(address, value, structure)?;
        }
        Ok(())
    }

    /// Reads a value and consumes the address when its read budget is exhausted.
    fn read(&mut self, address: Address, structure: &'static str) -> Result<Option<V>, SamError>;

    /// Makes an address available for reuse when the policy permits it.
    fn retire(&mut self, address: Address);

    /// A public flush: `paths` address-independent evictions that read no
    /// requested address, sent together as one request. The r-ary pointer
    /// issues one after every `paths` pointer-cell writes of a burst without
    /// reads (graph construction), as BlockOSAM's bounded writer does, so its
    /// pending writes stay bounded. It costs one round trip and is counted as
    /// one read of `structure` (and in `Stats::flushes`); it offsets `paths`
    /// pending writes.
    fn flush(&mut self, paths: usize, structure: &'static str) -> Result<(), SamError>;

    /// Returns current operation statistics.
    fn stats(&self) -> &Stats;

    /// Starts a new stash high-water window: `stats().stash.maximum` is reset
    /// to the current occupancy. Backends without a stash ignore this.
    fn reset_stash_maximum(&mut self) {}
}
