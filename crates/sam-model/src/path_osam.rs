use crate::{
    AccessPolicy, AccessStrategy, Address, BlockCodec, MemoryClass, ReadStrategy, SamError,
    SamSnapshot, SingleAccessMachine, StashStats, Stats, WriteStrategy,
};
use aes::{
    cipher::{Array, BlockCipherEncrypt, KeyInit},
    Aes128,
};
use osam_plus::{BlockValue, BucketSize, OsamPlus, PathCount, PathOsamPlus, StashSize, TreeIndex};
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::{collections::HashMap, marker::PhantomData};

#[derive(Clone, Copy, Debug, Default)]
struct AccessState {
    reads: usize,
    writes: usize,
}

/// Derives an address's tree leaf from the address itself.
///
/// OSAM needs no position map: every oblivious address is read at most once
/// and identifiers are never reused, so `leaf = PRF_k(identifier)` is, to the
/// server, as uniform and independent as a freshly sampled leaf. The client
/// keeps only the 128-bit key `k` instead of one position per address, and
/// pointer cells store nothing beyond the identifier.
struct LeafPrf {
    cipher: Aes128,
    height: u32,
}

impl LeafPrf {
    fn new(key: [u8; 16], block_capacity: u64) -> Self {
        Self {
            cipher: Aes128::new(&Array::from(key)),
            // Matches PathOsamPlus::new: height = log2(capacity) - 1.
            height: block_capacity.ilog2() - 1,
        }
    }

    fn leaf(&self, identifier: u64) -> TreeIndex {
        let mut block = Array::from(u128::from(identifier).to_le_bytes());
        self.cipher.encrypt_block(&mut block);
        let random = u64::from_le_bytes(block[..8].try_into().unwrap());
        let leaves = 1_u64 << self.height;
        leaves + (random & (leaves - 1))
    }
}

/// Cryptographic SAM adapter over [`PathOsamPlus`].
///
/// Pointer cells retain stable logical identifiers; each identifier's leaf is
/// recomputed from it on access (see `LeafPrf`), so the client holds no
/// position map. Per-address SAM policy checks (read/write limits) are also
/// client state and are off unless [`Self::with_policy_checks`] enables them.
pub struct PathOsamSam<V, C, const B: usize, const Z: BucketSize, const P: PathCount> {
    osam: PathOsamPlus<BlockValue<B>, Z, P>,
    codec: C,
    rng: StdRng,
    policy: AccessPolicy,
    strategy: AccessStrategy,
    ordered_evict: bool,
    leaves: LeafPrf,
    /// Per-address access counts, kept only while policy checks are enabled.
    access: Option<HashMap<u64, AccessState>>,
    plaintext: HashMap<u64, V>,
    next_plaintext: u64,
    stats: Stats,
    build_stats: Stats,
    build_max_stash: u64,
    marker: PhantomData<V>,
}

