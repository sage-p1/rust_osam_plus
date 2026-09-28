//! The graph handle, vertex lookup, construction and dynamic updates.

use super::tree::{self, Neighbor, NeighborSink};
use super::{
    DeletedObject, FanOut, GraphBackend, GraphInput, GraphLayout, GraphObject, StepKind, StepMark,
    Vertex,
};
use crate::{
    structures::{avl::SmartAvlTree, queue::SmartQueue, Item},
    MemoryClass, OperationCounts, SamError, SingleAccessMachine,
};
use std::marker::PhantomData;

type Result<T> = std::result::Result<T, SamError>;

/// Callback receiving `(visited vertex id, neighbor)` pairs.
pub(super) type VisitSink<'a, P, B, S> =
    dyn FnMut(u64, Neighbor<P>, &mut B, &mut S) -> Result<()> + 'a;

/// Length of each priming walk (see [`ObliviousGraph::prime`]).
pub const PRIME_WALK_LENGTH: usize = 50;

/// An oblivious graph whose entire structure lives in a SAM.
///
/// # SAM-resident state
/// * Every vertex is a [`Vertex`] behind a smart pointer. Its outgoing edges
///   form a balanced fan-out tree of [`FanOut`] records (see
///   [`GraphLayout::graph_branching_factor`] for the fanout `bf`): leaf `i`
///   sits at the base-`bf` digits of `i`, and each leaf holds `edge`, an
///   alias of the destination vertex's pointer, plus the edge weight and the
///   destination id (kept as data for reporting results).
/// * `v_ids`, an oblivious AVL tree `name -> id`, and `entry_points`, an
///   oblivious AVL tree `id -> entry pointer` (a pointer alias of the vertex).
///   Their nodes are raw SAM cells of the pointer backend.
/// * Traversal frontiers: breadth-first search uses a SAM queue of pointer
///   copies, depth-first search a SAM stack, and the vertices whose
///   `visited` flag must be reset afterwards are kept in another SAM queue.
///
/// Traversals follow pointers: a vertex's neighbors are reached by copying
/// the edge pointers stored in its leaves, never by looking an id up in a
/// client table.
///
/// # Client state
/// Only O(1) values: the two AVL root addresses (inside [`SmartAvlTree`]),
/// the next vertex id, the vertex count, the number of tombstones that may
/// still be referenced, the layout, and algorithm outputs (paths, costs,
/// counts) plus optional step marks. The one exception, as in the reference
/// design, is the priority queue of [`Self::dijkstra`] and [`Self::prim`]:
/// it is client-side and holds `(key, destination id, parent id, pointer)`
/// entries for the current frontier.
///
/// # Deletion
/// [`Self::delete_vertex`] deletes the vertex's own edges and fan-out nodes,
/// removes it from both AVL trees, and overwrites the vertex with a
/// [`DeletedObject`] tombstone, since other vertices' edges still alias it.
/// While tombstones may be referenced, every scan of a fan-out tree
/// dereferences each edge to recognize edges to tombstones; those are skipped
/// and removed from the scanned tree right away, keeping the order of the
/// remaining edges (so results always equal those of a graph from which the
/// deleted vertex's incoming edges were removed in place). Traversals that
/// reach a tombstone through a frontier entry skip it and delete that
/// pointer copy. [`Self::purge_tombstones`] scans every vertex once, after
/// which no edge references a tombstone and scans stop dereferencing edges.
#[derive(Clone, Debug)]
pub struct ObliviousGraph<P> {
    layout: GraphLayout,
    v_ids: SmartAvlTree,
    entry_points: SmartAvlTree,
    next_vertex_id: u64,
    vertex_count: u64,
    tombstones: u64,
    /// When `Some`, algorithms append a [`StepMark`] at every step boundary,
    /// so per-step costs can be reported for completed steps only. Callers
    /// clear it between trials.
    pub step_marks: Option<Vec<StepMark>>,
    marker: PhantomData<fn() -> P>,
}

