//! Fan-out trees: the outgoing edges of one vertex.
//!
//! A vertex with out-degree `d` and graph fanout `bf` has a tree of height
//! `h = tree_height(d)`: 0 without edges, otherwise the smallest `h >= 1` with
//! `bf^h >= d`. Leaf `i` sits at the base-`bf` digits of `i`, most significant
//! first: slot `digit[0]` of the vertex's `out_children`, then slot
//! `digit[1]` of that internal node's `children`, and so on. Leaves are
//! therefore packed to the left and the tree is balanced; every function
//! here preserves that invariant.
//!
//! In memory a record keeps only the occupied prefix of its slots (a
//! left-packed tree has no gaps), so the functions here grow and shrink the
//! slot lists; the codec pads them to the block layout.
//!
//! All functions operate on a vertex that the caller holds inside a scoped
//! pointer access (`with_value`), so the vertex is written back once, with
//! its updated slots, when the caller's access ends. Tree nodes are reached
//! only by following the pointers stored in the records.
//!
//! While a vertex is held, its own aliases must not be dereferenced, copied
//! or deleted: their paths lead to the value being accessed. So a self-loop
//! edge is never touched here; scans hand it out without a pointer (the
//! caller aliases the vertex once the access ends), and deletions return
//! such edges to the caller instead of deleting them.

use super::{FanOut, GraphBackend, GraphObject, Vertex};
use crate::{
    structures::{queue::SmartQueue, Item},
    MemoryClass, SamError, SingleAccessMachine,
};

type Result<T> = std::result::Result<T, SamError>;

/// One outgoing edge handed out by a scan.
pub(super) struct Neighbor<P> {
    /// A fresh alias of the destination's pointer, when requested and the
    /// edge is not a self-loop.
    pub pointer: Option<P>,
    pub destination: u64,
    pub weight: i64,
}

/// Callback receiving the neighbors of a scan, in leaf order.
pub(super) type NeighborSink<'a, P, B, S> =
    dyn FnMut(Neighbor<P>, &mut B, &mut S) -> Result<()> + 'a;

/// Callback visiting leaves; returns whether to continue.
type LeafVisitor<'a, P, B, S> = dyn FnMut(&mut FanOut<P>, &mut B, &mut S) -> Result<bool> + 'a;

/// Height of a tree holding `out_degree` leaves.
pub(super) fn tree_height(out_degree: u64, fanout: usize) -> u64 {
    if out_degree == 0 {
        return 0;
    }
    let fanout = fanout.max(2) as u64;
    let mut capacity = fanout;
    let mut height = 1;
    while capacity < out_degree {
        capacity = capacity.saturating_mul(fanout);
        height += 1;
    }
    height
}

/// `fanout^height`, the number of leaves a tree of that height holds.
fn capacity(height: u64, fanout: usize) -> u64 {
    (0..height).fold(1_u64, |capacity, _| capacity.saturating_mul(fanout as u64))
}

/// Base-`fanout` digits of `index`, most significant first.
fn digits(index: u64, height: u64, fanout: usize) -> Vec<usize> {
    let mut digits = vec![0; height as usize];
    let mut rest = index;
    for digit in digits.iter_mut().rev() {
        *digit = (rest % fanout as u64) as usize;
        rest /= fanout as u64;
    }
    digits
}

fn corrupt(message: &'static str) -> SamError {
    SamError::InvalidPointerCell(message)
}

pub(super) fn vertex_mut<P>(object: &mut GraphObject<P>) -> Result<&mut Vertex<P>> {
    match object {
        GraphObject::Vertex(vertex) => Ok(vertex),
        _ => Err(corrupt("expected a vertex")),
    }
}

fn node_mut<P>(object: &mut GraphObject<P>, leaf: bool) -> Result<&mut FanOut<P>> {
    match object {
        GraphObject::FanOut(fan_out) if fan_out.is_leaf == leaf => Ok(fan_out),
        _ if leaf => Err(corrupt("expected a fan-out leaf")),
        _ => Err(corrupt("expected an internal fan-out node")),
    }
}

