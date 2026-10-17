//! Integration tests of the demo platform (PLAN §4.3) through its public seams: the
//! `TelemetryProvider` / `PlatformServices` pair from `demo_platform`, driven directly and
//! through the real `Sampler`. Covers determinism, plausible ranges for every series over
//! a simulated hour, the machine profile, the process population and its churn (including
//! the producer's exited-row retention), identity-checked End task, and the cost of a full
//! hour of sampling.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use taskmanager_core::platform::demo::{demo_platform, DemoProvider, DEFAULT_DEMO_SEED};
use taskmanager_core::platform::{ActionError, PlatformServices, TelemetryProvider};
use taskmanager_core::process::{Pid, ProcessKey, ProcessKind, ProcessTable};
use taskmanager_core::reading::Reading;
use taskmanager_core::telemetry::{PressureLevel, SensorKind, Snapshot, EXITED_PROCESS_RETENTION};
use taskmanager_core::Sampler;
use web_time::Instant;

const GIB: f64 = (1u64 << 30) as f64;

fn secs(s: u64) -> Duration {
    Duration::from_secs(s)
}

fn table(s: &Snapshot) -> &ProcessTable {
    s.processes
        .value()
        .expect("the demo always lists processes")
}

fn value<T: Copy>(r: Reading<T>, what: &str) -> T {
    r.into_value()
        .unwrap_or_else(|| panic!("{what} should be measured"))
}

/// One snapshot per second for `n` seconds straight from a provider.
fn run(provider: &mut DemoProvider, t0: Instant, n: u64) -> Vec<Snapshot> {
    (0..n).map(|i| provider.sample(t0 + secs(i))).collect()
}

/// An hour of snapshots through the real sampler (seed 42), and how long it took.
fn sampled_hour() -> (Vec<Arc<Snapshot>>, Duration) {
    let (provider, _services) = demo_platform(DEFAULT_DEMO_SEED);
    let started = Instant::now();
    let mut sampler = Sampler::new(Box::new(provider));
    let t0 = Instant::now();
    let snapshots = (0..3600)
        .filter_map(|i| sampler.tick(t0 + secs(i)))
        .collect::<Vec<_>>();
    (snapshots, started.elapsed())
}

#[test]
fn same_seed_and_times_give_identical_snapshots() {
    let t0 = Instant::now();
    let (mut a, _) = demo_platform(7);
    let (mut b, _) = demo_platform(7);
    assert_eq!(a.machine_info(), b.machine_info());
    let run_a = run(&mut a, t0, 120);
    let run_b = run(&mut b, t0, 120);
    assert_eq!(run_a, run_b);

    // The clock is relative to the first sample: a later start gives the same machine.
    let (mut c, _) = demo_platform(7);
    let shifted = t0 + secs(1000);
    let run_c = run(&mut c, shifted, 120);
    for (x, mut y) in run_a.iter().zip(run_c) {
        y.taken_at = x.taken_at;
        assert_eq!(*x, y);
    }
}

#[test]
fn different_seeds_differ() {
    let t0 = Instant::now();
    let (mut a, _) = demo_platform(1);
    let (mut b, _) = demo_platform(2);
    let ra = run(&mut a, t0, 5);
    let rb = run(&mut b, t0, 5);
    assert_ne!(ra[4].cpu.total_pct, rb[4].cpu.total_pct);
    assert_ne!(ra[4].memory.used, rb[4].memory.used);
    let names = |s: &Snapshot| table(s).iter().map(|r| r.name.clone()).collect::<Vec<_>>();
    assert_ne!(names(&ra[4]), names(&rb[4]));
}

