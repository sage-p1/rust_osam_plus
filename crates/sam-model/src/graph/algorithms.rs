//! Graph algorithms over [`ObliviousGraph`].
//!
//! Every algorithm finds its start vertex through the SAM-resident lookup
//! trees (once per run) and reaches every other vertex only by following
//! edge pointers:
//!
//! * Traversals (BFS, DFS, Dijkstra, Prim) keep their frontier and visited
//!   set on the client, so client state grows with the frontier. A frontier
//!   entry names the visited vertex and edge it came from; the edge pointer
//!   is copied only when the entry is popped for a visit, so no SAM work is spent on entries that are
//!   never visited and nothing in SAM needs resetting afterwards.
//! * Walks (random walk, PageRank) copy the chosen edge pointer at each move;
//!   triangle counting queues its level-one and level-two neighbors as
//!   pointer copies in SAM queues.
//!
//! Every pointer copy is deleted by the time an algorithm returns.
//! Vertices are reported by their ids.

use super::core::{entry_item, split_entry, validate_max_neighbors};
use super::{tree, GraphBackend, ObliviousGraph};
use crate::{structures::queue::SmartQueue, MemoryClass, SamError, SingleAccessMachine};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, BinaryHeap, HashSet, VecDeque},
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

/// A client-side frontier entry. Heap order: smallest `(key, vertex,
/// parent)` first.
struct Candidate {
    key: i64,
    vertex: u64,
    parent: u64,
    /// Where the pointer to `vertex` is: an edge of a visited vertex, or
    /// (`None`) the start pointer.
    source: Option<Source>,
}

/// Edge `edge` of the `visit`-th visited vertex.
#[derive(Clone, Copy)]
struct Source {
    visit: usize,
    edge: u64,
}

/// One frontier entry of the `visit`-th visited vertex was popped: deletes
/// its held pointer once none remain.
fn release<P, B, S>(
    visit: usize,
    held: &mut [Option<P>],
    pending: &mut [usize],
    backend: &mut B,
    sam: &mut S,
) -> Result<()>
where
    P: Clone,
    B: GraphBackend<P>,
    S: SingleAccessMachine<B::Cell>,
{
    pending[visit] -= 1;
    if pending[visit] == 0 {
        if let Some(mut pointer) = held[visit].take() {
            backend.delete(sam, &mut pointer)?;
        }
    }
    Ok(())
}

impl Candidate {
    fn order(&self) -> (i64, u64, u64) {
        (self.key, self.vertex, self.parent)
    }
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.order() == other.order()
    }
}

impl Eq for Candidate {}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        other.order().cmp(&self.order())
    }
}

/// The order in which a search pops its frontier.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Order {
    /// Breadth-first: a FIFO queue.
    Fifo,
    /// Depth-first: a LIFO stack.
    Lifo,
    /// Dijkstra: smallest path cost first.
    PathCost,
    /// Prim: smallest edge weight first.
    EdgeWeight,
}

/// One visited vertex: its key (path cost for Dijkstra, edge weight for
/// Prim, 0 otherwise) and the vertex it was reached from.
struct Visit {
    vertex: u64,
    key: i64,
    parent: u64,
}

/// A client-side frontier.
enum Frontier {
    Queue(VecDeque<Candidate>),
    Stack(Vec<Candidate>),
    Heap(BinaryHeap<Candidate>),
}

impl Frontier {
    fn new(order: Order) -> Self {
        match order {
            Order::Fifo => Self::Queue(VecDeque::new()),
            Order::Lifo => Self::Stack(Vec::new()),
            Order::PathCost | Order::EdgeWeight => Self::Heap(BinaryHeap::new()),
        }
    }

    fn push(&mut self, candidate: Candidate) {
        match self {
            Self::Queue(queue) => queue.push_back(candidate),
            Self::Stack(stack) => stack.push(candidate),
            Self::Heap(heap) => heap.push(candidate),
        }
    }

