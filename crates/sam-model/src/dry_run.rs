use crate::{
    AccessPolicy, Address, MemoryClass, SamError, SamSnapshot, SingleAccessMachine, SnapshotBlock,
    Stats,
};
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct Cell<V> {
    reads: usize,
    writes: usize,
    value: Option<V>,
}

impl<V> Default for Cell<V> {
    fn default() -> Self {
        Self {
            reads: 0,
            writes: 0,
            value: None,
        }
    }
}

/// Fast, non-cryptographic SAM implementation for graph construction and tests.
#[derive(Clone, Debug)]
pub struct DryRunSam<V> {
    policy: AccessPolicy,
    next_oblivious: u64,
    next_plaintext: u64,
    oblivious: HashMap<u64, Cell<V>>,
    plaintext: HashMap<u64, Cell<V>>,
    reusable_oblivious: Vec<u64>,
    reusable_plaintext: Vec<u64>,
    stats: Stats,
}

impl<V: Clone> DryRunSam<V> {
    /// Creates an empty dry-run SAM.
    pub fn new(policy: AccessPolicy) -> Self {
        Self {
            policy,
            // PathOsamPlus reserves Identifier::MAX and begins allocation at 1.
            next_oblivious: 1,
            next_plaintext: 1,
            oblivious: HashMap::new(),
            plaintext: HashMap::new(),
            reusable_oblivious: Vec::new(),
            reusable_plaintext: Vec::new(),
            stats: Stats::default(),
        }
    }

    /// Returns the configured access policy.
    pub fn policy(&self) -> AccessPolicy {
        self.policy
    }

    /// Returns true when an address is currently live.
    pub fn contains(&self, address: Address) -> bool {
        match address {
            Address::Oblivious(identifier) => self.oblivious.contains_key(&identifier),
            Address::Plaintext(identifier) => self.plaintext.contains_key(&identifier),
        }
    }

    /// Number of written, not yet consumed oblivious blocks (the blocks a
    /// snapshot would contain).
    pub fn live_blocks(&self) -> usize {
        self.oblivious
            .values()
            .filter(|cell| cell.value.is_some())
            .count()
    }

    /// Produces the live state that a cryptographic backend must import.
    pub fn snapshot(&self) -> SamSnapshot<V> {
        let mut blocks: Vec<_> = self
            .oblivious
            .iter()
            .filter_map(|(identifier, cell)| {
                cell.value.clone().map(|value| SnapshotBlock {
                    identifier: *identifier,
                    value,
                    reads: cell.reads,
                    writes: cell.writes,
                })
            })
            .collect();
        blocks.sort_by_key(|block| block.identifier);
        SamSnapshot {
            blocks,
            next_identifier: self.next_oblivious,
            stats: self.stats.clone(),
        }
    }

    fn structure_stats_mut(&mut self, structure: &'static str) -> &mut crate::StructureStats {
        self.stats.by_structure.entry(structure).or_default()
    }
}

impl<V: Clone> SingleAccessMachine<V> for DryRunSam<V> {
    fn alloc(&mut self, class: MemoryClass, structure: &'static str) -> Address {
        match class {
            MemoryClass::Oblivious => {
                if self.policy.max_reads > 1 {
                    if let Some(identifier) = self.reusable_oblivious.pop() {
                        self.oblivious.insert(identifier, Cell::default());
                        return Address::Oblivious(identifier);
                    }
                }

                let identifier = self.next_oblivious;
                self.next_oblivious += 1;
                self.oblivious.insert(identifier, Cell::default());
                self.stats.operations.allocations += 1;
                self.structure_stats_mut(structure).allocations += 1;
                Address::Oblivious(identifier)
            }
            MemoryClass::Plaintext => {
                let identifier = self.reusable_plaintext.pop().unwrap_or_else(|| {
                    let identifier = self.next_plaintext;
                    self.next_plaintext += 1;
                    identifier
                });
                self.plaintext.insert(identifier, Cell::default());
                Address::Plaintext(identifier)
            }
        }
    }

