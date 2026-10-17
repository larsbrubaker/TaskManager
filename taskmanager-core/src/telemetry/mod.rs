//! Telemetry: samples of the machine, their history and the sampler (PLAN §4.2).
//!
//! - [`snapshot`] — `Snapshot`, one sample of everything, every value a `Reading`.
//! - [`machine`] — `MachineInfo`, the static facts (CPU, memory, disks, NICs, GPU/NPU)
//!   and the `ProcessorBrand` derivation.
//! - [`history`] — `History`, a fixed ring per series with the compressed 1x/2x/4x/8x view.
//! - [`series`] — `SeriesId` and `HistoryStore`, which keep every graphed series aligned.
//! - [`producer`] — `SampleProducer`, the sampling side (worker thread natively): polls a
//!   [`TelemetryProvider`](crate::platform::TelemetryProvider) on a drift-free schedule,
//!   numbers the snapshots (generations), reconciles process tables, emits
//!   `Arc<Snapshot>`.
//! - [`state`] — `TelemetryState`, the UI side: latest snapshot, history, machine info,
//!   health.
//! - [`sampler`] — `Sampler`, both halves on one thread (wasm, demo, tests).

pub mod history;
pub mod machine;
pub mod producer;
pub mod sampler;
pub mod series;
pub mod snapshot;
pub mod state;

pub use history::{History, DEFAULT_HISTORY_CAPACITY, MAX_COMPRESSION_REGIONS};
pub use machine::{
    processor_brand, CoreClass, CpuInfo, DiskInfo, DiskKind, GpuInfo, MachineInfo, MemoryInfo,
    NetInfo, NetKind, NpuInfo,
};
pub use producer::{
    SampleProducer, DEFAULT_SAMPLE_INTERVAL, EXITED_PROCESS_RETENTION, MAX_SAMPLE_INTERVAL,
    MIN_SAMPLE_INTERVAL,
};
pub use sampler::Sampler;
pub use series::{snapshot_series, HistoryStore, SeriesId};
pub use snapshot::{
    CoreSample, CpuSample, DiskSample, GpuSample, MemorySample, NetSample, NpuSample, PowerSample,
    PowerSource, PressureLevel, SensorKind, SensorSample, Snapshot, ThermalState,
};
pub use state::TelemetryState;
