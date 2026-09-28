//! Graph algorithms over [`ObliviousGraph`].
//!
//! Every algorithm starts from pointers obtained through the SAM-resident
//! lookup trees and reaches further vertices only by following edge
//! pointers. Frontiers are SAM queues and stacks of pointer copies (the
//! priority queue of Dijkstra and Prim is client-side, see
//! [`ObliviousGraph`]); every pointer copy is deleted by the time an
//! algorithm returns. Vertices are reported by their ids.

use super::core::{entry_item, split_entry, validate_max_neighbors};
use super::tree::Neighbor;
use super::{GraphBackend, ObliviousGraph, StepKind};
use crate::{
    structures::{queue::SmartQueue, stack::SmartStack, Item},
    MemoryClass, SamError, SingleAccessMachine,
};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, BinaryHeap},
};

type Result<T> = std::result::Result<T, SamError>;

/// Result returned by [`ObliviousGraph::dijkstra`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortestPathResult {
    pub total_cost: i64,
    pub costs: BTreeMap<usize, i64>,
    pub edges: Vec<(usize, usize, i64)>,
}

/// Result returned by [`ObliviousGraph::prim`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpanningTreeResult {
    pub total_cost: i64,
    pub edges: Vec<(usize, usize, i64)>,
}

/// Result returned by [`ObliviousGraph::directed_triangle_count_detailed`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TriangleCountResult {
    pub triangles: Vec<(usize, usize, usize)>,
    /// Whether every capped neighbor retrieval returned `max_neighbors` entries.
    pub complete: bool,
}

/// Visit counts and normalized ratios returned by [`ObliviousGraph::pagerank`].
#[derive(Clone, Debug, PartialEq)]
pub struct PageRankResult {
    pub visits: BTreeMap<usize, usize>,
    pub ratios: BTreeMap<usize, f64>,
}

/// A client-side priority-queue entry: smallest `(key, vertex, parent)` first.
struct Candidate<P> {
    key: i64,
    vertex: u64,
    parent: u64,
    pointer: P,
}

impl<P> Candidate<P> {
    fn order(&self) -> (i64, u64, u64) {
        (self.key, self.vertex, self.parent)
    }
}

impl<P> PartialEq for Candidate<P> {
    fn eq(&self, other: &Self) -> bool {
        self.order() == other.order()
    }
}

impl<P> Eq for Candidate<P> {}

impl<P> PartialOrd for Candidate<P> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<P> Ord for Candidate<P> {
    fn cmp(&self, other: &Self) -> Ordering {
        other.order().cmp(&self.order())
    }
}

/// Which priority a [`ObliviousGraph::best_first`] search uses.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Priority {
    /// Path cost (Dijkstra); the label is the cost.
    PathCost,
    /// Edge weight (Prim); the label is the parent id.
    EdgeWeight,
}

pub(super) fn validate_max_steps(max_steps: Option<usize>) -> Result<()> {
    if max_steps == Some(0) {
        return Err(SamError::InvalidParameter("max_steps must be positive"));
    }
    Ok(())
}

