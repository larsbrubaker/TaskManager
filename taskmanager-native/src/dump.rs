//! `--dump-snapshot [N]`: sample the real machine without a window and print what the
//! native platform measured, then exit. The headless check of `platform` (like HDTreeMap's
//! `--dump-top`): run it on each OS to see every reading, its availability and the health.
//!
//! It drives the provider through the core's `SampleProducer` (as the app will), takes N
//! samples one second apart, prints one line per sample and a summary of the last: CPU and
//! kernel share, memory composition, the top ten processes by CPU with their keys, disks,
//! interfaces, sensors and provider health. The text formatting is pure ([`summary`],
//! [`sample_line`]) so it is tested on a hand-built snapshot.

use std::fmt::Write as _;
use std::thread::sleep;
use std::time::{Duration, Instant};

use taskmanager_core::format::{format_bytes, format_percent, format_rate, format_uptime};
use taskmanager_core::telemetry::{CoreClass, PressureLevel};
use taskmanager_core::{MachineInfo, ProviderHealth, Reading, SampleProducer, Snapshot};

use crate::platform::native_platform;

/// How many sensors the summary lists before eliding the rest.
const MAX_SENSORS: usize = 12;

/// Sample `samples` times, one second apart, printing as we go.
pub fn run(samples: u32) {
    let (provider, _services) = native_platform();
    let mut producer = SampleProducer::new(Box::new(provider));
    let info = producer.machine_info();
    print!("{}", machine_text(&info));
    let mut last = None;
    for i in 0..samples {
        if i > 0 {
            sleep(Duration::from_secs(1));
        }
        let started = Instant::now();
        let snapshot = producer.force_sample(Instant::now());
        println!(
            "{}  ({} ms)",
            sample_line(&snapshot),
            started.elapsed().as_millis()
        );
        last = Some(snapshot);
    }
    if let Some(snapshot) = last {
        print!("{}", summary(&snapshot));
    }
}

/// A reading as text: the value through `show`, or why there is none.
fn reading<T>(value: &Reading<T>, show: impl FnOnce(&T) -> String) -> String {
    match value {
        Reading::Value(v) => show(v),
        Reading::Unavailable => "Unavailable".into(),
        Reading::Unsupported => "Unsupported".into(),
    }
}

fn pct(value: &Reading<f32>) -> String {
    reading(value, |v| format_percent(f64::from(*v)))
}

fn bytes(value: &Reading<u64>) -> String {
    reading(value, |v| format_bytes(*v))
}

fn rate(value: &Reading<f64>) -> String {
    reading(value, |v| format_rate(*v))
}

fn text(value: &Reading<String>) -> String {
    reading(value, String::clone)
}

fn health(health: &ProviderHealth) -> String {
    match health {
        ProviderHealth::Healthy => "Healthy".into(),
        ProviderHealth::Degraded(why) => format!("Degraded: {why}"),
    }
}

fn classes(classes: &[CoreClass]) -> String {
    if classes.is_empty() {
        return "single class / unknown".into();
    }
    classes
        .iter()
        .map(
            |c| match (c.logical_indices.first(), c.logical_indices.last()) {
                (Some(first), Some(last)) => format!("{} cpu{first}-{last}", c.name),
                _ => c.name.clone(),
            },
        )
        .collect::<Vec<_>>()
        .join(", ")
}

