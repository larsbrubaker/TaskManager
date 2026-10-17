//! Parsers for Linux `/proc` text the native Linux platform samples each tick: per-CPU
//! times (`/proc/stat`), memory composition (`/proc/meminfo`), memory pressure stall
//! information (`/proc/pressure/memory`) and the system thread count (`/proc/loadavg`).
//!
//! The native side (`taskmanager-native/src/platform/linux.rs`) reads the files; these
//! functions are pure so they are tested on every OS from fixture text.

use std::collections::HashMap;

use crate::telemetry::PressureLevel;

/// Cumulative time one CPU (or all of them) spent in each state, in clock ticks
/// (`/proc/stat` `cpu` lines; see proc(5)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
}

impl CpuTimes {
    /// Every state (guest time is already counted in user/nice).
    pub fn total(&self) -> u64 {
        self.user
            + self.nice
            + self.system
            + self.idle
            + self.iowait
            + self.irq
            + self.softirq
            + self.steal
    }

    /// Time in the kernel: system calls plus interrupt handling.
    pub fn kernel(&self) -> u64 {
        self.system + self.irq + self.softirq
    }

    /// Kernel share of all time between `earlier` and `self`, 0–100. `None` when no time
    /// passed or a counter went backwards (CPU hot-unplugged and back, counter reset).
    pub fn kernel_pct_since(&self, earlier: &CpuTimes) -> Option<f32> {
        let total = self.total().checked_sub(earlier.total())?;
        let kernel = self.kernel().checked_sub(earlier.kernel())?;
        if total == 0 || kernel > total {
            return None;
        }
        Some((kernel as f64 / total as f64 * 100.0) as f32)
    }
}

/// The `cpu` lines of `/proc/stat`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcStatCpus {
    /// The aggregate `cpu` line.
    pub total: Option<CpuTimes>,
    /// `cpuN` lines as (N, times), in file order. Offline CPUs have no line.
    pub per_cpu: Vec<(usize, CpuTimes)>,
}

/// Parse the `cpu` lines of `/proc/stat`. Lines with fewer than the four original fields
/// are skipped; missing later fields (older kernels) read as zero.
pub fn parse_proc_stat_cpus(text: &str) -> ProcStatCpus {
    let mut out = ProcStatCpus::default();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let Some(label) = words.next() else { continue };
        let Some(suffix) = label.strip_prefix("cpu") else {
            continue;
        };
        let values: Vec<u64> = words.map_while(|w| w.parse().ok()).collect();
        if values.len() < 4 {
            continue;
        }
        let at = |i: usize| values.get(i).copied().unwrap_or(0);
        let times = CpuTimes {
            user: at(0),
            nice: at(1),
            system: at(2),
            idle: at(3),
            iowait: at(4),
            irq: at(5),
            softirq: at(6),
            steal: at(7),
        };
        if suffix.is_empty() {
            out.total = Some(times);
        } else if let Ok(index) = suffix.parse() {
            out.per_cpu.push((index, times));
        }
    }
    out
}

/// `/proc/meminfo`: field name → bytes. Values in `kB` are converted to bytes; unitless
/// counts (`HugePages_Total`) are kept as they are.
pub fn parse_meminfo(text: &str) -> HashMap<String, u64> {
    let mut fields = HashMap::new();
    for line in text.lines() {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let mut words = rest.split_whitespace();
        let Some(Ok(value)) = words.next().map(str::parse::<u64>) else {
            continue;
        };
        let bytes = match words.next() {
            Some(unit) if unit.eq_ignore_ascii_case("kb") => value.saturating_mul(1024),
            _ => value,
        };
        fields.insert(name.trim().to_string(), bytes);
    }
    fields
}

/// One resource's pressure stall information (`/proc/pressure/<resource>`): the share of
/// the last 10 s in which some (or all) tasks were stalled waiting for it, 0–100.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Psi {
    pub some_avg10: f32,
    /// Absent for the CPU resource on older kernels.
    pub full_avg10: Option<f32>,
}

impl Psi {
    /// Our pressure level: Critical when all tasks stalled ≥ 10 % or some ≥ 40 % of the
    /// last 10 s; Elevated when some stalled ≥ 10 %; otherwise Normal. (Thresholds are
    /// ours: PSI has no official levels.)
    pub fn level(&self) -> PressureLevel {
        let full = self.full_avg10.unwrap_or(0.0);
        if full >= 10.0 || self.some_avg10 >= 40.0 {
            PressureLevel::Critical
        } else if self.some_avg10 >= 10.0 {
            PressureLevel::Elevated
        } else {
            PressureLevel::Normal
        }
    }
}

