//! The demo machine's load model: every moving system-level number as a pure function of
//! the seed and the simulated time.
//!
//! [`Load::at`] evaluates the model at `t` seconds of simulated time. Because nothing here
//! keeps state, the curves are the same whether the sampler asks every 0.5 s or every 5 s,
//! and the same seed always draws the same day. [`super::machine`] turns a `Load` into a
//! [`Snapshot`](crate::telemetry::Snapshot), integrating the byte and page counters.
//!
//! The shapes follow a quiet laptop: CPU 5–38 % with bursts to ~90 % every few minutes
//! lasting seconds; Efficiency cores carry light load and Super cores take over as it
//! grows; kernel time about a quarter of user time; temperatures following a lagged
//! average of the load (45–70 °C); power 5–30 W tracking it; memory drifting slowly
//! between 17 and 21 GB with pressure mostly Normal; spiky disk traffic whose active time
//! comes from the rates; bursty Wi-Fi with receive above send; GPU 0–30 %; a Neural Engine
//! that is idle apart from rare short jobs.

use super::profile::{GIB, LOGICAL_CPUS, MEMORY_TOTAL, SUPER_CORES};
use super::rng::{fbm, noise, pulse};

// Noise channels: one per independent curve.
const CH_CPU: u64 = 0x100;
const CH_BURST: u64 = 0x110;
const CH_CORE: u64 = 0x200;
const CH_KERNEL: u64 = 0x300;
const CH_DIE: u64 = 0x400;
const CH_POWER: u64 = 0x500;
const CH_MEM: u64 = 0x600;
const CH_WIRED: u64 = 0x610;
const CH_CACHE: u64 = 0x620;
const CH_SWAP: u64 = 0x630;
const CH_DREAD: u64 = 0x700;
const CH_DWRITE: u64 = 0x710;
const CH_DREAD_BASE: u64 = 0x720;
const CH_DWRITE_BASE: u64 = 0x730;
const CH_DRESP: u64 = 0x740;
const CH_NETRX: u64 = 0x800;
const CH_NETTX: u64 = 0x810;
const CH_NETBASE: u64 = 0x820;
const CH_LOOP: u64 = 0x830;
const CH_AWDL: u64 = 0x840;
const CH_GPU: u64 = 0x900;
const CH_GPUMEM: u64 = 0x910;
const CH_NPU: u64 = 0xA00;
const CH_SSD: u64 = 0xB00;

/// Every system-level number of the demo machine at one instant.
#[derive(Clone, Debug, PartialEq)]
pub struct Load {
    /// Per logical CPU utilisation, percent.
    pub core_util: [f32; LOGICAL_CPUS],
    /// Per logical CPU kernel time, percent (part of `core_util`).
    pub core_kernel: [f32; LOGICAL_CPUS],
    /// Mean of `core_util`.
    pub total_pct: f32,
    /// Mean of `core_kernel`.
    pub kernel_pct: f32,
    /// How far into a CPU burst we are, 0–1 (which process bursts is the process
    /// model's business).
    pub burst: f64,
    /// Which burst (stable for its duration), for picking the busy process.
    pub burst_slot: i64,
    /// Die temperatures, °C.
    pub die_c: [f32; 4],
    pub cpu_hotspot_c: f32,
    pub soc_hotspot_c: f32,
    pub ssd_c: f32,
    pub cpu_power_w: f32,
    pub gpu_power_w: f32,
    pub ane_power_w: f32,
    pub system_power_w: f32,
    pub super_clock_mhz: f32,
    pub efficiency_clock_mhz: f32,
    pub gpu_clock_mhz: f32,
    /// Mean utilisation of the Super class, percent.
    pub super_active_pct: f32,
    /// Mean utilisation of the Efficiency class, percent.
    pub efficiency_active_pct: f32,
    /// Memory, bytes.
    pub mem_used: u64,
    pub mem_wired: u64,
    pub mem_compressed: u64,
    pub mem_app: u64,
    pub mem_cached: u64,
    pub swap_used: u64,
    /// Memory pressure, 0–100.
    pub pressure_pct: f32,
    /// Pages per second.
    pub page_in_rate: f64,
    /// Pages per second.
    pub page_out_rate: f64,
    /// Bytes per second.
    pub disk_read_bps: f64,
    /// Bytes per second.
    pub disk_write_bps: f64,
    pub disk_active_pct: f32,
    pub disk_response_ms: f32,
    /// Wi-Fi, bytes per second.
    pub wifi_rx_bps: f64,
    pub wifi_tx_bps: f64,
    /// Loopback, bytes per second (each direction).
    pub loopback_bps: f64,
    /// Peer-to-peer wireless, bytes per second.
    pub awdl_rx_bps: f64,
    pub awdl_tx_bps: f64,
    /// GPU utilisation, percent.
    pub gpu_pct: f32,
    pub gpu_memory_used: u64,
    /// Neural Engine active residency, percent.
    pub npu_pct: f32,
}