/// The static facts, a few lines.
pub fn machine_text(info: &MachineInfo) -> String {
    let cpu = &info.cpu;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Machine: {} · {} · {} {}",
        text(&info.machine_name),
        text(&info.model),
        text(&info.os_name),
        text(&info.os_version)
    );
    let _ = writeln!(
        out,
        "CPU: {} ({}) · {} physical / {} logical · classes: {} · L1 {} L2 {} L3 {} · VM {}",
        text(&cpu.brand),
        text(&cpu.vendor),
        reading(&cpu.physical_cores, usize::to_string),
        cpu.logical_cores,
        classes(&cpu.core_classes),
        bytes(&cpu.l1),
        bytes(&cpu.l2),
        bytes(&cpu.l3),
        reading(&cpu.is_vm, bool::to_string),
    );
    let _ = writeln!(
        out,
        "Memory: {} · {} disks · {} interfaces · primary IP {}",
        bytes(&info.memory.total),
        info.disks.len(),
        info.networks.len(),
        text(&info.primary_ip)
    );
    for disk in &info.disks {
        let _ = writeln!(
            out,
            "  disk {:<28} {:<20} {:?} {} model {}{}",
            disk.id,
            disk.name,
            disk.kind,
            bytes(&disk.capacity),
            text(&disk.model),
            if disk.is_system { " [system]" } else { "" }
        );
    }
    for net in &info.networks {
        let _ = writeln!(
            out,
            "  net  {:<10} {:<24} {:?} mac {} ipv4 [{}] link {}",
            net.id,
            net.name,
            net.kind,
            text(&net.mac),
            net.ipv4.join(", "),
            reading(&net.link_speed_bps, |b| format!("{} Mb/s", b / 1_000_000))
        );
    }
    out
}

/// One line per sample.
pub fn sample_line(s: &Snapshot) -> String {
    let processes = reading(&s.processes, |t| t.live_count().to_string());
    format!(
        "#{:<3} CPU {:>11}  kernel {:>11}  memory {} / {}  processes {}  threads {}  health {}",
        s.generation,
        pct(&s.cpu.total_pct),
        pct(&s.cpu.kernel_pct),
        bytes(&s.memory.used),
        bytes(&s.memory.total),
        processes,
        reading(&s.thread_count, u64::to_string),
        health(&s.health)
    )
}