/// Stores `leaf` at the slot named by `digits`, creating the internal nodes
/// of a new (leftmost-first) subtree on the way.
fn place<P, B, S>(
    children: &mut Vec<Option<P>>,
    digits: &[usize],
    leaf: P,
    backend: &mut B,
    sam: &mut S,
) -> Result<()>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    let (&first, rest) = digits.split_first().ok_or(corrupt("empty fan-out path"))?;
    if first == children.len() {
        children.push(None);
    }
    let slot = children
        .get_mut(first)
        .ok_or(corrupt("fan-out tree is not packed to the left"))?;
    if rest.is_empty() {
        if slot.is_some() {
            return Err(corrupt("fan-out slot is already occupied"));
        }
        *slot = Some(leaf);
        return Ok(());
    }
    if let Some(child) = slot.as_mut() {
        return backend.with_value(sam, child, |object, backend, sam| {
            place(
                &mut node_mut(object, false)?.children,
                rest,
                leaf,
                backend,
                sam,
            )
        });
    }
    // The first leaf of a new subtree: its remaining digits are all zero.
    if rest.iter().any(|digit| *digit != 0) {
        return Err(corrupt("fan-out tree is not packed to the left"));
    }
    let mut node = leaf;
    for _ in rest {
        node = backend.new_pointer(sam, GraphObject::FanOut(FanOut::internal(vec![Some(node)])))?;
    }
    *slot = Some(node);
    Ok(())
}

/// Removes and returns the leaf pointer at `digits`, deleting internal nodes
/// that become empty.
fn take<P, B, S>(
    children: &mut Vec<Option<P>>,
    digits: &[usize],
    backend: &mut B,
    sam: &mut S,
) -> Result<P>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    let (&first, rest) = digits.split_first().ok_or(corrupt("empty fan-out path"))?;
    let slot = children
        .get_mut(first)
        .ok_or(corrupt("missing fan-out slot"))?;
    let leaf = if rest.is_empty() {
        slot.take().ok_or(corrupt("missing fan-out leaf"))?
    } else {
        let child = slot.as_mut().ok_or(corrupt("missing fan-out node"))?;
        let (leaf, empty) = backend.with_value(sam, child, |object, backend, sam| {
            let node = node_mut(object, false)?;
            let leaf = take(&mut node.children, rest, backend, sam)?;
            Ok((leaf, node.children.is_empty()))
        })?;
        if empty {
            let mut node = slot.take().ok_or(corrupt("missing fan-out node"))?;
            backend.delete(sam, &mut node)?;
        }
        leaf
    };
    // Keep only the occupied prefix (a removed last leaf leaves a gap at the end).
    while matches!(children.last(), Some(None)) {
        children.pop();
    }
    Ok(leaf)
}

/// Runs `operation` on the slot named by `digits`.
fn with_slot<P, B, S, T, F>(
    children: &mut [Option<P>],
    digits: &[usize],
    backend: &mut B,
    sam: &mut S,
    operation: F,
) -> Result<T>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
    F: FnOnce(&mut Option<P>, &mut B, &mut S) -> Result<T>,
{
    let (&first, rest) = digits.split_first().ok_or(corrupt("empty fan-out path"))?;
    let slot = children
        .get_mut(first)
        .ok_or(corrupt("missing fan-out slot"))?;
    if rest.is_empty() {
        return operation(slot, backend, sam);
    }
    let child = slot.as_mut().ok_or(corrupt("missing fan-out node"))?;
    backend.with_value(sam, child, |object, backend, sam| {
        with_slot(
            &mut node_mut(object, false)?.children,
            rest,
            backend,
            sam,
            operation,
        )
    })
}

