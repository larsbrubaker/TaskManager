//! `Snapshot`: one sample of the whole machine (PLAN §4.2).
//!
//! A [`TelemetryProvider`](crate::platform::TelemetryProvider) produces one `Snapshot` per
//! sampler tick: system CPU totals and per-CPU load, memory composition, per-disk and
//! per-NIC rates, GPU/NPU activity, power and thermals, sensors and the process table.
//! The provider computes rates and deltas itself (it owns the previous counters); the
//! [`SampleProducer`](super::SampleProducer) stamps the generation number and the
//! provider's health, and the [`TelemetryState`](super::TelemetryState) records it into
//! the [`HistoryStore`](super::HistoryStore). Snapshots are immutable once produced and
//! cross from the sampling thread to the UI as `Arc<Snapshot>`.
//!
//! Every measured field is a [`Reading`], and every sub-struct's `Default` is "nothing
//! measured" (`Unavailable` everywhere, empty device lists), so a provider fills in only
//! what it actually measured. Device lists are keyed by a stable `id` that matches the
//! corresponding entry in [`MachineInfo`](super::MachineInfo).

use std::time::Duration;

use web_time::Instant;

use crate::platform::ProviderHealth;
use crate::process::ProcessTable;
use crate::reading::Reading;

/// One sample of the whole machine.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// Monotonic sample number, assigned by the [`SampleProducer`](super::SampleProducer)
    /// (the first snapshot is generation 1; providers leave it 0).
    pub generation: u64,
    /// When the sample was taken.
    pub taken_at: Instant,
    /// Time since the provider's previous sample (the window its rates cover); zero for
    /// the first sample, whose rates are `Unavailable`.
    pub elapsed: Duration,
    pub cpu: CpuSample,
    pub memory: MemorySample,
    pub disks: Vec<DiskSample>,
    pub networks: Vec<NetSample>,
    pub gpus: Vec<GpuSample>,
    pub npus: Vec<NpuSample>,
    pub power: PowerSample,
    pub sensors: Vec<SensorSample>,
    /// The process list. A provider whose listing failed returns `Unavailable`; the
    /// producer then substitutes the last known table with its live measurements
    /// `Unavailable` (see `ProcessTable::carried_forward`), so after the producer this is
    /// `Value` whenever any listing has ever succeeded.
    pub processes: Reading<ProcessTable>,
    /// Threads across all processes.
    pub thread_count: Reading<u64>,
    /// Time since the machine booted.
    pub uptime: Reading<Duration>,
    /// The provider's health as of this sample, stamped by the producer (providers leave
    /// it at the default).
    pub health: ProviderHealth,
}

impl Snapshot {
    /// An empty snapshot taken at `taken_at`: nothing measured, no devices, no processes.
    /// Providers start from this and fill in what they measured.
    pub fn new(taken_at: Instant) -> Self {
        Self {
            generation: 0,
            taken_at,
            elapsed: Duration::ZERO,
            cpu: CpuSample::default(),
            memory: MemorySample::default(),
            disks: Vec::new(),
            networks: Vec::new(),
            gpus: Vec::new(),
            npus: Vec::new(),
            power: PowerSample::default(),
            sensors: Vec::new(),
            processes: Reading::Unavailable,
            thread_count: Reading::Unavailable,
            uptime: Reading::Unavailable,
            health: ProviderHealth::Healthy,
        }
    }

    /// The disk sample with this id.
    pub fn disk(&self, id: &str) -> Option<&DiskSample> {
        self.disks.iter().find(|d| d.id == id)
    }

    /// The network interface sample with this id.
    pub fn network(&self, id: &str) -> Option<&NetSample> {
        self.networks.iter().find(|n| n.id == id)
    }

    /// The GPU sample with this id.
    pub fn gpu(&self, id: &str) -> Option<&GpuSample> {
        self.gpus.iter().find(|g| g.id == id)
    }

    /// The NPU sample with this id.
    pub fn npu(&self, id: &str) -> Option<&NpuSample> {
        self.npus.iter().find(|n| n.id == id)
    }

    /// The sensor sample with this id.
    pub fn sensor(&self, id: &str) -> Option<&SensorSample> {
        self.sensors.iter().find(|s| s.id == id)
    }
}

/// Whole-CPU load, clocks and per-logical-CPU load.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CpuSample {
    /// Utilisation across all logical CPUs, 0–100.
    pub total_pct: Reading<f32>,
    /// Time spent in the kernel across all logical CPUs, 0–100 (part of `total_pct`).
    pub kernel_pct: Reading<f32>,
    /// Current (average) clock.
    pub frequency_mhz: Reading<f32>,
    /// Package / die temperature.
    pub temperature_c: Reading<f32>,
    /// Package power.
    pub power_w: Reading<f32>,
    /// One entry per logical CPU, indexed like `CpuInfo::core_classes` indices.
    pub per_cpu: Vec<CoreSample>,
}

/// One logical CPU's load.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CoreSample {
    /// Utilisation, 0–100.
    pub util_pct: Reading<f32>,
    /// Kernel time, 0–100 (part of `util_pct`).
    pub kernel_pct: Reading<f32>,
}

/// How hard the OS is working to find memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PressureLevel {
    Normal,
    Elevated,
    Critical,
}

