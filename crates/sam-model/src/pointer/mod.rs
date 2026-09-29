//! Smart-pointer implementations over [`crate::SingleAccessMachine`].

mod backend;
mod balanced;
mod cache;
mod cell_codec;
mod multi_write;
mod original;
mod rary;
mod recursive;

pub use backend::{
    CacheablePointerBackend, CachedDeref, CachedRoots, MultiWritePointers, OriginalPointers,
    PointerHandle, PointerKind, RaryPointers, RawValueCells, SmartPointer, SmartPointerBackend,
};
pub use balanced::{
    balanced_deref_reads, balanced_height, balanced_update_reads, BalancedCell,
    BalancedCellValueCodec, BalancedEntry, BalancedMeta, BalancedPointer, BalancedPointers,
    BalancedWriteback,
};
pub use cache::{CacheObject, CachedPointer, CachedPointers};
pub use cell_codec::{
    FixedSizeCodec, MultiWriteCellValueCodec, OriginalCellValueCodec, RaryCellValueCodec,
    StringValueCodec, U64ValueCodec, ValueCodec,
};
pub use multi_write::{MultiWriteCell, MultiWritePointer};
pub use original::{OriginalCell, OriginalNode, OriginalPointer, OriginalWriteback};
pub use rary::{RaryCell, RaryPointer, UNRESOLVED_RARY_INDEX};
pub use recursive::{RecursivePointer, RecursivePointers};