impl<V: Clone, C: BlockCodec<V, B>, const B: usize, const Z: BucketSize, const P: PathCount>
    PathOsamSam<V, C, B, Z, P>
{
    /// Installs a completed dry-run snapshot into a fresh Path OSAM+ tree.
    ///
    /// Installation places every block in one client-side pass and writes
    /// each bucket once (`PathOsamPlus::bulk_load`), as when the client uploads
    /// memory it built locally. The configured access strategy applies after
    /// installation, so r-ary benchmark writes remain local.
    #[allow(clippy::too_many_arguments)]
    pub fn from_snapshot(
        snapshot: SamSnapshot<V>,
        block_capacity: u64,
        overflow_size: StashSize,
        encrypted: bool,
        policy: AccessPolicy,
        strategy: AccessStrategy,
        ordered_evict: bool,
        seed: u64,
        codec: C,
    ) -> Result<Self, SamError> {
        if policy.max_reads != 1 {
            return Err(SamError::InvalidParameter(
                "Path OSAM+ adapter currently requires single-read addresses",
            ));
        }
        if snapshot.next_identifier.saturating_sub(1) > block_capacity {
            return Err(SamError::InvalidParameter(
                "snapshot identifiers exceed Path OSAM+ capacity",
            ));
        }

        let mut osam =
            PathOsamPlus::<BlockValue<B>, Z, P>::new(block_capacity, overflow_size, encrypted)
                .map_err(|error| SamError::Backend(error.to_string()))?;
        let mut rng = StdRng::seed_from_u64(seed);
        let leaves = LeafPrf::new(rng.gen(), block_capacity);
        let mut access = HashMap::new();

        for expected in 1..snapshot.next_identifier {
            let (identifier, _sampled_leaf) = osam
                .alloc(&mut rng)
                .map_err(|error| SamError::Backend(error.to_string()))?;
            if identifier != expected {
                return Err(SamError::Backend(format!(
                    "Path OSAM+ allocated identifier {identifier}, expected {expected}"
                )));
            }
            // Allocated-but-unwritten addresses are meaningful to the original
            // pointer's queue: reading its terminal cell returns `None`. Keep
            // access state for every allocated identifier, not only live blocks.
            access.insert(identifier, AccessState::default());
        }

        let mut installed = Vec::with_capacity(snapshot.blocks.len());
        for block in &snapshot.blocks {
            if block.identifier == 0 || block.identifier >= snapshot.next_identifier {
                return Err(SamError::InvalidAddress(Address::Oblivious(
                    block.identifier,
                )));
            }
            let position = leaves.leaf(block.identifier);
            let encoded = codec.encode(&block.value)?;
            installed.push((block.identifier, position, BlockValue::new(encoded)));
            access.insert(
                block.identifier,
                AccessState {
                    reads: block.reads,
                    writes: block.writes,
                },
            );
        }

        // The client built this memory locally: place every block in one pass
        // and upload each bucket once (see `PathOsamPlus::bulk_load`).
        osam.bulk_load(installed)
            .map_err(|error| SamError::Backend(error.to_string()))?;

        let build_max_stash = osam.max_occupancy();
        let stats = Stats {
            stash: Some(StashStats::default()),
            ..Stats::default()
        };
        Ok(Self {
            osam,
            codec,
            rng,
            policy,
            strategy,
            ordered_evict,
            leaves,
            access: Some(access),
            plaintext: HashMap::new(),
            next_plaintext: 1,
            stats,
            build_stats: snapshot.stats,
            build_max_stash,
            marker: PhantomData,
        })
    }

    /// Enables or disables per-address SAM policy checks. Disabling drops the
    /// per-address table, leaving O(1) client state besides the stash; this
    /// is how benchmarks should run. Checks cannot be re-enabled afterwards.
    pub fn with_policy_checks(mut self, enabled: bool) -> Self {
        if !enabled {
            self.access = None;
        }
        self
    }

    /// Whether per-address SAM policy checks are active.
    pub fn policy_checks(&self) -> bool {
        self.access.is_some()
    }

    /// Dry-run counters captured before installation.
    pub fn build_stats(&self) -> &Stats {
        &self.build_stats
    }

    /// Maximum stash occupancy while installing the initial tree.
    pub fn build_max_stash_occupancy(&self) -> u64 {
        self.build_max_stash
    }

    /// Maximum occupancy tracked internally by Path OSAM+ across all phases.
    pub fn backend_max_stash_occupancy(&self) -> u64 {
        self.osam.max_occupancy()
    }

    /// Explicitly evicts multiple paths, useful for installation or tuning.
    pub fn evict_multi_paths(&mut self) -> Result<(), SamError> {
        self.osam
            .evict_multi_paths(self.ordered_evict, &mut self.rng)
            .map_err(|error| SamError::Backend(error.to_string()))?;
        self.observe_stash();
        Ok(())
    }

    fn observe_stash(&mut self) {
        let occupancy = self.osam.stash_occupancy();
        self.stats
            .stash
            .get_or_insert_with(StashStats::default)
            .observe(occupancy);
    }

    fn structure_stats_mut(&mut self, structure: &'static str) -> &mut crate::StructureStats {
        self.stats.by_structure.entry(structure).or_default()
    }

    fn check_write(&self, identifier: u64) -> Result<(), SamError> {
        let Some(access) = &self.access else {
            return Ok(());
        };
        let address = Address::Oblivious(identifier);
        let state = access
            .get(&identifier)
            .ok_or(SamError::InvalidAddress(address))?;
        if state.writes >= self.policy.max_writes {
            return Err(SamError::WriteLimitExceeded {
                address,
                limit: self.policy.max_writes,
            });
        }
        Ok(())
    }

    fn record_write(&mut self, identifier: u64) {
        if let Some(state) = self
            .access
            .as_mut()
            .and_then(|access| access.get_mut(&identifier))
        {
            state.writes += 1;
        }
    }
}

