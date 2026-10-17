//! `SeriesId` and `HistoryStore`: which numbers get graphed, and their histories.
//!
//! Every graph in the app (Summary CPU trace, Performance device thumbnails, per-core
//! grids, sensor cards) draws one [`History`] looked up by [`SeriesId`]. The
//! [`TelemetryState`](super::TelemetryState) calls [`HistoryStore::record`] once per snapshot, which
//! pushes one sample into every series, so all histories stay aligned sample-for-sample:
//! - a series in the snapshot gets its reading (whatever variant it is);
//! - a series known before but absent now (disk unplugged, interface down) gets
//!   `Unavailable`: a gap, never a fake zero;
//! - a series seen for the first time is back-filled with `Unavailable` for every earlier
//!   sample, so its newest sample lines up with everyone else's.
//!
//! A series that has been absent long enough that its whole ring is gaps is dropped.

use std::collections::{BTreeMap, HashSet};

use super::history::{History, DEFAULT_HISTORY_CAPACITY};
use super::snapshot::Snapshot;
use crate::reading::Reading;

/// One graphed quantity. Device-specific series carry the device id from the snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SeriesId {
    /// Percent.
    CpuTotal,
    /// Percent.
    CpuKernel,
    /// Percent, by logical CPU index.
    CpuCore(usize),
    /// Percent, by logical CPU index.
    CpuCoreKernel(usize),
    /// °C.
    CpuTemperature,
    /// Watts.
    CpuPower,
    /// Bytes (as f32: graph precision, not accounting precision).
    MemoryUsed,
    /// Percent.
    MemoryPressure,
    /// Percent active time.
    DiskActive(String),
    /// Bytes per second.
    DiskRead(String),
    /// Bytes per second.
    DiskWrite(String),
    /// Bytes per second.
    NetRx(String),
    /// Bytes per second.
    NetTx(String),
    /// Percent.
    GpuUtil(String),
    /// Percent.
    NpuActive(String),
    /// Watts.
    SystemPower,
    /// In the sensor's own unit.
    Sensor(String),
}

/// Every series a snapshot carries, with this sample's reading, in a stable order.
pub fn snapshot_series(s: &Snapshot) -> Vec<(SeriesId, Reading<f32>)> {
    let f64_to_f32 = |r: Reading<f64>| r.map(|v| v as f32);
    let mut out = vec![
        (SeriesId::CpuTotal, s.cpu.total_pct),
        (SeriesId::CpuKernel, s.cpu.kernel_pct),
        (SeriesId::CpuTemperature, s.cpu.temperature_c),
        (SeriesId::CpuPower, s.cpu.power_w),
        (SeriesId::MemoryUsed, s.memory.used.map(|b| b as f32)),
        (SeriesId::MemoryPressure, s.memory.pressure_pct),
        (SeriesId::SystemPower, s.power.system_w),
    ];
    for (i, core) in s.cpu.per_cpu.iter().enumerate() {
        out.push((SeriesId::CpuCore(i), core.util_pct));
        out.push((SeriesId::CpuCoreKernel(i), core.kernel_pct));
    }
    for d in &s.disks {
        out.push((SeriesId::DiskActive(d.id.clone()), d.active_pct));
        out.push((SeriesId::DiskRead(d.id.clone()), f64_to_f32(d.read_bps)));
        out.push((SeriesId::DiskWrite(d.id.clone()), f64_to_f32(d.write_bps)));
    }
    for n in &s.networks {
        out.push((SeriesId::NetRx(n.id.clone()), f64_to_f32(n.rx_bps)));
        out.push((SeriesId::NetTx(n.id.clone()), f64_to_f32(n.tx_bps)));
    }
    for g in &s.gpus {
        out.push((SeriesId::GpuUtil(g.id.clone()), g.util_pct));
    }
    for n in &s.npus {
        out.push((SeriesId::NpuActive(n.id.clone()), n.active_pct));
    }
    for sensor in &s.sensors {
        out.push((SeriesId::Sensor(sensor.id.clone()), sensor.value));
    }
    out
}

