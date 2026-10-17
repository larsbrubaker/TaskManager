//! The process table: sysinfo's process list, keyed by the OS's fine-grained start stamp.
//!
//! Each tick refreshes sysinfo's processes (CPU time, memory, disk I/O, executable, owner),
//! then for every process reads its start stamp through the OS module
//! (`os::read_identity`) to build its [`ProcessKey`] (PLAN §4.4). If the stamp cannot be
//! read (permission: Windows protected processes) the key falls back to sysinfo's
//! start time in whole seconds scaled to microseconds ([`fallback_stamp`]); such keys are
//! reported in [`ProcessListing::fallback_keys`] so the services re-check them the same
//! way. A fallback key is still unique in practice (a pid recycled within the same second
//! would be needed to collide) but weaker than a real stamp. On macOS (via `kinfo_proc`)
//! and Linux (`/proc`) the stamp is readable for every process, so fallbacks are a
//! Windows matter.
//!
//! Rates (CPU share, disk bytes per second) come from cumulative counters keyed by
//! `ProcessKey`, so a process seen for the first time, or a recycled pid, reports
//! `Unavailable` rather than a bogus first delta.

use std::collections::HashSet;
use std::time::Duration;

use sysinfo::{
    Process, ProcessRefreshKind, ProcessesToUpdate, System, ThreadKind, UpdateKind, Users,
};
use taskmanager_core::process::{Pid, ProcessKind, ProcessRow, ProcessStatus};
use taskmanager_core::{ProcessKey, ProcessTable, Reading};

use super::os;
use super::os_api::{StampError, ThreadInfo};
use super::rates::{per_second, CounterTracker};

/// A process key's start stamp when the OS stamp is unreadable: sysinfo's start time
/// (whole seconds since the epoch) in microseconds.
pub fn fallback_stamp(start_time_secs: u64) -> u64 {
    start_time_secs.saturating_mul(1_000_000)
}

/// CPU share of the whole machine, 0–100, from CPU milliseconds used over `elapsed` on
/// `logical_cpus` CPUs.
pub fn cpu_share(
    cpu_ms: Reading<u64>,
    elapsed: Option<Duration>,
    logical_cpus: usize,
) -> Reading<f32> {
    let per_sec = per_second(cpu_ms, elapsed);
    per_sec.map(|ms_per_sec| {
        let pct = ms_per_sec / 10.0 / logical_cpus.max(1) as f64;
        pct.clamp(0.0, 100.0) as f32
    })
}

/// sysinfo's scheduler state as ours.
fn status(status: sysinfo::ProcessStatus) -> ProcessStatus {
    use sysinfo::ProcessStatus as S;
    match status {
        S::Run | S::Waking => ProcessStatus::Running,
        S::Sleep
        | S::Idle
        | S::UninterruptibleDiskSleep
        | S::Parked
        | S::LockBlocked
        | S::Wakekill => ProcessStatus::Sleeping,
        S::Stop | S::Tracing | S::Suspended => ProcessStatus::Stopped,
        S::Zombie | S::Dead => ProcessStatus::Zombie,
        S::Unknown(_) => ProcessStatus::Unknown,
    }
}

/// One tick's listing.
#[derive(Debug)]
pub struct ProcessListing {
    pub table: ProcessTable,
    /// Keys whose start stamp is a [`fallback_stamp`].
    pub fallback_keys: HashSet<ProcessKey>,
    /// Sum of per-process thread counts, when every listed process reported one.
    pub thread_sum: Option<u64>,
}

/// Process sampling state: previous counters per key, the Applications classifier.
pub struct ProcessSampler {
    cpu_ms: CounterTracker<ProcessKey>,
    disk_read: CounterTracker<ProcessKey>,
    disk_written: CounterTracker<ProcessKey>,
    classifier: os::AppClassifier,
}

impl ProcessSampler {
    pub fn new() -> Self {
        Self {
            cpu_ms: CounterTracker::default(),
            disk_read: CounterTracker::default(),
            disk_written: CounterTracker::default(),
            classifier: os::AppClassifier::new(),
        }
    }

    /// List every process now. `Err` (with the reason) when the listing failed; problems
    /// that still produced a table are pushed onto `issues`. `threads` is this tick's
    /// OS thread information (per-pid counts on Windows).
    pub fn sample(
        &mut self,
        sys: &mut System,
        users: &mut Users,
        threads: Option<&ThreadInfo>,
        elapsed: Option<Duration>,
        issues: &mut Vec<String>,
    ) -> Result<ProcessListing, String> {
        let refresh = ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_user(UpdateKind::OnlyIfNotSet)
            // Threads come from the OS module; sysinfo's per-task walk (Linux) is costly.
            .without_tasks();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);
        if sys.processes().is_empty() {
            return Err("process listing failed".into());
        }
        if let Err(why) = self.classifier.begin_tick() {
            issues.push(format!("application detection unavailable: {why}"));
        }
        self.cpu_ms.begin_tick();
        self.disk_read.begin_tick();
        self.disk_written.begin_tick();