impl<P: Clone> ObliviousGraph<P> {
    fn start<B, S>(&mut self, name: u64, backend: &mut B, sam: &mut S) -> Result<(P, u64)>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        self.get_pointer(name, backend, sam)?
            .ok_or(SamError::InvalidParameter("start vertex is missing"))
    }

    /// Resets the `visited` flag of every vertex in `touched` and deletes
    /// the queued pointer copies.
    fn reset_visited<B, S>(
        &self,
        touched: &mut SmartQueue,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<()>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        while !touched.is_empty() {
            let (pointer, _) = split_entry(touched.dequeue::<B, P, S>(sam)?)?;
            let mut pointer = pointer.ok_or(SamError::InvalidPointerCell(
                "visited queue held no pointer",
            ))?;
            backend.with_value(sam, &mut pointer, |object, _, _| {
                if let super::GraphObject::Vertex(vertex) = object {
                    vertex.visited = false;
                }
                Ok(())
            })?;
            backend.delete(sam, &mut pointer)?;
        }
        touched.dequeue::<B, P, S>(sam)?;
        Ok(())
    }

    /// Executes one random walk of at most `walk_length` moves and returns
    /// the visited vertex ids (starting vertex first). Each move picks
    /// `rng.gen_range(0..out_degree)`; a sink ends the walk early.
    pub fn random_walk<B, S, R>(
        &mut self,
        start_name: u64,
        walk_length: usize,
        rng: &mut R,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<Vec<usize>>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
        R: rand::Rng + ?Sized,
    {
        let (mut current, start) = self.start(start_name, backend, sam)?;
        let mut trace = vec![start as usize];
        for _ in 0..walk_length {
            let Some((next, destination)) =
                self.random_neighbor(&mut current, rng, backend, sam)?
            else {
                break;
            };
            backend.delete(sam, &mut current)?;
            current = next;
            trace.push(destination as usize);
            self.mark(StepKind::Step, sam.stats().operations);
        }
        backend.delete(sam, &mut current)?;
        self.mark(StepKind::Tail, sam.stats().operations);
        Ok(trace)
    }

    /// Breadth-first traversal (visited when dequeued), bounded by the
    /// number of visited vertices. The frontier is a SAM queue of
    /// `(pointer, parent id)` entries; each vertex's label is its parent.
    pub fn bfs<B, S>(
        &mut self,
        start_name: u64,
        max_steps: Option<usize>,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<Vec<usize>>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        self.traverse(start_name, max_steps, false, backend, sam)
    }

    /// Depth-first traversal (visited when popped; neighbors pushed in edge
    /// order), bounded by the number of visited vertices. The frontier is a
    /// SAM stack of `(pointer, parent id)` entries.
    pub fn dfs<B, S>(
        &mut self,
        start_name: u64,
        max_steps: Option<usize>,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<Vec<usize>>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        self.traverse(start_name, max_steps, true, backend, sam)
    }

    fn traverse<B, S>(
        &mut self,
        start_name: u64,
        max_steps: Option<usize>,
        depth_first: bool,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<Vec<usize>>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        /// A queue (BFS) or stack (DFS) of frontier entries in SAM.
        enum Frontier {
            Queue(SmartQueue),
            Stack(SmartStack),
        }
        impl Frontier {
            fn push<P: Clone, B: GraphBackend<P>, S: SingleAccessMachine<B::Cell>>(
                &mut self,
                sam: &mut S,
                item: Item<P>,
            ) -> Result<()> {
                match self {
                    Self::Queue(queue) => {
                        queue.enqueue::<B, P, S>(sam, MemoryClass::Oblivious, item)
                    }
                    Self::Stack(stack) => stack.push::<B, P, S>(sam, MemoryClass::Oblivious, item),
                }
            }
            fn pop<P: Clone, B: GraphBackend<P>, S: SingleAccessMachine<B::Cell>>(
                &mut self,
                sam: &mut S,
            ) -> Result<Option<Item<P>>> {
                match self {
                    Self::Queue(queue) if queue.is_empty() => Ok(None),
                    Self::Queue(queue) => queue.dequeue::<B, P, S>(sam).map(Some),
                    Self::Stack(stack) if stack.is_empty() => Ok(None),
                    Self::Stack(stack) => stack.pop::<B, P, S>(sam).map(Some),
                }
            }
        }

        validate_max_steps(max_steps)?;
        let (start, _) = self.start(start_name, backend, sam)?;
        let mut frontier = if depth_first {
            Frontier::Stack(SmartStack::init())
        } else {
            Frontier::Queue(SmartQueue::init(sam, MemoryClass::Oblivious))
        };
        frontier.push::<P, B, S>(sam, entry_item(Some(start), None))?;
        let mut touched = SmartQueue::init(sam, MemoryClass::Oblivious);
        let mut path = Vec::new();
        loop {
            if max_steps.is_some_and(|limit| path.len() >= limit) {
                break;
            }
            let Some(item) = frontier.pop::<P, B, S>(sam)? else {
                break;
            };
            let (pointer, label) = split_entry(item)?;
            let mut pointer = pointer.ok_or(SamError::InvalidPointerCell(
                "frontier entry has no pointer",
            ))?;
            let visited = self.visit(
                &mut pointer,
                label,
                backend,
                sam,
                &mut |from, neighbor, _, sam| {
                    // A self-loop leads back to this now visited vertex: skip it.
                    match neighbor.pointer {
                        Some(next) => {
                            frontier.push::<P, B, S>(sam, entry_item(Some(next), Some(from as i64)))
                        }
                        None => Ok(()),
                    }
                },
            )?;
            match visited {
                Some(id) => {
                    touched.enqueue::<B, P, S>(
                        sam,
                        MemoryClass::Oblivious,
                        Item::Pointer(pointer),
                    )?;
                    path.push(id as usize);
                    self.mark(StepKind::Step, sam.stats().operations);
                }
                None => backend.delete(sam, &mut pointer)?,
            }
        }
        self.mark(StepKind::Tail, sam.stats().operations);
        while let Some(item) = frontier.pop::<P, B, S>(sam)? {
            if let (Some(mut pointer), _) = split_entry(item)? {
                backend.delete(sam, &mut pointer)?;
            }
        }
        if let Frontier::Queue(queue) = &mut frontier {
            queue.dequeue::<B, P, S>(sam)?;
        }
        self.reset_visited(&mut touched, backend, sam)?;
        self.mark(StepKind::Cleanup, sam.stats().operations);
        Ok(path)
    }

    /// Dijkstra's algorithm with visited-on-pop semantics, bounded by the
    /// number of settled vertices. Ties pop by smaller vertex id, then
    /// smaller parent id.
    pub fn dijkstra<B, S>(
        &mut self,
        start_name: u64,
        max_steps: Option<usize>,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<ShortestPathResult>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        let (costs, edges) =
            self.best_first(start_name, max_steps, Priority::PathCost, backend, sam)?;
        let total_cost = costs.values().copied().try_fold(0_i64, |total, cost| {
            total
                .checked_add(cost)
                .ok_or_else(|| SamError::Backend("Dijkstra total weight overflow".into()))
        })?;
        Ok(ShortestPathResult {
            total_cost,
            costs,
            edges,
        })
    }

    /// Prim's algorithm over outgoing edges with visited-on-pop semantics,
    /// bounded by the number of tree vertices.
    pub fn prim<B, S>(
        &mut self,
        start_name: u64,
        max_steps: Option<usize>,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<SpanningTreeResult>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        let (_, edges) =
            self.best_first(start_name, max_steps, Priority::EdgeWeight, backend, sam)?;
        let total_cost = edges.iter().try_fold(0_i64, |total, (_, _, weight)| {
            total
                .checked_add(*weight)
                .ok_or_else(|| SamError::Backend("Prim total weight overflow".into()))
        })?;
        Ok(SpanningTreeResult { total_cost, edges })
    }

    /// Shared loop of Dijkstra and Prim: returns each settled vertex's key
    /// and the `(parent, vertex, key)` edges used to reach them.
    #[allow(clippy::type_complexity)]
    fn best_first<B, S>(
        &mut self,
        start_name: u64,
        max_steps: Option<usize>,
        priority: Priority,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<(BTreeMap<usize, i64>, Vec<(usize, usize, i64)>)>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        validate_max_steps(max_steps)?;
        let (pointer, start) = self.start(start_name, backend, sam)?;
        let mut heap = BinaryHeap::from([Candidate {
            key: 0,
            vertex: start,
            parent: start,
            pointer,
        }]);
        let mut touched = SmartQueue::init(sam, MemoryClass::Oblivious);
        let mut keys = BTreeMap::new();
        let mut edges = Vec::new();
        loop {
            if max_steps.is_some_and(|limit| keys.len() >= limit) {
                break;
            }
            let Some(candidate) = heap.pop() else {
                break;
            };
            let Candidate {
                key,
                parent,
                mut pointer,
                ..
            } = candidate;
            let label = match priority {
                Priority::PathCost => key,
                Priority::EdgeWeight => parent as i64,
            };
            let visited = self.visit(
                &mut pointer,
                Some(label),
                backend,
                sam,
                &mut |from, neighbor, _, _| {
                    let Neighbor {
                        pointer,
                        destination,
                        weight,
                    } = neighbor;
                    // A self-loop leads back to this now settled vertex: skip it.
                    let Some(pointer) = pointer else {
                        return Ok(());
                    };
                    let key = match priority {
                        Priority::PathCost => key.checked_add(weight).ok_or_else(|| {
                            SamError::Backend("Dijkstra path weight overflow".into())
                        })?,
                        Priority::EdgeWeight => weight,
                    };
                    heap.push(Candidate {
                        key,
                        vertex: destination,
                        parent: from,
                        pointer,
                    });
                    Ok(())
                },
            )?;
            match visited {
                Some(id) => {
                    touched.enqueue::<B, P, S>(
                        sam,
                        MemoryClass::Oblivious,
                        Item::Pointer(pointer),
                    )?;
                    keys.insert(id as usize, key);
                    if id != start {
                        edges.push((parent as usize, id as usize, key));
                    }
                    self.mark(StepKind::Step, sam.stats().operations);
                }
                None => backend.delete(sam, &mut pointer)?,
            }
        }
        self.mark(StepKind::Tail, sam.stats().operations);
        for mut candidate in heap.into_vec() {
            backend.delete(sam, &mut candidate.pointer)?;
        }
        self.reset_visited(&mut touched, backend, sam)?;
        self.mark(StepKind::Cleanup, sam.stats().operations);
        Ok((keys, edges))
    }

    /// Intersection of the outgoing neighborhoods (destination ids) of two
    /// vertices.
    pub fn contact_discovery<B, S>(
        &mut self,
        first_name: u64,
        second_name: u64,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<BTreeSet<usize>>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        let mut sets = [BTreeSet::new(), BTreeSet::new()];
        for (name, set) in [first_name, second_name].into_iter().zip(&mut sets) {
            let id = self
                .vertex_id(name, backend, sam)?
                .ok_or(SamError::InvalidParameter("start vertex is missing"))?;
            self.with_scan(id, None, false, backend, sam, &mut |neighbor, _, _| {
                set.insert(neighbor.destination as usize);
                Ok(())
            })?;
            self.mark(StepKind::Step, sam.stats().operations);
        }
        self.mark(StepKind::Tail, sam.stats().operations);
        let [first, second] = sets;
        Ok(first.intersection(&second).copied().collect())
    }

    /// Counts directed cycles `source -> first -> second -> source`.
    pub fn directed_triangle_count<B, S>(
        &mut self,
        source_name: u64,
        max_neighbors: Option<usize>,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<Vec<(usize, usize, usize)>>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        Ok(self
            .directed_triangle_count_detailed(source_name, max_neighbors, backend, sam)?
            .triangles)
    }

    /// Like [`Self::directed_triangle_count`], but also reports whether the
    /// trial ran to its full length: every neighbor retrieval at all three
    /// levels returned exactly `max_neighbors` entries. Without a cap every
    /// trial is complete. Each level considers the first `max_neighbors`
    /// edges; the first two levels are SAM queues of pointer copies.
    pub fn directed_triangle_count_detailed<B, S>(
        &mut self,
        source_name: u64,
        max_neighbors: Option<usize>,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<TriangleCountResult>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        validate_max_neighbors(max_neighbors)?;
        let full = |length: u64| max_neighbors.is_none_or(|limit| length == limit as u64);
        let source = self
            .vertex_id(source_name, backend, sam)?
            .ok_or(SamError::InvalidParameter("start vertex is missing"))?;
        let mut complete = true;
        let mut triangles = Vec::new();

        // Level-one and level-two neighbors as `(pointer, id)` entries; a
        // self-loop is queued without a pointer and aliased when dequeued.
        let mut firsts = SmartQueue::init(sam, MemoryClass::Oblivious);
        let count = self.with_scan(
            source,
            max_neighbors,
            true,
            backend,
            sam,
            &mut |neighbor, _, sam| {
                let item = entry_item(neighbor.pointer, Some(neighbor.destination as i64));
                firsts.enqueue::<B, P, S>(sam, MemoryClass::Oblivious, item)
            },
        )?;
        self.mark(StepKind::Step, sam.stats().operations);
        complete &= full(count);
        while !firsts.is_empty() {
            let (mut first_pointer, first) = self.dequeue_neighbor(&mut firsts, backend, sam)?;
            let mut seconds = SmartQueue::init(sam, MemoryClass::Oblivious);
            let count = self.scan_pointer(
                &mut first_pointer,
                max_neighbors,
                true,
                backend,
                sam,
                &mut |neighbor, _, sam| {
                    let item = entry_item(neighbor.pointer, Some(neighbor.destination as i64));
                    seconds.enqueue::<B, P, S>(sam, MemoryClass::Oblivious, item)
                },
            )?;
            backend.delete(sam, &mut first_pointer)?;
            self.mark(StepKind::Step, sam.stats().operations);
            complete &= full(count);
            while !seconds.is_empty() {
                let (mut second_pointer, second) =
                    self.dequeue_neighbor(&mut seconds, backend, sam)?;
                let mut closes = false;
                let count = self.scan_pointer(
                    &mut second_pointer,
                    max_neighbors,
                    false,
                    backend,
                    sam,
                    &mut |neighbor, _, _| {
                        closes |= neighbor.destination == source;
                        Ok(())
                    },
                )?;
                backend.delete(sam, &mut second_pointer)?;
                self.mark(StepKind::Step, sam.stats().operations);
                complete &= full(count);
                if closes {
                    triangles.push((source as usize, first as usize, second as usize));
                }
            }
            seconds.dequeue::<B, P, S>(sam)?;
        }
        firsts.dequeue::<B, P, S>(sam)?;
        self.mark(StepKind::Tail, sam.stats().operations);
        Ok(TriangleCountResult {
            triangles,
            complete,
        })
    }

    /// Dequeues a `(pointer, id)` neighbor entry; a self-loop marker gets a
    /// fresh pointer through the entry-point tree.
    fn dequeue_neighbor<B, S>(
        &mut self,
        queue: &mut SmartQueue,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<(P, u64)>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        let (pointer, id) = split_entry(queue.dequeue::<B, P, S>(sam)?)?;
        let id = id.ok_or(SamError::InvalidPointerCell("neighbor entry has no id"))? as u64;
        let pointer = match pointer {
            Some(pointer) => pointer,
            None => self
                .entry_copy(id, backend, sam)?
                .ok_or(SamError::InvalidPointerCell(
                    "self-loop vertex has no entry point",
                ))?,
        };
        Ok((pointer, id))
    }

    /// Random-walk PageRank approximation: at each of `walk_length` steps,
    /// with probability `damping_factor` the walk follows a random edge,
    /// otherwise (or at a sink, or at the start) it jumps to a random live
    /// vertex. Returns per-vertex visit counts and ratios.
    pub fn pagerank<B, S, R>(
        &mut self,
        walk_length: usize,
        damping_factor: f64,
        rng: &mut R,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<PageRankResult>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
        R: rand::Rng + ?Sized,
    {
        if walk_length == 0 {
            return Err(SamError::InvalidParameter(
                "PageRank walk length must be positive",
            ));
        }
        if !(0.0..=1.0).contains(&damping_factor) {
            return Err(SamError::InvalidParameter(
                "damping factor must be between zero and one",
            ));
        }
        let mut visits = BTreeMap::new();
        let mut current: Option<(P, u64)> = None;
        for _ in 0..walk_length {
            let probability = rng.gen::<f64>();
            if probability <= damping_factor {
                if let Some((mut pointer, _)) = current.take() {
                    current = self.random_neighbor(&mut pointer, rng, backend, sam)?;
                    backend.delete(sam, &mut pointer)?;
                }
            }
            if current.is_none() || probability > damping_factor {
                if let Some((mut pointer, _)) = current.take() {
                    backend.delete(sam, &mut pointer)?;
                }
                current = Some(self.random_vertex(rng, backend, sam)?);
            }
            let (_, id) = current.as_ref().expect("the walk has a current vertex");
            *visits.entry(*id as usize).or_insert(0) += 1;
            self.mark(StepKind::Step, sam.stats().operations);
        }
        if let Some((mut pointer, _)) = current.take() {
            backend.delete(sam, &mut pointer)?;
        }
        self.mark(StepKind::Tail, sam.stats().operations);
        let ratios = visits
            .iter()
            .map(|(vertex, count)| (*vertex, *count as f64 / walk_length as f64))
            .collect();
        Ok(PageRankResult { visits, ratios })
    }
}
