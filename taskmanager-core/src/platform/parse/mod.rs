//! Pure parsers for the OS text (and binary record) formats the native platform reads
//! (PLAN §4.3, §6): parsing lives in the core so it is unit-tested on every OS from
//! fixtures, while the reading itself stays in `taskmanager-native/src/platform/`.
//!
//! - [`proc_stat`] — Linux `/proc/<pid>/stat` (start-time stamp, thread count).
//! - [`desktop`] — freedesktop `.desktop` entries (Linux app detection).
//! - [`procfs`] — Linux `/proc/stat` CPU times, `/proc/meminfo`, PSI pressure,
//!   `/proc/loadavg` threads.
//! - [`sysfs`] — Linux `/sys` CPU cache descriptions and link speed.
//! - [`logical_processor`] — Windows `GetLogicalProcessorInformationEx` cache records.
//! - [`net`] — interface kind from its name; macOS `networksetup` hardware ports.

pub mod desktop;
pub mod logical_processor;
pub mod net;
pub mod proc_stat;
pub mod procfs;
pub mod sysfs;
