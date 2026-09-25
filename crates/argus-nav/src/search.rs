//! The searches (§12: "A* for individuals and flow fields for crowds").
//! Both are pure functions of a [`Grid`]: costs are whole numbers (a step
//! into a cell of cost 1 is [`STRAIGHT`], a diagonal one [`DIAGONAL`]), and
//! ties break by a fixed order, so the same grid gives the same path on
//! every run, after a seek, and in the browser.

use crate::grid::{Grid, DIAGONAL, STRAIGHT};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// A path found by A*.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    /// The cells from the start to the goal, both included.
    pub cells: Vec<u32>,
    /// What it costs, in cost units ([`STRAIGHT`] a step of cost 1).
    pub cost: u32,
    /// Cells the search took off the open list.
    pub expanded: u32,
}

/// Buffers a search reuses, so a search allocates nothing once warm.
#[derive(Default)]
pub struct Scratch {
    g: Vec<u32>,
    came: Vec<u32>,
    stamp: Vec<u32>,
    generation: u32,
    heap: BinaryHeap<Reverse<(u32, u32, u32)>>,
}

impl Scratch {
    fn begin(&mut self, n: usize) {
        if self.stamp.len() != n {
            self.g = vec![0; n];
            self.came = vec![0; n];
            self.stamp = vec![0; n];
            self.generation = 0;
        }
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.stamp.iter_mut().for_each(|s| *s = 0);
            self.generation = 1;
        }
        self.heap.clear();
    }
}

/// A* from `from` to `to`: the cheapest path, or none when the goal is
/// blocked or cannot be reached. The start may be blocked (an entity
/// standing in a wall walks out of it).
pub fn astar(grid: &Grid, from: u32, to: u32, s: &mut Scratch) -> Option<Route> {
    let n = grid.len();
    if from as usize >= n || to as usize >= n || !grid.open(to) {
        return None;
    }
    if from == to {
        return Some(Route {
            cells: vec![from],
            cost: 0,
            expanded: 0,
        });
    }
    s.begin(n);
    let (tc, tr) = grid.col_row(to);
    let least = grid.least as u32;
    let estimate = |i: u32| -> u32 {
        let (c, r) = grid.col_row(i);
        let (dx, dy) = (
            (c - tc).unsigned_abs() as u32,
            (r - tr).unsigned_abs() as u32,
        );
        let est = if grid.diagonal {
            let (lo, hi) = (dx.min(dy), dx.max(dy));
            STRAIGHT * (hi - lo) + DIAGONAL * lo
        } else {
            STRAIGHT * (dx + dy)
        };
        est * least
    };
    let gen = s.generation;
    s.g[from as usize] = 0;
    s.stamp[from as usize] = gen;
    s.came[from as usize] = from;
    s.heap.push(Reverse((estimate(from), estimate(from), from)));
    let mut expanded = 0u32;
    while let Some(Reverse((f, h, i))) = s.heap.pop() {
        let g = s.g[i as usize];
        // A stale entry: the cell was reached more cheaply since.
        if f != g + h {
            continue;
        }
        expanded += 1;
        if i == to {
            let mut cells = vec![to];
            let mut at = to;
            while at != from {
                at = s.came[at as usize];
                cells.push(at);
            }
            cells.reverse();
            return Some(Route {
                cells,
                cost: g,
                expanded,
            });
        }
        let (gs, cs, st) = (&mut s.g, &mut s.came, &mut s.stamp);
        let heap = &mut s.heap;
        grid.steps(i, |j, step| {
            let ng = g + step;
            let ju = j as usize;
            if st[ju] != gen || ng < gs[ju] {
                st[ju] = gen;
                gs[ju] = ng;
                cs[ju] = i;
                let hj = estimate(j);
                heap.push(Reverse((ng + hj, hj, j)));
            }
        });
    }
    None
}

