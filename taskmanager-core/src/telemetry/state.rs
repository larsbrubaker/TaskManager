//! `TelemetryState`: the consuming (UI) side of telemetry (PLAN §4.2).
//!
//! Receives `Arc<Snapshot>`s from a [`SampleProducer`](super::SampleProducer) (directly
//! in the single-threaded [`Sampler`](super::Sampler), or drained from a channel
//! natively) and keeps what the pages read: the latest snapshot, the
//! [`HistoryStore`], the shared [`MachineInfo`] and the provider health that came with
//! the latest snapshot.

use std::sync::Arc;

use super::history::DEFAULT_HISTORY_CAPACITY;
use super::machine::MachineInfo;
use super::series::HistoryStore;
use super::snapshot::Snapshot;
use crate::platform::ProviderHealth;

/// What the UI knows about the machine right now, and its history.
#[derive(Clone, Debug)]
pub struct TelemetryState {
    history: HistoryStore,
    latest: Option<Arc<Snapshot>>,
    machine: Arc<MachineInfo>,
    health: ProviderHealth,
}

impl TelemetryState {
    /// Empty state for `machine`, keeping `history_capacity` samples per series.
    pub fn new(machine: Arc<MachineInfo>, history_capacity: usize) -> Self {
        Self {
            history: HistoryStore::new(history_capacity),
            latest: None,
            machine,
            health: ProviderHealth::Healthy,
        }
    }

    /// Empty state for `machine` with the default history capacity.
    pub fn with_default_capacity(machine: Arc<MachineInfo>) -> Self {
        Self::new(machine, DEFAULT_HISTORY_CAPACITY)
    }

    /// Take a new snapshot: record its history and make it the latest. A snapshot whose
    /// generation is not newer than the latest (a duplicate or a stale delivery) is
    /// ignored; returns whether it was taken.
    pub fn ingest(&mut self, snapshot: Arc<Snapshot>) -> bool {
        if snapshot.generation <= self.generation() {
            return false;
        }
        self.history.record(&snapshot);
        self.health = snapshot.health.clone();
        self.latest = Some(snapshot);
        true
    }

    /// The latest snapshot, if any.
    pub fn latest(&self) -> Option<&Arc<Snapshot>> {
        self.latest.as_ref()
    }

    /// The latest snapshot's generation (0 before the first).
    pub fn generation(&self) -> u64 {
        self.latest.as_ref().map_or(0, |s| s.generation)
    }

    /// The static machine facts.
    pub fn machine(&self) -> &Arc<MachineInfo> {
        &self.machine
    }

    /// Replace the machine facts (after a refresh on the producer side).
    pub fn set_machine(&mut self, machine: Arc<MachineInfo>) {
        self.machine = machine;
    }

    /// Per-series history.
    pub fn history(&self) -> &HistoryStore {
        &self.history
    }

    /// The provider health that came with the latest snapshot (healthy before any).
    pub fn health(&self) -> &ProviderHealth {
        &self.health
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reading::Reading;
    use crate::telemetry::series::SeriesId;
    use web_time::Instant;

    fn snap(generation: u64, cpu: f32) -> Arc<Snapshot> {
        let mut s = Snapshot::new(Instant::now());
        s.generation = generation;
        s.cpu.total_pct = Reading::Value(cpu);
        s.health = ProviderHealth::Degraded(format!("g{generation}"));
        Arc::new(s)
    }

    #[test]
    fn ingest_records_history_and_keeps_the_arc() {
        let mut state = TelemetryState::new(Arc::new(MachineInfo::default()), 8);
        assert_eq!(state.generation(), 0);
        assert!(state.latest().is_none());
        assert_eq!(state.health(), &ProviderHealth::Healthy);

        let first = snap(1, 10.0);
        assert!(state.ingest(Arc::clone(&first)));
        assert!(state.latest().is_some_and(|l| Arc::ptr_eq(l, &first)));
        assert!(state.ingest(snap(2, 20.0)));
        assert_eq!(state.generation(), 2);
        assert_eq!(state.health(), &ProviderHealth::Degraded("g2".into()));
        let cpu: Vec<_> = state
            .history()
            .get(&SeriesId::CpuTotal)
            .map(|h| h.iter().collect())
            .unwrap_or_default();
        assert_eq!(cpu, vec![Reading::Value(10.0), Reading::Value(20.0)]);
    }

    #[test]
    fn stale_and_duplicate_snapshots_are_ignored() {
        let mut state = TelemetryState::with_default_capacity(Arc::new(MachineInfo::default()));
        assert!(state.ingest(snap(5, 1.0)));
        assert!(!state.ingest(snap(5, 2.0)));
        assert!(!state.ingest(snap(3, 3.0)));
        assert_eq!(state.history().samples_recorded(), 1);
        assert_eq!(state.generation(), 5);
    }

    #[test]
    fn machine_info_is_replaced_by_arc() {
        let mut state = TelemetryState::new(Arc::new(MachineInfo::default()), 4);
        let mut info = MachineInfo::default();
        info.cpu.logical_cores = 12;
        let info = Arc::new(info);
        state.set_machine(Arc::clone(&info));
        assert!(Arc::ptr_eq(state.machine(), &info));
    }
}