/// One [`History`] per series, all the same length.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryStore {
    capacity: usize,
    series: BTreeMap<SeriesId, History>,
    samples_recorded: u64,
}

impl Default for HistoryStore {
    fn default() -> Self {
        Self::new(DEFAULT_HISTORY_CAPACITY)
    }
}

impl HistoryStore {
    /// An empty store whose histories hold `capacity` samples each.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            series: BTreeMap::new(),
            samples_recorded: 0,
        }
    }

    /// Push one sample from `snapshot` into every series (see the module docs). If a
    /// snapshot repeats a series id (two sensors with one id), the first wins.
    pub fn record(&mut self, snapshot: &Snapshot) {
        let capacity = self.capacity;
        let earlier = self.samples_recorded;
        let mut seen = HashSet::new();
        for (id, reading) in snapshot_series(snapshot) {
            if seen.contains(&id) {
                continue;
            }
            let history = self.series.entry(id.clone()).or_insert_with(|| {
                let mut h = History::new(capacity);
                h.pad_unavailable(earlier);
                h
            });
            history.push(reading);
            seen.insert(id);
        }
        for (id, history) in self.series.iter_mut() {
            if !seen.contains(id) {
                history.push(Reading::Unavailable);
            }
        }
        self.series.retain(|id, h| {
            seen.contains(id) || h.len() < h.capacity() || h.iter().any(|r| r.is_value())
        });
        self.samples_recorded += 1;
    }

    /// The history of one series.
    pub fn get(&self, id: &SeriesId) -> Option<&History> {
        self.series.get(id)
    }

    /// Every series held, in `SeriesId` order (CPU totals, then per-core, …, sensors).
    pub fn ids(&self) -> impl Iterator<Item = &SeriesId> {
        self.series.keys()
    }

    /// Series held.
    pub fn len(&self) -> usize {
        self.series.len()
    }

    /// Whether no series is held (nothing recorded yet).
    pub fn is_empty(&self) -> bool {
        self.series.is_empty()
    }

    /// Samples each history holds at most.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Snapshots recorded so far.
    pub fn samples_recorded(&self) -> u64 {
        self.samples_recorded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::snapshot::{CoreSample, DiskSample, SensorKind, SensorSample};
    use web_time::Instant;

    fn snap(cpu: f32, disks: &[(&str, f64)]) -> Snapshot {
        let mut s = Snapshot::new(Instant::now());
        s.cpu.total_pct = Reading::Value(cpu);
        s.disks = disks
            .iter()
            .map(|(id, rate)| DiskSample {
                id: (*id).into(),
                read_bps: Reading::Value(*rate),
                ..DiskSample::default()
            })
            .collect();
        s
    }

    fn all(store: &HistoryStore, id: &SeriesId) -> Vec<Reading<f32>> {
        store
            .get(id)
            .map(|h| h.iter().collect())
            .unwrap_or_default()
    }

    #[test]
    fn snapshot_lists_every_series() {
        let mut s = snap(10.0, &[("disk0", 5.0)]);
        s.cpu.per_cpu = vec![CoreSample::default(); 2];
        s.sensors.push(SensorSample {
            id: "fan0".into(),
            kind: SensorKind::Fan,
            value: Reading::Value(1200.0),
        });
        s.memory.used = Reading::Value(8 << 30);
        let ids: Vec<SeriesId> = snapshot_series(&s).into_iter().map(|(id, _)| id).collect();
        assert!(ids.contains(&SeriesId::CpuCore(1)));
        assert!(ids.contains(&SeriesId::CpuCoreKernel(0)));
        assert!(ids.contains(&SeriesId::DiskWrite("disk0".into())));
        assert!(ids.contains(&SeriesId::Sensor("fan0".into())));
        let mem = snapshot_series(&s)
            .into_iter()
            .find(|(id, _)| *id == SeriesId::MemoryUsed)
            .map(|(_, r)| r);
        assert_eq!(mem, Some(Reading::Value((8u64 << 30) as f32)));
    }

    #[test]
    fn ids_are_listed_in_order() {
        let mut store = HistoryStore::new(4);
        assert!(store.is_empty());
        store.record(&snap(1.0, &[("b", 1.0), ("a", 1.0)]));
        let ids: Vec<&SeriesId> = store.ids().collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
        assert_eq!(ids.first(), Some(&&SeriesId::CpuTotal));
    }

    #[test]
    fn records_one_sample_per_series_per_snapshot() {
        let mut store = HistoryStore::new(10);
        store.record(&snap(10.0, &[]));
        store.record(&snap(20.0, &[]));
        assert_eq!(store.samples_recorded(), 2);
        assert_eq!(
            all(&store, &SeriesId::CpuTotal),
            vec![Reading::Value(10.0), Reading::Value(20.0)]
        );
        // Unmeasured scalars are recorded as the gaps they are.
        assert_eq!(
            all(&store, &SeriesId::CpuTemperature),
            vec![Reading::Unavailable; 2]
        );
        assert!(store
            .ids()
            .all(|id| store.get(id).map(History::len) == Some(2)));
    }

    #[test]
    fn a_vanished_device_leaves_a_gap_not_a_zero() {
        let mut store = HistoryStore::new(10);
        store.record(&snap(1.0, &[("usb", 100.0)]));
        store.record(&snap(1.0, &[]));
        store.record(&snap(1.0, &[("usb", 300.0)]));
        assert_eq!(
            all(&store, &SeriesId::DiskRead("usb".into())),
            vec![
                Reading::Value(100.0),
                Reading::Unavailable,
                Reading::Value(300.0)
            ]
        );
    }

    #[test]
    fn a_new_device_is_backfilled_to_align() {
        let mut store = HistoryStore::new(10);
        store.record(&snap(1.0, &[]));
        store.record(&snap(2.0, &[]));
        store.record(&snap(3.0, &[("disk1", 7.0)]));
        let disk = store
            .get(&SeriesId::DiskRead("disk1".into()))
            .cloned()
            .unwrap();
        let cpu = store.get(&SeriesId::CpuTotal).cloned().unwrap();
        assert_eq!(disk.len(), cpu.len());
        assert_eq!(disk.total_pushed(), cpu.total_pushed());
        assert_eq!(
            disk.iter().collect::<Vec<_>>(),
            vec![
                Reading::Unavailable,
                Reading::Unavailable,
                Reading::Value(7.0)
            ]
        );
    }

    #[test]
    fn backfill_beyond_capacity_keeps_bucket_alignment() {
        let mut store = HistoryStore::new(4);
        for i in 0..9 {
            store.record(&snap(i as f32, &[]));
        }
        store.record(&snap(9.0, &[("disk1", 1.0)]));
        let disk = store
            .get(&SeriesId::DiskRead("disk1".into()))
            .cloned()
            .unwrap();
        let cpu = store.get(&SeriesId::CpuTotal).cloned().unwrap();
        assert_eq!(disk.total_pushed(), cpu.total_pushed());
        assert_eq!(disk.len(), 4);
        assert_eq!(disk.compressed(4, 2).len(), cpu.compressed(4, 2).len());
    }

    #[test]
    fn long_gone_devices_are_dropped() {
        let mut store = HistoryStore::new(3);
        store.record(&snap(1.0, &[("usb", 1.0)]));
        for _ in 0..2 {
            store.record(&snap(1.0, &[]));
            assert!(store.get(&SeriesId::DiskRead("usb".into())).is_some());
        }
        // Its last value has now scrolled out of the ring: nothing left but gaps.
        store.record(&snap(1.0, &[]));
        assert!(store.get(&SeriesId::DiskRead("usb".into())).is_none());
        // Present-but-unmeasured series are never dropped.
        assert!(store.get(&SeriesId::CpuTemperature).is_some());
    }

    #[test]
    fn duplicate_ids_record_the_first() {
        let mut store = HistoryStore::new(3);
        let mut s = snap(1.0, &[("d", 1.0), ("d", 2.0)]);
        s.disks[1].active_pct = Reading::Value(50.0);
        store.record(&s);
        assert_eq!(
            all(&store, &SeriesId::DiskRead("d".into())),
            vec![Reading::Value(1.0)]
        );
    }
}