/// Visits the leaves below `children` (`levels` levels deep) in index order
/// until `visit` returns `false`; returns whether every leaf was visited.
/// Nodes after the stopping leaf are not accessed.
fn for_each_leaf<P, B, S>(
    children: &mut [Option<P>],
    levels: u64,
    backend: &mut B,
    sam: &mut S,
    visit: &mut LeafVisitor<'_, P, B, S>,
) -> Result<bool>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    for child in children.iter_mut() {
        let Some(child) = child.as_mut() else {
            break;
        };
        let more = backend.with_value(sam, child, |object, backend, sam| {
            if levels == 1 {
                visit(node_mut(object, true)?, backend, sam)
            } else {
                let node = node_mut(object, false)?;
                for_each_leaf(&mut node.children, levels - 1, backend, sam, visit)
            }
        })?;
        if !more {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Dismantles the tree below `children`: deletes every internal node and
/// hands each leaf pointer, in index order, to `sink`.
fn drain<P, B, S>(
    children: &mut Vec<Option<P>>,
    levels: u64,
    backend: &mut B,
    sam: &mut S,
    sink: &mut dyn FnMut(P, &mut B, &mut S) -> Result<()>,
) -> Result<()>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    for slot in children.iter_mut() {
        let Some(mut child) = slot.take() else {
            break;
        };
        if levels == 1 {
            sink(child, backend, sam)?;
            continue;
        }
        backend.with_value(sam, &mut child, |object, backend, sam| {
            let node = node_mut(object, false)?;
            drain(&mut node.children, levels - 1, backend, sam, sink)
        })?;
        backend.delete(sam, &mut child)?;
    }
    children.clear();
    Ok(())
}

/// Deletes a leaf and returns its edge pointer with the destination id.
pub(super) fn take_edge<P, B, S>(mut leaf: P, backend: &mut B, sam: &mut S) -> Result<(P, u64)>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    let edge = backend.with_value(sam, &mut leaf, |object, _, _| {
        let node = node_mut(object, true)?;
        let edge = node
            .edge
            .take()
            .ok_or(corrupt("fan-out leaf has no edge"))?;
        let destination = node
            .destination
            .ok_or(corrupt("fan-out leaf has no destination"))?;
        Ok((edge, destination))
    })?;
    backend.delete(sam, &mut leaf)?;
    Ok(edge)
}

/// Whether a leaf's destination is a live vertex. A self-loop is live (its
/// source is being held), anything else is checked by dereferencing the edge.
fn edge_is_live<P, B, S>(
    leaf: &mut FanOut<P>,
    source: u64,
    backend: &mut B,
    sam: &mut S,
) -> Result<bool>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    if leaf.destination == Some(source) {
        return Ok(true);
    }
    let edge = leaf
        .edge
        .as_mut()
        .ok_or(corrupt("fan-out leaf has no edge"))?;
    backend.with_value(sam, edge, |object, _, _| {
        Ok(!matches!(object, GraphObject::Deleted(_)))
    })
}

/// Appends `leaf` as the vertex's next edge (index `out_degree`), growing the
/// tree by one level when it is full.
pub(super) fn append<P, B, S>(
    vertex: &mut Vertex<P>,
    leaf: P,
    backend: &mut B,
    sam: &mut S,
) -> Result<()>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    let fanout = vertex.fanout;
    if vertex.height == 0 {
        vertex.height = 1;
    } else if vertex.out_degree == capacity(vertex.height, fanout) {
        if fanout < 2 {
            return Err(SamError::Backend(
                "this block holds one outgoing pointer, but the graph requires branching".into(),
            ));
        }
        // The old top level becomes the first child of a new root node.
        let old = std::mem::take(&mut vertex.out_children);
        let node = backend.new_pointer(sam, GraphObject::FanOut(FanOut::internal(old)))?;
        vertex.out_children = vec![Some(node)];
        vertex.height += 1;
    }
    let path = digits(vertex.out_degree, vertex.height, fanout);
    place(&mut vertex.out_children, &path, leaf, backend, sam)?;
    vertex.out_degree += 1;
    Ok(())
}

