//! `Sampler`: producer and consumer on one thread (PLAN §4.2).
//!
//! Natively the [`SampleProducer`] runs on a worker thread and sends `Arc<Snapshot>`s
//! (and refreshed `Arc<MachineInfo>`s) to a [`TelemetryState`] on the UI thread. On wasm,
//! in tests and for the demo there is no worker: the `Sampler` owns both halves and
//! passes each snapshot straight across. Callers pass `now` (the frame clock) and call
//! [`Sampler::tick`] as often as they like; the producer decides when a sample is due.

use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use super::history::DEFAULT_HISTORY_CAPACITY;
use super::machine::MachineInfo;
use super::producer::SampleProducer;
use super::series::HistoryStore;
use super::snapshot::Snapshot;
use super::state::TelemetryState;
use crate::platform::{ProviderHealth, TelemetryProvider};

/// A [`SampleProducer`] feeding a [`TelemetryState`] directly.
pub struct Sampler {
    producer: SampleProducer,
    state: TelemetryState,
}

impl Sampler {
    /// A sampler over `provider` with the default interval and history capacity. Reads
    /// the machine info now; takes no sample until the first [`tick`](Self::tick).
    pub fn new(provider: Box<dyn TelemetryProvider>) -> Self {
        Self::with_history_capacity(provider, DEFAULT_HISTORY_CAPACITY)
    }

    /// As [`new`](Self::new), keeping `capacity` samples per series.
    pub fn with_history_capacity(provider: Box<dyn TelemetryProvider>, capacity: usize) -> Self {
        let mut producer = SampleProducer::new(provider);
        let state = TelemetryState::new(producer.machine_info(), capacity);
        Self { producer, state }
    }

    /// Sample if one is due at `now` and record it; `None` if it was not time yet.
    pub fn tick(&mut self, now: Instant) -> Option<Arc<Snapshot>> {
        let snapshot = self.producer.tick(now)?;
        self.state.ingest(Arc::clone(&snapshot));
        Some(snapshot)
    }

    /// Sample and record now regardless of the interval (the schedule restarts).
    pub fn force_sample(&mut self, now: Instant) -> Arc<Snapshot> {
        let snapshot = self.producer.force_sample(now);
        self.state.ingest(Arc::clone(&snapshot));
        snapshot
    }

    /// How long after `now` the next sample is due.
    pub fn due_in(&self, now: Instant) -> Duration {
        self.producer.due_in(now)
    }

    /// The time between samples.
    pub fn interval(&self) -> Duration {
        self.producer.interval()
    }

    /// Set the time between samples (clamped); returns the interval in effect.
    pub fn set_interval(&mut self, interval: Duration) -> Duration {
        self.producer.set_interval(interval)
    }

    /// Re-read the machine facts from the provider.
    pub fn refresh_machine_info(&mut self) -> &Arc<MachineInfo> {
        let machine = self.producer.machine_info();
        self.state.set_machine(machine);
        self.state.machine()
    }

    /// Everything the UI reads.
    pub fn state(&self) -> &TelemetryState {
        &self.state
    }

    /// The latest snapshot, if any.
    pub fn latest(&self) -> Option<&Arc<Snapshot>> {
        self.state.latest()
    }

    /// The latest snapshot's generation (0 before the first).
    pub fn generation(&self) -> u64 {
        self.state.generation()
    }

    /// The static machine facts.
    pub fn machine(&self) -> &Arc<MachineInfo> {
        self.state.machine()
    }

    /// Per-series history.
    pub fn history(&self) -> &HistoryStore {
        self.state.history()
    }

    /// The provider health as of the latest snapshot.
    pub fn health(&self) -> &ProviderHealth {
        self.state.health()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reading::Reading;
    use crate::telemetry::series::SeriesId;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// CPU readings from a script (repeating the last); counts machine-info reads.
    struct ScriptedProvider {
        cpu: Vec<Reading<f32>>,
        calls: usize,
        machine_reads: Arc<AtomicUsize>,
    }

    impl TelemetryProvider for ScriptedProvider {
        fn machine_info(&mut self) -> MachineInfo {
            let n = self.machine_reads.fetch_add(1, Ordering::SeqCst) + 1;
            let mut info = MachineInfo::default();
            info.cpu.logical_cores = n;
            info
        }

        fn sample(&mut self, now: Instant) -> Snapshot {
            let mut s = Snapshot::new(now);
            if let Some(cpu) = self.cpu.get(self.calls).or(self.cpu.last()) {
                s.cpu.total_pct = *cpu;
            }
            self.calls += 1;
            s
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Degraded("no SMC".into())
        }
    }

    fn sampler(cpu: Vec<Reading<f32>>) -> (Sampler, Arc<AtomicUsize>) {
        let reads = Arc::new(AtomicUsize::new(0));
        let provider = ScriptedProvider {
            cpu,
            calls: 0,
            machine_reads: Arc::clone(&reads),
        };
        (
            Sampler::with_history_capacity(Box::new(provider), 16),
            reads,
        )
    }

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn ticks_feed_the_state() {
        let (mut s, _) = sampler(vec![
            Reading::Value(10.0),
            Reading::Unavailable,
            Reading::Value(30.0),
        ]);
        let t0 = Instant::now();
        assert!(s.latest().is_none());
        for i in 0..3 {
            assert!(s.tick(t0 + secs(i)).is_some());
            assert!(s.tick(t0 + secs(i)).is_none());
        }
        assert_eq!(s.generation(), 3);
        let cpu: Vec<_> = s
            .history()
            .get(&SeriesId::CpuTotal)
            .map(|h| h.iter().collect())
            .unwrap_or_default();
        assert_eq!(
            cpu,
            vec![
                Reading::Value(10.0),
                Reading::Unavailable,
                Reading::Value(30.0)
            ]
        );
        assert_eq!(s.history().capacity(), 16);
        assert_eq!(s.health(), &ProviderHealth::Degraded("no SMC".into()));
        assert_eq!(s.state().generation(), 3);
    }

    #[test]
    fn force_sample_is_recorded_too() {
        let (mut s, _) = sampler(vec![Reading::Value(1.0)]);
        let t0 = Instant::now();
        let forced = s.force_sample(t0);
        assert!(s.latest().is_some_and(|l| Arc::ptr_eq(l, &forced)));
        assert_eq!(s.due_in(t0), s.interval());
        assert_eq!(s.set_interval(secs(2)), secs(2));
        assert_eq!(s.history().samples_recorded(), 1);
    }

    #[test]
    fn machine_info_is_read_once_and_on_refresh() {
        let (mut s, reads) = sampler(vec![]);
        s.tick(Instant::now());
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(s.machine().cpu.logical_cores, 1);
        assert_eq!(s.refresh_machine_info().cpu.logical_cores, 2);
        assert_eq!(s.machine().cpu.logical_cores, 2);
    }

    #[test]
    fn sampler_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Sampler>();
    }
}