    fn pop(&mut self) -> Option<Candidate> {
        match self {
            Self::Queue(queue) => queue.pop_front(),
            Self::Stack(stack) => stack.pop(),
            Self::Heap(heap) => heap.pop(),
        }
    }
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
        }
        backend.delete(sam, &mut current)?;
        Ok(trace)
    }

    /// Breadth-first traversal (visited when dequeued), bounded by the
    /// number of visited vertices; see the module documentation.
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
    /// order), bounded by the number of visited vertices; see the
    /// module documentation.
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
        let order = if depth_first {
            Order::Lifo
        } else {
            Order::Fifo
        };
        Ok(self
            .search(start_name, max_steps, order, backend, sam)?
            .into_iter()
            .map(|visit| visit.vertex as usize)
            .collect())
    }

    /// The search loop shared by every traversal: pops frontier entries in
    /// `order`, skips vertices already visited, and visits the others until
    /// `max_steps` vertices are visited or the frontier is empty. Returns the
    /// visits in order.
    ///
    /// Vertices are reached only by following pointers (the start vertex
    /// through the lookup trees, once per run). The frontier and the visited
    /// set are client-side, so client state grows with the frontier, and
    /// pointer copies are made lazily:
    ///
    /// * visiting a vertex dereferences its pointer and scans its fan-out
    ///   tree for destination ids; each new destination becomes a frontier
    ///   entry `(destination id, visited source, edge index)`, without a copy;
    /// * the client holds the pointer of every visited vertex that still has
    ///   entries in the frontier;
    /// * an entry that is popped for a visit reopens its source and copies
    ///   only that edge; entries popped for vertices already visited, and
    ///   entries left in the frontier, cost nothing in SAM.
    ///
    /// A source's pointer is deleted once none of its entries remain, and
    /// every held pointer is deleted before the search returns.
    fn search<B, S>(
        &mut self,
        start_name: u64,
        max_steps: Option<usize>,
        order: Order,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<Vec<Visit>>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        validate_max_steps(max_steps)?;
        let check_live = self.tombstones() > 0;
        let (start_pointer, start) = self.start(start_name, backend, sam)?;
        let mut start_pointer = Some(start_pointer);
        let mut frontier = Frontier::new(order);
        frontier.push(Candidate {
            key: 0,
            vertex: start,
            parent: start,
            source: None,
        });
        // Pointers of visited vertices, by visit number, while they have
        // entries in the frontier, and how many entries each still has.
        let mut held: Vec<Option<P>> = Vec::new();
        let mut pending: Vec<usize> = Vec::new();
        // Breadth-first search enqueues each vertex once (its first
        // discovery fixes its position); the other orders may rediscover a
        // vertex later or with a better key, so they only skip vertices
        // already visited. Either way the visit order is that of pushing
        // every edge and skipping visited vertices when popped.
        let mut discovered = HashSet::from([start]);
        let mut visited = HashSet::new();
        let mut visits = Vec::new();
        let result = (|| {
            while max_steps.is_none_or(|limit| visits.len() < limit) {
                let Some(Candidate {
                    key,
                    vertex: id,
                    parent,
                    source,
                }) = frontier.pop()
                else {
                    break;
                };
                let fresh = visited.insert(id);
                let mut pointer = match source {
                    None => start_pointer
                        .take()
                        .ok_or(SamError::InvalidPointerCell("start entry popped twice"))?,
                    Some(Source { visit, .. }) if !fresh => {
                        release(visit, &mut held, &mut pending, backend, sam)?;
                        continue;
                    }
                    Some(Source { visit, edge }) => {
                        let from = held[visit]
                            .as_mut()
                            .ok_or(SamError::InvalidPointerCell("frontier source was released"))?;
                        let pointer = backend.with_value(sam, from, |object, backend, sam| {
                            tree::with_leaf(
                                tree::vertex_mut(object)?,
                                edge,
                                backend,
                                sam,
                                |leaf, backend, sam| {
                                    let edge = leaf.edge.as_mut().ok_or(
                                        SamError::InvalidPointerCell("fan-out leaf has no edge"),
                                    )?;
                                    backend.copy_pointer(sam, edge)
                                },
                            )
                        })?;
                        release(visit, &mut held, &mut pending, backend, sam)?;
                        pointer
                    }
                };
                let number = held.len();
                let mut entries = 0;
                let (visited, discovered, frontier) = (&visited, &mut discovered, &mut frontier);
                backend.with_value(sam, &mut pointer, |object, backend, sam| {
                    let mut edge = 0;
                    tree::scan(
                        tree::vertex_mut(object)?,
                        None,
                        false,
                        check_live,
                        backend,
                        sam,
                        &mut |neighbor, _, _| {
                            // Scans skip (and then remove) edges to deleted
                            // vertices, so live edges keep their scan order
                            // as their index.
                            let index = edge;
                            edge += 1;
                            let destination = neighbor.destination;
                            let new = match order {
                                Order::Fifo => discovered.insert(destination),
                                _ => !visited.contains(&destination),
                            };
                            if !new {
                                return Ok(());
                            }
                            let key = match order {
                                Order::PathCost => {
                                    key.checked_add(neighbor.weight).ok_or_else(|| {
                                        SamError::Backend("Dijkstra path weight overflow".into())
                                    })?
                                }
                                Order::EdgeWeight => neighbor.weight,
                                Order::Fifo | Order::Lifo => 0,
                            };
                            frontier.push(Candidate {
                                key,
                                vertex: destination,
                                parent: id,
                                source: Some(Source {
                                    visit: number,
                                    edge: index,
                                }),
                            });
                            entries += 1;
                            Ok(())
                        },
                    )
                })?;
                if entries == 0 {
                    backend.delete(sam, &mut pointer)?;
                    held.push(None);
                } else {
                    held.push(Some(pointer));
                }
                pending.push(entries);
                visits.push(Visit {
                    vertex: id,
                    key,
                    parent,
                });
            }
            Ok(())
        })();
        for mut pointer in held.into_iter().flatten().chain(start_pointer) {
            backend.delete(sam, &mut pointer)?;
        }
        result.map(|()| visits)
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
            self.best_first(start_name, max_steps, Order::PathCost, backend, sam)?;
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
        let (_, edges) = self.best_first(start_name, max_steps, Order::EdgeWeight, backend, sam)?;
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
        order: Order,
        backend: &mut B,
        sam: &mut S,
    ) -> Result<(BTreeMap<usize, i64>, Vec<(usize, usize, i64)>)>
    where
        B: GraphBackend<P>,
        S: SingleAccessMachine<B::Cell>,
    {
        let visits = self.search(start_name, max_steps, order, backend, sam)?;
        let start = visits.first().map(|visit| visit.vertex);
        let keys = visits
            .iter()
            .map(|visit| (visit.vertex as usize, visit.key))
            .collect();
        let edges = visits
            .iter()
            .filter(|visit| Some(visit.vertex) != start)
            .map(|visit| (visit.parent as usize, visit.vertex as usize, visit.key))
            .collect();
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
        }
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
                complete &= full(count);
                if closes {
                    triangles.push((source as usize, first as usize, second as usize));
                }
            }
            seconds.dequeue::<B, P, S>(sam)?;
        }
        firsts.dequeue::<B, P, S>(sam)?;
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
        }
        if let Some((mut pointer, _)) = current.take() {
            backend.delete(sam, &mut pointer)?;
        }
        let ratios = visits
            .iter()
            .map(|(vertex, count)| (*vertex, *count as f64 / walk_length as f64))
            .collect();
        Ok(PageRankResult { visits, ratios })
    }
}
