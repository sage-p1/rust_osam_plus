//! Native data-structure model for SAM and OSAM experiments.
//!
//! This crate deliberately separates the logical SAM interface from the
//! cryptographic backend. Graph construction can use [`DryRunSam`] at native
//! memory speed. Its live snapshot uses stable logical identifiers so a later
//! adapter can bulk-load the completed state into `PathOsamPlus` without
//! rewriting addresses embedded in pointer cells.

mod codec;
mod dry_run;
mod error;
mod graph;
mod path_osam;
mod read_evicted;
mod sam;
pub mod structures;

pub mod pointer;

pub use codec::{BlockCodec, U64Codec};
pub use dry_run::DryRunSam;
pub use error::SamError;
pub use graph::{
    BalancedGraphPointerCodec, CachedGraphPointerCodec, DeletedObject, FanOut, GraphBackend, GraphInput, GraphLayout,
    GraphObject, GraphPointerCodec, GraphValueCodec, MultiWriteGraphPointerCodec, NoMovePointers,
    ObliviousGraph, OriginalGraphPointerCodec, PageRankResult, RaryGraphPointerCodec,
    RecursiveGraphPointerCodec, ShortestPathResult, SpanningTreeResult, Tagged,
    TaggedGraphPointerCodec, TriangleCountResult, Vertex, WeightedEdge, GRAPH_POINTER_BYTES,
    PRIME_WALK_LENGTH,
};
pub use path_osam::PathOsamSam;
pub use read_evicted::{ReadEvictedWrites, FLUSH_STRUCTURE};
pub use sam::{
    AccessPolicy, AccessStrategy, Address, MemoryClass, OperationCounts, ReadStrategy, SamSnapshot,
    SingleAccessMachine, SnapshotBlock, StashStats, Stats, StructureStats, WriteStrategy,
};