#[test]
fn first_sample_has_no_rates_but_everything_else() {
    let (mut p, _) = demo_platform(3);
    let t0 = Instant::now();
    let first = p.sample(t0);
    assert_eq!(first.elapsed, Duration::ZERO);
    assert_eq!(first.taken_at, t0);
    assert_eq!(first.generation, 0, "the producer stamps generations");
    assert!(first.cpu.total_pct.is_unavailable() && first.cpu.kernel_pct.is_unavailable());
    assert!(first
        .cpu
        .per_cpu
        .iter()
        .all(|c| c.util_pct.is_unavailable()));
    assert!(first.disks.iter().all(|d| d.read_bps.is_unavailable()));
    assert!(first.networks.iter().all(|n| n.rx_bps.is_unavailable()));
    assert!(first.memory.page_ins.is_unavailable());
    assert!(first.memory.used.is_value() && first.cpu.temperature_c.is_value());
    assert!(first.disks.iter().all(|d| d.total_read.is_value()));
    assert!(table(&first).iter().all(|r| r.cpu_pct.is_unavailable()));

    let second = p.sample(t0 + Duration::from_millis(1500));
    assert_eq!(second.elapsed, Duration::from_millis(1500));
    assert!(second.cpu.total_pct.is_value());
    assert!(second.networks.iter().all(|n| n.rx_bps.is_value()));
}

#[test]
fn machine_profile_is_the_reference_mac_with_generic_names() {
    let (mut p, services) = demo_platform(DEFAULT_DEMO_SEED);
    assert!(services.is_demo());
    assert_eq!(services.open_url("https://example.com"), Ok(()));
    let info = p.machine_info();
    assert_eq!(info.cpu.brand, Reading::Value("Apple M5".to_string()));
    assert_eq!(info.memory.total, Reading::Value(24 * (1u64 << 30)));
    assert_eq!(info.networks.len(), 18);
    assert_eq!(info.gpus.len(), 1);
    assert_eq!(info.npus.len(), 1);
    let disk = info.system_disk().expect("a system disk");
    assert_eq!(disk.model, Reading::Value("APPLE SSD AP1024Z".to_string()));
    assert!(info.cpu.base_mhz.is_unavailable() && info.cpu.l3.is_unavailable());
    assert!(info.cpu.l1.is_value() && info.cpu.l2.is_value());

    // Per-core samples line up with the core classes.
    p.sample(Instant::now());
    let s = p.sample(Instant::now() + secs(1));
    let class_cores: usize = info
        .cpu
        .core_classes
        .iter()
        .map(|c| c.logical_indices.len())
        .sum();
    assert_eq!(s.cpu.per_cpu.len(), info.cpu.logical_cores);
    assert_eq!(class_cores, info.cpu.logical_cores);
    let mut indices: Vec<usize> = info
        .cpu
        .core_classes
        .iter()
        .flat_map(|c| c.logical_indices.iter().copied())
        .collect();
    indices.sort_unstable();
    assert_eq!(indices, (0..10).collect::<Vec<_>>());
    for d in &s.disks {
        assert!(info.disk(&d.id).is_some());
    }
    for n in &s.networks {
        assert!(info.network(&n.id).is_some());
    }

    // No real user names: users are root, demo or service accounts.
    let users: HashSet<String> = table(&s)
        .iter()
        .filter_map(|r| r.user.value().cloned())
        .collect();
    assert!(users.contains("demo") && users.contains("root"));
    assert!(users
        .iter()
        .all(|u| u == "demo" || u == "root" || u.starts_with('_')));
}

#[test]
fn process_population_and_grouping() {
    let (mut p, _) = demo_platform(DEFAULT_DEMO_SEED);
    let t0 = Instant::now();
    p.sample(t0);
    let s = p.sample(t0 + secs(1));
    let t = table(&s);
    assert!((500..=600).contains(&t.len()), "{} processes", t.len());
    let groups = t.grouped();
    let top_apps = groups
        .applications
        .iter()
        .filter(|r| r.parent == Some(Pid(1)))
        .count();
    assert!((7..=12).contains(&top_apps), "{top_apps} applications");
    assert!(groups.applications.len() > top_apps, "apps have helpers");
    assert!(
        groups.background.len() >= 480,
        "{}",
        groups.background.len()
    );

    let launchd = t.live_by_pid(Pid(1)).expect("pid 1");
    assert_eq!(launchd.user, Reading::Value("root".into()));
    assert!(launchd.cpu_pct.is_unavailable() && launchd.memory.is_unavailable());
    // Helpers hang under their app in the tree.
    let browser = t
        .iter()
        .find(|r| r.name == "Web Browser")
        .expect("the browser runs");
    assert!(t.children_of(&browser.key).len() >= 5);
    assert!(browser.memory.value().is_some_and(|&m| m > 500 << 20));

    // Most processes idle; a few busy; everybody arm64 with a start time.
    let cpus: Vec<f32> = t
        .iter()
        .filter_map(|r| r.cpu_pct.value().copied())
        .collect();
    let idle = cpus.iter().filter(|&&c| c < 0.1).count();
    assert!(idle * 10 > cpus.len() * 8, "{idle} of {} idle", cpus.len());
    assert!(t.iter().all(|r| r.arch == Reading::Value("arm64".into())));
    assert!(t
        .iter()
        .all(|r| r.started_unix_ms.is_value() && (r.exe.is_some() || r.key.pid == Pid(0))));
    let threads: u64 = t
        .iter()
        .filter_map(|r| r.threads.value())
        .map(|&n| u64::from(n))
        .sum();
    assert!(value(s.thread_count, "threads") >= threads);
    let max_energy = t
        .iter()
        .filter_map(|r| r.energy_score.value().copied())
        .fold(0.0f32, f32::max);
    assert!((max_energy - 100.0).abs() < 1e-3, "{max_energy}");
}

