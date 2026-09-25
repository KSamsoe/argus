//! A built navigation grid: cells in rows from row 0 at the bottom, each a
//! step cost (0 blocked). What searches read; built by the store from a
//! [`crate::NavGrid`] and what it covers.

/// Cost units a straight step into a cell of cost 1 takes.
pub const STRAIGHT: u32 = 10;
/// Cost units a diagonal step into a cell of cost 1 takes (10 √2, rounded).
pub const DIAGONAL: u32 = 14;

/// A built grid.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    /// World position of cell (0, 0)'s lower-left corner.
    pub origin: [f32; 2],
    /// World units a cell.
    pub cell: f32,
    /// Width in cells.
    pub width: u32,
    /// Height in cells.
    pub height: u32,
    /// Eight-way moves.
    pub diagonal: bool,
    /// Step cost of each cell, row by row from row 0 (0: blocked).
    pub cost: Vec<u8>,
    /// A hash of the geometry and the costs: equal hashes, equal grids.
    pub hash: u64,
    /// The lowest cost of an open cell (what A*'s estimate scales by).
    pub least: u8,
}

/// FNV-1a over bytes, continuing from `h`.
pub fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// FNV-1a's starting value.
pub const FNV: u64 = 0xcbf29ce484222325;

impl Grid {
    /// A grid of `width` x `height` cells of `fill`.
    pub fn new(
        origin: [f32; 2],
        cell: f32,
        width: u32,
        height: u32,
        diagonal: bool,
        fill: u8,
    ) -> Grid {
        let mut g = Grid {
            origin,
            cell: if cell > 0.0 { cell } else { 1.0 },
            width,
            height,
            diagonal,
            cost: vec![fill; width as usize * height as usize],
            hash: 0,
            least: 1,
        };
        g.seal();
        g
    }

    /// Work out the hash and the least cost after the costs changed.
    pub fn seal(&mut self) {
        let mut h = FNV;
        for v in [self.origin[0], self.origin[1], self.cell] {
            h = fnv(h, &v.to_le_bytes());
        }
        h = fnv(h, &self.width.to_le_bytes());
        h = fnv(h, &self.height.to_le_bytes());
        h = fnv(h, &[self.diagonal as u8]);
        h = fnv(h, &self.cost);
        self.hash = h;
        self.least = self
            .cost
            .iter()
            .copied()
            .filter(|&c| c > 0)
            .min()
            .unwrap_or(1);
    }

    /// Cells in all.
    pub fn len(&self) -> usize {
        self.cost.len()
    }

    /// No cells.
    pub fn is_empty(&self) -> bool {
        self.cost.is_empty()
    }

    /// The cell at column `c`, row `r`, if on the grid.
    pub fn index(&self, c: i64, r: i64) -> Option<u32> {
        (c >= 0 && r >= 0 && c < self.width as i64 && r < self.height as i64)
            .then(|| (r as u32) * self.width + c as u32)
    }

    /// Column and row of a cell.
    pub fn col_row(&self, i: u32) -> (i64, i64) {
        ((i % self.width) as i64, (i / self.width) as i64)
    }

    /// The column and row a world point is in (maybe off the grid).
    pub fn cell_of(&self, p: [f32; 2]) -> (i64, i64) {
        (
            ((p[0] - self.origin[0]) / self.cell).floor() as i64,
            ((p[1] - self.origin[1]) / self.cell).floor() as i64,
        )
    }

    /// The cell a world point is in, if on the grid.
    pub fn locate(&self, p: [f32; 2]) -> Option<u32> {
        let (c, r) = self.cell_of(p);
        self.index(c, r)
    }

    /// Whether a world point is on the grid.
    pub fn contains(&self, p: [f32; 2]) -> bool {
        self.locate(p).is_some()
    }

    /// A cell's centre in the world.
    pub fn centre(&self, i: u32) -> [f32; 2] {
        let (c, r) = self.col_row(i);
        [
            self.origin[0] + (c as f32 + 0.5) * self.cell,
            self.origin[1] + (r as f32 + 0.5) * self.cell,
        ]
    }

    /// Whether a cell is open.
    pub fn open(&self, i: u32) -> bool {
        self.cost[i as usize] > 0
    }

    /// Open cells.
    pub fn open_cells(&self) -> usize {
        self.cost.iter().filter(|&&c| c > 0).count()
    }

    /// The cells one step from `i` that can be entered from it, with what
    /// the step costs, in a fixed order (east, west, north, south, then
    /// the diagonals), so every search breaks ties the same way. A diagonal
    /// needs both cells beside it open.
    pub fn steps(&self, i: u32, mut f: impl FnMut(u32, u32)) {
        let (c, r) = self.col_row(i);
        let w = self.width as i64;
        let h = self.height as i64;
        let open = |c: i64, r: i64| -> Option<u32> {
            if c < 0 || r < 0 || c >= w || r >= h {
                return None;
            }
            let j = (r * w + c) as u32;
            (self.cost[j as usize] > 0).then_some(j)
        };
        let e = open(c + 1, r);
        let we = open(c - 1, r);
        let n = open(c, r + 1);
        let s = open(c, r - 1);
        for j in [e, we, n, s].into_iter().flatten() {
            f(j, self.cost[j as usize] as u32 * STRAIGHT);
        }
        if self.diagonal {
            let d = [
                (e.is_some() && n.is_some(), c + 1, r + 1),
                (we.is_some() && n.is_some(), c - 1, r + 1),
                (e.is_some() && s.is_some(), c + 1, r - 1),
                (we.is_some() && s.is_some(), c - 1, r - 1),
            ];
            for (ok, dc, dr) in d {
                if ok {
                    if let Some(j) = open(dc, dr) {
                        f(j, self.cost[j as usize] as u32 * DIAGONAL);
                    }
                }
            }
        }
    }

    /// One character a cell for rows `r0..=r1` and columns `c0..=c1`, top
    /// row first, every `step`th cell: `#` blocked, `.` cost 1, `2` to `9`,
    /// `+` above 9.
    pub fn rows(&self, c0: i64, r0: i64, c1: i64, r1: i64, step: usize) -> Vec<String> {
        let mut out = Vec::new();
        let mut r = r1;
        while r >= r0 {
            let mut line = String::new();
            let mut c = c0;
            while c <= c1 {
                line.push(match self.index(c, r).map(|i| self.cost[i as usize]) {
                    None => ' ',
                    Some(0) => '#',
                    Some(1) => '.',
                    Some(v @ 2..=9) => (b'0' + v) as char,
                    Some(_) => '+',
                });
                c += step as i64;
            }
            out.push(line);
            r -= step as i64;
        }
        out
    }
}
