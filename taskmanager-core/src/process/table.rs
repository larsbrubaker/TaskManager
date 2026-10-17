//! `ProcessRow` and `ProcessTable`: the process list in one snapshot (PLAN §5.1, §5.3).
//!
//! The [`TelemetryProvider`](crate::platform::TelemetryProvider) fills a `ProcessTable`
//! in each [`Snapshot`](crate::telemetry::Snapshot); rows are identified by
//! [`ProcessKey`]. Values the OS would not give us (another user's memory, a protected
//! process's CPU time) are `Unavailable` readings, which the Processes page shows as
//! "Access denied" rather than a zero.
//!
//! The [`SampleProducer`](crate::telemetry::SampleProducer) calls
//! [`ProcessTable::merge_exited`] so a process that just quit stays listed (in red) for a
//! few seconds, and [`ProcessTable::carried_forward`] when a listing fails, so a failed
//! read is never mistaken for every process exiting.
//!
//! Lookups (by key, by pid, parent, children) go through an index built once per table on
//! first use (see [`super::tree`]); rows are therefore only reachable read-only, and the
//! few mutating methods drop the index.

use std::collections::HashSet;
use std::fmt;
use std::sync::OnceLock;
use std::time::Duration;

use web_time::Instant;

use super::key::{Pid, ProcessKey};
use super::tree::TableIndex;
use crate::reading::Reading;

/// What the scheduler says the process is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ProcessStatus {
    Running,
    Sleeping,
    Stopped,
    Zombie,
    /// Gone from the OS; kept briefly by [`ProcessTable::merge_exited`].
    Exited,
    #[default]
    Unknown,
}

/// Which group the Processes page lists the process under.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ProcessKind {
    /// Has a window / is a user-facing app.
    Application,
    #[default]
    Background,
}

/// One process.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProcessRow {
    pub key: ProcessKey,
    /// Parent pid as the OS reports it (it may since have been recycled; see the tree).
    pub parent: Option<Pid>,
    /// Display name.
    pub name: String,
    /// Executable path.
    pub exe: Option<String>,
    /// Owning user name.
    pub user: Reading<String>,
    pub status: ProcessStatus,
    /// Share of total CPU capacity, 0–100.
    pub cpu_pct: Reading<f32>,
    /// Share of GPU capacity, 0–100.
    pub gpu_pct: Reading<f32>,
    /// Share of NPU capacity, 0–100.
    pub npu_pct: Reading<f32>,
    /// Memory footprint / resident set, bytes.
    pub memory: Reading<u64>,
    /// Private (unshared) memory, bytes.
    pub private_memory: Reading<u64>,
    /// Largest footprint since the process started, bytes.
    pub peak_memory: Reading<u64>,
    pub threads: Reading<u32>,
    /// Bytes per second.
    pub disk_read_bps: Reading<f64>,
    /// Bytes per second.
    pub disk_write_bps: Reading<f64>,
    /// Bytes read since the process started.
    pub disk_total_read: Reading<u64>,
    /// Bytes written since the process started.
    pub disk_total_written: Reading<u64>,
    /// Bytes per second received.
    pub net_rx_bps: Reading<f64>,
    /// Bytes per second sent.
    pub net_tx_bps: Reading<f64>,
    /// Estimated energy demand, 0–100, relative to the other processes in the same
    /// snapshot. How it is estimated is provider-defined (the OS's own energy impact
    /// where there is one, otherwise a weighting of CPU, GPU, disk and network); the
    /// Energy page footnote explains it.
    pub energy_score: Reading<f32>,
    /// Total CPU time consumed.
    pub cpu_time: Reading<Duration>,
    /// Start time, Unix milliseconds.
    pub started_unix_ms: Reading<u64>,
    pub kind: ProcessKind,
    /// "arm64", "x86_64", "x86_64 (Rosetta)", …
    pub arch: Reading<String>,
    /// Scheduling priority / nice value as the OS reports it.
    pub priority: Reading<i32>,
    /// Page faults since the process started.
    pub page_faults: Reading<u64>,
    /// When the process was first seen gone (set by [`ProcessTable::merge_exited`]).
    pub exited_at: Option<Instant>,
}

impl ProcessRow {
    /// A row with nothing measured yet.
    pub fn new(key: ProcessKey, name: impl Into<String>) -> Self {
        Self {
            key,
            name: name.into(),
            ..Self::default()
        }
    }

    /// Whether the process has exited (and is only kept for display).
    pub fn is_exited(&self) -> bool {
        self.status == ProcessStatus::Exited
    }