/// Removes the leaf at `index` and returns it: the last leaf moves into its
/// slot, and the tree loses a level once the remaining leaves fit below.
pub(super) fn remove_at<P, B, S>(
    vertex: &mut Vertex<P>,
    index: u64,
    backend: &mut B,
    sam: &mut S,
) -> Result<P>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    if index >= vertex.out_degree {
        return Err(SamError::InvalidParameter("edge index is out of range"));
    }
    let fanout = vertex.fanout;
    let last = vertex.out_degree - 1;
    let moved = take(
        &mut vertex.out_children,
        &digits(last, vertex.height, fanout),
        backend,
        sam,
    )?;
    let removed = if index == last {
        moved
    } else {
        with_slot(
            &mut vertex.out_children,
            &digits(index, vertex.height, fanout),
            backend,
            sam,
            |slot, _, _| slot.replace(moved).ok_or(corrupt("missing fan-out leaf")),
        )?
    };
    vertex.out_degree -= 1;
    if vertex.out_degree == 0 {
        vertex.height = 0;
    }
    while vertex.height > 1 && vertex.out_degree <= capacity(vertex.height - 1, fanout) {
        // Every leaf now lies below the first top-level node: lift its children.
        if vertex.out_children.len() != 1 {
            return Err(corrupt("fan-out tree is not packed to the left"));
        }
        let mut node = vertex.out_children[0]
            .take()
            .ok_or(corrupt("missing fan-out node"))?;
        let children = backend.with_value(sam, &mut node, |object, _, _| {
            Ok(std::mem::take(&mut node_mut(object, false)?.children))
        })?;
        backend.delete(sam, &mut node)?;
        vertex.out_children = children;
        vertex.height -= 1;
    }
    Ok(removed)
}

/// Index of the first leaf whose destination is `destination`.
pub(super) fn find<P, B, S>(
    vertex: &mut Vertex<P>,
    destination: u64,
    backend: &mut B,
    sam: &mut S,
) -> Result<Option<u64>>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    let mut index = 0;
    let mut found = None;
    let height = vertex.height;
    if height > 0 {
        for_each_leaf(
            &mut vertex.out_children,
            height,
            backend,
            sam,
            &mut |leaf, _, _| {
                if leaf.destination == Some(destination) {
                    found = Some(index);
                    return Ok(false);
                }
                index += 1;
                Ok(true)
            },
        )?;
    }
    Ok(found)
}

/// Runs `operation` on the leaf at `index`.
pub(super) fn with_leaf<P, B, S, T, F>(
    vertex: &mut Vertex<P>,
    index: u64,
    backend: &mut B,
    sam: &mut S,
    operation: F,
) -> Result<T>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
    F: FnOnce(&mut FanOut<P>, &mut B, &mut S) -> Result<T>,
{
    if index >= vertex.out_degree {
        return Err(SamError::InvalidParameter("edge index is out of range"));
    }
    let path = digits(index, vertex.height, vertex.fanout);
    with_slot(
        &mut vertex.out_children,
        &path,
        backend,
        sam,
        |slot, backend, sam| {
            let leaf = slot.as_mut().ok_or(corrupt("missing fan-out leaf"))?;
            backend.with_value(sam, leaf, |object, backend, sam| {
                operation(node_mut(object, true)?, backend, sam)
            })
        },
    )
}

/// Hands up to `limit` outgoing edges, in index order, to `sink`, with a
/// fresh pointer alias each when `copy_edges` (except self-loops). With `check_live`, edges to
/// deleted vertices are skipped (not counted) and, if any were found, removed
/// from the tree afterwards (see [`remove_dead`]). Returns the number handed
/// out.
pub(super) fn scan<P, B, S>(
    vertex: &mut Vertex<P>,
    limit: Option<u64>,
    copy_edges: bool,
    check_live: bool,
    backend: &mut B,
    sam: &mut S,
    sink: &mut NeighborSink<'_, P, B, S>,
) -> Result<u64>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    let (mut emitted, mut dead) = (0, 0);
    let source = vertex.id;
    let height = vertex.height;
    if height == 0 || limit == Some(0) {
        return Ok(0);
    }
    for_each_leaf(
        &mut vertex.out_children,
        height,
        backend,
        sam,
        &mut |leaf, backend, sam| {
            if check_live && !edge_is_live(leaf, source, backend, sam)? {
                dead += 1;
                return Ok(true);
            }
            let destination = leaf
                .destination
                .ok_or(corrupt("fan-out leaf has no destination"))?;
            let pointer = if copy_edges && destination != source {
                let edge = leaf
                    .edge
                    .as_mut()
                    .ok_or(corrupt("fan-out leaf has no edge"))?;
                Some(backend.copy_pointer(sam, edge)?)
            } else {
                None
            };
            sink(
                Neighbor {
                    pointer,
                    destination,
                    weight: leaf.weight,
                },
                backend,
                sam,
            )?;
            emitted += 1;
            Ok(limit.is_none_or(|limit| emitted < limit))
        },
    )?;
    if dead > 0 {
        remove_dead(vertex, backend, sam)?;
    }
    Ok(emitted)
}

