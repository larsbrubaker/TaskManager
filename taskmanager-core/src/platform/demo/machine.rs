//! `DemoMachine`: the whole simulated computer behind the demo provider and services.
//!
//! Holds the seed, the static [`MachineInfo`], the [`Population`] of processes and the
//! running byte counters, and keeps the simulated clock: the first sample is simulated
//! time zero and every later one is the real time since then, so identical seeds sampled
//! at identical offsets give identical snapshots on every target. [`DemoMachine::sample`]
//! evaluates the [`Load`] model, advances the process churn, measures the processes
//! ([`super::rows`]) and assembles the [`Snapshot`]. [`DemoMachine::end_process`] is what
//! the demo's "End task" does.
//!
//! Following the provider contract, values computed over an interval (CPU utilisation,
//! disk and network rates, disk active and response time, page-ins/outs, Neural Engine
//! residency, cluster residency sensors) are `Unavailable` on the first sample; levels
//! read directly (memory, temperatures, power, GPU utilisation, totals) are values from
//! the start.

use std::time::Duration;

use web_time::Instant;

use crate::platform::ActionError;
use crate::process::{ProcessKey, ProcessTable};
use crate::reading::Reading;
use crate::telemetry::{
    CoreSample, DiskSample, GpuSample, MachineInfo, NetSample, NpuSample, PowerSource,
    PressureLevel, SensorKind, SensorSample, Snapshot, ThermalState,
};

use super::load::Load;
use super::population::Population;
use super::profile::{
    self, NetRole, CLOCK_SENSORS, DISK_ID, GPU_ID, GPU_MEMORY_BUDGET, INTERFACES, MEMORY_TOTAL,
    NPU_ID, POWER_SENSORS, SWAP_TOTAL, TEMPERATURE_SENSORS, UTILIZATION_SENSORS,
};
use super::rng::Rng;
use super::rows::sample_rows;

/// Wall-clock time of the first sample, Unix milliseconds (2026-10-12 08:00 UTC). Fixed so
/// that start times are reproducible.
pub const DEMO_EPOCH_UNIX_MS: u64 = 1_791_792_000_000;

/// The simulated machine. Share it as `Arc<Mutex<DemoMachine>>` between a
/// [`DemoProvider`](super::DemoProvider) and [`DemoServices`](super::DemoServices).
#[derive(Clone, Debug, PartialEq)]
pub struct DemoMachine {
    seed: u64,
    info: MachineInfo,
    population: Population,
    /// The instant of the first sample (simulated time zero).
    origin: Option<Instant>,
    /// The instant of the previous sample.
    last: Option<Instant>,
    boot_unix_ms: u64,
    battery_start_pct: f64,
    disk_read_total: f64,
    disk_written_total: f64,
    /// Per interface in [`INTERFACES`] order: (received, sent) bytes.
    net_totals: Vec<(f64, f64)>,
}

impl DemoMachine {
    /// A machine drawn from `seed`.
    pub fn seeded(seed: u64) -> Self {
        let mut rng = Rng::new(seed, 0);
        let boot_uptime_s = rng.range(1.5, 4.0) * 86_400.0;
        let population = Population::new(seed, boot_uptime_s);
        let net_totals = INTERFACES
            .iter()
            .map(|&(_, _, _, role)| match role {
                NetRole::Primary => (boot_uptime_s * 60e3, boot_uptime_s * 9e3),
                NetRole::Loopback => (boot_uptime_s * 5e3, boot_uptime_s * 5e3),
                NetRole::Awdl => (boot_uptime_s * 40.0, boot_uptime_s * 25.0),
                NetRole::Idle => (0.0, 0.0),
            })
            .collect();
        Self {
            seed,
            info: profile::machine_info(seed),
            population,
            origin: None,
            last: None,
            boot_unix_ms: DEMO_EPOCH_UNIX_MS - (boot_uptime_s * 1000.0) as u64,
            battery_start_pct: rng.range(70.0, 90.0),
            disk_read_total: boot_uptime_s * 2.5e6,
            disk_written_total: boot_uptime_s * 1.2e6,
            net_totals,
        }
    }

    /// The seed this machine was drawn from.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The static facts.
    pub fn machine_info(&self) -> MachineInfo {
        self.info.clone()
    }

    /// End the simulated process `key` (and its descendants). Refuses with
    /// `IdentityChanged` if the pid now belongs to another process, `NotFound` if it is
    /// gone, and `PermissionDenied` for root-owned processes.
    pub fn end_process(&mut self, key: ProcessKey) -> Result<(), ActionError> {
        self.population.end(key)
    }

