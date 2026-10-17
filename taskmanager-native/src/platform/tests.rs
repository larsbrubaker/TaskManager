//! Tests of the native platform against the real machine the tests run on: a sample has
//! plausible values, our own process keeps a stable key, a child we spawn is keyed by a
//! fine-grained stamp, is ended through `end_process` and is then gone, and a stale key is
//! refused (PLAN §4.4).

use std::process::{Child, Command, Stdio};
use std::thread::sleep;

use super::*;
use processes::fallback_stamp;
#[cfg(target_os = "macos")]
use sysinfo::ProcessesToUpdate;
use taskmanager_core::process::ProcessRow;

/// Long enough between samples for sysinfo's CPU deltas.
const GAP: Duration = Duration::from_millis(300);

fn sample(provider: &mut NativeProvider) -> Snapshot {
    provider.sample(Instant::now())
}

fn row(snapshot: &Snapshot, pid: u32) -> Option<ProcessRow> {
    snapshot
        .processes
        .value()
        .and_then(|t| t.live_by_pid(taskmanager_core::Pid(pid)).cloned())
}

/// A child that sleeps for a while without output.
fn spawn_sleeper() -> Child {
    #[cfg(unix)]
    let mut command = Command::new("sleep");
    #[cfg(unix)]
    command.arg("30");
    #[cfg(windows)]
    let mut command = Command::new("ping");
    #[cfg(windows)]
    command.args(["-n", "30", "127.0.0.1"]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn a sleeping child")
}

#[test]
fn provider_can_move_to_the_sampler_thread() {
    fn assert_send<T: Send>() {}
    assert_send::<NativeProvider>();
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<NativeServices>();
}

#[test]
fn a_real_sample_is_plausible() {
    let (mut provider, services) = native_platform();
    assert!(!services.is_demo());
    let info = provider.machine_info();
    assert!(info.cpu.logical_cores >= 1);
    assert!(info.memory.total.value().is_some_and(|t| *t > 0));

    let first = sample(&mut provider);
    assert!(
        first.cpu.total_pct.is_unavailable(),
        "first CPU load is not measured"
    );
    assert!(first.disks.iter().all(|d| d.read_bps.is_unavailable()));
    assert!(first.networks.iter().all(|n| n.rx_bps.is_unavailable()));
    sleep(GAP);
    let second = sample(&mut provider);
    assert!(second.elapsed >= GAP);

    let total = *second.cpu.total_pct.value().expect("CPU load measured");
    assert!((0.0..=100.0).contains(&total));
    assert_eq!(second.cpu.per_cpu.len(), info.cpu.logical_cores);
    for core in &second.cpu.per_cpu {
        let util = *core.util_pct.value().expect("per-core load measured");
        assert!((0.0..=100.0).contains(&util));
        if let Some(kernel) = core.kernel_pct.value() {
            assert!(*kernel >= 0.0 && *kernel <= util);
        }
    }
    let memory = &second.memory;
    let (used, total) = (memory.used.value().copied(), memory.total.value().copied());
    assert!(used.zip(total).is_some_and(|(u, t)| u <= t), "{memory:?}");
    assert!(second.uptime.is_value());
    assert!(second.thread_count.value().is_some_and(|n| *n > 0));
    for d in &second.disks {
        assert!(d.read_bps.value().is_some_and(|r| *r >= 0.0), "{d:?}");
    }
    let table = second.processes.value().expect("process listing");
    assert!(table.len() > 5);
    // macOS (kinfo_proc) and Linux (/proc) read a real stamp for every process, including
    // other users'; only Windows' protected processes fall back to whole seconds.
    #[cfg(unix)]
    assert!(
        lock(&services.shared).fallback_keys.is_empty(),
        "{} keys fell back to sysinfo seconds",
        lock(&services.shared).fallback_keys.len()
    );
    assert!(table
        .iter()
        .all(|r| r.cpu_pct.value().is_none_or(|c| (0.0..=100.0).contains(c))));
}

#[test]
fn our_own_process_keeps_its_key() {
    let (mut provider, _services) = native_platform();
    let me = std::process::id();
    let first = row(&sample(&mut provider), me).expect("we are listed");
    sleep(GAP);
    let second_snapshot = sample(&mut provider);
    let second = row(&second_snapshot, me).expect("still listed");
    assert_eq!(first.key, second.key);
    assert!(second.memory.value().is_some_and(|m| *m > 0));
    assert!(
        second.cpu_pct.is_value(),
        "seen twice, so its CPU share is measured"
    );
    assert!(second.user.is_value());
    let stamp = os::read_identity(me).expect("our own stamp").stamp;
    assert_eq!(second.key.start_time, stamp);
}

#[test]
fn a_spawned_child_is_keyed_finely_and_ended_by_key() {
    let (mut provider, services) = native_platform();
    let mut child = spawn_sleeper();
    let pid = child.id();
    let listed = row(&sample(&mut provider), pid).expect("child is listed");
    let identity = os::read_identity(pid).expect("child's stamp is readable");
    assert_eq!(listed.key, ProcessKey::new(pid, identity.stamp));
    assert!(
        !lock(&services.shared).fallback_keys.contains(&listed.key),
        "the child's key uses the OS stamp, not sysinfo's seconds"
    );
    #[cfg(target_os = "macos")]
    {
        // Microseconds: the whole seconds agree with sysinfo's start time.
        let mut sys = System::new();
        let spid = sysinfo::Pid::from_u32(pid);
        sys.refresh_processes(ProcessesToUpdate::Some(&[spid]), true);
        let secs = sys
            .process(spid)
            .expect("sysinfo sees the child")
            .start_time();
        assert_eq!(listed.key.start_time / 1_000_000, secs);
        assert_ne!(listed.key.start_time, fallback_stamp(secs));
    }

    services
        .end_process(listed.key, true)
        .expect("end the child");
    child.wait().expect("reap the child");
    assert_eq!(
        services.end_process(listed.key, true),
        Err(ActionError::NotFound)
    );
    assert!(row(&sample(&mut provider), pid).is_none_or(|r| r.key != listed.key));
}

#[test]
fn a_stale_key_is_refused() {
    let (mut provider, services) = native_platform();
    let mut child = spawn_sleeper();
    let pid = child.id();
    let listed = row(&sample(&mut provider), pid).expect("child is listed");
    let stale = ProcessKey::new(pid, listed.key.start_time.wrapping_add(1));
    assert_eq!(
        services.end_process(stale, true),
        Err(ActionError::IdentityChanged)
    );
    assert!(
        child.try_wait().expect("query the child").is_none(),
        "still running"
    );
    services
        .end_process(listed.key, true)
        .expect("end the child");
    child.wait().expect("reap the child");
}

#[test]
fn open_url_only_opens_web_links() {
    let (_provider, services) = native_platform();
    assert_eq!(
        services.open_url("file:///etc/passwd"),
        Err(ActionError::Unsupported)
    );
    assert_eq!(
        services.open_url("/Applications/Calculator.app"),
        Err(ActionError::Unsupported)
    );
}

#[test]
fn health_reports_the_latest_sample() {
    let (mut provider, _services) = native_platform();
    sample(&mut provider);
    // A healthy machine is the norm; if a subsystem fails here, the text must say which.
    match provider.health() {
        ProviderHealth::Healthy => {}
        ProviderHealth::Degraded(why) => assert!(!why.is_empty()),
    }
}

#[test]
fn fallback_keys_are_never_acted_on() {
    let (_provider, services) = native_platform();
    let mut child = spawn_sleeper();
    let pid = child.id();
    // Pretend the provider could only key the child by sysinfo's whole seconds.
    let key = ProcessKey::new(pid, fallback_stamp(1_700_000_000));
    lock(&services.shared).fallback_keys.insert(key);
    assert_eq!(
        services.end_process(key, true),
        Err(ActionError::PermissionDenied)
    );
    assert!(
        child.try_wait().expect("query the child").is_none(),
        "still running"
    );
    child.kill().expect("kill the child");
    child.wait().expect("reap the child");
}

/// Another user's I/O counters are unreadable on Linux: their disk figures are gaps, not 0.
#[cfg(target_os = "linux")]
#[test]
fn unreadable_disk_io_is_unavailable() {
    // SAFETY: `geteuid` has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        return; // root reads everything
    }
    let (mut provider, _services) = native_platform();
    sample(&mut provider);
    sleep(GAP);
    let snapshot = sample(&mut provider);
    let init = row(&snapshot, 1).expect("pid 1 is listed");
    assert!(!os::disk_io_readable(1));
    assert!(init.disk_total_read.is_unavailable() && init.disk_read_bps.is_unavailable());
    let me = row(&snapshot, std::process::id()).expect("we are listed");
    assert!(me.disk_total_read.is_value());
}
