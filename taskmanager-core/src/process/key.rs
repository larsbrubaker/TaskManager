//! Process identity: `Pid` and `ProcessKey` (PLAN §4.4).
//!
//! A pid alone is not an identity: the OS recycles pids, so a stale selection or a delayed
//! "End task" could hit an unrelated process. A [`ProcessKey`] pairs the pid with the
//! process's start time. Tables select by key, and every action takes a key and has the
//! platform re-check the start time immediately before acting
//! (see [`PlatformServices`](crate::platform::PlatformServices)).

use std::fmt;

/// An OS process id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pid(pub u32);

impl fmt::Display for Pid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A process's identity: pid plus start time.
///
/// `start_time` must be a fine-grained creation stamp that is monotonic within a boot
/// and stable for the life of the process, so that two processes which held the same pid
/// always differ. Use the OS's own creation time at its native resolution:
/// - Linux: `/proc/<pid>/stat` field 22 `starttime` (clock ticks since boot);
/// - macOS: `proc_pidinfo(PROC_PIDTBSDINFO)` `pbi_start_tvsec`/`pbi_start_tvusec`,
///   combined as microseconds;
/// - Windows: the creation `FILETIME` from `GetProcessTimes` (100 ns units).
///
/// sysinfo's `Process::start_time()` (whole seconds of wall clock) is **not**
/// sufficient: a pid recycled within the same second would keep the key. The value is
/// only compared for equality and ordering within one provider and one boot. Keys order
/// by pid, then start time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessKey {
    pub pid: Pid,
    pub start_time: u64,
}

impl ProcessKey {
    pub fn new(pid: u32, start_time: u64) -> Self {
        Self {
            pid: Pid(pid),
            start_time,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn recycled_pid_is_a_different_process() {
        let first = ProcessKey::new(812, 1_000);
        let recycled = ProcessKey::new(812, 9_000);
        assert_ne!(first, recycled);
        let set: HashSet<_> = [first, recycled, first].into_iter().collect();
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn keys_order_by_pid_then_start() {
        let mut keys = vec![
            ProcessKey::new(20, 1),
            ProcessKey::new(3, 50),
            ProcessKey::new(3, 7),
        ];
        keys.sort();
        assert_eq!(
            keys,
            vec![
                ProcessKey::new(3, 7),
                ProcessKey::new(3, 50),
                ProcessKey::new(20, 1)
            ]
        );
        assert_eq!(Pid(42).to_string(), "42");
    }
}