    /// Sample the machine at `now`.
    pub fn sample(&mut self, now: Instant) -> Snapshot {
        let origin = *self.origin.get_or_insert(now);
        let t = now.saturating_duration_since(origin).as_secs_f64();
        let elapsed = self.last.map(|last| now.saturating_duration_since(last));
        self.last = Some(now);
        let dt = elapsed.map_or(0.0, |e| e.as_secs_f64());
        let rate = |v: f32| reading(elapsed.is_some(), v);

        let load = Load::at(self.seed, t);
        self.population.advance(t);
        let (rows, threads) = sample_rows(
            self.seed,
            &mut self.population,
            &load,
            t,
            elapsed,
            self.boot_unix_ms,
        );

        let mut s = Snapshot::new(now);
        s.elapsed = elapsed.unwrap_or(Duration::ZERO);
        s.cpu.total_pct = rate(load.total_pct);
        s.cpu.kernel_pct = rate(load.kernel_pct);
        // Apple silicon does not publish a current clock for the whole CPU; the cluster
        // clocks are sensors below.
        s.cpu.frequency_mhz = Reading::Unavailable;
        s.cpu.temperature_c = Reading::Value(load.die_c.iter().sum::<f32>() / 4.0);
        s.cpu.power_w = Reading::Value(load.cpu_power_w);
        s.cpu.per_cpu = load
            .core_util
            .iter()
            .zip(load.core_kernel.iter())
            .map(|(&util, &kernel)| CoreSample {
                util_pct: rate(util),
                kernel_pct: rate(kernel),
            })
            .collect();
        self.fill_memory(&mut s, &load, dt, elapsed.is_some());
        self.fill_devices(&mut s, &load, dt, elapsed.is_some());
        s.power.system_w = Reading::Value(load.system_power_w);
        s.power.source = Reading::Value(PowerSource::Ac);
        s.power.battery_pct = Reading::Value((self.battery_start_pct + 0.01 * t).min(100.0) as f32);
        s.power.thermal_state = Reading::Value(ThermalState::Nominal);
        s.sensors = sensors(&load, elapsed.is_some());
        s.processes = Reading::Value(ProcessTable::new(rows));
        s.thread_count = Reading::Value(threads);
        s.uptime = Reading::Value(Duration::from_secs_f64(self.population.boot_uptime_s() + t));
        s
    }

    fn fill_memory(&self, s: &mut Snapshot, load: &Load, dt: f64, rates: bool) {
        let m = &mut s.memory;
        m.total = Reading::Value(MEMORY_TOTAL);
        m.used = Reading::Value(load.mem_used);
        m.available = Reading::Value(MEMORY_TOTAL - load.mem_used);
        m.cached = Reading::Value(load.mem_cached);
        m.compressed = Reading::Value(load.mem_compressed);
        m.wired = Reading::Value(load.mem_wired);
        // macOS has no commit charge.
        m.committed = Reading::Unsupported;
        m.app = Reading::Value(load.mem_app);
        m.swap_used = Reading::Value(load.swap_used.min(SWAP_TOTAL));
        m.swap_total = Reading::Value(SWAP_TOTAL);
        m.pressure_pct = Reading::Value(load.pressure_pct);
        m.pressure_level = Reading::Value(match load.pressure_pct {
            p if p >= 80.0 => PressureLevel::Critical,
            p if p >= 40.0 => PressureLevel::Elevated,
            _ => PressureLevel::Normal,
        });
        m.page_ins = reading(rates, (load.page_in_rate * dt) as u64);
        m.page_outs = reading(rates, (load.page_out_rate * dt) as u64);
    }

