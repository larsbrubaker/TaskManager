//! Processes: identity, the per-sample process table, grouping and the parent/child tree.
//!
//! - [`key`] — `Pid` and `ProcessKey` (pid + start time), the identity every selection
//!   and action uses (PLAN §4.4).
//! - [`table`] — `ProcessRow` / `ProcessTable`: one row per process in a
//!   [`Snapshot`](crate::telemetry::Snapshot), lookups, top-N by CPU, Applications /
//!   Background grouping, retention of recently exited processes (PLAN §5.3) and
//!   carry-forward when a listing fails.
//! - [`tree`] — the parent/child forest the Processes page expands, and the per-table
//!   lookup index.
//! - [`kind`] — pure Application / Background heuristics over OS facts.
//! - [`proc_stat`] — parser for Linux `/proc/<pid>/stat` (the start-time stamp).
//! - [`desktop`] — parser for freedesktop `.desktop` entries (Linux app detection).

pub mod desktop;
pub mod key;
pub mod kind;
pub mod proc_stat;
pub mod table;
pub mod tree;

pub use key::{Pid, ProcessKey};
pub use table::{ProcessGroups, ProcessKind, ProcessRow, ProcessStatus, ProcessTable};
pub use tree::ProcessNode;
