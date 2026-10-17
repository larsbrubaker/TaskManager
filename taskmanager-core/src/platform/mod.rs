//! The platform seam: what the core asks of an OS (PLAN §4.3).
//!
//! The core never touches the OS. A shell hands it two objects:
//! - a [`TelemetryProvider`], which the
//!   [`SampleProducer`](crate::telemetry::SampleProducer) polls for [`MachineInfo`] once
//!   and a [`Snapshot`] per tick (natively on a worker thread);
//! - a [`PlatformServices`], for actions (end a process, open a URL) and, in later
//!   milestones, inventories.
//!
//! `DemoPlatform` (core, `platform/demo/`) implements both for a simulated machine on
//! every target; `NativePlatform` (taskmanager-native) implements them per OS. Widgets
//! ask [`PlatformServices::is_demo`] instead of using `cfg`.
//!
//! ## Provider and services sharing state
//!
//! The provider moves to the sampling thread, the services stay with the UI, yet one must
//! see what the other does: ending a demo process must make it vanish from the next demo
//! snapshot, and a native provider may cache handles the services reuse. A platform
//! therefore builds both halves over one shared state, `Arc<Mutex<State>>`, cloning the
//! `Arc` into each:
//!
//! ```text
//! let machine = Arc::new(Mutex::new(DemoMachine::seeded(42)));
//! let provider = DemoProvider { machine: Arc::clone(&machine) };   // -> SampleProducer
//! let services = DemoServices { machine };                         // -> UI
//! ```
//!
//! `std::sync::Mutex` works on wasm32 too (single-threaded there, so it never blocks).
//! Hold the lock only for the duration of one call, never across a sample.
//!
//! Submodules:
//! - [`demo`] — the seeded simulated machine (`demo_platform(seed)`), on every target.

pub mod demo;

use std::fmt;

use web_time::Instant;

use crate::process::ProcessKey;
use crate::telemetry::{MachineInfo, Snapshot};

/// Whether a telemetry provider is working fully.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ProviderHealth {
    #[default]
    Healthy,
    /// Sampling works but something is missing or failing; the text says what, for the
    /// status bar ("Disk counters unavailable: permission denied").
    Degraded(String),
}

/// Source of machine telemetry.
///
/// `Send` because natively the sampler runs on a worker thread (PLAN §4.2).
pub trait TelemetryProvider: Send {
    /// The static facts about the machine. Called when the sampler starts and when the
    /// app asks for a refresh (devices added or removed).
    fn machine_info(&mut self) -> MachineInfo;

    /// Sample everything now. The provider keeps whatever previous counters it needs and
    /// returns rates over the time since its previous call (`Snapshot::elapsed`); rates
    /// on the first call are `Unavailable`, never zero. It sets `taken_at` to `now` and
    /// leaves `generation` and `health` alone: the producer stamps them. If the process
    /// listing fails it returns `processes: Unavailable` (never an empty table). It must
    /// not retain exited processes or carry tables forward itself; the producer does
    /// that (`ProcessTable::merge_exited` / `carried_forward`).
    fn sample(&mut self, now: Instant) -> Snapshot;

    /// How the provider is doing (as of its latest sample).
    fn health(&self) -> ProviderHealth;
}

/// Why a platform action did not happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionError {
    /// The pid now belongs to a different process (its start time changed), so the
    /// action was refused.
    IdentityChanged,
    /// The target no longer exists.
    NotFound,
    /// The OS refused (needs elevation, or belongs to another user).
    PermissionDenied,
    /// This platform (or the demo) cannot do this.
    Unsupported,
    /// The OS reported another failure.
    Failed(String),
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionError::IdentityChanged => {
                f.write_str("the process changed identity (its PID was reused)")
            }
            ActionError::NotFound => f.write_str("not found"),
            ActionError::PermissionDenied => f.write_str("permission denied"),
            ActionError::Unsupported => f.write_str("not supported on this platform"),
            ActionError::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for ActionError {}

/// Actions (and, from later milestones, inventories) a platform offers the UI.
///
/// `Send + Sync` so the UI can hand it to background tasks (an inventory scan, a
/// confirmation flow) behind an `Arc`; share state with the provider as described in
/// the module docs.
pub trait PlatformServices: Send + Sync {
    /// Whether this is the simulated demo machine (the web build, or `--demo`).
    fn is_demo(&self) -> bool;

    /// End the process `key` names: politely (`force == false`: SIGTERM / WM_CLOSE) or
    /// forcibly (SIGKILL / TerminateProcess).
    ///
    /// Implementors must re-read the target's start time immediately before acting and
    /// return [`ActionError::IdentityChanged`] if it no longer matches `key.start_time`
    /// (PLAN §4.4), and [`ActionError::NotFound`] if the pid is gone.
    fn end_process(&self, key: ProcessKey, force: bool) -> Result<(), ActionError>;

    /// Open `url` in the user's browser.
    fn open_url(&self, url: &str) -> Result<(), ActionError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_can_be_shared_across_threads() {
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn PlatformServices>();
        assert_send_sync::<std::sync::Arc<dyn PlatformServices>>();
    }

    #[test]
    fn health_defaults_to_healthy() {
        assert_eq!(ProviderHealth::default(), ProviderHealth::Healthy);
    }

    #[test]
    fn action_errors_read_as_sentences() {
        assert_eq!(
            ActionError::PermissionDenied.to_string(),
            "permission denied"
        );
        assert_eq!(ActionError::Failed("boom".into()).to_string(), "boom");
        assert!(ActionError::IdentityChanged.to_string().contains("reused"));
    }
}