/// How far into a burst `t` is (0–1), the burst's height (0–1) and its slot.
fn burst(seed: u64, t: f64) -> (f64, f64, i64) {
    const WINDOW: f64 = 90.0;
    let (env, strength) = pulse(seed, CH_BURST, t, WINDOW, 0.3, (3.0, 14.0), 1.0);
    (env, strength, (t / WINDOW).floor() as i64)
}

/// Whole-machine CPU load as a fraction 0–1 (before per-core variation).
fn cpu_fraction(seed: u64, t: f64) -> f64 {
    let base = 0.05 + 0.33 * fbm(seed, CH_CPU, t, 45.0).powf(1.6);
    let (env, strength, _) = burst(seed, t);
    let peak = 0.65 + 0.27 * strength;
    base + (peak - base) * env
}

/// Exponentially weighted average of the load over the last ~30 s: what heats the die.
fn lagged_fraction(seed: u64, t: f64) -> f64 {
    let mut sum = 0.0;
    let mut weights = 0.0;
    let mut w = 1.0;
    for k in 0..10 {
        sum += w * cpu_fraction(seed, t - 3.0 * f64::from(k));
        weights += w;
        w *= 0.8;
    }
    sum / weights
}

/// Class means (Super, Efficiency) for a machine load `l`: light work lands on the
/// Efficiency cores; the Super cores pick up what they cannot hold.
fn class_means(l: f64) -> (f64, f64) {
    let n = LOGICAL_CPUS as f64;
    let n_super = SUPER_CORES as f64;
    let n_eff = n - n_super;
    let eff = (n * l * 0.8 / n_eff).min(0.75 + 0.2 * l);
    let sup = ((n * l - n_eff * eff) / n_super).clamp(0.005, 1.0);
    (sup, eff)
}

fn spread_cores(seed: u64, t: f64, range: std::ops::Range<usize>, mean: f64, out: &mut [f32]) {
    let multipliers: Vec<f64> = range
        .clone()
        .map(|i| 0.6 + 0.8 * noise(seed, CH_CORE + i as u64, t, 3.0 + 0.7 * i as f64))
        .collect();
    let avg = multipliers.iter().sum::<f64>() / multipliers.len() as f64;
    for (i, m) in range.zip(multipliers) {
        out[i] = ((mean * m / avg).clamp(0.003, 1.0) * 100.0) as f32;
    }
}

fn mean(values: &[f32]) -> f32 {
    values.iter().sum::<f32>() / values.len() as f32
}

impl Load {
    /// The machine at `t` seconds of simulated time.
    pub fn at(seed: u64, t: f64) -> Self {
        let mut load = Self::cpu(seed, t);
        load.thermals_and_power(seed, t);
        load.memory(seed, t);
        load.io(seed, t);
        load
    }