    fn write(
        &mut self,
        address: Address,
        value: V,
        structure: &'static str,
    ) -> Result<(), SamError> {
        let (cells, counted) = match address {
            Address::Oblivious(_) => (&mut self.oblivious, true),
            Address::Plaintext(_) => (&mut self.plaintext, false),
        };
        let identifier = match address {
            Address::Oblivious(identifier) | Address::Plaintext(identifier) => identifier,
        };
        let cell = cells
            .get_mut(&identifier)
            .ok_or(SamError::InvalidAddress(address))?;
        if cell.writes >= self.policy.max_writes {
            return Err(SamError::WriteLimitExceeded {
                address,
                limit: self.policy.max_writes,
            });
        }
        cell.writes += 1;
        cell.value = Some(value);

        if counted {
            self.stats.operations.writes += 1;
            self.structure_stats_mut(structure).writes += 1;
            self.stats.write_batches += 1;
            self.stats.max_write_batches =
                self.stats.max_write_batches.max(self.stats.write_batches);
        }
        Ok(())
    }

    fn read(&mut self, address: Address, structure: &'static str) -> Result<Option<V>, SamError> {
        let (cells, counted) = match address {
            Address::Oblivious(_) => (&mut self.oblivious, true),
            Address::Plaintext(_) => (&mut self.plaintext, false),
        };
        let identifier = match address {
            Address::Oblivious(identifier) | Address::Plaintext(identifier) => identifier,
        };
        let cell = cells
            .get_mut(&identifier)
            .ok_or(SamError::InvalidAddress(address))?;
        if cell.reads >= self.policy.max_reads {
            return Err(SamError::ReadLimitExceeded {
                address,
                limit: self.policy.max_reads,
            });
        }
        cell.reads += 1;
        let value = cell.value.clone();
        let consumed = cell.reads >= self.policy.max_reads;
        if consumed {
            cells.remove(&identifier);
        }

        if counted {
            self.stats.operations.reads += 1;
            self.structure_stats_mut(structure).reads += 1;
            self.stats.write_batches = self.stats.write_batches.saturating_sub(1);
        }
        Ok(value)
    }

    fn retire(&mut self, address: Address) {
        match address {
            Address::Oblivious(identifier) if self.policy.max_reads > 1 => {
                self.oblivious.remove(&identifier);
                if !self.reusable_oblivious.contains(&identifier) {
                    self.reusable_oblivious.push(identifier);
                }
            }
            Address::Plaintext(identifier) => {
                self.plaintext.remove(&identifier);
                if !self.reusable_plaintext.contains(&identifier) {
                    self.reusable_plaintext.push(identifier);
                }
            }
            Address::Oblivious(_) => {}
        }
    }

    fn stats(&self) -> &Stats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST: &str = "test";

    #[test]
    fn single_read_consumes_an_address() {
        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let address = sam.alloc(MemoryClass::Oblivious, TEST);
        sam.write(address, 7_u64, TEST).unwrap();
        assert_eq!(sam.read(address, TEST).unwrap(), Some(7));
        assert_eq!(
            sam.read(address, TEST),
            Err(SamError::InvalidAddress(address))
        );
        assert_eq!(
            sam.stats().operations,
            crate::OperationCounts {
                allocations: 1,
                reads: 1,
                writes: 1,
            }
        );
    }

    #[test]
    fn recursive_policy_recycles_retired_addresses() {
        let mut sam = DryRunSam::<u64>::new(AccessPolicy::RECURSIVE);
        let first = sam.alloc(MemoryClass::Oblivious, TEST);
        sam.retire(first);
        let recycled = sam.alloc(MemoryClass::Oblivious, TEST);
        assert_eq!(first, recycled);
        assert_eq!(sam.stats().operations.allocations, 1);
    }

    #[test]
    fn snapshot_is_sorted_and_excludes_consumed_blocks() {
        let mut sam = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let first = sam.alloc(MemoryClass::Oblivious, TEST);
        let second = sam.alloc(MemoryClass::Oblivious, TEST);
        sam.write(first, 10_u64, TEST).unwrap();
        sam.write(second, 20_u64, TEST).unwrap();
        sam.read(first, TEST).unwrap();
        let snapshot = sam.snapshot();
        assert_eq!(snapshot.blocks.len(), 1);
        assert_eq!(snapshot.blocks[0].identifier, 2);
        assert_eq!(snapshot.blocks[0].value, 20);
        assert_eq!(snapshot.blocks[0].reads, 0);
        assert_eq!(snapshot.blocks[0].writes, 1);
        assert_eq!(snapshot.next_identifier, 3);
    }
}
