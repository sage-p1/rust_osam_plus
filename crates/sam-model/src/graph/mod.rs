//! The oblivious graph: fixed-size records, codecs, and the SAM-resident
//! graph itself.
//!
//! Every piece of graph state lives in the SAM: vertices and their fan-out
//! trees behind smart pointers, and the vertex lookup tables (`name -> id`
//! and `id -> entry pointer`) as oblivious AVL trees whose nodes are raw SAM
//! cells. See [`ObliviousGraph`] for the client state that remains.

mod algorithms;
mod core;
mod no_move;
mod tree;

pub use self::core::{ObliviousGraph, PRIME_WALK_LENGTH};
pub use algorithms::{PageRankResult, ShortestPathResult, SpanningTreeResult, TriangleCountResult};
pub use no_move::{NoMovePointers, Tagged, TaggedGraphPointerCodec};

use crate::{
    pointer::{
        BalancedPointer, CachedPointer, MultiWritePointer, OriginalPointer, PointerKind,
        RaryPointer, RawValueCells, RecursivePointer, ValueCodec,
    },
    structures::{AvlKey, AvlNode, Item, QueueEntry, StackEntry},
    Address, SamError,
};
use std::collections::BTreeSet;

/// Bytes occupied by a persisted graph pointer.
pub const GRAPH_POINTER_BYTES: usize = 8;
const FIXED_ENVELOPE_BYTES: usize = 4;
const FAN_OUT_METADATA_BYTES: usize = 36;
const GRAPH_METADATA_BYTES: usize = FAN_OUT_METADATA_BYTES;
/// Largest graph-structure cell payload: an AVL node with an integer key and
/// a pointer value (tag 1, key 9, item 9, children 16, balance fields 6).
/// Queue and stack entries (tag 1, next 8, `(pointer, label)` tuple 20) are
/// smaller.
const MAX_STRUCTURE_PAYLOAD_BYTES: usize = 41;

/// A pointer backend the graph can run on: its SAM cells hold graph objects
/// behind smart pointers and, as raw cells, the graph's AVL trees, queues and
/// stacks.
pub trait GraphBackend<P: Clone>: RawValueCells<GraphObject<P>, Pointer = P> {}

impl<P: Clone, B: RawValueCells<GraphObject<P>, Pointer = P>> GraphBackend<P> for B {}

/// Root-cell metadata of the balanced pointer (see `pointer::BalancedCellValueCodec`).
const BALANCED_ROOT_OVERHEAD: usize = 10;

/// Exact fixed-block layout selected for graph records.
///
/// This fanout belongs to the graph's outgoing-edge tree. It is deliberately
/// independent of [`PointerKind::MultiWriteRary::branching_factor`], which
/// controls the physical smart-pointer tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphLayout {
    block_size: usize,
    pointer_cell_overhead: usize,
    graph_branching_factor: usize,
    /// False for a [`Self::python_sized`] layout, which does not fit the block.
    exact: bool,
}

impl GraphLayout {
    /// Computes the number of eight-byte child pointers that fit after all
    /// fixed graph and pointer-cell metadata.
    pub fn for_pointer_kind(
        block_size: usize,
        pointer_kind: PointerKind,
    ) -> Result<Self, SamError> {
        let pointer_cell_overhead = match pointer_kind {
            PointerKind::Original => 41,
            PointerKind::MultiWrite | PointerKind::MultiWriteRary { .. } => 1,
            // Tag 1 + alias count 4 + height 1 + child-list address 4.
            PointerKind::Balanced { .. } => BALANCED_ROOT_OVERHEAD,
            PointerKind::Recursive => 0,
        };

        let minimum_internal_cell =
            match pointer_kind {
                PointerKind::Original | PointerKind::MultiWrite => 23,
                // Envelope 4 + tag 1 + parent 8 + group length 1 + 8 per slot.
                PointerKind::MultiWriteRary { branching_factor } => 14_usize
                    .checked_add(8_usize.checked_mul(branching_factor).ok_or_else(|| {
                        SamError::Backend("r-ary pointer cell size overflow".into())
                    })?)
                    .ok_or_else(|| SamError::Backend("r-ary pointer cell size overflow".into()))?,
                // Envelope 4 + tag 1 + parent 4 + group length 1 + 8 per member
                // (node and child-list addresses, 4 bytes each).
                PointerKind::Balanced {
                    branching_factor, ..
                } => 10_usize
                    .checked_add(8_usize.checked_mul(branching_factor).ok_or_else(|| {
                        SamError::Backend("balanced pointer cell size overflow".into())
                    })?)
                    .ok_or_else(|| SamError::Backend("balanced pointer cell size overflow".into()))?,
                PointerKind::Recursive => 0,
            };
        if block_size < minimum_internal_cell {
            return Err(SamError::Backend(format!(
                "block size {block_size} cannot hold a {pointer_kind:?} internal pointer cell requiring {minimum_internal_cell} bytes"
            )));
        }
        // The graph's AVL trees, queues and stacks are raw cells of the same
        // SAM: a one-byte cell tag (none for the recursive pointer, whose cell
        // is the value itself) around a structure payload.
        let raw_cell_tag = usize::from(pointer_kind != PointerKind::Recursive);
        let structure_cell = FIXED_ENVELOPE_BYTES + raw_cell_tag + MAX_STRUCTURE_PAYLOAD_BYTES;
        if block_size < structure_cell {
            return Err(SamError::Backend(format!(
                "block size {block_size} cannot hold the graph's SAM-resident AVL/queue/stack cells, which require {structure_cell} bytes"
            )));
        }

        let fixed = FIXED_ENVELOPE_BYTES
            .checked_add(pointer_cell_overhead)
            .and_then(|size| size.checked_add(GRAPH_METADATA_BYTES))
            .ok_or_else(|| SamError::Backend("graph block layout overflow".into()))?;
        let available = block_size.checked_sub(fixed).ok_or_else(|| {
            SamError::Backend(format!(
                "block size {block_size} leaves no room for an outgoing-edge pointer after {fixed} bytes of fixed metadata"
            ))
        })?;
        let graph_branching_factor = available / GRAPH_POINTER_BYTES;
        if graph_branching_factor == 0 {
            return Err(SamError::Backend(format!(
                "block size {block_size} leaves only {available} bytes for outgoing edges; at least {GRAPH_POINTER_BYTES} bytes are required"
            )));
        }
        Ok(Self {
            block_size,
            pointer_cell_overhead,
            graph_branching_factor,
            exact: true,
        })
    }

