//! A small, seedable random number generator (xoshiro256**).
//!
//! Templates must give the same paradigm for the same seed on every platform and
//! with every version of the app, so we keep our own generator instead of relying
//! on a crate whose streams may change between releases.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct Rng {
    s: [u64; 4],
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

static ENTROPY: AtomicU64 = AtomicU64::new(0);

/// A seed that differs on every call (time + counter).
pub fn fresh_seed() -> u64 {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let c = ENTROPY.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
    let mut s = t ^ c.rotate_left(17);
    splitmix64(&mut s)
}

impl Rng {
    /// `None` = a different stream every time (like Python's `random.Random(None)`).
    pub fn new(seed: Option<u64>) -> Self {
        let mut st = seed.unwrap_or_else(fresh_seed);
        let mut s = [0u64; 4];
        for x in &mut s {
            *x = splitmix64(&mut st);
        }
        Rng { s }
    }

    pub fn seeded(seed: u64) -> Self {
        Self::new(Some(seed))
    }

    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Uniform in [0, 1).
    pub fn random(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform in [a, b].
    pub fn uniform(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.random()
    }

    /// Uniform integer in 0..n (n > 0).
    pub fn below(&mut self, n: usize) -> usize {
        if n <= 1 {
            return 0;
        }
        // rejection sampling to avoid modulo bias
        let n64 = n as u64;
        let zone = u64::MAX - (u64::MAX % n64);
        loop {
            let v = self.next_u64();
            if v < zone {
                return (v % n64) as usize;
            }
        }
    }

    /// Integer in [lo, hi] inclusive.
    pub fn randint(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        lo + self.below((hi - lo + 1) as usize) as i64
    }

    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i + 1);
            v.swap(i, j);
        }
    }

    pub fn choice<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }

    /// `k` distinct elements of `v` in random order.
    pub fn sample<T: Clone>(&mut self, v: &[T], k: usize) -> Vec<T> {
        let mut idx: Vec<usize> = (0..v.len()).collect();
        let k = k.min(v.len());
        for i in 0..k {
            let j = i + self.below(idx.len() - i);
            idx.swap(i, j);
        }
        idx[..k].iter().map(|&i| v[i].clone()).collect()
    }

    /// `k` distinct numbers from `lo..hi`.
    pub fn sample_range(&mut self, lo: usize, hi: usize, k: usize) -> Vec<usize> {
        let v: Vec<usize> = (lo..hi.max(lo)).collect();
        self.sample(&v, k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_in_range() {
        let mut a = Rng::seeded(1);
        let mut b = Rng::seeded(1);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut r = Rng::seeded(7);
        for _ in 0..1000 {
            let x = r.uniform(2.0, 6.0);
            assert!((2.0..=6.0).contains(&x));
            assert!(r.below(5) < 5);
        }
        let s = r.sample_range(0, 10, 4);
        assert_eq!(s.len(), 4);
        let mut d = s.clone();
        d.sort();
        d.dedup();
        assert_eq!(d.len(), 4);
    }
}