    fn fill_devices(&mut self, s: &mut Snapshot, load: &Load, dt: f64, rates: bool) {
        self.disk_read_total += load.disk_read_bps * dt;
        self.disk_written_total += load.disk_write_bps * dt;
        s.disks.push(DiskSample {
            id: DISK_ID.to_string(),
            read_bps: reading(rates, load.disk_read_bps),
            write_bps: reading(rates, load.disk_write_bps),
            active_pct: reading(rates, load.disk_active_pct),
            response_ms: reading(rates, load.disk_response_ms),
            total_read: Reading::Value(self.disk_read_total as u64),
            total_written: Reading::Value(self.disk_written_total as u64),
        });
        for (&(id, _, _, role), totals) in INTERFACES.iter().zip(self.net_totals.iter_mut()) {
            let (rx, tx) = match role {
                NetRole::Primary => (load.wifi_rx_bps, load.wifi_tx_bps),
                NetRole::Loopback => (load.loopback_bps, load.loopback_bps),
                NetRole::Awdl => (load.awdl_rx_bps, load.awdl_tx_bps),
                NetRole::Idle => (0.0, 0.0),
            };
            totals.0 += rx * dt;
            totals.1 += tx * dt;
            s.networks.push(NetSample {
                id: id.to_string(),
                rx_bps: reading(rates, rx),
                tx_bps: reading(rates, tx),
                total_rx: Reading::Value(totals.0 as u64),
                total_tx: Reading::Value(totals.1 as u64),
            });
        }
        s.gpus.push(GpuSample {
            id: GPU_ID.to_string(),
            util_pct: Reading::Value(load.gpu_pct),
            memory_used: Reading::Value(load.gpu_memory_used.min(GPU_MEMORY_BUDGET)),
            memory_total: Reading::Value(GPU_MEMORY_BUDGET),
        });
        s.npus.push(NpuSample {
            id: NPU_ID.to_string(),
            active_pct: reading(rates, load.npu_pct),
        });
    }
}

fn reading<T>(known: bool, value: T) -> Reading<T> {
    if known {
        Reading::Value(value)
    } else {
        Reading::Unavailable
    }
}

fn sensors(load: &Load, rates: bool) -> Vec<SensorSample> {
    let sensor = |id: &str, kind, value| SensorSample {
        id: id.to_string(),
        kind,
        value,
    };
    let temps = [
        load.die_c[0],
        load.die_c[1],
        load.die_c[2],
        load.die_c[3],
        load.cpu_hotspot_c,
        load.soc_hotspot_c,
        load.ssd_c,
    ];
    let powers = [
        load.cpu_power_w,
        load.gpu_power_w,
        load.ane_power_w,
        load.system_power_w,
    ];
    let clocks = [
        load.super_clock_mhz,
        load.efficiency_clock_mhz,
        load.gpu_clock_mhz,
    ];
    let active = [load.super_active_pct, load.efficiency_active_pct];
    let mut out = Vec::new();
    for ((id, _), v) in TEMPERATURE_SENSORS.iter().zip(temps) {
        out.push(sensor(id, SensorKind::Temperature, Reading::Value(v)));
    }
    for ((id, _), v) in POWER_SENSORS.iter().zip(powers) {
        out.push(sensor(id, SensorKind::Power, Reading::Value(v)));
    }
    for ((id, _), v) in CLOCK_SENSORS.iter().zip(clocks) {
        out.push(sensor(id, SensorKind::Clock, Reading::Value(v)));
    }
    for ((id, _), v) in UTILIZATION_SENSORS.iter().zip(active) {
        out.push(sensor(id, SensorKind::Utilization, reading(rates, v)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::demo::profile::GIB;

    #[test]
    fn memory_parts_fit_the_total() {
        let mut m = DemoMachine::seeded(8);
        let t0 = Instant::now();
        for i in 0..50u64 {
            let s = m.sample(t0 + Duration::from_secs(i * 37));
            let mem = &s.memory;
            let used = mem.used.value().copied().unwrap_or(0);
            let parts = mem.wired.value().copied().unwrap_or(0)
                + mem.compressed.value().copied().unwrap_or(0)
                + mem.app.value().copied().unwrap_or(0);
            assert!(parts <= used + GIB / 1024, "{parts} > {used}");
            let avail = mem.available.value().copied().unwrap_or(0);
            assert!(mem.cached.value().copied().unwrap_or(u64::MAX) <= avail);
        }
    }

    #[test]
    fn started_times_are_before_the_epoch_and_uptime_grows() {
        let mut m = DemoMachine::seeded(8);
        let t0 = Instant::now();
        let first = m.sample(t0);
        let later = m.sample(t0 + Duration::from_secs(10));
        let up = |s: &Snapshot| s.uptime.value().copied().unwrap_or_default();
        let grew = (up(&later) - up(&first)).as_secs_f64();
        assert!((grew - 10.0).abs() < 1e-3, "{grew}");
        let table = first.processes.value().cloned().unwrap_or_default();
        assert!(table.iter().all(|r| r
            .started_unix_ms
            .value()
            .is_some_and(|&ms| ms <= DEMO_EPOCH_UNIX_MS)));
    }
}
