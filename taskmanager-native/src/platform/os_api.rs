//! The vocabulary the per-OS modules (`macos.rs`, `windows.rs`, `linux.rs`) share with the
//! sysinfo-based parts of the native platform.
//!
//! Each OS module provides the same items (checked by the compiler on that OS):
//!
//! - `ZERO_USAGE_MEANS_DENIED: bool` — whether sysinfo's all-zero memory and CPU time for
//!   a process means "not allowed to read" rather than a measured zero;
//! - `read_identity(pid) -> Result<Identity, StampError>` — the fine-grained start stamp
//!   (PLAN §4.4) and, where the same read gives it, the thread count;
//! - `end_verified(key, force)` — re-read the stamp, refuse on mismatch, then end the
//!   process; `signal(pid, force)` — end it without re-reading (the caller verified);
//! - `is_system_mount(path)` — whether a volume is the one the OS boots from;
//! - `sysinfo_uid(u32) -> Option<sysinfo::Uid>` — a raw uid from an identity read as
//!   sysinfo's `Uid` (Unix; `None` on Windows);
//! - `KernelCpu` (`new`, `sample`), `MemoryExtras` (`new`, `fill`), `Threads` (`new`,
//!   `sample`), `AppClassifier` (`new`, `begin_tick`, `is_application`);
//! - `machine_extras(&mut MachineInfo)` — core classes, caches, interface names and kinds.

use std::collections::HashMap;

use taskmanager_core::{ActionError, Reading};

/// Why a process's start stamp could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StampError {
    /// The process does not exist (any more).
    NotFound,
    /// It exists but we may not inspect it.
    Denied,
    /// Another failure, described.
    Failed(String),
}

impl StampError {
    /// The action error an identity check that failed this way implies.
    pub fn into_action_error(self) -> ActionError {
        match self {
            StampError::NotFound => ActionError::NotFound,
            StampError::Denied => ActionError::PermissionDenied,
            StampError::Failed(why) => ActionError::Failed(why),
        }
    }
}

/// What one identity read tells us about a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    /// The OS creation stamp at its native resolution (`ProcessKey::start_time`).
    pub stamp: u64,
    /// Thread count, when the same read provides it (Linux `/proc/<pid>/stat`).
    pub threads: Option<u32>,
    /// The owner's real uid, when the same read provides it (macOS), for processes
    /// sysinfo could not attribute.
    pub uid: Option<u32>,
}

/// Kernel share of CPU time since the previous sample, 0–100.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KernelSplit {
    /// Across all logical CPUs (`None`: no time passed).
    pub total: Option<f32>,
    /// Per logical CPU in sysinfo's order; empty when the OS has no per-CPU split.
    pub per_cpu: Vec<Option<f32>>,
}

/// Thread counts this tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThreadInfo {
    /// Threads across the whole system.
    pub total: Reading<u64>,
    /// Threads per pid, when the OS lists them all at once (Windows).
    pub per_pid: Option<HashMap<u32, u32>>,
}