/// The full summary of one snapshot.
pub fn summary(s: &Snapshot) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "\n=== Generation {} · elapsed {:.2} s · uptime {} ===",
        s.generation,
        s.elapsed.as_secs_f64(),
        reading(&s.uptime, |u| format_uptime(*u))
    );
    let cpu = &s.cpu;
    let _ = writeln!(
        out,
        "CPU: total {} · kernel {} · clock {}",
        pct(&cpu.total_pct),
        pct(&cpu.kernel_pct),
        reading(&cpu.frequency_mhz, |f| format!("{f:.0} MHz"))
    );
    let cores: Vec<String> = cpu
        .per_cpu
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{i}:{}/{}", pct(&c.util_pct), pct(&c.kernel_pct)))
        .collect();
    let _ = writeln!(out, "  per CPU (load/kernel): {}", cores.join("  "));
    let m = &s.memory;
    let _ = writeln!(
        out,
        "Memory: used {} of {} · available {} · cached {} · compressed {} · wired {} · app {} · committed {}",
        bytes(&m.used),
        bytes(&m.total),
        bytes(&m.available),
        bytes(&m.cached),
        bytes(&m.compressed),
        bytes(&m.wired),
        bytes(&m.app),
        bytes(&m.committed)
    );
    let level = |l: &PressureLevel| format!("{l:?}");
    let _ = writeln!(
        out,
        "  swap {} of {} · pressure {} ({}) · page ins {} outs {}",
        bytes(&m.swap_used),
        bytes(&m.swap_total),
        pct(&m.pressure_pct),
        reading(&m.pressure_level, level),
        reading(&m.page_ins, u64::to_string),
        reading(&m.page_outs, u64::to_string)
    );
    match s.processes.value() {
        Some(table) => {
            let _ = writeln!(out, "Top 10 processes by CPU (key = pid @ start stamp):");
            for row in table.top_by_cpu(10) {
                let _ = writeln!(
                    out,
                    "  {:>7} @ {:<20} {:>11} {:>11} {:>12} {:<14} {:<11} {}",
                    row.key.pid.0,
                    row.key.start_time,
                    pct(&row.cpu_pct),
                    bytes(&row.memory),
                    rate(&row.disk_read_bps),
                    text(&row.user),
                    format!("{:?}", row.kind),
                    row.name
                );
            }
            let groups = table.grouped();
            let _ = writeln!(
                out,
                "  {} applications, {} background",
                groups.applications.len(),
                groups.background.len()
            );
        }
        None => {
            let _ = writeln!(
                out,
                "Processes: {}",
                reading(&s.processes, |_| String::new())
            );
        }
    }
    for d in &s.disks {
        let _ = writeln!(
            out,
            "Disk {:<28} read {:>12} write {:>12} (total read {} written {})",
            d.id,
            rate(&d.read_bps),
            rate(&d.write_bps),
            bytes(&d.total_read),
            bytes(&d.total_written)
        );
    }
    for n in &s.networks {
        let _ = writeln!(
            out,
            "Net  {:<10} rx {:>12} tx {:>12} (total rx {} tx {})",
            n.id,
            rate(&n.rx_bps),
            rate(&n.tx_bps),
            bytes(&n.total_rx),
            bytes(&n.total_tx)
        );
    }
    let _ = writeln!(out, "Sensors ({}):", s.sensors.len());
    for sensor in s.sensors.iter().take(MAX_SENSORS) {
        let value = reading(&sensor.value, |v| format!("{v:.1}"));
        let _ = writeln!(out, "  {:<40} {:?} {}", sensor.id, sensor.kind, value);
    }
    if s.sensors.len() > MAX_SENSORS {
        let _ = writeln!(out, "  … {} more", s.sensors.len() - MAX_SENSORS);
    }
    let _ = writeln!(out, "Health: {}", health(&s.health));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use taskmanager_core::process::{ProcessRow, ProcessTable};
    use taskmanager_core::telemetry::{CoreSample, SensorKind, SensorSample};
    use taskmanager_core::ProcessKey;

    #[test]
    fn summary_shows_values_and_honest_gaps() {
        let mut s = Snapshot::new(Instant::now());
        s.generation = 7;
        s.cpu.total_pct = Reading::Value(12.5);
        s.cpu.per_cpu = vec![CoreSample {
            util_pct: Reading::Value(50.0),
            kernel_pct: Reading::Unavailable,
        }];
        s.memory.compressed = Reading::Unsupported;
        let mut row = ProcessRow::new(ProcessKey::new(42, 123_456), "worker");
        row.cpu_pct = Reading::Value(3.0);
        s.processes = Reading::Value(ProcessTable::new(vec![row]));
        s.sensors.push(SensorSample {
            id: "temp/cpu".into(),
            kind: SensorKind::Temperature,
            value: Reading::Value(48.0),
        });
        s.health = ProviderHealth::Degraded("disk counters unavailable".into());

        let text = summary(&s);
        assert!(text.contains("Generation 7"), "{text}");
        assert!(text.contains("total 12.5% · kernel Unavailable"), "{text}");
        assert!(text.contains("0:50.0%/Unavailable"), "{text}");
        assert!(text.contains("compressed Unsupported"), "{text}");
        assert!(text.contains("42 @ 123456"), "{text}");
        assert!(text.contains("temp/cpu") && text.contains("48.0"), "{text}");
        assert!(
            text.contains("Degraded: disk counters unavailable"),
            "{text}"
        );

        let line = sample_line(&s);
        assert!(line.starts_with("#7"), "{line}");
        assert!(line.contains("processes 1"), "{line}");
    }

    #[test]
    fn a_failed_listing_says_so() {
        let s = Snapshot::new(Instant::now());
        assert!(summary(&s).contains("Processes: Unavailable"));
        assert!(sample_line(&s).contains("processes Unavailable"));
        assert!(machine_text(&MachineInfo::default()).contains("primary IP Unavailable"));
    }
}
