//! Deterministic randomness for the demo machine: a seeded generator, a stateless hash
//! and smooth value noise.
//!
//! The demo must give identical snapshots for the same seed and the same sample times on
//! every target (wasm32 included), so it uses no OS entropy and no `rand` crate:
//! - [`Rng`] (xorshift64*, seeded through SplitMix64) drives one-off choices made in
//!   event order (building the process population, scheduling churn);
//! - [`hash`] / [`unit`] map `(seed, channel, index)` to a value with no state at all;
//! - [`noise`] interpolates hashed lattice values smoothly over simulated time, so load
//!   curves are pure functions of the simulated clock (independent of how often the
//!   sampler asks), and [`pulse`] places short random events (bursts, spikes) on it.

/// SplitMix64's finaliser: a strong 64-bit mix.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A stateless hash of `(seed, channel, index)`.
pub fn hash(seed: u64, channel: u64, index: i64) -> u64 {
    mix(mix(seed ^ mix(channel)) ^ (index as u64))
}

/// [`hash`] as a uniform value in `[0, 1)`.
pub fn unit(seed: u64, channel: u64, index: i64) -> f64 {
    (hash(seed, channel, index) >> 11) as f64 / (1u64 << 53) as f64
}

/// Smooth value noise in `[0, 1]` over `t` seconds, with lattice points every `period`
/// seconds: hashed lattice values joined by a smoothstep, so the curve is continuous and
/// has no corners.
pub fn noise(seed: u64, channel: u64, t: f64, period: f64) -> f64 {
    let x = t / period;
    let cell = x.floor();
    let frac = x - cell;
    let i = cell as i64;
    let a = unit(seed, channel, i);
    let b = unit(seed, channel, i + 1);
    let s = frac * frac * (3.0 - 2.0 * frac);
    a + (b - a) * s
}

/// Three octaves of [`noise`] (slow, medium, fast) weighted 0.5 / 0.3 / 0.2, in `[0, 1]`.
pub fn fbm(seed: u64, channel: u64, t: f64, slow_period: f64) -> f64 {
    0.5 * noise(seed, channel, t, slow_period)
        + 0.3 * noise(seed, channel.wrapping_add(1), t, slow_period / 3.7)
        + 0.2 * noise(seed, channel.wrapping_add(2), t, slow_period / 12.1)
}

/// A short random event in each `window`-second slot: with probability `chance` the slot
/// holds one event lasting `min_len..max_len` seconds (starting in the first part of the
/// slot, so it never spills into the next). Returns `(envelope, strength)`: `envelope` is
/// 0 outside the event and rises/falls smoothly to 1 within it (`ramp` seconds each way);
/// `strength` is a per-event value in `[0, 1)` (0 when there is no event).
pub fn pulse(
    seed: u64,
    channel: u64,
    t: f64,
    window: f64,
    chance: f64,
    (min_len, max_len): (f64, f64),
    ramp: f64,
) -> (f64, f64) {
    let slot = (t / window).floor() as i64;
    if unit(seed, channel, slot) >= chance {
        return (0.0, 0.0);
    }
    let len = min_len + (max_len - min_len) * unit(seed, channel ^ 0xA1, slot);
    let latest_start = (window - len).max(0.0);
    let start = slot as f64 * window + latest_start * unit(seed, channel ^ 0xB2, slot);
    let into = t - start;
    if into < 0.0 || into > len {
        return (0.0, 0.0);
    }
    let edge = into.min(len - into);
    let env = if ramp > 0.0 {
        let x = (edge / ramp).min(1.0);
        x * x * (3.0 - 2.0 * x)
    } else {
        1.0
    };
    (env, unit(seed, channel ^ 0xC3, slot))
}

/// xorshift64* seeded through SplitMix64: small, fast and identical on every target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// A generator for `seed` on stream `stream` (independent streams for independent
    /// uses, so one part of the simulation drawing more numbers never shifts another).
    pub fn new(seed: u64, stream: u64) -> Self {
        let state = mix(seed ^ mix(stream ^ 0x5EED));
        Self {
            state: if state == 0 {
                0x2545_F491_4F6C_DD1D
            } else {
                state
            },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    /// Uniform integer in `lo..=hi`.
    pub fn int(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + self.next_u64() % (hi - lo + 1)
    }

    /// True with probability `p`.
    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    /// A uniformly chosen element (`None` for an empty slice).
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        let i = (self.next_u64() % items.len() as u64) as usize;
        items.get(i)
    }

    /// Shuffle in place (Fisher–Yates).
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = (self.next_u64() % (i as u64 + 1)) as usize;
            items.swap(i, j);
        }
    }

    /// A log-uniform value in `[lo, hi)`: most values near `lo`, a long tail to `hi`.
    pub fn log_range(&mut self, lo: f64, hi: f64) -> f64 {
        (lo.ln() + (hi.ln() - lo.ln()) * self.unit()).exp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence_and_streams_differ() {
        let mut a = Rng::new(7, 1);
        let mut b = Rng::new(7, 1);
        let mut c = Rng::new(7, 2);
        let xs: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
        let ys: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
        let zs: Vec<u64> = (0..8).map(|_| c.next_u64()).collect();
        assert_eq!(xs, ys);
        assert_ne!(xs, zs);
    }

    #[test]
    fn ranges_are_respected() {
        let mut r = Rng::new(1, 0);
        for _ in 0..1000 {
            let u = r.unit();
            assert!((0.0..1.0).contains(&u));
            let v = r.range(5.0, 6.0);
            assert!((5.0..6.0).contains(&v));
            let n = r.int(3, 5);
            assert!((3..=5).contains(&n));
            let l = r.log_range(1.0, 100.0);
            assert!((1.0..100.0).contains(&l));
        }
        assert_eq!(r.int(9, 9), 9);
        assert!(r.pick::<u8>(&[]).is_none());
    }

    #[test]
    fn noise_is_smooth_bounded_and_pure() {
        let mut prev = noise(3, 9, 0.0, 10.0);
        for i in 1..2000 {
            let t = f64::from(i) * 0.05;
            let v = noise(3, 9, t, 10.0);
            assert!((0.0..=1.0).contains(&v));
            assert!((v - prev).abs() < 0.02, "jump at {t}");
            prev = v;
            assert_eq!(v, noise(3, 9, t, 10.0));
        }
        let f = fbm(3, 9, 12.5, 40.0);
        assert!((0.0..=1.0).contains(&f));
    }

    #[test]
    fn pulses_stay_inside_their_slot() {
        let mut seen = 0;
        for i in 0..100_000 {
            let t = f64::from(i) * 0.1;
            let (env, strength) = pulse(5, 1, t, 60.0, 0.5, (3.0, 12.0), 1.0);
            assert!((0.0..=1.0).contains(&env));
            if env > 0.0 {
                seen += 1;
                assert!((0.0..1.0).contains(&strength));
            }
        }
        // ~half of ~167 slots hold an event of 3-12 s: thousands of 0.1 s steps.
        assert!(seen > 1000, "{seen}");
    }
}