        let logical_cpus = sys.cpus().len();
        let per_pid_threads = threads.and_then(|t| t.per_pid.as_ref());
        let mut users_refreshed = false;
        let mut fallback_keys = HashSet::new();
        let mut thread_sum = Some(0u64);
        let mut rows = Vec::with_capacity(sys.processes().len());
        for (pid, process) in sys.processes() {
            if process.thread_kind() == Some(ThreadKind::Userland) {
                continue;
            }
            let pid = pid.as_u32();
            let (key, stamp_threads, stamp_uid) = match os::read_identity(pid) {
                Ok(identity) => (
                    ProcessKey::new(pid, identity.stamp),
                    identity.threads,
                    identity.uid,
                ),
                // Exited between the listing and the stamp read: not in this table.
                Err(StampError::NotFound) => continue,
                Err(StampError::Denied | StampError::Failed(_)) => {
                    let key = ProcessKey::new(pid, fallback_stamp(process.start_time()));
                    fallback_keys.insert(key);
                    (key, None, None)
                }
            };
            let threads =
                stamp_threads.or_else(|| per_pid_threads.and_then(|m| m.get(&pid).copied()));
            thread_sum = thread_sum.zip(threads).map(|(sum, n)| sum + u64::from(n));

            let mut row = ProcessRow::new(key, process.name().to_string_lossy());
            row.parent = process.parent().map(|p| Pid(p.as_u32()));
            row.exe = process.exe().map(|p| p.to_string_lossy().into_owned());
            row.user = user_name(process, stamp_uid, users, &mut users_refreshed);
            row.status = status(process.status());
            row.threads = threads.into();
            row.kind = if self
                .classifier
                .is_application(pid, row.exe.as_deref(), &row.name)
            {
                ProcessKind::Application
            } else {
                ProcessKind::Background
            };
            if process.start_time() > 0 {
                row.started_unix_ms = Reading::Value(process.start_time().saturating_mul(1000));
            }
            self.fill_usage(&mut row, process, elapsed, logical_cpus);
            rows.push(row);
        }
        Ok(ProcessListing {
            table: ProcessTable::new(rows),
            fallback_keys,
            thread_sum,
        })
    }

    /// CPU share and time, memory, disk rates and totals. On platforms where sysinfo's
    /// all-zero usage means "not allowed to read", those stay `Unavailable`.
    fn fill_usage(
        &mut self,
        row: &mut ProcessRow,
        process: &Process,
        elapsed: Option<Duration>,
        logical_cpus: usize,
    ) {
        let cpu_ms = process.accumulated_cpu_time();
        let memory = process.memory();
        let disk = process.disk_usage();
        let denied = os::ZERO_USAGE_MEANS_DENIED && cpu_ms == 0 && memory == 0;
        if denied {
            return;
        }
        let key = row.key;
        row.cpu_pct = cpu_share(self.cpu_ms.delta(key, cpu_ms), elapsed, logical_cpus);
        row.cpu_time = Reading::Value(Duration::from_millis(cpu_ms));
        row.memory = Reading::Value(memory);
        row.disk_read_bps = self.disk_read.rate(key, disk.total_read_bytes, elapsed);
        row.disk_write_bps = self
            .disk_written
            .rate(key, disk.total_written_bytes, elapsed);
        row.disk_total_read = Reading::Value(disk.total_read_bytes);
        row.disk_total_written = Reading::Value(disk.total_written_bytes);
    }
}

/// The owner's user name: sysinfo's uid for the process, else the uid the identity read
/// found (macOS root processes sysinfo may not inspect). The user list is refreshed at
/// most once per tick, when a uid is new.
fn user_name(
    process: &Process,
    identity_uid: Option<u32>,
    users: &mut Users,
    refreshed: &mut bool,
) -> Reading<String> {
    let uid = match process.user_id() {
        Some(uid) => uid.clone(),
        None => match identity_uid.and_then(os::sysinfo_uid) {
            Some(uid) => uid,
            None => return Reading::Unavailable,
        },
    };
    if users.get_user_by_id(&uid).is_none() && !*refreshed {
        users.refresh();
        *refreshed = true;
    }
    users
        .get_user_by_id(&uid)
        .map(|u| u.name().to_string())
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_share_is_of_the_whole_machine_and_clamped() {
        let second = Some(Duration::from_secs(1));
        // 500 CPU-ms in one second on 4 CPUs: half of one CPU, 12.5 % of the machine.
        assert_eq!(
            cpu_share(Reading::Value(500), second, 4),
            Reading::Value(12.5)
        );
        assert_eq!(
            cpu_share(Reading::Value(9_000), second, 4),
            Reading::Value(100.0)
        );
        assert_eq!(
            cpu_share(Reading::Unavailable, second, 4),
            Reading::Unavailable
        );
        assert_eq!(cpu_share(Reading::Value(10), None, 4), Reading::Unavailable);
        assert_eq!(
            cpu_share(Reading::Value(1_000), second, 0),
            Reading::Value(100.0)
        );
    }

    #[test]
    fn fallback_stamps_are_whole_seconds_in_microseconds() {
        assert_eq!(fallback_stamp(1_700_000_000), 1_700_000_000_000_000);
        assert_eq!(fallback_stamp(u64::MAX), u64::MAX);
    }

    #[test]
    fn scheduler_states_map() {
        assert_eq!(status(sysinfo::ProcessStatus::Run), ProcessStatus::Running);
        assert_eq!(
            status(sysinfo::ProcessStatus::Sleep),
            ProcessStatus::Sleeping
        );
        assert_eq!(status(sysinfo::ProcessStatus::Stop), ProcessStatus::Stopped);
        assert_eq!(
            status(sysinfo::ProcessStatus::Zombie),
            ProcessStatus::Zombie
        );
        assert_eq!(
            status(sysinfo::ProcessStatus::Unknown(7)),
            ProcessStatus::Unknown
        );
    }
}