/// Every cell's cost to reach one cell: a flow field.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    /// The cell it flows to.
    pub target: u32,
    /// Each cell's cost to reach it, in cost units; `u32::MAX` unreached.
    pub dist: Vec<u32>,
    /// Cells reached (the target among them).
    pub reached: u32,
    /// The grid's hash it was found on.
    pub grid: u64,
}

impl Field {
    /// A cell's cost to reach the target, if reached.
    pub fn cost(&self, i: u32) -> Option<u32> {
        self.dist
            .get(i as usize)
            .copied()
            .filter(|&d| d != u32::MAX)
    }
}

/// The flow field toward `target`, out to `range` cost units (0: the whole
/// grid), by Dijkstra with a bucket for each cost (Dial's algorithm: the
/// costs are small whole numbers). A step into a cell costs that cell's
/// cost, so a cell's value is what walking from it to the target costs.
pub fn flow(grid: &Grid, target: u32, range: u32) -> Field {
    let n = grid.len();
    let mut dist = vec![u32::MAX; n];
    let mut reached = 0u32;
    if (target as usize) < n {
        let most = grid.cost.iter().copied().max().unwrap_or(1).max(1) as usize;
        let span = most * if grid.diagonal { DIAGONAL } else { STRAIGHT } as usize + 1;
        let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); span];
        dist[target as usize] = 0;
        buckets[0].push(target);
        let mut pending = 1usize;
        let mut d = 0u32;
        while pending > 0 {
            let b = d as usize % span;
            while let Some(i) = buckets[b].pop() {
                pending -= 1;
                if dist[i as usize] != d {
                    continue;
                }
                reached += 1;
                // Nothing walks into a blocked cell (a target standing in a
                // wall is reached by nothing but itself).
                let into = grid.cost[i as usize] as u32;
                if into == 0 {
                    continue;
                }
                neighbours(grid, i, |j, straight| {
                    let nd = d + into * if straight { STRAIGHT } else { DIAGONAL };
                    if (range == 0 || nd <= range) && nd < dist[j as usize] {
                        dist[j as usize] = nd;
                        buckets[nd as usize % span].push(j);
                        pending += 1;
                    }
                });
            }
            d += 1;
        }
    }
    Field {
        target,
        dist,
        reached,
        grid: grid.hash,
    }
}

/// The open cells a step from `i` leads to (east, west, north, south, then
/// the diagonals when the grid allows them and both cells beside are open),
/// and whether the step is straight.
fn neighbours(grid: &Grid, i: u32, mut f: impl FnMut(u32, bool)) {
    let (c, r) = grid.col_row(i);
    let open = |c: i64, r: i64| grid.index(c, r).filter(|&j| grid.open(j));
    let e = open(c + 1, r);
    let w = open(c - 1, r);
    let n = open(c, r + 1);
    let s = open(c, r - 1);
    for j in [e, w, n, s].into_iter().flatten() {
        f(j, true);
    }
    if grid.diagonal {
        for (ok, dc, dr) in [
            (e.is_some() && n.is_some(), 1, 1),
            (w.is_some() && n.is_some(), -1, 1),
            (e.is_some() && s.is_some(), 1, -1),
            (w.is_some() && s.is_some(), -1, -1),
        ] {
            if ok {
                if let Some(j) = open(c + dc, r + dr) {
                    f(j, false);
                }
            }
        }
    }
}

/// The step down a field from `at`: the neighbour whose cost to the target
/// plus the step to it is least (ties by the fixed order); none at the
/// target or where the field does not reach.
pub fn downhill(grid: &Grid, field: &Field, at: u32) -> Option<u32> {
    let here = field.cost(at)?;
    if at == field.target || here == 0 {
        return None;
    }
    let mut best: Option<(u32, u32)> = None;
    grid.steps(at, |j, step| {
        if let Some(d) = field.cost(j) {
            let total = d.saturating_add(step);
            if best.is_none_or(|(b, _)| total < b) {
                best = Some((total, j));
            }
        }
    });
    best.map(|(_, j)| j)
}