/// Removes every edge to a deleted vertex, keeping the remaining edges in
/// their order: the tree is dismantled into a SAM queue of live leaves and
/// rebuilt from it. Returns the number of edges removed.
pub(super) fn remove_dead<P, B, S>(
    vertex: &mut Vertex<P>,
    backend: &mut B,
    sam: &mut S,
) -> Result<u64>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    let source = vertex.id;
    let height = vertex.height;
    let mut kept = SmartQueue::init(sam, MemoryClass::Oblivious);
    let mut removed = 0;
    drain(
        &mut vertex.out_children,
        height,
        backend,
        sam,
        &mut |mut leaf, backend, sam| {
            let live = backend.with_value(sam, &mut leaf, |object, backend, sam| {
                let node = node_mut(object, true)?;
                let live = edge_is_live(node, source, backend, sam)?;
                if !live {
                    if let Some(mut edge) = node.edge.take() {
                        backend.delete(sam, &mut edge)?;
                    }
                }
                Ok(live)
            })?;
            if live {
                kept.enqueue::<B, P, S>(sam, MemoryClass::Oblivious, Item::Pointer(leaf))
            } else {
                removed += 1;
                backend.delete(sam, &mut leaf)
            }
        },
    )?;
    vertex.height = 0;
    vertex.out_degree = 0;
    while !kept.is_empty() {
        match kept.dequeue::<B, P, S>(sam)? {
            Item::Pointer(leaf) => append(vertex, leaf, backend, sam)?,
            _ => return Err(corrupt("leaf queue held a non-pointer")),
        }
    }
    kept.dequeue::<B, P, S>(sam)?;
    Ok(removed)
}

/// Deletes every edge and fan-out node of the vertex, except that the edge
/// pointers of self-loops are moved to `self_loops` for the caller to delete
/// once the vertex is released.
pub(super) fn clear<P, B, S>(
    vertex: &mut Vertex<P>,
    self_loops: &mut SmartQueue,
    backend: &mut B,
    sam: &mut S,
) -> Result<()>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    let source = vertex.id;
    let height = vertex.height;
    drain(
        &mut vertex.out_children,
        height,
        backend,
        sam,
        &mut |leaf, backend, sam| {
            let (mut edge, destination) = take_edge(leaf, backend, sam)?;
            if destination == source {
                self_loops.enqueue::<B, P, S>(sam, MemoryClass::Oblivious, Item::Pointer(edge))
            } else {
                backend.delete(sam, &mut edge)
            }
        },
    )?;
    vertex.height = 0;
    vertex.out_degree = 0;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_and_digits_follow_the_base_fanout_layout() {
        assert_eq!(tree_height(0, 3), 0);
        assert_eq!(tree_height(1, 3), 1);
        assert_eq!(tree_height(3, 3), 1);
        assert_eq!(tree_height(4, 3), 2);
        assert_eq!(tree_height(9, 3), 2);
        assert_eq!(tree_height(10, 3), 3);
        assert_eq!(capacity(0, 3), 1);
        assert_eq!(capacity(2, 3), 9);
        assert_eq!(digits(7, 2, 3), vec![2, 1]);
        assert_eq!(digits(7, 3, 3), vec![0, 2, 1]);
    }
}
