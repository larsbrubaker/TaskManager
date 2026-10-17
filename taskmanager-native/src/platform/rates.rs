//! Turning cumulative OS counters into per-second rates and per-sample deltas.
//!
//! The native provider samples cumulative totals (bytes a disk or interface moved, a
//! process's CPU milliseconds) and reports change since its previous sample. These helpers
//! keep the previous totals keyed by device id or `ProcessKey` and follow the provider
//! contract (PLAN §4.3): no previous total (first sample, a new device or process), no
//! elapsed time or a counter that went backwards (reset, wrap) is `Unavailable`, never a
//! zero.

use std::collections::HashMap;
use std::hash::Hash;
use std::time::Duration;

use taskmanager_core::Reading;

/// Previous totals per key, swapped each tick so keys that vanished are forgotten.
#[derive(Debug)]
pub struct CounterTracker<K> {
    previous: HashMap<K, u64>,
    current: HashMap<K, u64>,
}

impl<K> Default for CounterTracker<K> {
    fn default() -> Self {
        Self {
            previous: HashMap::new(),
            current: HashMap::new(),
        }
    }
}

impl<K: Hash + Eq> CounterTracker<K> {
    /// Start a new sample: this tick's totals become the previous ones.
    pub fn begin_tick(&mut self) {
        self.previous = std::mem::take(&mut self.current);
    }

    /// Record `total` for `key`; return the change since the previous tick.
    pub fn delta(&mut self, key: K, total: u64) -> Reading<u64> {
        let previous = self.previous.get(&key).copied();
        self.current.insert(key, total);
        delta_between(previous, total)
    }

    /// Record `total` for `key`; return its change per second over `elapsed`.
    pub fn rate(&mut self, key: K, total: u64, elapsed: Option<Duration>) -> Reading<f64> {
        let delta = self.delta(key, total);
        per_second(delta, elapsed)
    }
}

/// `now - previous`, or `Unavailable` without a previous total or if it went backwards.
pub fn delta_between(previous: Option<u64>, now: u64) -> Reading<u64> {
    match previous {
        Some(previous) if now >= previous => Reading::Value(now - previous),
        _ => Reading::Unavailable,
    }
}

/// `delta / elapsed` in units per second; `Unavailable` without a positive elapsed time.
pub fn per_second(delta: Reading<u64>, elapsed: Option<Duration>) -> Reading<f64> {
    match elapsed {
        Some(elapsed) if !elapsed.is_zero() => delta.map(|d| d as f64 / elapsed.as_secs_f64()),
        _ => Reading::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sample_is_unavailable_then_rates_follow() {
        let mut t = CounterTracker::default();
        let second = Some(Duration::from_secs(1));
        t.begin_tick();
        assert_eq!(t.rate("en0", 1_000, second), Reading::Unavailable);
        t.begin_tick();
        assert_eq!(
            t.rate("en0", 3_000, Some(Duration::from_millis(500))),
            Reading::Value(4_000.0)
        );
        t.begin_tick();
        assert_eq!(
            t.rate("en0", 3_000, second),
            Reading::Value(0.0),
            "a measured zero"
        );
    }

    #[test]
    fn resets_missing_time_and_vanished_keys_are_unavailable() {
        let mut t = CounterTracker::default();
        t.begin_tick();
        t.delta("disk", 500);
        t.begin_tick();
        assert_eq!(
            t.delta("disk", 100),
            Reading::Unavailable,
            "counter went backwards"
        );
        t.begin_tick();
        assert_eq!(
            t.rate("disk", 300, None),
            Reading::Unavailable,
            "no elapsed time"
        );
        assert_eq!(
            per_second(Reading::Value(5), Some(Duration::ZERO)),
            Reading::Unavailable
        );
        // A key absent for one tick starts over.
        t.begin_tick();
        t.begin_tick();
        assert_eq!(t.delta("disk", 900), Reading::Unavailable);
        assert_eq!(delta_between(Some(2), 7), Reading::Value(5));
    }
}