/// Parse a PSI file. `None` without a `some` line.
pub fn parse_psi(text: &str) -> Option<Psi> {
    let avg10 = |line: &str| {
        line.split_whitespace()
            .find_map(|w| w.strip_prefix("avg10="))
            .and_then(|v| v.parse::<f32>().ok())
    };
    let mut some = None;
    let mut full = None;
    for line in text.lines() {
        if line.starts_with("some ") {
            some = avg10(line);
        } else if line.starts_with("full ") {
            full = avg10(line);
        }
    }
    Some(Psi {
        some_avg10: some?,
        full_avg10: full,
    })
}

/// The number of scheduling entities (threads) from `/proc/loadavg`'s fourth field,
/// `running/total`.
pub fn parse_loadavg_threads(text: &str) -> Option<u64> {
    let field = text.split_whitespace().nth(3)?;
    field.split_once('/')?.1.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "\
cpu  10132153 290696 3084719 46828483 16683 0 25195 0 175628 0
cpu0 1393280 32966 572056 13343292 6130 0 17875 0 23933 0
cpu1 1335513 3134 363112 14139930 2131 0 3047 0 8392 0
cpu3 100 0 50 850
intr 1462898 0 0 0
ctxt 3024234
btime 1700000000
";

    #[test]
    fn reads_aggregate_and_per_cpu_lines() {
        let cpus = parse_proc_stat_cpus(STAT);
        let total = cpus.total.expect("aggregate line");
        assert_eq!(total.user, 10_132_153);
        assert_eq!(total.softirq, 25_195);
        assert_eq!(cpus.per_cpu.len(), 3);
        assert_eq!(cpus.per_cpu[0].0, 0);
        assert_eq!(cpus.per_cpu[1].1.system, 363_112);
        // An old four-field line, and a gap where cpu2 is offline.
        assert_eq!(cpus.per_cpu[2].0, 3);
        assert_eq!(cpus.per_cpu[2].1.iowait, 0);
        assert_eq!(cpus.per_cpu[2].1.total(), 1000);
    }

    #[test]
    fn kernel_share_is_system_plus_interrupts_over_all_time() {
        let a = CpuTimes {
            user: 100,
            system: 50,
            idle: 850,
            ..CpuTimes::default()
        };
        let b = CpuTimes {
            user: 160,
            system: 70,
            irq: 5,
            softirq: 5,
            idle: 910,
            ..CpuTimes::default()
        };
        // 30 kernel ticks of 150.
        assert_eq!(b.kernel_pct_since(&a), Some(20.0));
        assert_eq!(a.kernel_pct_since(&a), None, "no time passed");
        assert_eq!(a.kernel_pct_since(&b), None, "counters went backwards");
    }

    #[test]
    fn meminfo_values_become_bytes() {
        let info = parse_meminfo(
            "MemTotal:       16303428 kB\nCached:          4021464 kB\n\
             SwapCached:            0 kB\nHugePages_Total:       0\nbogus line\n",
        );
        assert_eq!(info.get("MemTotal"), Some(&(16_303_428 * 1024)));
        assert_eq!(info.get("Cached"), Some(&(4_021_464 * 1024)));
        assert_eq!(info.get("SwapCached"), Some(&0));
        assert_eq!(info.get("HugePages_Total"), Some(&0));
        assert_eq!(info.len(), 4);
    }

    #[test]
    fn psi_reads_avg10_and_maps_to_levels() {
        let psi = parse_psi(
            "some avg10=12.50 avg60=3.00 avg300=1.00 total=123456\n\
             full avg10=2.25 avg60=0.50 avg300=0.10 total=45678\n",
        )
        .expect("parses");
        assert_eq!(psi.some_avg10, 12.5);
        assert_eq!(psi.full_avg10, Some(2.25));
        assert_eq!(psi.level(), PressureLevel::Elevated);
        let calm = parse_psi("some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n").expect("parses");
        assert_eq!(calm.full_avg10, None);
        assert_eq!(calm.level(), PressureLevel::Normal);
        let stalled = Psi {
            some_avg10: 20.0,
            full_avg10: Some(15.0),
        };
        assert_eq!(stalled.level(), PressureLevel::Critical);
        assert_eq!(parse_psi("full avg10=1.0\n"), None);
    }

    #[test]
    fn loadavg_gives_the_thread_total() {
        assert_eq!(
            parse_loadavg_threads("0.52 0.58 0.59 3/1234 56789\n"),
            Some(1234)
        );
        assert_eq!(parse_loadavg_threads("0.52 0.58 0.59\n"), None);
        assert_eq!(parse_loadavg_threads("0.52 0.58 0.59 x 1\n"), None);
    }
}