    fn cpu(seed: u64, t: f64) -> Self {
        let l = cpu_fraction(seed, t);
        let (env, _, slot) = burst(seed, t);
        let (sup, eff) = class_means(l);
        let mut core_util = [0.0f32; LOGICAL_CPUS];
        spread_cores(seed, t, 0..SUPER_CORES, sup, &mut core_util);
        spread_cores(seed, t, SUPER_CORES..LOGICAL_CPUS, eff, &mut core_util);
        let mut core_kernel = [0.0f32; LOGICAL_CPUS];
        for (i, k) in core_kernel.iter_mut().enumerate() {
            let share = 0.17 + 0.06 * noise(seed, CH_KERNEL + i as u64, t, 7.0);
            *k = core_util[i] * share as f32;
        }
        let gpu_pct =
            (30.0 * (0.03 + 0.97 * fbm(seed, CH_GPU, t, 60.0).powf(2.5))).min(30.0) as f32;
        let (npu_env, npu_strength) = pulse(seed, CH_NPU, t, 120.0, 0.12, (2.0, 10.0), 0.6);
        Self {
            total_pct: mean(&core_util),
            kernel_pct: mean(&core_kernel),
            super_active_pct: mean(&core_util[..SUPER_CORES]),
            efficiency_active_pct: mean(&core_util[SUPER_CORES..]),
            core_util,
            core_kernel,
            burst: env,
            burst_slot: slot,
            die_c: [0.0; 4],
            cpu_hotspot_c: 0.0,
            soc_hotspot_c: 0.0,
            ssd_c: 0.0,
            cpu_power_w: 0.0,
            gpu_power_w: 0.0,
            ane_power_w: 0.0,
            system_power_w: 0.0,
            super_clock_mhz: 0.0,
            efficiency_clock_mhz: 0.0,
            gpu_clock_mhz: 0.0,
            mem_used: 0,
            mem_wired: 0,
            mem_compressed: 0,
            mem_app: 0,
            mem_cached: 0,
            swap_used: 0,
            pressure_pct: 0.0,
            page_in_rate: 0.0,
            page_out_rate: 0.0,
            disk_read_bps: 0.0,
            disk_write_bps: 0.0,
            disk_active_pct: 0.0,
            disk_response_ms: 0.0,
            wifi_rx_bps: 0.0,
            wifi_tx_bps: 0.0,
            loopback_bps: 0.0,
            awdl_rx_bps: 0.0,
            awdl_tx_bps: 0.0,
            gpu_pct,
            gpu_memory_used: 0,
            npu_pct: (npu_env * (5.0 + 35.0 * npu_strength)) as f32,
        }
    }

    fn thermals_and_power(&mut self, seed: u64, t: f64) {
        let lag = lagged_fraction(seed, t);
        let base = 46.5 + 22.0 * lag;
        for (j, die) in self.die_c.iter_mut().enumerate() {
            let jitter = noise(seed, CH_DIE + j as u64, t, 13.0) - 0.5;
            *die = (base + (j as f64 - 1.5) * 0.6 + jitter) as f32;
        }
        let die_max = self.die_c.iter().copied().fold(f32::MIN, f32::max);
        let die_mean = mean(&self.die_c);
        let l = f64::from(self.total_pct) / 100.0;
        let g = f64::from(self.gpu_pct) / 100.0;
        self.cpu_hotspot_c = die_max + 1.5 + (3.0 * l) as f32;
        self.soc_hotspot_c = die_mean + 0.8 + (4.0 * g) as f32;
        self.ssd_c = (35.0 + 6.0 * lag + 3.0 * noise(seed, CH_SSD, t, 120.0)) as f32;

        let wobble = 0.95 + 0.1 * noise(seed, CH_POWER, t, 5.0);
        self.cpu_power_w = ((0.4 + 19.0 * l.powf(1.15)) * wobble) as f32;
        self.gpu_power_w = (0.05 + 8.0 * g) as f32;
        self.ane_power_w = 0.025 * self.npu_pct;
        self.system_power_w = 4.2 + self.cpu_power_w + self.gpu_power_w + self.ane_power_w;

        let sup = f64::from(self.super_active_pct) / 100.0;
        let eff = f64::from(self.efficiency_active_pct) / 100.0;
        self.super_clock_mhz = (1260.0 + 3200.0 * sup.powf(0.6)) as f32;
        self.efficiency_clock_mhz = (970.0 + 1900.0 * eff.powf(0.6)) as f32;
        self.gpu_clock_mhz = (340.0 + 1100.0 * (g / 0.3).min(1.0)) as f32;
    }

    fn memory(&mut self, seed: u64, t: f64) {
        let gib = GIB as f64;
        let u = fbm(seed, CH_MEM, t, 900.0);
        let used = (17.0 + 4.0 * u) * gib;
        let wired = (2.6 + 0.5 * noise(seed, CH_WIRED, t, 400.0)) * gib;
        let compressed = (0.8 + 2.2 * u * (0.8 + 0.2 * noise(seed, CH_MEM ^ 1, t, 200.0))) * gib;
        let available = MEMORY_TOTAL as f64 - used;
        self.mem_used = used as u64;
        self.mem_wired = wired as u64;
        self.mem_compressed = compressed as u64;
        self.mem_app = (used - wired - compressed) as u64;
        self.mem_cached = (available * (0.4 + 0.3 * noise(seed, CH_CACHE, t, 300.0))) as u64;
        self.swap_used = ((0.4 + 0.9 * u + 0.1 * noise(seed, CH_SWAP, t, 500.0)) * gib) as u64;
        self.pressure_pct = (12.0 + 50.0 * u.powf(2.2)) as f32;
        self.page_out_rate = if self.pressure_pct > 35.0 {
            f64::from(self.pressure_pct - 35.0) * 20.0
        } else {
            0.0
        };
    }