impl<V: Clone, C: BlockCodec<V, B>, const B: usize, const Z: BucketSize, const P: PathCount>
    SingleAccessMachine<V> for PathOsamSam<V, C, B, Z, P>
{
    fn alloc(&mut self, class: MemoryClass, structure: &'static str) -> Address {
        match class {
            MemoryClass::Oblivious => {
                let (identifier, _sampled_leaf) = self
                    .osam
                    .alloc(&mut self.rng)
                    .expect("validated Path OSAM+ allocation failed");
                if let Some(access) = self.access.as_mut() {
                    access.insert(identifier, AccessState::default());
                }
                self.stats.operations.allocations += 1;
                self.structure_stats_mut(structure).allocations += 1;
                Address::Oblivious(identifier)
            }
            MemoryClass::Plaintext => {
                let identifier = self.next_plaintext;
                self.next_plaintext += 1;
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
        match address {
            Address::Plaintext(identifier) => {
                self.plaintext.insert(identifier, value);
                Ok(())
            }
            Address::Oblivious(identifier) => {
                self.check_write(identifier)?;
                let position = self.leaves.leaf(identifier);
                let block = BlockValue::new(self.codec.encode(&value)?);
                match self.strategy.writes {
                    WriteStrategy::EvictPath => self
                        .osam
                        .write(
                            identifier,
                            position,
                            block,
                            self.ordered_evict,
                            &mut self.rng,
                        )
                        .map_err(|error| SamError::Backend(error.to_string()))?,
                    WriteStrategy::Local => self
                        .osam
                        .local_write(identifier, position, block)
                        .map_err(|error| SamError::Backend(error.to_string()))?,
                }
                self.record_write(identifier);
                self.stats.operations.writes += 1;
                self.structure_stats_mut(structure).writes += 1;
                self.stats.write_batches += 1;
                self.stats.max_write_batches =
                    self.stats.max_write_batches.max(self.stats.write_batches);
                self.observe_stash();
                Ok(())
            }
        }
    }

    fn read(&mut self, address: Address, structure: &'static str) -> Result<Option<V>, SamError> {
        match address {
            Address::Plaintext(identifier) => Ok(self.plaintext.get(&identifier).cloned()),
            Address::Oblivious(identifier) => {
                if let Some(access) = &self.access {
                    let state = access
                        .get(&identifier)
                        .ok_or(SamError::InvalidAddress(address))?;
                    if state.reads >= self.policy.max_reads {
                        return Err(SamError::ReadLimitExceeded {
                            address,
                            limit: self.policy.max_reads,
                        });
                    }
                }
                let position = self.leaves.leaf(identifier);
                let block = match self.strategy.reads {
                    ReadStrategy::SinglePath => {
                        self.osam
                            .read(identifier, position, self.ordered_evict, &mut self.rng)
                    }
                    ReadStrategy::MultiPath => self.osam.read_multi_paths(
                        identifier,
                        position,
                        self.ordered_evict,
                        &mut self.rng,
                    ),
                }
                .map_err(|error| SamError::Backend(error.to_string()))?;

                // A single-read address is dead once read.
                if let Some(access) = self.access.as_mut() {
                    access.remove(&identifier);
                }
                self.stats.operations.reads += 1;
                self.structure_stats_mut(structure).reads += 1;
                self.stats.write_batches = self.stats.write_batches.saturating_sub(1);
                self.observe_stash();
                block
                    .map(|value| self.codec.decode(&value.data))
                    .transpose()
            }
        }
    }

    fn write_batch(
        &mut self,
        writes: Vec<(Address, V)>,
        structure: &'static str,
    ) -> Result<(), SamError> {
        if self.strategy.writes != WriteStrategy::Local
            || writes
                .iter()
                .any(|(address, _)| matches!(address, Address::Plaintext(_)))
        {
            for (address, value) in writes {
                self.write(address, value, structure)?;
            }
            return Ok(());
        }

        let mut batch = Vec::with_capacity(writes.len());
        let mut identifiers = Vec::with_capacity(writes.len());
        for (address, value) in &writes {
            let Address::Oblivious(identifier) = *address else {
                unreachable!("plaintext writes were handled above")
            };
            self.check_write(identifier)?;
            batch.push((
                identifier,
                self.leaves.leaf(identifier),
                BlockValue::new(self.codec.encode(value)?),
            ));
            identifiers.push(identifier);
        }

        self.osam
            .local_write_batch(batch)
            .map_err(|error| SamError::Backend(error.to_string()))?;
        for identifier in identifiers {
            self.record_write(identifier);
        }
        let count =
            u64::try_from(writes.len()).map_err(|error| SamError::Backend(error.to_string()))?;
        self.stats.operations.writes += count;
        self.structure_stats_mut(structure).writes += count;
        self.stats.write_batches += count;
        self.stats.max_write_batches = self.stats.max_write_batches.max(self.stats.write_batches);
        self.observe_stash();
        Ok(())
    }

    fn retire(&mut self, _address: Address) {
        // Path OSAM+ addresses are single-read and are already consumed by read.
    }

    fn flush(&mut self, paths: usize, structure: &'static str) -> Result<(), SamError> {
        // `evict_multi_paths` evicts P paths; round up to `paths`.
        for _ in 0..paths.div_ceil(P).max(1) {
            self.osam
                .evict_multi_paths(self.ordered_evict, &mut self.rng)
                .map_err(|error| SamError::Backend(error.to_string()))?;
        }
        self.stats.flushes += 1;
        self.stats.operations.reads += 1;
        self.structure_stats_mut(structure).reads += 1;
        self.stats.write_batches = self.stats.write_batches.saturating_sub(paths as u64);
        self.observe_stash();
        Ok(())
    }

    fn stats(&self) -> &Stats {
        &self.stats
    }

    fn reset_stash_maximum(&mut self) {
        if let Some(stash) = self.stats.stash.as_mut() {
            stash.maximum = stash.current;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccessStrategy, DryRunSam};

    #[test]
    fn leaves_are_derived_from_addresses_without_a_position_map() {
        let prf = LeafPrf::new([7; 16], 1 << 12);
        let leaves = 1_u64 << prf.height;
        let mut counts = vec![0_u32; 16];
        for identifier in 1..=16_000 {
            let leaf = prf.leaf(identifier);
            assert!((leaves..2 * leaves).contains(&leaf));
            // Deterministic: the same address always maps to the same leaf.
            assert_eq!(leaf, prf.leaf(identifier));
            counts[((leaf - leaves) * 16 / leaves) as usize] += 1;
        }
        // Roughly uniform over the leaves (1,000 expected per sixteenth).
        assert!(
            counts.iter().all(|&count| (850..1150).contains(&count)),
            "{counts:?}"
        );
        // A different key gives unrelated leaves.
        let other = LeafPrf::new([8; 16], 1 << 12);
        let same = (1..=1_000)
            .filter(|&id| prf.leaf(id) == other.leaf(id))
            .count();
        assert!(same < 20);
    }

    #[test]
    fn runs_with_policy_checks_disabled() {
        use crate::pointer::{
            FixedSizeCodec, RaryCell, RaryCellValueCodec, RaryPointer, U64ValueCodec,
        };
        let mut dry = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut pointers = RaryPointer::install(&mut dry, 5_u64, 8, 4).unwrap();
        let codec = FixedSizeCodec::new(RaryCellValueCodec::new(U64ValueCodec));
        let mut sam = PathOsamSam::<RaryCell<u64>, _, 128, 4, 1>::from_snapshot(
            dry.snapshot(),
            128,
            80,
            true,
            AccessPolicy::MULTI_WRITE,
            AccessStrategy::MULTI_WRITE_RARY,
            true,
            3,
            codec,
        )
        .unwrap()
        .with_policy_checks(false);
        assert!(!sam.policy_checks());
        pointers[1].put(&mut sam, 9).unwrap();
        for pointer in &mut pointers {
            assert_eq!(pointer.get(&mut sam).unwrap(), 9);
        }
    }

    const STRUCTURE: &str = "adapter-test";

    #[test]
    fn imports_a_snapshot_and_tracks_runtime_stash() {
        let mut dry_run = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let address = dry_run.alloc(MemoryClass::Oblivious, STRUCTURE);
        dry_run.write(address, 42_u64, STRUCTURE).unwrap();
        let snapshot = dry_run.snapshot();

        let mut sam = PathOsamSam::<u64, _, 64, 4, 1>::from_snapshot(
            snapshot,
            64,
            40,
            false,
            AccessPolicy::MULTI_WRITE,
            AccessStrategy::MULTI_WRITE_RARY,
            true,
            7,
            crate::U64Codec,
        )
        .unwrap();
        assert_eq!(sam.build_stats().operations.allocations, 1);
        assert_eq!(sam.read(address, STRUCTURE).unwrap(), Some(42));
        let stash = sam.stats().stash.unwrap();
        assert_eq!(stash.samples, 1);
        assert!(sam.backend_max_stash_occupancy() >= stash.maximum);
    }

    #[test]
    fn rary_strategy_keeps_runtime_writes_local() {
        let dry_run = DryRunSam::<u64>::new(AccessPolicy::MULTI_WRITE);
        let mut sam = PathOsamSam::<u64, _, 64, 4, 1>::from_snapshot(
            dry_run.snapshot(),
            64,
            40,
            false,
            AccessPolicy::MULTI_WRITE,
            AccessStrategy::MULTI_WRITE_RARY,
            true,
            9,
            crate::U64Codec,
        )
        .unwrap();
        let address = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
        sam.write(address, 5, STRUCTURE).unwrap();
        assert_eq!(sam.stats().operations.writes, 1);
        assert_eq!(sam.stats().stash.unwrap().samples, 1);
        assert_eq!(sam.read(address, STRUCTURE).unwrap(), Some(5));
    }

    #[test]
    fn rary_strategy_batches_local_writes() {
        let dry_run = DryRunSam::<u64>::new(AccessPolicy::MULTI_WRITE);
        let mut sam = PathOsamSam::<u64, _, 64, 4, 1>::from_snapshot(
            dry_run.snapshot(),
            64,
            40,
            false,
            AccessPolicy::MULTI_WRITE,
            AccessStrategy::MULTI_WRITE_RARY,
            true,
            11,
            crate::U64Codec,
        )
        .unwrap();
        let first = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
        let second = sam.alloc(MemoryClass::Oblivious, STRUCTURE);
        sam.write_batch(vec![(first, 1), (second, 2)], STRUCTURE)
            .unwrap();
        assert_eq!(sam.stats().operations.writes, 2);
        assert_eq!(sam.stats().stash.unwrap().samples, 1);
        assert_eq!(sam.read(first, STRUCTURE).unwrap(), Some(1));
        assert_eq!(sam.read(second, STRUCTURE).unwrap(), Some(2));
    }

    #[test]
    fn encrypted_snapshot_round_trip() {
        let mut dry_run = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let address = dry_run.alloc(MemoryClass::Oblivious, STRUCTURE);
        dry_run.write(address, 99_u64, STRUCTURE).unwrap();
        let mut sam = PathOsamSam::<u64, _, 64, 4, 1>::from_snapshot(
            dry_run.snapshot(),
            64,
            40,
            true,
            AccessPolicy::MULTI_WRITE,
            AccessStrategy::MULTI_WRITE_RARY,
            true,
            13,
            crate::U64Codec,
        )
        .unwrap();
        assert_eq!(sam.read(address, STRUCTURE).unwrap(), Some(99));
    }
}
