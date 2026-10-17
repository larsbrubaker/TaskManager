//! Per-sample process measurements for the demo machine: turns the [`Population`] and the
//! system [`Load`] at one instant into [`ProcessRow`]s.
//!
//! The process numbers agree with the system ones: CPU shares add up to just under the
//! machine's total (the rest is unattributed kernel work), GPU and Neural Engine shares
//! split the device's utilisation among the processes that use it, and network rates
//! split the Wi-Fi traffic among the networked processes. Most processes sit near zero;
//! during a CPU burst one busy app process takes most of it. Energy scores weigh CPU,
//! GPU, disk and network and are relative to the hungriest process in the sample.
//!
//! Following the provider contract, momentary rates (CPU, GPU, NPU, disk, network,
//! energy) are `Unavailable` on the first sample; root-owned processes report their task
//! measurements as `Unavailable` (the UI's "Access denied"), as they would to a normal
//! user on a real Mac. Running totals (CPU time, page faults, disk bytes) accumulate
//! over the elapsed time.

use std::time::Duration;

use crate::process::{ProcessRow, ProcessStatus};
use crate::reading::Reading;

use super::load::Load;
use super::population::{Population, SimProcess};
use super::profile::LOGICAL_CPUS;
use super::rng::{hash, noise};

/// Share of the machine's CPU total attributed to processes.
const ATTRIBUTED_CPU: f64 = 0.92;
/// Share of Wi-Fi traffic attributed to processes.
const ATTRIBUTED_NET: f64 = 0.85;

/// What one process does this instant (before readability rules apply).
struct Activity {
    cpu_pct: f64,
    gpu_pct: f64,
    npu_pct: f64,
    memory: u64,
    threads: u32,
    disk_read_bps: f64,
    disk_write_bps: f64,
    net_rx_bps: f64,
    net_tx_bps: f64,
}

fn reading<T>(known: bool, value: T) -> Reading<T> {
    if known {
        Reading::Value(value)
    } else {
        Reading::Unavailable
    }
}

/// The process that soaks up the current CPU burst. It must stay the same for the whole
/// burst while other processes come and go, so it is chosen by a stable property: among
/// the hungry, unrestricted, long-lived processes (no scheduled exit, never restarted),
/// the one whose key hashes lowest for this burst. Only ending that very process moves
/// the burst elsewhere.
fn burst_target(seed: u64, procs: &[SimProcess], slot: i64) -> Option<usize> {
    let rank = |p: &SimProcess| {
        let key = (u64::from(p.key.pid.0) << 40) ^ p.key.start_time;
        hash(seed ^ key, 0xB0, slot)
    };
    (0..procs.len())
        .filter(|&i| {
            let p = &procs[i];
            p.weight >= 0.3 && !p.restricted && !p.restarts && p.exit_at.is_none()
        })
        .min_by_key(|&i| rank(&procs[i]))
}

fn activities(seed: u64, procs: &[SimProcess], load: &Load, t: f64) -> Vec<Activity> {
    let mut raw: Vec<f64> = procs
        .iter()
        .map(|p| {
            let period = 4.0 + (p.channel % 23) as f64;
            let n = noise(seed, p.channel, t, period);
            p.weight * (0.03 + 2.0 * n * n * n)
        })
        .collect();
    if load.burst > 0.0 {
        if let Some(i) = burst_target(seed, procs, load.burst_slot) {
            let others: f64 = raw.iter().sum();
            raw[i] += others * 4.0 * load.burst;
        }
    }
    let raw_sum: f64 = raw.iter().sum();
    let cpu_scale = if raw_sum > 0.0 {
        f64::from(load.total_pct) * ATTRIBUTED_CPU / raw_sum
    } else {
        0.0
    };
    let gpu_shares: f64 = procs.iter().map(|p| p.gpu_share).sum();
    let net_noise: Vec<f64> = procs
        .iter()
        .map(|p| {
            if p.networked {
                0.05 + noise(seed, p.channel ^ 0x4E7, t, 5.0)
            } else {
                0.0
            }
        })
        .collect();
    let net_sum: f64 = net_noise.iter().sum();
    let npu_users = procs.iter().filter(|p| p.uses_npu).count().max(1) as f64;

    procs
        .iter()
        .zip(raw)
        .zip(net_noise)
        .map(|((p, raw), net)| {
            let drift = noise(seed, p.channel ^ 0x3E3, t, 90.0);
            let disk = noise(seed, p.channel ^ 0xD15, t, 3.0).powi(4);
            let net_share = if net_sum > 0.0 { net / net_sum } else { 0.0 };
            Activity {
                cpu_pct: (raw * cpu_scale).min(100.0),
                gpu_pct: if gpu_shares > 0.0 {
                    f64::from(load.gpu_pct) * p.gpu_share / gpu_shares
                } else {
                    0.0
                },
                npu_pct: if p.uses_npu {
                    f64::from(load.npu_pct) / npu_users
                } else {
                    0.0
                },
                memory: (p.memory_base * (0.92 + 0.16 * drift)) as u64,
                threads: p.threads_base + (3.0 * drift) as u32,
                disk_read_bps: p.disk_bps * disk,
                disk_write_bps: p.disk_bps * 0.4 * disk * drift,
                net_rx_bps: load.wifi_rx_bps * ATTRIBUTED_NET * net_share,
                net_tx_bps: load.wifi_tx_bps * ATTRIBUTED_NET * net_share,
            }
        })
        .collect()
}

/// Energy demand before normalising: CPU, plus GPU, disk and network weighted in.
fn energy_raw(a: &Activity) -> f64 {
    a.cpu_pct
        + 0.6 * a.gpu_pct
        + 0.4 * (a.disk_read_bps + a.disk_write_bps) / 1e6
        + 0.6 * (a.net_rx_bps + a.net_tx_bps) / 1e6
}