/// Memory composition. Sizes are bytes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemorySample {
    pub total: Reading<u64>,
    pub used: Reading<u64>,
    pub available: Reading<u64>,
    /// File cache.
    pub cached: Reading<u64>,
    /// Held by the memory compressor (macOS, Windows).
    pub compressed: Reading<u64>,
    /// Wired / non-paged.
    pub wired: Reading<u64>,
    /// Committed (Windows commit charge, Linux `Committed_AS`).
    pub committed: Reading<u64>,
    /// App memory (macOS anonymous pages).
    pub app: Reading<u64>,
    pub swap_used: Reading<u64>,
    pub swap_total: Reading<u64>,
    /// Memory pressure, 0–100.
    pub pressure_pct: Reading<f32>,
    pub pressure_level: Reading<PressureLevel>,
    /// Pages read in from backing store since the previous sample.
    pub page_ins: Reading<u64>,
    /// Pages written out to backing store since the previous sample.
    pub page_outs: Reading<u64>,
}

/// One physical disk's activity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiskSample {
    /// Matches `DiskInfo::id`.
    pub id: String,
    /// Bytes per second.
    pub read_bps: Reading<f64>,
    /// Bytes per second.
    pub write_bps: Reading<f64>,
    /// Active time, 0–100.
    pub active_pct: Reading<f32>,
    /// Average response time in milliseconds.
    pub response_ms: Reading<f32>,
    /// Bytes read since boot.
    pub total_read: Reading<u64>,
    /// Bytes written since boot.
    pub total_written: Reading<u64>,
}

/// One network interface's throughput.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetSample {
    /// Matches `NetInfo::id`.
    pub id: String,
    /// Bytes per second received.
    pub rx_bps: Reading<f64>,
    /// Bytes per second sent.
    pub tx_bps: Reading<f64>,
    /// Bytes received since the interface came up.
    pub total_rx: Reading<u64>,
    /// Bytes sent since the interface came up.
    pub total_tx: Reading<u64>,
}

/// One GPU's activity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuSample {
    /// Matches `GpuInfo::id`.
    pub id: String,
    /// Utilisation, 0–100.
    pub util_pct: Reading<f32>,
    /// Bytes of GPU memory in use.
    pub memory_used: Reading<u64>,
    /// Bytes of GPU memory available to it (dedicated, or the unified-memory budget).
    pub memory_total: Reading<u64>,
}

/// One NPU's activity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NpuSample {
    /// Matches `NpuInfo::id`.
    pub id: String,
    /// Active residency, 0–100.
    pub active_pct: Reading<f32>,
}

/// Where the machine is drawing power from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PowerSource {
    Ac,
    Battery,
}

/// The OS's thermal state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThermalState {
    Nominal,
    Fair,
    Serious,
    Critical,
}

/// Machine-wide power and thermal state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PowerSample {
    /// Whole-system power draw.
    pub system_w: Reading<f32>,
    pub source: Reading<PowerSource>,
    /// Battery charge, 0–100 (`Unsupported` without a battery).
    pub battery_pct: Reading<f32>,
    pub thermal_state: Reading<ThermalState>,
}

/// What a sensor measures (and so its unit).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SensorKind {
    /// Degrees Celsius.
    Temperature,
    /// Watts.
    Power,
    /// Megahertz.
    Clock,
    /// Percent, 0–100.
    Utilization,
    /// Revolutions per minute.
    Fan,
    /// Volts.
    Voltage,
}

/// One hardware sensor reading.
#[derive(Clone, Debug, PartialEq)]
pub struct SensorSample {
    /// Stable id, unique across the machine (e.g. `"cpu/die"`).
    pub id: String,
    pub kind: SensorKind,
    /// In the unit `kind` implies.
    pub value: Reading<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_snapshot_measured_nothing() {
        let s = Snapshot::new(Instant::now());
        assert_eq!(s.generation, 0);
        assert!(s.cpu.total_pct.is_unavailable());
        assert!(s.memory.total.is_unavailable());
        assert!(s.memory.pressure_level.is_unavailable());
        assert!(s.power.source.is_unavailable());
        assert!(s.uptime.is_unavailable());
        assert!(s.disks.is_empty() && s.processes.is_unavailable());
        assert_eq!(s.health, ProviderHealth::Healthy);
    }

    #[test]
    fn devices_are_found_by_id() {
        let mut s = Snapshot::new(Instant::now());
        s.disks.push(DiskSample {
            id: "disk0".into(),
            read_bps: Reading::Value(1.0),
            ..DiskSample::default()
        });
        s.networks.push(NetSample {
            id: "en0".into(),
            ..NetSample::default()
        });
        s.sensors.push(SensorSample {
            id: "cpu/die".into(),
            kind: SensorKind::Temperature,
            value: Reading::Value(51.0),
        });
        assert_eq!(
            s.disk("disk0").map(|d| d.read_bps),
            Some(Reading::Value(1.0))
        );
        assert!(s.disk("disk1").is_none());
        assert!(s.network("en0").is_some());
        assert!(s.gpu("gpu0").is_none() && s.npu("ane0").is_none());
        assert_eq!(
            s.sensor("cpu/die").map(|x| x.value),
            Some(Reading::Value(51.0))
        );
    }
}