#[test]
fn every_series_stays_plausible_for_an_hour() {
    let (snapshots, _) = sampled_hour();
    assert_eq!(snapshots.len(), 3600);
    let later = &snapshots[1..];
    let mut totals = Vec::new();
    let mut pressure_normal = 0;
    let mut npu_idle = 0;
    let (mut rx_sum, mut tx_sum) = (0.0, 0.0);
    let mut max_read = 0.0f64;
    let mut prev_disk_total = 0u64;
    for s in later {
        let total = value(s.cpu.total_pct, "cpu");
        let kernel = value(s.cpu.kernel_pct, "kernel");
        assert!((2.0..=96.0).contains(&total), "cpu {total}");
        assert!(
            kernel >= total * 0.12 && kernel <= total * 0.3,
            "{kernel} of {total}"
        );
        assert_eq!(s.cpu.per_cpu.len(), 10);
        for c in &s.cpu.per_cpu {
            assert!((0.0..=100.0).contains(&value(c.util_pct, "core")));
        }
        totals.push(total);
        let temp = value(s.cpu.temperature_c, "temp");
        assert!((45.0..=70.0).contains(&temp), "temp {temp}");
        let watts = value(s.power.system_w, "power");
        assert!((5.0..=30.0).contains(&watts), "power {watts}");
        assert!(value(s.cpu.power_w, "cpu power") < watts);

        let used = value(s.memory.used, "used") as f64 / GIB;
        assert!((17.0..=21.0).contains(&used), "memory {used}");
        let pressure = value(s.memory.pressure_level, "pressure");
        assert_ne!(pressure, PressureLevel::Critical);
        pressure_normal += usize::from(pressure == PressureLevel::Normal);

        let disk = &s.disks[0];
        let read = value(disk.read_bps, "read");
        let active = value(disk.active_pct, "active");
        assert!(read >= 0.0 && value(disk.write_bps, "write") >= 0.0);
        assert!((0.0..=100.0).contains(&active));
        max_read = max_read.max(read);
        let disk_total = value(disk.total_read, "disk total");
        assert!(disk_total >= prev_disk_total, "totals never go backwards");
        prev_disk_total = disk_total;

        assert_eq!(s.networks.len(), 18);
        let wifi = s.network("en0").expect("wi-fi");
        rx_sum += value(wifi.rx_bps, "rx");
        tx_sum += value(wifi.tx_bps, "tx");
        assert!(s.networks.iter().all(|n| value(n.rx_bps, "rx") >= 0.0));

        let gpu = value(s.gpus[0].util_pct, "gpu");
        assert!((0.0..=30.0).contains(&gpu), "gpu {gpu}");
        let npu = value(s.npus[0].active_pct, "npu");
        assert!((0.0..=100.0).contains(&npu));
        npu_idle += usize::from(npu == 0.0);

        for sensor in &s.sensors {
            let v = value(sensor.value, &sensor.id);
            let ok = match sensor.kind {
                SensorKind::Temperature => (30.0..=95.0).contains(&v),
                SensorKind::Power => (0.0..=30.0).contains(&v),
                SensorKind::Clock => (300.0..=5000.0).contains(&v),
                _ => (0.0..=100.0).contains(&v),
            };
            assert!(ok, "{} = {v}", sensor.id);
        }
        let live = table(s).live_count();
        assert!((480..=620).contains(&live), "{live} live processes");
    }
    let n = later.len();
    let max = totals.iter().copied().fold(0.0f32, f32::max);
    let min = totals.iter().copied().fold(100.0f32, f32::min);
    assert!(max > 60.0, "an hour holds a burst (max {max})");
    assert!(min < 15.0, "and quiet stretches (min {min})");
    assert!(pressure_normal * 10 > n * 8, "pressure mostly normal");
    assert!(npu_idle * 10 > n * 8, "neural engine mostly idle");
    assert!(npu_idle < n, "but not never used");
    assert!(rx_sum > tx_sum, "receive above send");
    assert!(max_read > 10e6, "disk reads are spiky (max {max_read})");
}