    /// The Python benchmark's sizing, which ignores per-cell metadata: a
    /// fanout of `floor((block_size - 16) / 8)`. Records laid out this way may
    /// not fit `block_size` once encoded, so this is only meaningful for the
    /// in-memory dry-run SAM, which never encodes blocks.
    pub fn python_sized(block_size: usize, pointer_kind: PointerKind) -> Result<Self, SamError> {
        let graph_branching_factor = block_size.saturating_sub(16) / GRAPH_POINTER_BYTES;
        if graph_branching_factor == 0 {
            return Err(SamError::Backend(format!(
                "block size {block_size} gives no Python-sized outgoing-edge slots"
            )));
        }
        let pointer_cell_overhead = match pointer_kind {
            PointerKind::Original => 41,
            PointerKind::MultiWrite | PointerKind::MultiWriteRary { .. } => 1,
            // Tag 1 + alias count 4 + height 1 + child-list address 4.
            PointerKind::Balanced { .. } => BALANCED_ROOT_OVERHEAD,
            PointerKind::Recursive => 0,
        };
        Ok(Self {
            block_size,
            pointer_cell_overhead,
            graph_branching_factor,
            exact: false,
        })
    }

    /// The same layout, marked as not fitting its block (reported as
    /// `layout=python-sized`); only meaningful for the dry-run SAM.
    pub fn marked_inexact(mut self) -> Self {
        self.exact = false;
        self
    }

    /// The same layout with a smaller outgoing-edge fanout (records then use
    /// less of the block). Useful to exercise deep fan-out trees on small
    /// graphs; `fanout` must be positive and at most the computed fanout.
    pub fn with_fanout(mut self, fanout: usize) -> Result<Self, SamError> {
        if fanout == 0 || fanout > self.graph_branching_factor {
            return Err(SamError::InvalidParameter(
                "fanout must be positive and at most the layout's fanout",
            ));
        }
        self.graph_branching_factor = fanout;
        Ok(self)
    }

    /// Whether encoded records are guaranteed to fit the block (false for a
    /// [`Self::python_sized`] layout).
    pub fn is_exact(self) -> bool {
        self.exact
    }

    /// Configured OSAM block size.
    pub fn block_size(self) -> usize {
        self.block_size
    }

    /// Bytes used by the root representation of the selected pointer type.
    pub fn pointer_cell_overhead(self) -> usize {
        self.pointer_cell_overhead
    }

    /// Maximum child pointers in a vertex or fanout graph record.
    pub fn graph_branching_factor(self) -> usize {
        self.graph_branching_factor
    }

    /// The unattainable upper bound before metadata, useful in reports.
    pub fn ideal_pointer_slots(self) -> usize {
        self.block_size / GRAPH_POINTER_BYTES
    }
}

