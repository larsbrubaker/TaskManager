//! `SampleProducer`: the sampling side of telemetry (PLAN §4.2).
//!
//! Owns the [`TelemetryProvider`] and decides when a sample is due. Each sample gets the
//! next generation number (1, 2, 3, …) and the provider's health, has its process table
//! reconciled with the previous one, and is frozen into an `Arc<Snapshot>` for the
//! consumer ([`TelemetryState`](super::TelemetryState)). Natively the producer lives on a
//! worker thread and the `Arc`s cross to the UI without copying; on wasm (and in tests)
//! the [`Sampler`](super::Sampler) runs both halves on one thread.
//!
//! Process reconciliation:
//! - listing succeeded: processes missing since last time are kept as `Exited` rows for
//!   [`EXITED_PROCESS_RETENTION`] (`ProcessTable::merge_exited`);
//! - listing failed (`Unavailable` / `Unsupported`): the last known table is carried
//!   forward with its live measurements `Unavailable` (`ProcessTable::carried_forward`)
//!   so a failed read never looks like every process exiting.
//!
//! Scheduling does not drift: each due time is the previous due time plus the interval,
//! so callers that tick on a coarse clock (a 60 Hz frame loop) still get evenly spaced
//! samples. If the producer falls a whole interval behind (machine asleep, worker
//! stalled), it resynchronises to `now` instead of firing a burst of catch-up samples.

use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use super::machine::MachineInfo;
use super::snapshot::Snapshot;
use crate::platform::{ProviderHealth, TelemetryProvider};
use crate::reading::Reading;

/// The default time between samples.
pub const DEFAULT_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
/// The shortest settable interval.
pub const MIN_SAMPLE_INTERVAL: Duration = Duration::from_millis(500);
/// The longest settable interval.
pub const MAX_SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
/// How long a process that exited stays listed (PLAN §5.3).
pub const EXITED_PROCESS_RETENTION: Duration = Duration::from_secs(3);

/// Polls a provider on an interval and produces numbered, reconciled snapshots.
pub struct SampleProducer {
    provider: Box<dyn TelemetryProvider>,
    interval: Duration,
    /// When the latest sample was *due* (not when it was taken); the next is due one
    /// interval later.
    anchor: Option<Instant>,
    generation: u64,
    previous: Option<Arc<Snapshot>>,
}

impl SampleProducer {
    /// A producer over `provider` with the default interval. Takes no sample until the
    /// first [`tick`](Self::tick).
    pub fn new(provider: Box<dyn TelemetryProvider>) -> Self {
        Self {
            provider,
            interval: DEFAULT_SAMPLE_INTERVAL,
            anchor: None,
            generation: 0,
            previous: None,
        }
    }

    /// The time between samples.
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Set the time between samples, clamped to
    /// [`MIN_SAMPLE_INTERVAL`]..=[`MAX_SAMPLE_INTERVAL`]; returns the interval in effect.
    /// The next sample is due one new interval after the latest one was due.
    pub fn set_interval(&mut self, interval: Duration) -> Duration {
        self.interval = interval.clamp(MIN_SAMPLE_INTERVAL, MAX_SAMPLE_INTERVAL);
        self.interval
    }

    /// How long after `now` the next sample is due (zero if due already). A worker
    /// thread can sleep this long between ticks.
    pub fn due_in(&self, now: Instant) -> Duration {
        match self.anchor {
            None => Duration::ZERO,
            Some(anchor) => (anchor + self.interval).saturating_duration_since(now),
        }
    }

    /// Sample if one is due at `now`; `None` if it is not time yet.
    pub fn tick(&mut self, now: Instant) -> Option<Arc<Snapshot>> {
        let anchor = match self.anchor {
            None => now,
            Some(previous) => {
                let due = previous + self.interval;
                if now < due {
                    return None;
                }
                if now.saturating_duration_since(due) >= self.interval {
                    now
                } else {
                    due
                }
            }
        };
        self.anchor = Some(anchor);
        Some(self.sample(now))
    }

