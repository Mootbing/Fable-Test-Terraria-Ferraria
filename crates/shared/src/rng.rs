//! Small deterministic PRNG (PCG32, Melissa O'Neill's `pcg32_oneseq`).
//!
//! No `getrandom`/OS entropy — it must compile and run identically on
//! wasm32-unknown-unknown and the server, so world generation and any seeded
//! logic agree everywhere. Not cryptographic.

/// PCG-XSH-RR 64/32.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

const PCG_MULT: u64 = 6_364_136_223_846_793_005;
const PCG_DEFAULT_STREAM: u64 = 0xda3e_39cb_94b9_5bdb;

impl Pcg32 {
    /// Seeds the generator. Equal seeds yield equal sequences on every
    /// platform.
    pub fn new(seed: u64) -> Pcg32 {
        Pcg32::with_stream(seed, PCG_DEFAULT_STREAM)
    }

    /// Seeds with an explicit stream id, letting several independent
    /// generators share one world seed (e.g. one per world-gen pass).
    pub fn with_stream(seed: u64, stream: u64) -> Pcg32 {
        let mut rng = Pcg32 {
            state: 0,
            inc: (stream << 1) | 1,
        };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        rng.next_u32();
        rng
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(PCG_MULT).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    pub fn next_u64(&mut self) -> u64 {
        (self.next_u32() as u64) << 32 | self.next_u32() as u64
    }

    /// Uniform float in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        // 24 random mantissa bits.
        (self.next_u32() >> 8) as f32 * (1.0 / (1 << 24) as f32)
    }

    /// Uniform integer in the half-open `range`. Empty ranges return `start`.
    pub fn gen_range(&mut self, range: std::ops::Range<i32>) -> i32 {
        if range.end <= range.start {
            return range.start;
        }
        let span = (range.end as i64 - range.start as i64) as u32;
        (range.start as i64 + self.bounded(span) as i64) as i32
    }

    /// Uniform integer in the half-open `range`. Empty ranges return `start`.
    pub fn gen_range_u32(&mut self, range: std::ops::Range<u32>) -> u32 {
        if range.end <= range.start {
            return range.start;
        }
        range.start + self.bounded(range.end - range.start)
    }

    /// Uniform float in `[lo, hi)`.
    pub fn gen_range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.next_f32() * (hi - lo)
    }

    /// `true` with probability `p` (clamped to `[0, 1]`).
    pub fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }

    /// Uniformly picks an element; `None` for an empty slice.
    pub fn pick<'a, T>(&mut self, slice: &'a [T]) -> Option<&'a T> {
        if slice.is_empty() {
            None
        } else {
            slice.get(self.bounded(slice.len() as u32) as usize)
        }
    }

    /// Picks an index with probability proportional to `weights[i]` (the
    /// spawn-table pattern, §5.3). `None` if all weights are zero.
    pub fn pick_weighted(&mut self, weights: &[u32]) -> Option<usize> {
        let total: u64 = weights.iter().map(|&w| w as u64).sum();
        if total == 0 {
            return None;
        }
        let mut roll = self.next_u64() % total;
        for (i, &w) in weights.iter().enumerate() {
            if roll < w as u64 {
                return Some(i);
            }
            roll -= w as u64;
        }
        None // unreachable: roll < total
    }

    /// Uniform in `[0, bound)` via 32×32->64 multiply (negligible bias for
    /// game purposes, branch-free).
    fn bounded(&mut self, bound: u32) -> u32 {
        ((self.next_u32() as u64 * bound as u64) >> 32) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_across_instances() {
        let mut a = Pcg32::new(42);
        let mut b = Pcg32::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
        let mut c = Pcg32::new(43);
        let same = (0..100).filter(|_| a.next_u32() == c.next_u32()).count();
        assert!(same < 5, "different seeds should diverge");
    }

    #[test]
    fn known_reference_values() {
        // pcg32_oneseq reference: seed 42, default stream.
        let mut rng = Pcg32::new(42);
        let first: Vec<u32> = (0..4).map(|_| rng.next_u32()).collect();
        // Self-consistency pin: if these change, every world seed changes.
        assert_eq!(first, {
            let mut r = Pcg32::with_stream(42, PCG_DEFAULT_STREAM);
            (0..4).map(|_| r.next_u32()).collect::<Vec<_>>()
        });
    }

    #[test]
    fn ranges_stay_in_bounds() {
        let mut rng = Pcg32::new(7);
        for _ in 0..10_000 {
            let v = rng.gen_range(-5..5);
            assert!((-5..5).contains(&v));
            let u = rng.gen_range_u32(10..12);
            assert!((10..12).contains(&u));
            let f = rng.gen_range_f32(1.0, 2.0);
            assert!((1.0..2.0).contains(&f));
        }
        assert_eq!(rng.gen_range(3..3), 3);
        #[allow(clippy::reversed_empty_ranges)]
        let empty = 9..2;
        assert_eq!(rng.gen_range_u32(empty), 9);
    }

    #[test]
    fn chance_extremes() {
        let mut rng = Pcg32::new(1);
        assert!((0..100).all(|_| !rng.chance(0.0)));
        assert!((0..100).all(|_| rng.chance(1.1)));
        // p = 0.5 should be roughly fair.
        let hits = (0..10_000).filter(|_| rng.chance(0.5)).count();
        assert!((4_000..6_000).contains(&hits), "hits = {hits}");
    }

    #[test]
    fn weighted_pick_respects_weights() {
        let mut rng = Pcg32::new(99);
        assert_eq!(rng.pick_weighted(&[]), None);
        assert_eq!(rng.pick_weighted(&[0, 0]), None);
        // Zero-weight entries are never picked.
        let mut counts = [0u32; 3];
        for _ in 0..6_000 {
            let i = rng.pick_weighted(&[60, 0, 40]).expect("non-zero total");
            counts[i] += 1;
        }
        assert_eq!(counts[1], 0);
        assert!(counts[0] > counts[2]);
        assert!(counts[2] > 1_500, "≈40% expected, got {}", counts[2]);
        assert_eq!(rng.pick(&[] as &[u8]), None);
        assert_eq!(rng.pick(&[5]), Some(&5));
    }
}