/// A graph vertex stored behind a smart pointer. `out_children` holds the
/// top level of its fan-out tree of height `height` (see [`ObliviousGraph`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Vertex<P> {
    pub id: u64,
    /// The occupied prefix of the top-level slots (leaves are packed to the
    /// left, so occupied slots are always a prefix). Encoding pads it to the
    /// layout's `fanout` slots; decoding trims the empty tail.
    pub out_children: Vec<Option<P>>,
    /// The graph fanout: slots per record in the block layout. In memory
    /// only; the codec restores it from its layout.
    pub fanout: usize,
    pub out_degree: u64,
    pub height: u64,
    /// `visited` and `label` are not used by the algorithms (traversals keep
    /// their visited set on the client). They are kept so records have the
    /// Python layout (36 bytes of metadata), which decides the graph fanout
    /// at each block size.
    pub visited: bool,
    pub label: Option<i64>,
}

/// Leaf or internal node in an outgoing-edge tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FanOut<P> {
    /// The occupied prefix of the child slots (empty for a leaf); see
    /// [`Vertex::out_children`].
    pub children: Vec<Option<P>>,
    pub edge: Option<P>,
    pub weight: i64,
    pub destination: Option<u64>,
    pub is_leaf: bool,
    pub label: Option<i64>,
}

impl<P> Vertex<P> {
    /// A vertex without outgoing edges.
    pub fn new(id: u64, fanout: usize) -> Self {
        Self {
            id,
            out_children: Vec::new(),
            fanout,
            out_degree: 0,
            height: 0,
            visited: false,
            label: None,
        }
    }
}

impl<P> FanOut<P> {
    /// A leaf holding one outgoing edge: an alias of the destination's pointer.
    pub fn leaf(edge: P, weight: i64, destination: u64) -> Self {
        Self {
            children: Vec::new(),
            edge: Some(edge),
            weight,
            destination: Some(destination),
            is_leaf: true,
            label: None,
        }
    }

    /// An internal node with the given child slots.
    pub fn internal(children: Vec<Option<P>>) -> Self {
        Self {
            children,
            edge: None,
            weight: 0,
            destination: None,
            is_leaf: false,
            label: None,
        }
    }
}

/// Tombstone left behind when a vertex is removed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeletedObject {
    pub id: Option<u64>,
}

/// Every payload shape used by the oblivious graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphObject<P> {
    Vertex(Vertex<P>),
    FanOut(FanOut<P>),
    Deleted(DeletedObject),
    /// A raw `SmartQueue` cell (not behind a smart pointer).
    Queue(QueueEntry<P>),
    /// A raw `SmartStack` cell.
    Stack(StackEntry<P>),
    /// A raw `SmartAVLTree` node.
    Avl(Box<AvlNode<P>>),
}

/// Converts a client pointer handle to one persisted eight-byte identifier.
pub trait GraphPointerCodec<P>: Clone {
    fn identifier(&self, pointer: &P) -> Result<u64, SamError>;
    fn decode_identifier(&self, identifier: u64) -> Result<P, SamError>;
}

macro_rules! address_pointer_codec {
    ($codec:ident, $pointer:ty) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $codec;

        impl GraphPointerCodec<$pointer> for $codec {
            fn identifier(&self, pointer: &$pointer) -> Result<u64, SamError> {
                oblivious_identifier(pointer.head())
            }

            fn decode_identifier(&self, identifier: u64) -> Result<$pointer, SamError> {
                Ok(<$pointer>::from_persisted_head(Address::Oblivious(
                    checked_identifier(identifier)?,
                )))
            }
        }
    };
}

address_pointer_codec!(MultiWriteGraphPointerCodec, MultiWritePointer);
address_pointer_codec!(OriginalGraphPointerCodec, OriginalPointer);
address_pointer_codec!(RecursiveGraphPointerCodec, RecursivePointer);

/// Graph-pointer codec for the balanced pointer, whose handle is one address.
#[derive(Clone, Copy, Debug, Default)]
pub struct BalancedGraphPointerCodec;

impl GraphPointerCodec<BalancedPointer> for BalancedGraphPointerCodec {
    fn identifier(&self, pointer: &BalancedPointer) -> Result<u64, SamError> {
        oblivious_identifier(pointer.head())
    }

    fn decode_identifier(&self, identifier: u64) -> Result<BalancedPointer, SamError> {
        Ok(BalancedPointer::from_head(Address::Oblivious(
            checked_identifier(identifier)?,
        )))
    }
}

/// Graph-pointer codec retaining the separately configured r-ary pointer fanout.
#[derive(Clone, Copy, Debug)]
pub struct RaryGraphPointerCodec {
    pointer_branching_factor: usize,
}

impl RaryGraphPointerCodec {
    pub fn new(pointer_branching_factor: usize) -> Self {
        Self {
            pointer_branching_factor,
        }
    }
}

