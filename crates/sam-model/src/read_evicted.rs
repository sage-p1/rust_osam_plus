//! OSAM+'s write buffering: writes stay in the client stash and reads evict.
//!
//! With [`crate::WriteStrategy::Local`] an OSAM+ write goes to the stash
//! without a ReadAndRm or an eviction, and every read evicts `evictions`
//! paths ([`crate::ReadStrategy::MultiPath`]). A run of writes with no read in
//! between (graph construction is almost all writes) would then grow the
//! stash without bound, so [`ReadEvictedWrites`] adds the public flush rule of
//! BlockOSAM's bounded writer: once `batch` oblivious writes have accumulated
//! (net of what reads absorbed), it issues one public flush of `batch` paths.
//! A read absorbs up to `read_evictions` pending writes, the paths it evicts.
//!
//! A flush is one round trip and is counted as one read of [`FLUSH_STRUCTURE`]
//! (see [`SingleAccessMachine::flush`]), so charging OSAM+ its reads charges
//! it every eviction round trip.

use crate::{Address, MemoryClass, SamError, SingleAccessMachine, Stats};

/// Structure label of the flushes, so they show up in per-structure counts.
pub const FLUSH_STRUCTURE: &str = "OsamPlusFlush";

/// A SAM wrapper that bounds the writes pending between reads; see the
/// module docs. With `enabled == false` it forwards every call unchanged.
pub struct ReadEvictedWrites<'a, S> {
    sam: &'a mut S,
    read_evictions: usize,
    batch: usize,
    pending: usize,
    enabled: bool,
}

impl<'a, S> ReadEvictedWrites<'a, S> {
    /// Wraps `sam`: reads evict `read_evictions` paths, and a flush of
    /// `batch` paths follows every `batch` pending writes (both at least 1).
    pub fn new(sam: &'a mut S, read_evictions: usize, batch: usize, enabled: bool) -> Self {
        Self {
            sam,
            read_evictions: read_evictions.max(1),
            batch: batch.max(1),
            pending: 0,
            enabled,
        }
    }

    /// Writes buffered since the last read or flush.
    pub fn pending(&self) -> usize {
        self.pending
    }
}

impl<V: Clone, S: SingleAccessMachine<V>> SingleAccessMachine<V> for ReadEvictedWrites<'_, S> {
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
        if self.enabled && matches!(address, Address::Oblivious(_)) {
            self.pending += 1;
            if self.pending >= self.batch {
                self.pending = 0;
                self.sam.flush(self.batch, FLUSH_STRUCTURE)?;
            }
        }
        Ok(())
    }

    fn write_batch(
        &mut self,
        writes: Vec<(Address, V)>,
        structure: &'static str,
    ) -> Result<(), SamError> {
        if !self.enabled {
            return self.sam.write_batch(writes, structure);
        }
        for (address, value) in writes {
            self.write(address, value, structure)?;
        }
        Ok(())
    }

    fn read(&mut self, address: Address, structure: &'static str) -> Result<Option<V>, SamError> {
        let value = self.sam.read(address, structure)?;
        if matches!(address, Address::Oblivious(_)) {
            self.pending = self.pending.saturating_sub(self.read_evictions);
        }
        Ok(value)
    }

    fn retire(&mut self, address: Address) {
        self.sam.retire(address)
    }

    fn flush(&mut self, paths: usize, structure: &'static str) -> Result<(), SamError> {
        self.pending = self.pending.saturating_sub(paths);
        self.sam.flush(paths, structure)
    }

    fn stats(&self) -> &Stats {
        self.sam.stats()
    }

    fn reset_stash_maximum(&mut self) {
        self.sam.reset_stash_maximum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccessPolicy, DryRunSam};

    #[test]
    fn two_writes_without_a_read_flush_once() {
        let mut dry: DryRunSam<u64> = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut sam = ReadEvictedWrites::new(&mut dry, 2, 2, true);
        let a = sam.alloc(MemoryClass::Oblivious, "t");
        let b = sam.alloc(MemoryClass::Oblivious, "t");
        sam.write(a, 1, "t").unwrap();
        sam.write(b, 2, "t").unwrap(); // second pending write: one flush
        assert_eq!(sam.stats().flushes, 1);
        sam.write(a, 3, "t").unwrap();
        sam.read(a, "t").unwrap(); // the read's evictions absorb it
        sam.write(b, 4, "t").unwrap();
        assert_eq!(sam.stats().flushes, 1);
        assert_eq!(sam.stats().operations.reads, 2); // one read + one flush
        assert_eq!(sam.stats().by_structure[FLUSH_STRUCTURE].reads, 1);
    }

    #[test]
    fn disabled_forwards_unchanged() {
        let mut dry: DryRunSam<u64> = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut sam = ReadEvictedWrites::new(&mut dry, 2, 2, false);
        let a = sam.alloc(MemoryClass::Oblivious, "t");
        for i in 0..10 {
            sam.write(a, i, "t").unwrap();
        }
        assert_eq!(sam.stats().flushes, 0);
    }
}