    /// Set the momentary measurements (CPU/GPU/NPU share, memory, threads, disk and
    /// network rates, energy) to `Unavailable`: we no longer know them. Identity,
    /// cumulative totals and static facts are kept.
    pub fn clear_live_measurements(&mut self) {
        self.cpu_pct = Reading::Unavailable;
        self.gpu_pct = Reading::Unavailable;
        self.npu_pct = Reading::Unavailable;
        self.memory = Reading::Unavailable;
        self.private_memory = Reading::Unavailable;
        self.threads = Reading::Unavailable;
        self.disk_read_bps = Reading::Unavailable;
        self.disk_write_bps = Reading::Unavailable;
        self.net_rx_bps = Reading::Unavailable;
        self.net_tx_bps = Reading::Unavailable;
        self.energy_score = Reading::Unavailable;
    }

    /// Turn this into the retained row of a process that exited at `at`.
    fn mark_exited(&mut self, at: Instant) {
        self.status = ProcessStatus::Exited;
        self.exited_at = Some(at);
        self.clear_live_measurements();
    }

    /// Whether this is an exited row whose retention has run out at `now`.
    fn retention_over(&self, now: Instant, keep_for: Duration) -> bool {
        self.exited_at
            .is_some_and(|at| now.saturating_duration_since(at) >= keep_for)
    }
}

/// The processes split into the Processes page's two groups.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProcessGroups<'a> {
    pub applications: Vec<&'a ProcessRow>,
    pub background: Vec<&'a ProcessRow>,
}

/// Every process in one snapshot.
#[derive(Clone, Default)]
pub struct ProcessTable {
    rows: Vec<ProcessRow>,
    /// Built on first lookup; dropped by every mutation.
    index: OnceLock<TableIndex>,
}

impl PartialEq for ProcessTable {
    fn eq(&self, other: &Self) -> bool {
        self.rows == other.rows
    }
}

impl fmt::Debug for ProcessTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProcessTable")
            .field("rows", &self.rows)
            .finish()
    }
}

impl ProcessTable {
    pub fn new(rows: Vec<ProcessRow>) -> Self {
        Self {
            rows,
            index: OnceLock::new(),
        }
    }

    /// The rows, in provider order (exited rows appended).
    pub fn rows(&self) -> &[ProcessRow] {
        &self.rows
    }

    /// Take the rows back out.
    pub fn into_rows(self) -> Vec<ProcessRow> {
        self.rows
    }

    /// Append a row.
    pub fn push(&mut self, row: ProcessRow) {
        self.rows.push(row);
        self.index = OnceLock::new();
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &ProcessRow> {
        self.rows.iter()
    }

    pub(super) fn index(&self) -> &TableIndex {
        self.index.get_or_init(|| TableIndex::build(&self.rows))
    }

    /// The position of the row with this identity in [`rows`](Self::rows).
    pub fn position(&self, key: &ProcessKey) -> Option<usize> {
        self.index().by_key.get(key).copied()
    }

    /// The row with this identity.
    pub fn get(&self, key: &ProcessKey) -> Option<&ProcessRow> {
        self.position(key).map(|i| &self.rows[i])
    }

    /// The running (not exited) process with this pid.
    pub fn live_by_pid(&self, pid: Pid) -> Option<&ProcessRow> {
        self.index().live_by_pid.get(&pid).map(|&i| &self.rows[i])
    }

    /// Processes not yet exited.
    pub fn live_count(&self) -> usize {
        self.rows.iter().filter(|r| !r.is_exited()).count()
    }

    /// The `n` busiest live processes by CPU, busiest first. Only measured CPU values
    /// count (a process whose CPU is unreadable is not ranked as idle); ties go to the
    /// lower pid, then the earlier start.
    pub fn top_by_cpu(&self, n: usize) -> Vec<&ProcessRow> {
        let mut ranked: Vec<(&ProcessRow, f32)> = self
            .rows
            .iter()
            .filter(|r| !r.is_exited())
            .filter_map(|r| r.cpu_pct.value().map(|cpu| (r, *cpu)))
            .collect();
        ranked.sort_by(|(a, a_cpu), (b, b_cpu)| {
            b_cpu.total_cmp(a_cpu).then_with(|| a.key.cmp(&b.key))
        });
        ranked.into_iter().take(n).map(|(r, _)| r).collect()
    }

    /// Applications and background processes, each in table order.
    pub fn grouped(&self) -> ProcessGroups<'_> {
        let (applications, background) = self
            .rows
            .iter()
            .partition(|r| r.kind == ProcessKind::Application);
        ProcessGroups {
            applications,
            background,
        }
    }