impl<P: Clone> ObliviousGraph<P> {
    /// An empty graph.
    pub fn new(layout: GraphLayout) -> Self {
        Self {
            layout,
            // Names and ids are integers: the trees order them directly.
            v_ids: SmartAvlTree::new(false, true),
            entry_points: SmartAvlTree::new(false, true),
            next_vertex_id: 0,
            vertex_count: 0,
            tombstones: 0,
            step_marks: None,
            marker: PhantomData,
        }
    }

    /// The record layout this graph was built with.
    pub fn layout(&self) -> GraphLayout {
        self.layout
    }

    /// Fanout of the outgoing-edge trees.
    pub fn graph_branching_factor(&self) -> usize {
        self.layout.graph_branching_factor()
    }

    /// Number of live vertices.
    pub fn vertex_count(&self) -> u64 {
        self.vertex_count
    }

    /// The id the next added vertex receives; ids are never reused.
    pub fn next_vertex_id(&self) -> u64 {
        self.next_vertex_id
    }

    /// Deleted vertices whose tombstones may still be referenced by edges.
    pub fn tombstones(&self) -> u64 {
        self.tombstones
    }

    /// Builds the graph in bulk: one smart copy call per destination for all
    /// of its incoming edges, then each source's fan-out tree bottom-up, then
    /// both lookup trees at once. The result is the graph that
    /// [`Self::build_dynamic`] produces (vertex ids in `input.vertices`
    /// order, each vertex's edges in input order).
    pub fn build_static<B, S>(
        input: &GraphInput,
        layout: GraphLayout,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<Self>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        let fanout = layout.graph_branching_factor();
        let names = &input.vertices;
        let ids = names
            .iter()
            .enumerate()
            .map(|(id, name)| (*name, id))
            .collect::<std::collections::BTreeMap<_, _>>();
        if ids.len() != names.len() {
            return Err(SamError::InvalidParameter("duplicate vertex name"));
        }
        let mut adjacency = vec![Vec::<(usize, i64)>::new(); names.len()];
        for edge in &input.edges {
            let source = *ids
                .get(&edge.source)
                .ok_or(SamError::InvalidParameter("edge source is missing"))?;
            let destination = *ids
                .get(&edge.destination)
                .ok_or(SamError::InvalidParameter("edge destination is missing"))?;
            adjacency[source].push((destination, edge.weight));
        }
        if fanout == 1 && adjacency.iter().any(|edges| edges.len() > 1) {
            return Err(SamError::Backend(
                "this block holds one outgoing pointer, but the input graph requires branching"
                    .into(),
            ));
        }

        let mut entries = (0..names.len())
            .map(|id| backend.new_pointer(sam, GraphObject::Vertex(Vertex::new(id as u64, fanout))))
            .collect::<Result<Vec<_>>>()?;
        let mut incoming = vec![Vec::<(usize, usize)>::new(); names.len()];
        for (source, edges) in adjacency.iter().enumerate() {
            for (index, (destination, _)) in edges.iter().enumerate() {
                incoming[*destination].push((source, index));
            }
        }
        let mut aliases = adjacency
            .iter()
            .map(|edges| vec![None; edges.len()])
            .collect::<Vec<_>>();
        for (destination, positions) in incoming.iter().enumerate() {
            if positions.is_empty() {
                continue;
            }
            let copies = backend.copy_many(sam, &mut entries[destination], positions.len())?;
            for ((source, index), pointer) in positions.iter().zip(copies) {
                aliases[*source][*index] = Some(pointer);
            }
        }

        for (source, edges) in adjacency.iter().enumerate() {
            if edges.is_empty() {
                continue;
            }
            let mut level = Vec::with_capacity(edges.len());
            for (index, (destination, weight)) in edges.iter().enumerate() {
                let edge = aliases[source][index]
                    .take()
                    .ok_or(SamError::InvalidPointerCell("missing destination alias"))?;
                level.push(backend.new_pointer(
                    sam,
                    GraphObject::FanOut(FanOut::leaf(edge, *weight, *destination as u64, fanout)),
                )?);
            }
            // Grouping consecutive runs of `fanout` nodes level by level puts
            // leaf i at the base-fanout digits of i, as appends would.
            while level.len() > fanout {
                let mut parents = Vec::with_capacity(level.len().div_ceil(fanout));
                let mut members = level.into_iter().peekable();
                while members.peek().is_some() {
                    let mut children = members.by_ref().take(fanout).map(Some).collect::<Vec<_>>();
                    children.resize_with(fanout, || None);
                    parents.push(
                        backend
                            .new_pointer(sam, GraphObject::FanOut(FanOut::internal(children)))?,
                    );
                }
                level = parents;
            }
            let mut out_children = level.into_iter().map(Some).collect::<Vec<_>>();
            out_children.resize_with(fanout, || None);
            let out_degree = edges.len() as u64;
            backend.put(
                sam,
                &mut entries[source],
                GraphObject::Vertex(Vertex {
                    id: source as u64,
                    out_children,
                    out_degree,
                    height: tree::tree_height(out_degree, fanout),
                    visited: false,
                    label: None,
                }),
            )?;
        }

        let mut graph = Self::new(layout);
        graph.v_ids.build_tree::<B, P, S, _>(
            sam,
            names
                .iter()
                .enumerate()
                .map(|(id, name)| (*name, Item::Int(id as i64))),
        )?;
        graph.entry_points.build_tree::<B, P, S, _>(
            sam,
            entries
                .into_iter()
                .enumerate()
                .map(|(id, pointer)| (id as u64, Item::Pointer(pointer))),
        )?;
        graph.next_vertex_id = names.len() as u64;
        graph.vertex_count = names.len() as u64;
        Ok(graph)
    }