impl GraphPointerCodec<RaryPointer> for RaryGraphPointerCodec {
    fn identifier(&self, pointer: &RaryPointer) -> Result<u64, SamError> {
        if pointer.branching_factor() != self.pointer_branching_factor {
            return Err(SamError::Backend(
                "r-ary pointer fanout does not match its graph codec".into(),
            ));
        }
        oblivious_identifier(pointer.head())
    }

    fn decode_identifier(&self, identifier: u64) -> Result<RaryPointer, SamError> {
        RaryPointer::from_persisted_head(
            Address::Oblivious(checked_identifier(identifier)?),
            self.pointer_branching_factor,
        )
    }
}

/// Cached graph pointers persist as the raw backend pointer they wrap, using
/// that backend's codec: stored cells hold real SAM⁺ pointers, and the cache
/// keeps no per-pointer client state.
#[derive(Clone, Copy, Debug, Default)]
pub struct CachedGraphPointerCodec<C> {
    raw: C,
}

impl<C> CachedGraphPointerCodec<C> {
    pub fn new(raw: C) -> Self {
        Self { raw }
    }
}

impl<P, C: GraphPointerCodec<P>> GraphPointerCodec<CachedPointer<P>>
    for CachedGraphPointerCodec<C>
{
    fn identifier(&self, pointer: &CachedPointer<P>) -> Result<u64, SamError> {
        let raw = pointer
            .raw()
            .ok_or(SamError::InvalidPointerCell("deleted cached graph pointer"))?;
        self.raw.identifier(raw)
    }

    fn decode_identifier(&self, identifier: u64) -> Result<CachedPointer<P>, SamError> {
        Ok(CachedPointer::from_raw(
            self.raw.decode_identifier(identifier)?,
        ))
    }
}

/// Variable-size graph encoding placed inside [`crate::pointer::FixedSizeCodec`].
#[derive(Clone, Debug)]
pub struct GraphValueCodec<C> {
    pointers: C,
    graph_branching_factor: usize,
}

impl<C> GraphValueCodec<C> {
    pub fn new(pointers: C, graph_branching_factor: usize) -> Result<Self, SamError> {
        if graph_branching_factor == 0 {
            return Err(SamError::InvalidParameter(
                "graph branching factor must be positive",
            ));
        }
        Ok(Self {
            pointers,
            graph_branching_factor,
        })
    }

    pub fn graph_branching_factor(&self) -> usize {
        self.graph_branching_factor
    }
}

impl<P, C: GraphPointerCodec<P>> ValueCodec<GraphObject<P>> for GraphValueCodec<C> {
    fn encode_value(&self, value: &GraphObject<P>, output: &mut Vec<u8>) -> Result<(), SamError> {
        match value {
            GraphObject::Vertex(vertex) => {
                check_children(&vertex.out_children, self.graph_branching_factor)?;
                output.push(0);
                put_u64(vertex.id, output);
                put_u64(vertex.out_degree, output);
                put_u64(vertex.height, output);
                output.push(u8::from(vertex.visited));
                put_option_i64(vertex.label, output);
                self.put_children(&vertex.out_children, output)?;
            }
            GraphObject::FanOut(fan_out) => {
                check_children(&fan_out.children, self.graph_branching_factor)?;
                output.push(1);
                self.put_pointer(&fan_out.edge, output)?;
                output.extend_from_slice(&fan_out.weight.to_le_bytes());
                put_option_u64(fan_out.destination, output);
                output.push(u8::from(fan_out.is_leaf));
                put_option_i64(fan_out.label, output);
                self.put_children(&fan_out.children, output)?;
            }
            GraphObject::Deleted(deleted) => {
                output.push(2);
                put_option_u64(deleted.id, output);
            }
            GraphObject::Queue(entry) => {
                output.push(3);
                put_u64(address_identifier(entry.next)?, output);
                self.put_item(&entry.value, output)?;
            }
            GraphObject::Stack(entry) => {
                output.push(4);
                put_u64(
                    entry.next.map(address_identifier).transpose()?.unwrap_or(0),
                    output,
                );
                self.put_item(&entry.value, output)?;
            }
            GraphObject::Avl(node) => {
                output.push(5);
                match &node.key {
                    AvlKey::Hash(hash) => {
                        output.push(0);
                        output.extend_from_slice(hash);
                    }
                    AvlKey::Int(key) => {
                        output.push(1);
                        output.extend_from_slice(&key.to_le_bytes());
                    }
                }
                self.put_item(&node.value, output)?;
                for child in node.children {
                    put_u64(
                        child.map(address_identifier).transpose()?.unwrap_or(0),
                        output,
                    );
                }
                // Heights and balances are tiny: one signed byte each, with
                // i8::MIN for an absent child balance.
                for field in [
                    node.height,
                    node.left_height,
                    node.right_height,
                    node.balance,
                ] {
                    put_small(Some(field), output)?;
                }
                put_small(node.left_balance, output)?;
                put_small(node.right_balance, output)?;
            }
        }
        Ok(())
    }