    /// Keep processes that vanished since `previous` as `Exited` rows for `keep_for`.
    ///
    /// Call on the new table with the previous snapshot's (already merged) table. A row in
    /// `previous` whose key is missing here is appended, marked exited at `now` (or kept
    /// with its original exit time if it was already an exited row) until `keep_for` has
    /// passed. A zero `keep_for` keeps nothing.
    pub fn merge_exited(&mut self, previous: &ProcessTable, now: Instant, keep_for: Duration) {
        let current: HashSet<ProcessKey> = self.rows.iter().map(|r| r.key).collect();
        let before = self.rows.len();
        for row in &previous.rows {
            if current.contains(&row.key) {
                continue;
            }
            let exited_at = row.exited_at.unwrap_or(now);
            if now.saturating_duration_since(exited_at) >= keep_for {
                continue;
            }
            let mut kept = row.clone();
            kept.mark_exited(exited_at);
            self.rows.push(kept);
        }
        if self.rows.len() != before {
            self.index = OnceLock::new();
        }
    }

    /// The table to show when this tick's listing failed: this (last known) table with
    /// every live row's momentary measurements `Unavailable` (not `Exited`: we do not
    /// know that anything quit), and exited rows whose retention ran out dropped.
    pub fn carried_forward(&self, now: Instant, keep_for: Duration) -> ProcessTable {
        let rows = self
            .rows
            .iter()
            .filter(|r| !r.retention_over(now, keep_for))
            .map(|r| {
                let mut row = r.clone();
                if !row.is_exited() {
                    row.clear_live_measurements();
                }
                row
            })
            .collect();
        ProcessTable::new(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pid: u32, start: u64, cpu: Reading<f32>) -> ProcessRow {
        ProcessRow {
            cpu_pct: cpu,
            status: ProcessStatus::Running,
            ..ProcessRow::new(ProcessKey::new(pid, start), format!("p{pid}"))
        }
    }

    #[test]
    fn lookup_by_key_never_matches_a_recycled_pid() {
        let table = ProcessTable::new(vec![row(7, 500, Reading::Value(1.0))]);
        assert!(table.get(&ProcessKey::new(7, 500)).is_some());
        assert!(table.get(&ProcessKey::new(7, 100)).is_none());
        assert_eq!(table.position(&ProcessKey::new(7, 500)), Some(0));
        assert_eq!(
            table.live_by_pid(Pid(7)).map(|r| r.key.start_time),
            Some(500)
        );
        assert!(table.live_by_pid(Pid(8)).is_none());
    }

    #[test]
    fn mutations_rebuild_the_index() {
        let mut table = ProcessTable::new(vec![row(1, 1, Reading::Value(1.0))]);
        assert!(table.get(&ProcessKey::new(2, 1)).is_none());
        table.push(row(2, 1, Reading::Value(1.0)));
        assert_eq!(table.position(&ProcessKey::new(2, 1)), Some(1));

        let previous = ProcessTable::new(vec![row(3, 1, Reading::Value(1.0))]);
        table.merge_exited(&previous, Instant::now(), Duration::from_secs(3));
        assert_eq!(table.position(&ProcessKey::new(3, 1)), Some(2));
        assert!(table.live_by_pid(Pid(3)).is_none());
    }

    #[test]
    fn duplicate_keys_resolve_to_the_first_row() {
        let mut second = row(4, 1, Reading::Value(9.0));
        second.name = "dup".into();
        let table = ProcessTable::new(vec![row(4, 1, Reading::Value(1.0)), second]);
        assert_eq!(table.position(&ProcessKey::new(4, 1)), Some(0));
    }

    #[test]
    fn equality_ignores_the_index() {
        let a = ProcessTable::new(vec![row(1, 1, Reading::Value(1.0))]);
        let b = a.clone();
        let _ = a.get(&ProcessKey::new(1, 1));
        assert_eq!(a, b);
        assert!(format!("{a:?}").contains("rows"));
    }

    #[test]
    fn top_by_cpu_ranks_values_only_with_pid_tiebreak() {
        let mut exited = row(1, 1, Reading::Value(99.0));
        exited.status = ProcessStatus::Exited;
        let table = ProcessTable::new(vec![
            row(30, 1, Reading::Value(5.0)),
            row(10, 1, Reading::Unavailable),
            row(20, 1, Reading::Value(12.5)),
            row(5, 1, Reading::Value(5.0)),
            row(40, 1, Reading::Unsupported),
            row(50, 1, Reading::Value(0.0)),
            exited,
        ]);
        let top: Vec<u32> = table.top_by_cpu(3).iter().map(|r| r.key.pid.0).collect();
        assert_eq!(top, vec![20, 5, 30]);
        let all: Vec<u32> = table.top_by_cpu(100).iter().map(|r| r.key.pid.0).collect();
        assert_eq!(all, vec![20, 5, 30, 50]);
        assert!(table.top_by_cpu(0).is_empty());
    }

    #[test]
    fn grouping_splits_apps_from_background() {
        let mut app = row(2, 1, Reading::Value(1.0));
        app.kind = ProcessKind::Application;
        let table = ProcessTable::new(vec![
            row(1, 1, Reading::Unavailable),
            app,
            row(3, 1, Reading::Unavailable),
        ]);
        let groups = table.grouped();
        assert_eq!(groups.applications.len(), 1);
        assert_eq!(groups.applications[0].key.pid, Pid(2));
        let bg: Vec<u32> = groups.background.iter().map(|r| r.key.pid.0).collect();
        assert_eq!(bg, vec![1, 3]);
    }

    #[test]
    fn vanished_processes_are_kept_briefly_as_exited() {
        let t0 = Instant::now();
        let keep = Duration::from_secs(3);
        let mut rows = vec![
            row(1, 1, Reading::Value(2.0)),
            row(2, 1, Reading::Value(40.0)),
        ];
        rows[1].memory = Reading::Value(1 << 20);
        rows[1].net_rx_bps = Reading::Value(10.0);
        rows[1].peak_memory = Reading::Value(1 << 21);
        rows[1].user = Reading::Value("lars".into());
        let previous = ProcessTable::new(rows);

        let mut now = ProcessTable::new(vec![row(1, 1, Reading::Value(3.0))]);
        now.merge_exited(&previous, t0, keep);
        assert_eq!(now.len(), 2);
        assert_eq!(now.live_count(), 1);
        let gone = now.get(&ProcessKey::new(2, 1)).cloned().unwrap();
        assert!(gone.is_exited());
        assert_eq!(gone.exited_at, Some(t0));
        assert!(gone.cpu_pct.is_unavailable() && gone.memory.is_unavailable());
        assert!(gone.net_rx_bps.is_unavailable());
        assert_eq!(gone.peak_memory, Reading::Value(1 << 21));
        assert_eq!(gone.user, Reading::Value("lars".into()));
        assert!(now.top_by_cpu(5).iter().all(|r| !r.is_exited()));
        assert!(now.live_by_pid(Pid(2)).is_none());

        // Still within the window: kept, with the original exit time.
        let mut later = ProcessTable::new(vec![row(1, 1, Reading::Value(3.0))]);
        later.merge_exited(&now, t0 + Duration::from_secs(2), keep);
        assert_eq!(
            later.get(&ProcessKey::new(2, 1)).and_then(|r| r.exited_at),
            Some(t0)
        );

        // Past the window: dropped.
        let mut much_later = ProcessTable::new(vec![row(1, 1, Reading::Value(3.0))]);
        much_later.merge_exited(&later, t0 + Duration::from_secs(3), keep);
        assert_eq!(much_later.len(), 1);
    }

    #[test]
    fn a_recycled_pid_does_not_hide_the_exit() {
        let t0 = Instant::now();
        let previous = ProcessTable::new(vec![row(9, 100, Reading::Value(1.0))]);
        let mut now = ProcessTable::new(vec![row(9, 200, Reading::Value(1.0))]);
        now.merge_exited(&previous, t0, Duration::from_secs(3));
        assert_eq!(now.len(), 2);
        assert!(now
            .get(&ProcessKey::new(9, 100))
            .is_some_and(|r| r.is_exited()));
        assert_eq!(now.live_by_pid(Pid(9)).map(|r| r.key.start_time), Some(200));
    }

    #[test]
    fn zero_retention_keeps_nothing() {
        let previous = ProcessTable::new(vec![row(1, 1, Reading::Value(1.0))]);
        let mut now = ProcessTable::default();
        now.merge_exited(&previous, Instant::now(), Duration::ZERO);
        assert!(now.is_empty());
    }

    #[test]
    fn carried_forward_keeps_rows_but_forgets_live_values() {
        let t0 = Instant::now();
        let keep = Duration::from_secs(3);
        let mut live = row(1, 1, Reading::Value(25.0));
        live.cpu_time = Reading::Value(Duration::from_secs(7));
        let fresh_at = t0 + Duration::from_secs(9);
        let mut fresh_exit = row(2, 1, Reading::Unavailable);
        fresh_exit.mark_exited(fresh_at);
        let mut stale_exit = row(3, 1, Reading::Unavailable);
        stale_exit.mark_exited(t0);
        let table = ProcessTable::new(vec![live, fresh_exit, stale_exit]);

        let carried = table.carried_forward(t0 + Duration::from_secs(10), keep);
        assert_eq!(carried.len(), 2);
        let kept = carried.get(&ProcessKey::new(1, 1)).cloned().unwrap();
        assert_eq!(kept.status, ProcessStatus::Running, "not marked exited");
        assert!(kept.cpu_pct.is_unavailable());
        assert_eq!(kept.cpu_time, Reading::Value(Duration::from_secs(7)));
        assert!(carried
            .get(&ProcessKey::new(2, 1))
            .is_some_and(|r| r.exited_at == Some(fresh_at)));
        assert!(carried.get(&ProcessKey::new(3, 1)).is_none());
    }
}