    /// Sample now regardless of the interval; the schedule restarts from `now`.
    pub fn force_sample(&mut self, now: Instant) -> Arc<Snapshot> {
        self.anchor = Some(now);
        self.sample(now)
    }

    /// The generation of the latest snapshot (0 before the first).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Read the static machine facts from the provider.
    pub fn machine_info(&mut self) -> Arc<MachineInfo> {
        Arc::new(self.provider.machine_info())
    }

    /// The provider's current health.
    pub fn health(&self) -> ProviderHealth {
        self.provider.health()
    }

    fn sample(&mut self, now: Instant) -> Arc<Snapshot> {
        let mut snapshot = self.provider.sample(now);
        self.generation += 1;
        snapshot.generation = self.generation;
        snapshot.health = self.provider.health();
        let previous_table = self.previous.as_ref().and_then(|p| p.processes.value());
        if let Some(previous) = previous_table {
            match &mut snapshot.processes {
                Reading::Value(table) => {
                    table.merge_exited(previous, now, EXITED_PROCESS_RETENTION);
                }
                Reading::Unavailable | Reading::Unsupported => {
                    snapshot.processes =
                        Reading::Value(previous.carried_forward(now, EXITED_PROCESS_RETENTION));
                }
            }
        }
        let snapshot = Arc::new(snapshot);
        self.previous = Some(Arc::clone(&snapshot));
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{ProcessKey, ProcessRow, ProcessStatus, ProcessTable};

    /// Replays a script of process listings (`None` = the listing failed); repeats the
    /// last entry when it runs out.
    struct ScriptedProvider {
        listings: Vec<Option<Vec<u32>>>,
        calls: usize,
    }

    impl TelemetryProvider for ScriptedProvider {
        fn machine_info(&mut self) -> MachineInfo {
            MachineInfo::default()
        }

        fn sample(&mut self, now: Instant) -> Snapshot {
            let mut s = Snapshot::new(now);
            s.generation = 999; // the producer must overwrite this
            let listing = self.listings.get(self.calls).or(self.listings.last());
            self.calls += 1;
            if let Some(Some(pids)) = listing {
                let rows = pids
                    .iter()
                    .map(|&pid| ProcessRow {
                        cpu_pct: Reading::Value(1.0),
                        status: ProcessStatus::Running,
                        ..ProcessRow::new(ProcessKey::new(pid, 1), "p")
                    })
                    .collect();
                s.processes = Reading::Value(ProcessTable::new(rows));
            }
            s
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Degraded(format!("call {}", self.calls))
        }
    }

    fn producer(listings: Vec<Option<Vec<u32>>>) -> SampleProducer {
        SampleProducer::new(Box::new(ScriptedProvider { listings, calls: 0 }))
    }

    /// Seconds, rounded to whole milliseconds so sums compare exactly.
    fn secs(s: f64) -> Duration {
        Duration::from_millis((s * 1000.0).round() as u64)
    }

    fn table(s: &Snapshot) -> &ProcessTable {
        s.processes.value().expect("a process table")
    }

    #[test]
    fn samples_only_when_due_and_stamps_generation_and_health() {
        let mut p = producer(vec![]);
        let t0 = Instant::now();
        assert_eq!(p.due_in(t0), Duration::ZERO);
        let first = p.tick(t0).expect("first tick samples");
        assert_eq!(first.generation, 1);
        assert_eq!(first.health, ProviderHealth::Degraded("call 1".into()));
        assert!(p.tick(t0 + secs(0.5)).is_none());
        assert_eq!(p.due_in(t0 + secs(0.25)), secs(0.75));
        assert!(p.tick(t0 + secs(0.999)).is_none());
        assert_eq!(p.tick(t0 + secs(1.0)).map(|s| s.generation), Some(2));
        assert_eq!(p.generation(), 2);
    }

    #[test]
    fn frame_clock_stepping_does_not_drift() {
        let mut p = producer(vec![]);
        let t0 = Instant::now();
        let frame = Duration::from_micros(16_700);
        let mut taken = Vec::new();
        for i in 0..599u32 {
            if let Some(s) = p.tick(t0 + frame * i) {
                taken.push(s.taken_at.saturating_duration_since(t0));
            }
        }
        assert_eq!(taken.len(), 10, "{taken:?}");
        for (n, at) in taken.iter().enumerate() {
            let ideal = Duration::from_secs(n as u64);
            assert!(
                *at >= ideal && *at - ideal < frame,
                "sample {n} at {at:?} drifted from {ideal:?}"
            );
        }
    }

    #[test]
    fn falling_a_whole_interval_behind_resyncs_without_a_burst() {
        let mut p = producer(vec![]);
        let t0 = Instant::now();
        p.tick(t0);
        // Asleep for 10 s: one sample on waking, then the schedule restarts from there.
        assert!(p.tick(t0 + secs(10.3)).is_some());
        assert!(p.tick(t0 + secs(10.4)).is_none());
        assert_eq!(p.due_in(t0 + secs(10.4)), secs(0.9));
        assert!(p.tick(t0 + secs(11.3)).is_some());
    }

    #[test]
    fn force_sample_restarts_the_schedule() {
        let mut p = producer(vec![]);
        let t0 = Instant::now();
        p.tick(t0);
        assert_eq!(p.force_sample(t0 + secs(0.1)).generation, 2);
        assert!(p.tick(t0 + secs(1.0)).is_none());
        assert!(p.tick(t0 + secs(1.1)).is_some());
        assert_eq!(p.generation(), 3);
    }

    #[test]
    fn interval_is_clamped() {
        let mut p = producer(vec![]);
        assert_eq!(p.interval(), DEFAULT_SAMPLE_INTERVAL);
        assert_eq!(p.set_interval(secs(0.1)), MIN_SAMPLE_INTERVAL);
        assert_eq!(p.set_interval(secs(60.0)), MAX_SAMPLE_INTERVAL);
        assert_eq!(p.set_interval(secs(2.0)), secs(2.0));
    }

    #[test]
    fn exited_processes_are_retained_then_dropped() {
        let mut p = producer(vec![Some(vec![1, 2]), Some(vec![1])]);
        let t0 = Instant::now();
        let gone = ProcessKey::new(2, 1);
        p.tick(t0);
        let s = p.tick(t0 + secs(1.0)).expect("due");
        assert!(table(&s).get(&gone).is_some_and(|r| r.is_exited()));
        let s = p.tick(t0 + secs(3.0)).expect("due");
        assert!(table(&s).get(&gone).is_some());
        let s = p.tick(t0 + secs(4.0)).expect("due");
        assert!(table(&s).get(&gone).is_none());
        assert_eq!(table(&s).len(), 1);
    }

    #[test]
    fn a_failed_listing_carries_the_last_table_forward() {
        let mut p = producer(vec![Some(vec![1, 2]), None, None, Some(vec![1, 2])]);
        let t0 = Instant::now();
        p.tick(t0);
        for i in 1..=2 {
            let s = p.tick(t0 + secs(f64::from(i))).expect("due");
            let t = table(&s);
            assert_eq!(t.len(), 2, "rows kept, not exited");
            assert!(t.iter().all(|r| !r.is_exited()));
            assert!(t.iter().all(|r| r.cpu_pct.is_unavailable()));
        }
        // Listing works again: live values are back, nobody exited.
        let s = p.tick(t0 + secs(3.0)).expect("due");
        let t = table(&s);
        assert_eq!(t.len(), 2);
        assert!(t.iter().all(|r| r.cpu_pct.is_value() && !r.is_exited()));
    }

    #[test]
    fn a_failed_first_listing_stays_unavailable() {
        let mut p = producer(vec![None]);
        let s = p.tick(Instant::now()).expect("due");
        assert!(s.processes.is_unavailable());
    }

    #[test]
    fn producer_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<SampleProducer>();
        assert_send::<Arc<Snapshot>>();
    }
}