    fn decode_value(&self, input: &mut &[u8]) -> Result<GraphObject<P>, SamError> {
        match get_u8(input)? {
            0 => Ok(GraphObject::Vertex(Vertex {
                id: get_u64(input)?,
                out_degree: get_u64(input)?,
                height: get_u64(input)?,
                visited: get_bool(input)?,
                label: get_option_i64(input)?,
                out_children: self.get_children(input)?,
                fanout: self.graph_branching_factor,
            })),
            1 => Ok(GraphObject::FanOut(FanOut {
                edge: self.get_pointer(input)?,
                weight: get_i64(input)?,
                destination: get_option_u64(input)?,
                is_leaf: get_bool(input)?,
                label: get_option_i64(input)?,
                children: self.get_children(input)?,
            })),
            2 => Ok(GraphObject::Deleted(DeletedObject {
                id: get_option_u64(input)?,
            })),
            3 => Ok(GraphObject::Queue(QueueEntry {
                next: identifier_address(get_u64(input)?)?
                    .ok_or_else(|| SamError::Backend("queue entry without a next cell".into()))?,
                value: self.get_item(input)?,
            })),
            4 => Ok(GraphObject::Stack(StackEntry {
                next: identifier_address(get_u64(input)?)?,
                value: self.get_item(input)?,
            })),
            5 => {
                let key = match get_u8(input)? {
                    0 => {
                        let mut hash = [0_u8; 32];
                        for byte in &mut hash {
                            *byte = get_u8(input)?;
                        }
                        AvlKey::Hash(hash)
                    }
                    1 => AvlKey::Int(get_i64(input)?),
                    _ => return Err(SamError::Backend("invalid AVL key tag".into())),
                };
                let value = self.get_item(input)?;
                let children = [
                    identifier_address(get_u64(input)?)?,
                    identifier_address(get_u64(input)?)?,
                ];
                let mut field = || {
                    get_small(input)?
                        .ok_or_else(|| SamError::Backend("absent AVL height field".into()))
                };
                let (height, left_height, right_height, balance) =
                    (field()?, field()?, field()?, field()?);
                Ok(GraphObject::Avl(Box::new(AvlNode {
                    key,
                    value,
                    children,
                    height,
                    left_height,
                    right_height,
                    balance,
                    left_balance: get_small(input)?,
                    right_balance: get_small(input)?,
                })))
            }
            _ => Err(SamError::Backend("invalid graph object tag".into())),
        }
    }
}

impl<C> GraphValueCodec<C> {
    fn put_item<P>(&self, item: &Item<P>, output: &mut Vec<u8>) -> Result<(), SamError>
    where
        C: GraphPointerCodec<P>,
    {
        match item {
            Item::None => output.push(0),
            Item::Int(value) => {
                output.push(1);
                output.extend_from_slice(&value.to_le_bytes());
            }
            Item::Pointer(pointer) => {
                output.push(2);
                put_u64(
                    checked_identifier(self.pointers.identifier(pointer)?)?,
                    output,
                );
            }
            Item::Tuple(items) => {
                output.push(3);
                output.push(
                    u8::try_from(items.len())
                        .map_err(|_| SamError::Backend("item tuple too long".into()))?,
                );
                for item in items {
                    self.put_item(item, output)?;
                }
            }
        }
        Ok(())
    }

    fn get_item<P>(&self, input: &mut &[u8]) -> Result<Item<P>, SamError>
    where
        C: GraphPointerCodec<P>,
    {
        Ok(match get_u8(input)? {
            0 => Item::None,
            1 => Item::Int(get_i64(input)?),
            2 => Item::Pointer(self.pointers.decode_identifier(get_u64(input)?)?),
            3 => {
                let length = usize::from(get_u8(input)?);
                let mut items = Vec::with_capacity(length);
                for _ in 0..length {
                    items.push(self.get_item(input)?);
                }
                Item::Tuple(items)
            }
            _ => return Err(SamError::Backend("invalid item tag".into())),
        })
    }

    fn put_pointer<P>(&self, pointer: &Option<P>, output: &mut Vec<u8>) -> Result<(), SamError>
    where
        C: GraphPointerCodec<P>,
    {
        let identifier = match pointer {
            Some(pointer) => checked_identifier(self.pointers.identifier(pointer)?)?,
            None => 0,
        };
        put_u64(identifier, output);
        Ok(())
    }

    fn get_pointer<P>(&self, input: &mut &[u8]) -> Result<Option<P>, SamError>
    where
        C: GraphPointerCodec<P>,
    {
        match get_u64(input)? {
            0 => Ok(None),
            identifier => self.pointers.decode_identifier(identifier).map(Some),
        }
    }

