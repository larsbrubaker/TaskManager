//! The process table: sysinfo's process list, keyed by the OS's fine-grained start stamp.
//!
//! Each tick refreshes sysinfo's processes (CPU time, memory, disk I/O, executable, owner),
//! then for every process reads its start stamp through the OS module
//! (`os::read_identity`) to build its [`ProcessKey`] (PLAN §4.4). If the stamp cannot be
//! read (permission: Windows protected processes) the key falls back to sysinfo's
//! start time in whole seconds scaled to microseconds ([`fallback_stamp`]); such keys are
//! reported in [`ProcessListing::fallback_keys`], and the services refuse every action on
//! them (`PermissionDenied`): an identity we cannot read is an identity we cannot verify.
//! A fallback key still identifies the row in tables (a pid recycled within the same
//! second would be needed to collide) but never authorises an action. On macOS (via `kinfo_proc`)
//! and Linux (`/proc`) the stamp is readable for every process, so fallbacks are a
//! Windows matter.
//!
//! Identity reads are cached per (pid, sysinfo start seconds) on macOS and Windows
//! ([`IdentityCache`], `os::CACHE_IDENTITY`), so only processes new this tick cost a
//! syscall. A pid recycled within the same second would match the cached pair; the cache
//! also drops an entry whose CPU time went backwards (a new process starts near zero), and
//! actions never use cached stamps (`end_process` re-reads), so the residual risk is a
//! table row briefly keeping the old key, never an action on the wrong process. Linux
//! reads `/proc/<pid>/stat` every tick anyway (it also yields the thread count).
//!
//! Rates (CPU share, disk bytes per second) come from cumulative counters keyed by
//! `ProcessKey`, so a process seen for the first time, or a recycled pid, reports
//! `Unavailable` rather than a bogus first delta.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use sysinfo::{
    Process, ProcessRefreshKind, ProcessesToUpdate, System, ThreadKind, Uid, UpdateKind, Users,
};
use taskmanager_core::process::{Pid, ProcessKind, ProcessRow, ProcessStatus};
use taskmanager_core::{ProcessKey, ProcessTable, Reading};

use super::os;
use super::os_api::{Identity, StampError, ThreadInfo};
use super::rates::{per_second, CounterTracker};

/// A process key's start stamp when the OS stamp is unreadable: sysinfo's start time
/// (whole seconds since the epoch) in microseconds. Good enough to key a table row; the
/// services refuse to act on such keys (`ActionError::PermissionDenied`).
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
/// A remembered identity read: the identity (`None`: the OS refused it) and the process's
/// CPU time when it was read.
#[derive(Clone, Copy, Debug)]
struct CachedIdentity {
    identity: Option<Identity>,
    cpu_ms: u64,
}

/// Identity reads remembered per (pid, sysinfo start seconds) across ticks.
#[derive(Debug, Default)]
pub struct IdentityCache {
    enabled: bool,
    previous: HashMap<(u32, u64), CachedIdentity>,
    current: HashMap<(u32, u64), CachedIdentity>,
}

impl IdentityCache {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    /// Start a tick: entries not looked up this tick are forgotten at the next one.
    pub fn begin_tick(&mut self) {
        self.previous = std::mem::take(&mut self.current);
    }

    /// The identity of `pid` (started at `start_secs`, with `cpu_ms` of CPU time so far):
    /// remembered from the previous tick when the pair matches and the CPU time did not go
    /// backwards, otherwise read with `read`.
    pub fn lookup(
        &mut self,
        pid: u32,
        start_secs: u64,
        cpu_ms: u64,
        read: impl FnOnce(u32) -> Result<Identity, StampError>,
    ) -> Result<Identity, StampError> {
        if !self.enabled {
            return read(pid);
        }
        let pair = (pid, start_secs);
        let remembered = self.previous.get(&pair).filter(|c| cpu_ms >= c.cpu_ms);
        let identity = match remembered {
            Some(cached) => cached.identity,
            None => match read(pid) {
                Ok(identity) => Some(identity),
                Err(StampError::NotFound) => return Err(StampError::NotFound),
                Err(StampError::Denied | StampError::Failed(_)) => None,
            },
        };
        self.current
            .insert(pair, CachedIdentity { identity, cpu_ms });
        identity.ok_or(StampError::Denied)
    }
}