    /// Builds the graph one operation at a time: [`Self::add_vertex`] for
    /// every vertex, then [`Self::add_edge`] for every edge, in input order.
    pub fn build_dynamic<B, S>(
        input: &GraphInput,
        layout: GraphLayout,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<Self>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        let mut graph = Self::new(layout);
        for name in &input.vertices {
            graph.add_vertex(*name, backend, sam)?;
        }
        for edge in &input.edges {
            graph.add_edge(edge.source, edge.destination, edge.weight, backend, sam)?;
        }
        Ok(graph)
    }

    /// Looks a vertex name up in `v_ids`. (The backend is not used: it only
    /// names the SAM cell format of the AVL nodes.)
    pub fn vertex_id<B, S>(
        &mut self,
        name: u64,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<Option<u64>>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        let _: &mut B = backend;
        match self.v_ids.get::<B, P, S>(sam, name)? {
            None => Ok(None),
            Some(Item::Int(id)) => Ok(Some(id as u64)),
            Some(_) => Err(SamError::InvalidPointerCell(
                "vertex id entry is not an integer",
            )),
        }
    }

    /// Runs `operation` on the entry pointer stored for `id` while its AVL
    /// node is held; the pointer's refreshed state is written back with it.
    pub(super) fn with_entry<B, S, T>(
        &mut self,
        id: u64,
        backend: &mut B,
        sam: &mut S,
        operation: impl FnOnce(&mut P, &mut B, &mut S) -> Result<T>,
    ) -> Result<Option<T>>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        self.entry_points
            .update_with::<B, P, S, T, _>(backend, sam, id, |item, backend, sam| match item {
                Item::Pointer(pointer) => operation(pointer, backend, sam),
                _ => Err(SamError::InvalidPointerCell("entry point is not a pointer")),
            })
    }

    /// Runs `operation` on the vertex `id` through its stored entry pointer.
    pub(super) fn with_vertex<B, S, T>(
        &mut self,
        id: u64,
        backend: &mut B,
        sam: &mut S,
        operation: impl FnOnce(&mut Vertex<P>, &mut B, &mut S) -> Result<T>,
    ) -> Result<Option<T>>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        self.with_entry(id, backend, sam, |pointer, backend, sam| {
            backend.with_value(sam, pointer, |object, backend, sam| {
                operation(tree::vertex_mut(object)?, backend, sam)
            })
        })
    }

    /// A fresh pointer to the vertex `id`, if it is live.
    pub(super) fn entry_copy<B, S>(
        &mut self,
        id: u64,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<Option<P>>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        self.with_entry(id, backend, sam, |pointer, backend, sam| {
            backend.copy_pointer(sam, pointer)
        })
    }

    /// A fresh pointer to the vertex `name` (a copy of its entry pointer)
    /// and its id. The caller owns the copy and must delete it.
    pub fn get_pointer<B, S>(
        &mut self,
        name: u64,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<Option<(P, u64)>>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        let Some(id) = self.vertex_id(name, backend, sam)? else {
            return Ok(None);
        };
        let pointer = self
            .entry_copy(id, backend, sam)?
            .ok_or(SamError::InvalidPointerCell("vertex id has no entry point"))?;
        Ok(Some((pointer, id)))
    }

    /// Adds a vertex without edges and returns its id.
    pub fn add_vertex<B, S>(&mut self, name: u64, backend: &mut B, sam: &mut S) -> Result<u64>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        if self.vertex_id(name, backend, sam)?.is_some() {
            return Err(SamError::InvalidParameter("vertex name already exists"));
        }
        let id = self.next_vertex_id;
        let fanout = self.graph_branching_factor();
        let pointer = backend.new_pointer(sam, GraphObject::Vertex(Vertex::new(id, fanout)))?;
        self.v_ids
            .insert::<B, P, S>(sam, name, Item::Int(id as i64))?;
        self.entry_points
            .insert::<B, P, S>(sam, id, Item::Pointer(pointer))?;
        self.next_vertex_id += 1;
        self.vertex_count += 1;
        Ok(id)
    }

    /// Adds the edge `source -> destination` as the source's last edge.
    /// Both vertices must exist; parallel edges and self-loops are allowed.
    pub fn add_edge<B, S>(
        &mut self,
        source: u64,
        destination: u64,
        weight: i64,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<()>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        let source_id = self
            .vertex_id(source, backend, sam)?
            .ok_or(SamError::InvalidParameter("edge source is missing"))?;
        let (edge, destination_id) = self
            .get_pointer(destination, backend, sam)?
            .ok_or(SamError::InvalidParameter("edge destination is missing"))?;
        let fanout = self.graph_branching_factor();
        let leaf = backend.new_pointer(
            sam,
            GraphObject::FanOut(FanOut::leaf(edge, weight, destination_id, fanout)),
        )?;
        let check_live = self.tombstones > 0;
        self.with_vertex(source_id, backend, sam, |vertex, backend, sam| {
            if check_live {
                tree::remove_dead(vertex, backend, sam)?;
            }
            tree::append(vertex, leaf, backend, sam)
        })?
        .ok_or(SamError::InvalidPointerCell("vertex id has no entry point"))
    }

    /// Deletes the first edge `source -> destination`: the source's last
    /// edge moves into its place. Returns whether such an edge existed.
    pub fn delete_edge<B, S>(
        &mut self,
        source: u64,
        destination: u64,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<bool>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        let source_id = self
            .vertex_id(source, backend, sam)?
            .ok_or(SamError::InvalidParameter("edge source is missing"))?;
        let destination_id = self
            .vertex_id(destination, backend, sam)?
            .ok_or(SamError::InvalidParameter("edge destination is missing"))?;
        let check_live = self.tombstones > 0;
        let removed = self
            .with_vertex(source_id, backend, sam, |vertex, backend, sam| {
                if check_live {
                    tree::remove_dead(vertex, backend, sam)?;
                }
                let Some(index) = tree::find(vertex, destination_id, backend, sam)? else {
                    return Ok(None);
                };
                let leaf = tree::remove_at(vertex, index, backend, sam)?;
                // The edge may alias the held source (a self-loop): delete it
                // once the source is released.
                Ok(Some(tree::take_edge(leaf, backend, sam)?.0))
            })?
            .ok_or(SamError::InvalidPointerCell("vertex id has no entry point"))?;
        match removed {
            Some(mut edge) => {
                backend.delete(sam, &mut edge)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Deletes a vertex: its outgoing edges and fan-out nodes are deleted,
    /// it leaves both lookup trees, and a [`DeletedObject`] replaces it for
    /// the edges of other vertices that still alias it.
    pub fn delete_vertex<B, S>(&mut self, name: u64, backend: &mut B, sam: &mut S) -> Result<()>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        let id = self
            .vertex_id(name, backend, sam)?
            .ok_or(SamError::InvalidParameter("vertex is missing"))?;
        let mut self_loops = SmartQueue::init(sam, MemoryClass::Oblivious);
        self.with_entry(id, backend, sam, |pointer, backend, sam| {
            backend.with_value(sam, pointer, |object, backend, sam| {
                tree::clear(tree::vertex_mut(object)?, &mut self_loops, backend, sam)?;
                *object = GraphObject::Deleted(DeletedObject { id: Some(id) });
                Ok(())
            })
        })?
        .ok_or(SamError::InvalidPointerCell("vertex id has no entry point"))?;
        delete_queued_pointers::<P, B, S>(&mut self_loops, backend, sam)?;
        let Some(Item::Pointer(mut entry)) = self.entry_points.delete::<B, P, S>(sam, id)? else {
            return Err(SamError::InvalidPointerCell("entry point is not a pointer"));
        };
        self.v_ids.delete::<B, P, S>(sam, name)?;
        // Other aliases are incoming edges: the tombstone must stay visible.
        if !backend.is_single_reference(sam, &mut entry)? {
            self.tombstones += 1;
        }
        backend.delete(sam, &mut entry)?;
        self.vertex_count -= 1;
        Ok(())
    }

    /// Removes every edge to a deleted vertex from every live vertex, after
    /// which no tombstone is referenced. Returns the number of edges removed.
    pub fn purge_tombstones<B, S>(&mut self, backend: &mut B, sam: &mut S) -> Result<u64>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        let mut removed = 0;
        if self.tombstones > 0 {
            for id in 0..self.next_vertex_id {
                removed += self
                    .with_vertex(id, backend, sam, |vertex, backend, sam| {
                        // A checking scan removes the dead edges it finds.
                        let before = vertex.out_degree;
                        tree::scan(vertex, None, false, true, backend, sam, &mut |_, _, _| {
                            Ok(())
                        })?;
                        Ok(before - vertex.out_degree)
                    })?
                    .unwrap_or(0);
            }
        }
        self.tombstones = 0;
        Ok(removed)
    }

    /// Outgoing edges of `name` as `(destination id, weight)`, in order.
    pub fn neighbors<B, S>(
        &mut self,
        name: u64,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<Vec<(u64, i64)>>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        self.neighbors_limited(name, None, backend, sam)
    }

    /// The first `max_neighbors` outgoing edges of `name`; fan-out nodes
    /// after the last one returned are not accessed.
    pub fn neighbors_limited<B, S>(
        &mut self,
        name: u64,
        max_neighbors: Option<usize>,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<Vec<(u64, i64)>>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        validate_max_neighbors(max_neighbors)?;
        let id = self
            .vertex_id(name, backend, sam)?
            .ok_or(SamError::InvalidParameter("vertex is missing"))?;
        let mut neighbors = Vec::new();
        self.with_scan(
            id,
            max_neighbors,
            false,
            backend,
            sam,
            &mut |neighbor, _, _| {
                neighbors.push((neighbor.destination, neighbor.weight));
                Ok(())
            },
        )?;
        Ok(neighbors)
    }

    /// Scans the vertex `id` through its entry pointer; returns the count.
    pub(super) fn with_scan<B, S>(
        &mut self,
        id: u64,
        limit: Option<usize>,
        copy_edges: bool,
        backend: &mut B,
        sam: &mut S,
        sink: &mut NeighborSink<'_, P, B, S>,
    ) -> Result<u64>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        let check_live = self.tombstones > 0;
        self.with_vertex(id, backend, sam, |vertex, backend, sam| {
            tree::scan(
                vertex,
                limit.map(|limit| limit as u64),
                copy_edges,
                check_live,
                backend,
                sam,
                sink,
            )
        })?
        .ok_or(SamError::InvalidPointerCell("vertex id has no entry point"))
    }

    /// Scans the vertex behind `pointer` (0 for a tombstone).
    pub(super) fn scan_pointer<B, S>(
        &self,
        pointer: &mut P,
        limit: Option<usize>,
        copy_edges: bool,
        backend: &mut B,
        sam: &mut S,
        sink: &mut NeighborSink<'_, P, B, S>,
    ) -> Result<u64>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        let check_live = self.tombstones > 0;
        backend.with_value(sam, pointer, |object, backend, sam| match object {
            GraphObject::Deleted(_) => Ok(0),
            object => tree::scan(
                tree::vertex_mut(object)?,
                limit.map(|limit| limit as u64),
                copy_edges,
                check_live,
                backend,
                sam,
                sink,
            ),
        })
    }

    /// Dereferences `pointer`; if it holds an unvisited vertex, marks it
    /// visited with `label`, hands every outgoing edge (with a fresh pointer
    /// alias, none for a self-loop) to `sink` together with the vertex's id,
    /// and returns that id.
    /// Returns `None` for an already visited vertex or a tombstone.
    pub(super) fn visit<B, S>(
        &self,
        pointer: &mut P,
        label: Option<i64>,
        backend: &mut B,
        sam: &mut S,
        sink: &mut VisitSink<'_, P, B, S>,
    ) -> Result<Option<u64>>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        let check_live = self.tombstones > 0;
        backend.with_value(sam, pointer, |object, backend, sam| {
            let vertex = match object {
                GraphObject::Deleted(_) => return Ok(None),
                object => tree::vertex_mut(object)?,
            };
            if vertex.visited {
                return Ok(None);
            }
            vertex.visited = true;
            vertex.label = label;
            let id = vertex.id;
            tree::scan(
                vertex,
                None,
                true,
                check_live,
                backend,
                sam,
                &mut |neighbor, backend, sam| sink(id, neighbor, backend, sam),
            )?;
            Ok(Some(id))
        })
    }

    /// Picks a uniformly random outgoing edge of the vertex behind `pointer`
    /// (`rng.gen_range(0..out_degree)`) and returns a pointer to its
    /// destination with the destination's id; `None` at a sink or tombstone.
    /// Only the chosen leaf's path is accessed.
    pub(super) fn random_neighbor<B, S, R>(
        &self,
        pointer: &mut P,
        rng: &mut R,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<Option<(P, u64)>>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
        R: rand::Rng + ?Sized,
    {
        let check_live = self.tombstones > 0;
        let picked = backend.with_value(sam, pointer, |object, backend, sam| {
            let vertex = match object {
                GraphObject::Deleted(_) => return Ok(None),
                object => tree::vertex_mut(object)?,
            };
            if check_live {
                tree::remove_dead(vertex, backend, sam)?;
            }
            if vertex.out_degree == 0 {
                return Ok(None);
            }
            let source = vertex.id;
            let index = rng.gen_range(0..vertex.out_degree);
            tree::with_leaf(vertex, index, backend, sam, |leaf, backend, sam| {
                let destination = leaf.destination.ok_or(SamError::InvalidPointerCell(
                    "fan-out leaf has no destination",
                ))?;
                if destination == source {
                    return Ok(Some((None, destination)));
                }
                let edge = leaf
                    .edge
                    .as_mut()
                    .ok_or(SamError::InvalidPointerCell("fan-out leaf has no edge"))?;
                Ok(Some((Some(backend.copy_pointer(sam, edge)?), destination)))
            })
        })?;
        Ok(match picked {
            None => None,
            Some((Some(next), destination)) => Some((next, destination)),
            // A self-loop: alias the vertex now that it is released.
            Some((None, destination)) => Some((backend.copy_pointer(sam, pointer)?, destination)),
        })
    }

    /// A pointer to a uniformly random live vertex and its id: ids are drawn
    /// with `rng.gen_range(0..next_vertex_id)` until one is live.
    pub(super) fn random_vertex<B, S, R>(
        &mut self,
        rng: &mut R,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<(P, u64)>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
        R: rand::Rng + ?Sized,
    {
        if self.vertex_count == 0 {
            return Err(SamError::InvalidParameter("the graph has no vertices"));
        }
        loop {
            let id = rng.gen_range(0..self.next_vertex_id);
            if let Some(pointer) = self.entry_copy(id, backend, sam)? {
                return Ok((pointer, id));
            }
        }
    }

    /// Priming: `vertex_count / 10` random walks of [`PRIME_WALK_LENGTH`]
    /// steps from random vertices, which spreads the pointer trees before a
    /// benchmark. Returns the number of walks.
    pub fn prime<B, S, R>(&mut self, rng: &mut R, backend: &mut B, sam: &mut S) -> Result<u64>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
        R: rand::Rng + ?Sized,
    {
        let walks = self.vertex_count / 10;
        for _ in 0..walks {
            let (mut current, _) = self.random_vertex(rng, backend, sam)?;
            for _ in 0..PRIME_WALK_LENGTH {
                let Some((next, _)) = self.random_neighbor(&mut current, rng, backend, sam)? else {
                    break;
                };
                backend.delete(sam, &mut current)?;
                current = next;
            }
            backend.delete(sam, &mut current)?;
        }
        Ok(walks)
    }

    pub(super) fn mark(&mut self, kind: StepKind, operations: OperationCounts) {
        if let Some(marks) = self.step_marks.as_mut() {
            marks.push(StepMark { kind, operations });
        }
    }
}

pub(super) fn validate_max_neighbors(max_neighbors: Option<usize>) -> Result<()> {
    if max_neighbors == Some(0) {
        return Err(SamError::InvalidParameter("max_neighbors must be positive"));
    }
    Ok(())
}

/// Dequeues every pointer of a SAM queue, deletes it, and closes the queue.
pub(super) fn delete_queued_pointers<P, B, S>(
    queue: &mut SmartQueue,
    backend: &mut B,
    sam: &mut S,
) -> Result<()>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    while !queue.is_empty() {
        let (pointer, _) = split_entry(queue.dequeue::<B, P, S>(sam)?)?;
        if let Some(mut pointer) = pointer {
            backend.delete(sam, &mut pointer)?;
        }
    }
    queue.dequeue::<B, P, S>(sam)?;
    Ok(())
}

/// A frontier entry: an optional pointer (absent for a self-loop marker) and
/// an optional integer (a label or a vertex id).
pub(super) fn entry_item<P>(pointer: Option<P>, value: Option<i64>) -> Item<P> {
    Item::Tuple(vec![
        pointer.map_or(Item::None, Item::Pointer),
        value.map_or(Item::None, Item::Int),
    ])
}

/// Inverse of [`entry_item`]; a bare pointer is accepted as `(pointer, None)`.
pub(super) fn split_entry<P>(item: Item<P>) -> Result<(Option<P>, Option<i64>)> {
    let corrupt = || SamError::InvalidPointerCell("malformed frontier entry");
    match item {
        Item::Pointer(pointer) => Ok((Some(pointer), None)),
        Item::Tuple(items) => {
            let mut items = items.into_iter();
            let pointer = match items.next().ok_or_else(corrupt)? {
                Item::Pointer(pointer) => Some(pointer),
                Item::None => None,
                _ => return Err(corrupt()),
            };
            let value = match items.next().ok_or_else(corrupt)? {
                Item::Int(value) => Some(value),
                Item::None => None,
                _ => return Err(corrupt()),
            };
            Ok((pointer, value))
        }
        _ => Err(corrupt()),
    }
}