    fn put_children<P>(&self, children: &[Option<P>], output: &mut Vec<u8>) -> Result<(), SamError>
    where
        C: GraphPointerCodec<P>,
    {
        // Records keep only their occupied prefix in memory; blocks always
        // hold the layout's full slot count.
        for child in children {
            self.put_pointer(child, output)?;
        }
        for _ in children.len()..self.graph_branching_factor {
            self.put_pointer::<P>(&None, output)?;
        }
        Ok(())
    }

    fn get_children<P>(&self, input: &mut &[u8]) -> Result<Vec<Option<P>>, SamError>
    where
        C: GraphPointerCodec<P>,
    {
        let mut children = (0..self.graph_branching_factor)
            .map(|_| self.get_pointer(input))
            .collect::<Result<Vec<_>, _>>()?;
        while matches!(children.last(), Some(None)) {
            children.pop();
        }
        Ok(children)
    }
}

/// One weighted, directed input edge. Python can generate and serialize this
/// compact representation without participating in the benchmark itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeightedEdge {
    pub source: u64,
    pub destination: u64,
    pub weight: i64,
}

/// Native graph input, including isolated vertices.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphInput {
    pub vertices: Vec<u64>,
    pub edges: Vec<WeightedEdge>,
}

impl GraphInput {
    pub fn new(vertices: impl IntoIterator<Item = u64>, edges: Vec<WeightedEdge>) -> Self {
        let mut all = vertices.into_iter().collect::<BTreeSet<_>>();
        for edge in &edges {
            all.insert(edge.source);
            all.insert(edge.destination);
        }
        Self {
            vertices: all.into_iter().collect(),
            edges,
        }
    }
}

fn check_children<P>(children: &[Option<P>], expected: usize) -> Result<(), SamError> {
    if children.len() > expected {
        return Err(SamError::Backend(format!(
            "graph record has {} child slots but its block layout holds {expected}",
            children.len()
        )));
    }
    Ok(())
}

fn oblivious_identifier(head: Option<Address>) -> Result<u64, SamError> {
    match head {
        Some(Address::Oblivious(identifier)) => checked_identifier(identifier),
        Some(Address::Plaintext(_)) => Err(SamError::Backend(
            "graph pointers must use oblivious addresses".into(),
        )),
        None => Err(SamError::InvalidPointerCell("deleted graph pointer")),
    }
}

/// Eight-byte identifier of an oblivious address stored in a structure cell.
fn address_identifier(address: Address) -> Result<u64, SamError> {
    match address {
        Address::Oblivious(identifier) => checked_identifier(identifier),
        Address::Plaintext(_) => Err(SamError::Backend(
            "structure cells hold only oblivious addresses".into(),
        )),
    }
}

/// Inverse of [`address_identifier`], with zero meaning no address.
fn identifier_address(identifier: u64) -> Result<Option<Address>, SamError> {
    Ok((identifier != 0).then_some(Address::Oblivious(identifier)))
}

fn checked_identifier(identifier: u64) -> Result<u64, SamError> {
    if identifier == 0 {
        Err(SamError::Backend(
            "zero is reserved for an absent graph pointer".into(),
        ))
    } else {
        Ok(identifier)
    }
}

fn take<'a>(input: &mut &'a [u8], length: usize) -> Result<&'a [u8], SamError> {
    if input.len() < length {
        return Err(SamError::Backend("truncated graph encoding".into()));
    }
    let (head, tail) = input.split_at(length);
    *input = tail;
    Ok(head)
}

fn put_u64(value: u64, output: &mut Vec<u8>) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn get_u8(input: &mut &[u8]) -> Result<u8, SamError> {
    Ok(take(input, 1)?[0])
}

fn get_u64(input: &mut &[u8]) -> Result<u64, SamError> {
    Ok(u64::from_le_bytes(take(input, 8)?.try_into().unwrap()))
}

fn get_i64(input: &mut &[u8]) -> Result<i64, SamError> {
    Ok(i64::from_le_bytes(take(input, 8)?.try_into().unwrap()))
}

fn get_bool(input: &mut &[u8]) -> Result<bool, SamError> {
    match get_u8(input)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(SamError::Backend("invalid graph boolean".into())),
    }
}

fn put_option_u64(value: Option<u64>, output: &mut Vec<u8>) {
    match value {
        Some(value) => {
            output.push(1);
            put_u64(value, output);
        }
        None => output.extend_from_slice(&[0; 9]),
    }
}

fn get_option_u64(input: &mut &[u8]) -> Result<Option<u64>, SamError> {
    let present = get_u8(input)?;
    let value = get_u64(input)?;
    match present {
        0 => Ok(None),
        1 => Ok(Some(value)),
        _ => Err(SamError::Backend("invalid graph option tag".into())),
    }
}