/// This sample's process rows and the total thread count. `elapsed` is `None` on the
/// first sample (no rates yet). Advances each process's running totals.
pub fn sample_rows(
    seed: u64,
    population: &mut Population,
    load: &Load,
    t: f64,
    elapsed: Option<Duration>,
    boot_unix_ms: u64,
) -> (Vec<ProcessRow>, u64) {
    let acts = activities(seed, population.procs(), load, t);
    let energy_max = acts
        .iter()
        .zip(population.procs())
        .filter(|(_, p)| !p.restricted)
        .map(|(a, _)| energy_raw(a))
        .fold(0.0f64, f64::max);
    let rates = elapsed.is_some();
    let dt = elapsed.map_or(0.0, |e| e.as_secs_f64());
    let mut threads_total = 0u64;
    let rows = population
        .procs_mut()
        .iter_mut()
        .zip(acts)
        .map(|(p, a)| {
            threads_total += u64::from(a.threads);
            p.cpu_time_s += a.cpu_pct / 100.0 * LOGICAL_CPUS as f64 * dt;
            p.page_faults += (p.memory_base / 1e6 * 0.3 + a.cpu_pct * 20.0) * dt;
            p.disk_read_total += a.disk_read_bps * dt;
            p.disk_written_total += a.disk_write_bps * dt;
            p.peak_memory = p.peak_memory.max(a.memory);
            let open = !p.restricted;
            let energy = if energy_max > 0.0 {
                (100.0 * energy_raw(&a) / energy_max) as f32
            } else {
                0.0
            };
            ProcessRow {
                key: p.key,
                parent: p.parent,
                name: p.name.clone(),
                // The kernel has no executable file.
                exe: (!p.exe.is_empty()).then(|| p.exe.clone()),
                user: Reading::Value(p.user.clone()),
                status: ProcessStatus::Running,
                cpu_pct: reading(open && rates, a.cpu_pct as f32),
                gpu_pct: reading(open && rates, a.gpu_pct as f32),
                npu_pct: reading(open && rates, a.npu_pct as f32),
                memory: reading(open, a.memory),
                private_memory: reading(open, (a.memory as f64 * 0.72) as u64),
                peak_memory: reading(open, p.peak_memory),
                threads: reading(open, a.threads),
                disk_read_bps: reading(open && rates, a.disk_read_bps),
                disk_write_bps: reading(open && rates, a.disk_write_bps),
                disk_total_read: reading(open, p.disk_read_total as u64),
                disk_total_written: reading(open, p.disk_written_total as u64),
                net_rx_bps: reading(open && rates, a.net_rx_bps),
                net_tx_bps: reading(open && rates, a.net_tx_bps),
                energy_score: reading(open && rates, energy),
                cpu_time: reading(open, Duration::from_secs_f64(p.cpu_time_s)),
                started_unix_ms: Reading::Value(boot_unix_ms + (p.start_s * 1000.0) as u64),
                kind: p.kind,
                arch: Reading::Value("arm64".to_string()),
                priority: Reading::Value(p.priority),
                page_faults: reading(open, p.page_faults as u64),
                exited_at: None,
            }
        })
        .collect();
    (rows, threads_total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ProcessKey;

    #[test]
    fn cpu_shares_add_up_to_the_attributed_total() {
        let mut pop = Population::new(4, 90_000.0);
        let load = Load::at(4, 50.0);
        let (rows, threads) =
            sample_rows(4, &mut pop, &load, 50.0, Some(Duration::from_secs(1)), 0);
        assert!(threads > 1000);
        let acts = activities(4, pop.procs(), &load, 50.0);
        let sum: f64 = acts.iter().map(|a| a.cpu_pct).sum();
        let expected = f64::from(load.total_pct) * ATTRIBUTED_CPU;
        assert!((sum - expected).abs() < 0.01, "{sum} vs {expected}");
        let visible: f32 = rows.iter().filter_map(|r| r.cpu_pct.value()).sum();
        assert!(f64::from(visible) <= expected + 0.01);
    }

    #[test]
    fn burst_target_ignores_churn_around_it() {
        let pop = Population::new(4, 90_000.0);
        let procs = pop.procs().to_vec();
        for slot in 0..20 {
            let target = burst_target(4, &procs, slot).map(|i| procs[i].key);
            assert!(target.is_some());
            // A hungry tool starts in front of everyone, and a renderer restarts.
            let mut churned = procs.clone();
            let mut tool = churned[0].clone();
            tool.key = ProcessKey::new(99_999, 1);
            tool.weight = 5.0;
            tool.restricted = false;
            tool.exit_at = Some(3.0);
            churned.insert(0, tool);
            churned.retain(|p| !p.restarts);
            let again = burst_target(4, &churned, slot).map(|i| churned[i].key);
            assert_eq!(target, again, "slot {slot}");
        }
    }

    #[test]
    fn first_sample_has_no_rates_and_root_is_access_denied() {
        let mut pop = Population::new(4, 90_000.0);
        let load = Load::at(4, 0.0);
        let (rows, _) = sample_rows(4, &mut pop, &load, 0.0, None, 0);
        assert!(rows.iter().all(|r| r.cpu_pct.is_unavailable()));
        assert!(rows.iter().all(|r| r.net_rx_bps.is_unavailable()));
        let launchd = rows.iter().find(|r| r.key.pid.0 == 1);
        assert!(launchd.is_some_and(|r| r.memory.is_unavailable()));
        let open = rows
            .iter()
            .find(|r| r.user == Reading::Value("demo".into()));
        assert!(open.is_some_and(|r| r.memory.is_value()));
    }
}