    fn io(&mut self, seed: u64, t: f64) {
        let (r_env, r_strength) = pulse(seed, CH_DREAD, t, 6.0, 0.18, (0.8, 3.5), 0.4);
        let (w_env, w_strength) = pulse(seed, CH_DWRITE, t, 8.0, 0.15, (0.6, 2.5), 0.4);
        self.disk_read_bps = 40e3
            + 1.5e6 * noise(seed, CH_DREAD_BASE, t, 4.0).powi(3)
            + 180e6 * r_env * (0.1 + 0.9 * r_strength);
        self.disk_write_bps = 80e3
            + 3e6 * noise(seed, CH_DWRITE_BASE, t, 5.0).powi(3)
            + 90e6 * w_env * (0.1 + 0.9 * w_strength);
        let busy = 1.0 - (-(self.disk_read_bps + self.disk_write_bps) / 60e6).exp();
        self.disk_active_pct = (0.3 + 99.0 * busy).min(100.0) as f32;
        self.disk_response_ms =
            (0.1 + 0.9 * busy * busy + 0.05 * noise(seed, CH_DRESP, t, 3.0)) as f32;
        self.page_in_rate = 30.0 + self.disk_read_bps / 4096.0 * 0.05;

        let (rx_env, rx_strength) = pulse(seed, CH_NETRX, t, 20.0, 0.25, (1.0, 8.0), 0.5);
        let (tx_env, tx_strength) = pulse(seed, CH_NETTX, t, 45.0, 0.12, (1.0, 5.0), 0.5);
        let base = noise(seed, CH_NETBASE, t, 6.0);
        self.wifi_rx_bps = 3e3 + 40e3 * base * base + 6e6 * rx_env * (0.1 + 0.9 * rx_strength);
        self.wifi_tx_bps = 1.5e3
            + self.wifi_rx_bps * (0.1 + 0.15 * noise(seed, CH_NETTX ^ 1, t, 9.0))
            + 0.4e6 * tx_env * tx_strength;
        self.loopback_bps = 2e3 + 30e3 * noise(seed, CH_LOOP, t, 8.0).powi(3);
        let (a_env, a_strength) = pulse(seed, CH_AWDL, t, 30.0, 0.2, (0.5, 2.0), 0.2);
        self.awdl_rx_bps = 900.0 * a_env * (0.2 + a_strength);
        self.awdl_tx_bps = 0.6 * self.awdl_rx_bps;

        self.gpu_memory_used = ((0.6 + 1.6 * fbm(seed, CH_GPUMEM, t, 600.0)) * GIB as f64) as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_split_favours_efficiency_cores_at_low_load() {
        let (sup, eff) = class_means(0.1);
        assert!(eff > 2.0 * sup, "{sup} {eff}");
        let (sup, eff) = class_means(0.9);
        assert!(sup > 0.7 && eff > 0.7, "{sup} {eff}");
        for l in [0.05, 0.2, 0.4, 0.6, 0.9] {
            let (sup, eff) = class_means(l);
            let avg = (sup * 4.0 + eff * 6.0) / 10.0;
            assert!((avg - l).abs() < 0.01, "{l}: {avg}");
        }
    }

    #[test]
    fn temperature_lags_the_load() {
        // Find a burst and check the die is hotter shortly after its start than before.
        let seed = 11;
        let start = (0..20_000)
            .map(|i| f64::from(i) * 0.5)
            .find(|&t| burst(seed, t).0 > 0.99 && burst(seed, t - 2.0).0 == 0.0);
        let Some(t) = start else {
            panic!("no burst in 10 000 s");
        };
        let before = Load::at(seed, t - 3.0).die_c[0];
        let after = Load::at(seed, t + 6.0).die_c[0];
        assert!(after > before, "{before} -> {after}");
    }

    #[test]
    fn pure_function_of_seed_and_time() {
        assert_eq!(Load::at(5, 123.25), Load::at(5, 123.25));
        assert_ne!(Load::at(5, 123.25), Load::at(6, 123.25));
    }
}