/// One signed byte; `None` is encoded as `i8::MIN`.
fn put_small(value: Option<i64>, output: &mut Vec<u8>) -> Result<(), SamError> {
    let byte = match value {
        Some(value) => i8::try_from(value)
            .ok()
            .filter(|byte| *byte != i8::MIN)
            .ok_or_else(|| SamError::Backend(format!("AVL field {value} exceeds one byte")))?,
        None => i8::MIN,
    };
    output.push(byte as u8);
    Ok(())
}

fn get_small(input: &mut &[u8]) -> Result<Option<i64>, SamError> {
    let byte = get_u8(input)? as i8;
    Ok((byte != i8::MIN).then_some(i64::from(byte)))
}

fn put_option_i64(value: Option<i64>, output: &mut Vec<u8>) {
    match value {
        Some(value) => {
            output.push(1);
            output.extend_from_slice(&value.to_le_bytes());
        }
        None => output.extend_from_slice(&[0; 9]),
    }
}

fn get_option_i64(input: &mut &[u8]) -> Result<Option<i64>, SamError> {
    let present = get_u8(input)?;
    let value = get_i64(input)?;
    match present {
        0 => Ok(None),
        1 => Ok(Some(value)),
        _ => Err(SamError::Backend("invalid graph option tag".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        pointer::{
            FixedSizeCodec, MultiWriteCell, MultiWriteCellValueCodec, OriginalCell,
            OriginalCellValueCodec, RaryCell, RaryCellValueCodec,
        },
        BlockCodec,
    };

    #[test]
    fn graph_fanout_uses_remaining_bytes_and_not_rary_pointer_fanout() {
        let layout = GraphLayout::for_pointer_kind(
            4096,
            PointerKind::MultiWriteRary {
                branching_factor: 64,
            },
        )
        .unwrap();
        assert_eq!(layout.graph_branching_factor(), 506);
        assert_eq!(layout.ideal_pointer_slots(), 512);
        assert_eq!(layout.with_fanout(3).unwrap().graph_branching_factor(), 3);
        assert!(layout.with_fanout(507).is_err());
    }

    #[test]
    fn layout_errors_when_one_outgoing_pointer_does_not_fit() {
        let error = GraphLayout::for_pointer_kind(88, PointerKind::Original).unwrap_err();
        assert!(error.to_string().contains("at least 8 bytes"));
        assert_eq!(
            GraphLayout::for_pointer_kind(89, PointerKind::Original)
                .unwrap()
                .graph_branching_factor(),
            1
        );
    }

    #[test]
    fn graph_records_fill_the_computed_fixed_block() {
        let layout = GraphLayout::for_pointer_kind(64, PointerKind::MultiWrite).unwrap();
        assert_eq!(layout.graph_branching_factor(), 2);
        let codec = FixedSizeCodec::new(MultiWriteCellValueCodec::new(
            GraphValueCodec::new(MultiWriteGraphPointerCodec, layout.graph_branching_factor())
                .unwrap(),
        ));
        // In memory a record keeps only its occupied slots; the block holds
        // all `graph_branching_factor` of them.
        let value = MultiWriteCell::Root(GraphObject::Vertex(Vertex {
            id: 4,
            out_children: vec![Some(MultiWritePointer::from_persisted_head(
                Address::Oblivious(7),
            ))],
            fanout: 2,
            out_degree: 1,
            height: 1,
            visited: false,
            label: Some(9),
        }));
        let block: [u8; 64] = codec.encode(&value).unwrap();
        assert_eq!(codec.decode(&block).unwrap(), value);
        // A record padded in memory encodes to the same block.
        let MultiWriteCell::Root(GraphObject::Vertex(mut padded)) = value.clone() else {
            unreachable!()
        };
        padded.out_children.push(None);
        let padded = MultiWriteCell::Root(GraphObject::Vertex(padded));
        assert_eq!(codec.encode(&padded).unwrap(), block);
        // More slots than the layout holds is an error.
        let MultiWriteCell::Root(GraphObject::Vertex(mut wide)) = value else {
            unreachable!()
        };
        wide.out_children.extend([None, None]);
        let wide: Result<[u8; 64], _> =
            codec.encode(&MultiWriteCell::Root(GraphObject::Vertex(wide)));
        assert!(wide.is_err());
    }

    /// Every structure cell the graph writes, with a pointer item.
    fn structure_objects<P: Clone>(pointer: P) -> Vec<GraphObject<P>> {
        let entry = Item::Tuple(vec![Item::Pointer(pointer.clone()), Item::Int(-3)]);
        vec![
            GraphObject::Queue(QueueEntry {
                next: Address::Oblivious(5),
                value: entry.clone(),
            }),
            GraphObject::Stack(StackEntry {
                value: entry,
                next: None,
            }),
            GraphObject::Avl(Box::new(AvlNode {
                key: AvlKey::Int(i64::MAX),
                value: Item::Pointer(pointer),
                children: [Some(Address::Oblivious(9)), None],
                height: 2,
                left_height: 1,
                right_height: 0,
                balance: 1,
                left_balance: Some(0),
                right_balance: None,
            })),
            GraphObject::Deleted(DeletedObject { id: Some(3) }),
        ]
    }

    fn round_trip<V, C, const B: usize>(codec: &C, cells: Vec<V>)
    where
        V: Clone + Eq + std::fmt::Debug,
        C: BlockCodec<V, B>,
    {
        for cell in cells {
            let block: [u8; B] = codec.encode(&cell).unwrap();
            assert_eq!(codec.decode(&block).unwrap(), cell);
        }
    }

    #[test]
    fn structure_cells_with_pointer_items_round_trip_through_every_pointer_codec() {
        let at = Address::Oblivious(77);
        // The smallest block each backend's graph layout accepts.
        let multiwrite = MultiWritePointer::from_persisted_head(at);
        round_trip::<_, _, 64>(
            &FixedSizeCodec::new(MultiWriteCellValueCodec::new(
                GraphValueCodec::new(MultiWriteGraphPointerCodec, 2).unwrap(),
            )),
            structure_objects(multiwrite)
                .into_iter()
                .map(MultiWriteCell::Root)
                .collect(),
        );
        round_trip::<_, _, 64>(
            &FixedSizeCodec::new(MultiWriteCellValueCodec::new(
                GraphValueCodec::new(CachedGraphPointerCodec::new(MultiWriteGraphPointerCodec), 2)
                    .unwrap(),
            )),
            structure_objects(CachedPointer::from_raw(multiwrite))
                .into_iter()
                .map(MultiWriteCell::Root)
                .collect(),
        );
        let rary = RaryPointer::from_persisted_head(at, 4).unwrap();
        round_trip::<_, _, 64>(
            &FixedSizeCodec::new(RaryCellValueCodec::new(
                GraphValueCodec::new(RaryGraphPointerCodec::new(4), 2).unwrap(),
            )),
            structure_objects(rary.clone())
                .into_iter()
                .map(RaryCell::Root)
                .collect(),
        );
        round_trip::<_, _, 64>(
            &FixedSizeCodec::new(RaryCellValueCodec::new(
                GraphValueCodec::new(
                    CachedGraphPointerCodec::new(RaryGraphPointerCodec::new(4)),
                    2,
                )
                .unwrap(),
            )),
            structure_objects(CachedPointer::from_raw(rary))
                .into_iter()
                .map(RaryCell::Root)
                .collect(),
        );
        let original = OriginalPointer::from_persisted_head(at);
        round_trip::<_, _, 89>(
            &FixedSizeCodec::new(OriginalCellValueCodec::new(
                GraphValueCodec::new(OriginalGraphPointerCodec, 1).unwrap(),
            )),
            structure_objects(original)
                .into_iter()
                .map(OriginalCell::Raw)
                .collect(),
        );
        let recursive = RecursivePointer::from_persisted_head(at);
        round_trip::<_, _, 48>(
            &FixedSizeCodec::new(GraphValueCodec::new(RecursiveGraphPointerCodec, 1).unwrap()),
            structure_objects(recursive),
        );
    }

    #[test]
    fn structure_cells_that_cannot_fit_are_rejected_clearly() {
        // Every layout-accepted block holds the largest structure cell ...
        for kind in [
            PointerKind::Original,
            PointerKind::MultiWrite,
            PointerKind::MultiWriteRary {
                branching_factor: 2,
            },
            PointerKind::Recursive,
        ] {
            for block_size in 1..200 {
                if GraphLayout::for_pointer_kind(block_size, kind).is_ok() {
                    let tag = usize::from(kind != PointerKind::Recursive);
                    assert!(block_size >= FIXED_ENVELOPE_BYTES + tag + MAX_STRUCTURE_PAYLOAD_BYTES);
                }
            }
        }
        // ... and an oversized cell fails with an explicit size error.
        let codec = FixedSizeCodec::new(MultiWriteCellValueCodec::new(
            GraphValueCodec::new(MultiWriteGraphPointerCodec, 2).unwrap(),
        ));
        let hashed =
            MultiWriteCell::Root(GraphObject::<MultiWritePointer>::Avl(Box::new(AvlNode {
                key: AvlKey::Hash([7; 32]),
                value: Item::Int(1),
                children: [None, None],
                height: 1,
                left_height: 0,
                right_height: 0,
                balance: 0,
                left_balance: None,
                right_balance: None,
            })));
        let error = BlockCodec::<_, 64>::encode(&codec, &hashed).unwrap_err();
        assert!(error.to_string().contains("requires"), "{error}");
    }
}
