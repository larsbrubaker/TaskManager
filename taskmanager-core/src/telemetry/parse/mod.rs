//! Pure parsers for the OS text formats the native platform samples (PLAN §4.3: parsing
//! lives in the core so it is unit-tested on every OS from fixture text).
//!
//! - [`procfs`] — Linux `/proc/stat` CPU times, `/proc/meminfo`, PSI pressure,
//!   `/proc/loadavg` threads.
//! - [`sysfs`] — Linux `/sys` CPU cache descriptions and link speed.
//! - [`net`] — interface kind from its name; macOS `networksetup` hardware ports.
//!
//! Per-process formats live with the process model (`process::proc_stat`,
//! `process::desktop`). PLAN §6 sketches these under `platform/parse/`; they sit here
//! until the platform module (being built in parallel) settles.

pub mod net;
pub mod procfs;
pub mod sysfs;
