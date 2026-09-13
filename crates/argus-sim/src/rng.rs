//! Randomness from named streams (§6 "Randomness"). Each system that wants
//! randomness declares a stream name and gets a PCG32 seeded from
//! `hash(run_seed, stream_name)`, so adding or reordering a system does not
//! perturb the sequence any other system sees. Integer-only, so the same
//! seed gives the same sequence on every platform.

/// PCG32 (XSH RR), O'Neill 2014. 64-bit state, 32-bit output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

const MULT: u64 = 6364136223846793005;

impl Pcg32 {
    /// Seed with an initial state and a stream selector.
    pub fn new(seed: u64, stream: u64) -> Pcg32 {
        let mut r = Pcg32 {
            state: 0,
            inc: (stream << 1) | 1,
        };
        r.next_u32();
        r.state = r.state.wrapping_add(seed);
        r.next_u32();
        r
    }

    /// Next 32 random bits.
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MULT).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Next 64 random bits.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        ((self.next_u32() as u64) << 32) | self.next_u32() as u64
    }

    /// Uniform in `[0, 1)` with 24 bits of precision.
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    /// Uniform in `[0, 1)` with 53 bits of precision.
    #[inline]
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// Uniform integer in `[0, n)`; `n == 0` returns 0. Unbiased (Lemire).
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let mut x = self.next_u32();
        let mut m = x as u64 * n as u64;
        let mut l = m as u32;
        if l < n {
            let t = n.wrapping_neg() % n;
            while l < t {
                x = self.next_u32();
                m = x as u64 * n as u64;
                l = m as u32;
            }
        }
        (m >> 32) as u32
    }

    /// Uniform integer in `[lo, hi]` inclusive. Returns `lo` if `hi < lo`.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        let span = (hi - lo) as u64 + 1;
        let v = if span <= u32::MAX as u64 {
            self.below(span as u32) as u64
        } else {
            self.next_u64() % span
        };
        lo + v as i64
    }

    /// True with probability `p`.
    pub fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }

    /// Fisher-Yates shuffle.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u32 + 1) as usize;
            items.swap(i, j);
        }
    }

    /// Pick one element, or `None` if empty.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            Some(&items[self.below(items.len() as u32) as usize])
        }
    }

    /// Raw state, for snapshots.
    pub fn state(&self) -> (u64, u64) {
        (self.state, self.inc)
    }

    /// Rebuild from raw state.
    pub fn from_state(state: u64, inc: u64) -> Pcg32 {
        Pcg32 {
            state,
            inc: inc | 1,
        }
    }
}

/// Derive a stream's seed from the run seed and its name: FNV-1a over the
/// name, mixed with the run seed through SplitMix64. Stable across platforms.
pub fn stream_seed(run_seed: u64, stream: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in stream.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    splitmix(run_seed ^ h.rotate_left(17))
}

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// A stream ready to use: seeded from the run seed and its name.
pub fn stream(run_seed: u64, name: &str) -> Pcg32 {
    let s = stream_seed(run_seed, name);
    Pcg32::new(s, s.rotate_left(32) ^ 0xda3e39cb94b95bdb)
}