pub struct ProcessSampler {
    identities: IdentityCache,
    names: NameLookup<Uid>,
    cpu_ms: CounterTracker<ProcessKey>,
    disk_read: CounterTracker<ProcessKey>,
    disk_written: CounterTracker<ProcessKey>,
    classifier: os::AppClassifier,
}

impl ProcessSampler {
    pub fn new() -> Self {
        Self {
            identities: IdentityCache::new(os::CACHE_IDENTITY),
            names: NameLookup::default(),
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
        self.identities.begin_tick();
        self.cpu_ms.begin_tick();
        self.disk_read.begin_tick();
        self.disk_written.begin_tick();

        let logical_cpus = sys.cpus().len();
        let per_pid_threads = threads.and_then(|t| t.per_pid.as_ref());
        self.names.begin_tick();
        let mut fallback_keys = HashSet::new();
        let mut thread_sum = Some(0u64);
        let mut rows = Vec::with_capacity(sys.processes().len());
        for (pid, process) in sys.processes() {
            if process.thread_kind() == Some(ThreadKind::Userland) {
                continue;
            }
            let pid = pid.as_u32();
            let identity = self.identities.lookup(
                pid,
                process.start_time(),
                process.accumulated_cpu_time(),
                os::read_identity,
            );
            let (key, stamp_threads, stamp_uid) = match identity {
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
            row.user = user_name(process, stamp_uid, users, &mut self.names);
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
    /// all-zero usage means "not allowed to read", those stay `Unavailable`; disk figures
    /// also stay `Unavailable` when the OS module says the I/O counters are unreadable
    /// (Linux `/proc/<pid>/io` of another user's process).
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
        // sysinfo reports 0 bytes for I/O counters it could not read; only a readable
        // counter is a measurement.
        if !os::disk_io_readable(row.key.pid.0) {
            return;
        }
        row.disk_read_bps = self.disk_read.rate(key, disk.total_read_bytes, elapsed);
        row.disk_write_bps = self
            .disk_written
            .rate(key, disk.total_written_bytes, elapsed);
        row.disk_total_read = Reading::Value(disk.total_read_bytes);
        row.disk_total_written = Reading::Value(disk.total_written_bytes);
    }
}

/// The owner's user name: sysinfo's uid for the process, else the uid the identity read
/// found (macOS root processes sysinfo may not inspect).
fn user_name(
    process: &Process,
    identity_uid: Option<u32>,
    users: &mut Users,
    names: &mut NameLookup<Uid>,
) -> Reading<String> {
    let uid = match process.user_id() {
        Some(uid) => uid.clone(),
        None => match identity_uid.and_then(os::sysinfo_uid) {
            Some(uid) => uid,
            None => return Reading::Unavailable,
        },
    };
    let mut lookup_users = |uid: &Uid, refresh: bool| {
        if refresh {
            users.refresh();
        }
        users.get_user_by_id(uid).map(|u| u.name().to_string())
    };
    names.resolve(uid, &mut lookup_users).into()
}

/// When to re-read the user list: at most once per tick, and only for a uid we have not
/// already failed to name. A uid with no passwd entry (containers, deleted users) is
/// remembered across ticks so it does not force a refresh every second; any refresh
/// (caused by a genuinely new uid) gives the remembered ones another chance.
#[derive(Debug)]
pub struct NameLookup<K> {
    unknown: HashSet<K>,
    refreshed_this_tick: bool,
}

impl<K> Default for NameLookup<K> {
    fn default() -> Self {
        Self {
            unknown: HashSet::new(),
            refreshed_this_tick: false,
        }
    }
}

impl<K: std::hash::Hash + Eq> NameLookup<K> {
    pub fn begin_tick(&mut self) {
        self.refreshed_this_tick = false;
    }

    /// The name of `key`. `lookup(key, refresh)` reads the name, re-reading the list
    /// first when `refresh` is set.
    pub fn resolve(
        &mut self,
        key: K,
        lookup: &mut impl FnMut(&K, bool) -> Option<String>,
    ) -> Option<String> {
        if let Some(name) = lookup(&key, false) {
            return Some(name);
        }
        if self.unknown.contains(&key) || self.refreshed_this_tick {
            self.unknown.insert(key);
            return None;
        }
        self.refreshed_this_tick = true;
        self.unknown.clear();
        let name = lookup(&key, true);
        if name.is_none() {
            self.unknown.insert(key);
        }
        name
    }
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

    fn id(stamp: u64) -> Identity {
        Identity {
            stamp,
            threads: None,
            uid: None,
        }
    }

    #[test]
    fn identities_are_read_once_per_pid_and_start_second() {
        let reads = std::cell::Cell::new(0);
        let read = |stamp: u64| {
            let reads = &reads;
            move |_pid: u32| {
                reads.set(reads.get() + 1);
                Ok(id(stamp))
            }
        };
        let mut cache = IdentityCache::new(true);
        cache.begin_tick();
        assert_eq!(cache.lookup(7, 100, 5, read(1)), Ok(id(1)));
        cache.begin_tick();
        assert_eq!(cache.lookup(7, 100, 9, read(2)), Ok(id(1)), "remembered");
        assert_eq!(reads.get(), 1);
        cache.begin_tick();
        // CPU time went backwards: a different process holds the pid now.
        assert_eq!(cache.lookup(7, 100, 3, read(3)), Ok(id(3)));
        cache.begin_tick();
        // Another start second is another process.
        assert_eq!(cache.lookup(7, 101, 3, read(4)), Ok(id(4)));
        cache.begin_tick();
        cache.begin_tick();
        assert_eq!(
            cache.lookup(7, 101, 3, read(5)),
            Ok(id(5)),
            "absent a tick: forgotten"
        );
        assert_eq!(reads.get(), 4);
    }

    #[test]
    fn refusals_are_remembered_and_gone_processes_are_not() {
        let mut cache = IdentityCache::new(true);
        cache.begin_tick();
        assert_eq!(
            cache.lookup(9, 1, 0, |_| Err(StampError::Denied)),
            Err(StampError::Denied)
        );
        cache.begin_tick();
        assert_eq!(
            cache.lookup(9, 1, 0, |_| Ok(id(5))),
            Err(StampError::Denied)
        );
        assert_eq!(
            cache.lookup(8, 1, 0, |_| Err(StampError::NotFound)),
            Err(StampError::NotFound)
        );
        cache.begin_tick();
        assert_eq!(cache.lookup(8, 1, 0, |_| Ok(id(6))), Ok(id(6)));
        let mut off = IdentityCache::new(false);
        off.begin_tick();
        off.lookup(1, 1, 1, |_| Ok(id(1))).ok();
        off.begin_tick();
        assert_eq!(
            off.lookup(1, 1, 1, |_| Ok(id(2))),
            Ok(id(2)),
            "disabled: always read"
        );
    }

    #[test]
    fn unknown_uids_do_not_refresh_the_user_list_every_tick() {
        let mut names = NameLookup::default();
        let mut refreshes = 0;
        let mut known = vec![(0u32, "root".to_string())];
        let mut lookup = |uid: &u32, refresh: bool| {
            if refresh {
                refreshes += 1;
            }
            known.iter().find(|(k, _)| k == uid).map(|(_, n)| n.clone())
        };
        for _tick in 0..5 {
            names.begin_tick();
            assert_eq!(names.resolve(0, &mut lookup).as_deref(), Some("root"));
            assert_eq!(names.resolve(4242, &mut lookup), None);
        }
        assert_eq!(
            refreshes, 1,
            "only the first sighting of uid 4242 refreshes"
        );

        // A genuinely new uid refreshes (once per tick), and gives 4242 another chance.
        known.push((501, "me".into()));
        let mut refreshes = 0;
        let mut lookup = |uid: &u32, refresh: bool| {
            if refresh {
                refreshes += 1;
            }
            known.iter().find(|(k, _)| k == uid).map(|(_, n)| n.clone())
        };
        names.begin_tick();
        assert_eq!(names.resolve(501, &mut lookup).as_deref(), Some("me"));
        assert_eq!(
            names.resolve(777, &mut lookup),
            None,
            "no second refresh this tick"
        );
        names.begin_tick();
        assert_eq!(names.resolve(777, &mut lookup), None);
        assert_eq!(names.resolve(4242, &mut lookup), None);
        assert_eq!(refreshes, 2);
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