#[test]
fn an_hour_of_sampling_is_cheap() {
    let (snapshots, took) = sampled_hour();
    assert_eq!(snapshots.len(), 3600);
    println!("3600 demo sampler ticks took {took:?}");
    assert!(took < secs(10), "3600 ticks took {took:?}");
}

#[test]
fn churn_exits_are_retained_by_the_sampler_then_dropped() {
    let (provider, _) = demo_platform(DEFAULT_DEMO_SEED);
    let mut sampler = Sampler::new(Box::new(provider));
    let t0 = Instant::now();
    let snaps: Vec<Arc<Snapshot>> = (0..180)
        .filter_map(|i| sampler.tick(t0 + secs(i)))
        .collect();
    let retention = EXITED_PROCESS_RETENTION.as_secs() as usize;
    let mut checked = 0;
    for i in 1..snaps.len().saturating_sub(retention + 1) {
        let before = table(&snaps[i - 1]);
        let now = table(&snaps[i]);
        for row in now.iter().filter(|r| r.is_exited()) {
            let Some(was) = before.get(&row.key) else {
                continue;
            };
            if was.is_exited() {
                continue;
            }
            // Just exited: kept for the retention window, then gone.
            for later in &snaps[i..i + retention] {
                assert!(table(later).get(&row.key).is_some_and(|r| r.is_exited()));
            }
            assert!(table(&snaps[i + retention]).get(&row.key).is_none());
            checked += 1;
        }
    }
    assert!(checked >= 5, "only {checked} exits seen in 3 minutes");

    // New processes appear too, with fresh keys.
    let first: HashSet<ProcessKey> = table(&snaps[0]).iter().map(|r| r.key).collect();
    let last = table(&snaps[snaps.len() - 1]);
    assert!(last.iter().any(|r| !first.contains(&r.key)));
}

#[test]
fn end_process_checks_identity_and_removes_the_process() {
    let (mut p, services) = demo_platform(DEFAULT_DEMO_SEED);
    let t0 = Instant::now();
    let s = p.sample(t0);
    let t = table(&s);
    let browser = t.iter().find(|r| r.name == "Web Browser").expect("browser");
    let helpers: Vec<ProcessKey> = t.children_of(&browser.key).iter().map(|r| r.key).collect();
    assert!(!helpers.is_empty());

    let stale = ProcessKey::new(browser.key.pid.0, browser.key.start_time + 1);
    assert_eq!(
        services.end_process(stale, false),
        Err(ActionError::IdentityChanged)
    );
    assert_eq!(services.end_process(browser.key, true), Ok(()));
    assert_eq!(
        services.end_process(browser.key, false),
        Err(ActionError::NotFound)
    );
    let launchd = t.live_by_pid(Pid(1)).expect("pid 1").key;
    assert_eq!(
        services.end_process(launchd, false),
        Err(ActionError::PermissionDenied)
    );

    let next = p.sample(t0 + secs(1));
    let t = table(&next);
    assert!(t.get(&browser.key).is_none(), "gone from the next snapshot");
    assert!(
        helpers.iter().all(|k| t.get(k).is_none()),
        "helpers quit with it"
    );
    assert!(t
        .iter()
        .all(|r| r.kind != ProcessKind::Application || r.name != "Web Browser"));
}
