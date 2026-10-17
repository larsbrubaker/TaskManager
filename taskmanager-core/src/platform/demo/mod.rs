//! The demo platform: a deterministic simulated machine for the web build, `--demo`, tests
//! and screenshots (PLAN §4.3). Runs on every target, wasm32 included: no threads, no
//! system clock, no OS entropy.
//!
//! [`demo_platform`] builds a [`DemoProvider`] (the [`TelemetryProvider`]) and
//! [`DemoServices`] (the [`PlatformServices`]) over one shared
//! `Arc<Mutex<`[`DemoMachine`]`>>`, so ending a process through the services removes it
//! from the provider's next snapshot.
//!
//! Everything derives from the seed and the simulated clock (time since the first
//! sample), so the same seed sampled at the same offsets yields identical snapshots.
//!
//! - [`rng`] — seeded generator, stateless hash, smooth value noise and pulses.
//! - [`profile`] — the static machine (an "Apple M5" class laptop) and its device ids.
//! - [`load`] — system load curves as pure functions of time.
//! - [`catalog`] — generic names for apps, helpers, daemons, tools and users.
//! - [`population`] — the process list and its churn.
//! - [`rows`] — per-sample process measurements.
//! - [`machine`] — `DemoMachine`, which ties them into snapshots.

pub mod catalog;
pub mod load;
pub mod machine;
pub mod population;
pub mod profile;
pub mod rng;
pub mod rows;

use std::sync::{Arc, Mutex, MutexGuard};

use web_time::Instant;

use crate::platform::{ActionError, PlatformServices, ProviderHealth, TelemetryProvider};
use crate::process::ProcessKey;
use crate::telemetry::{MachineInfo, Snapshot};

pub use machine::{DemoMachine, DEMO_EPOCH_UNIX_MS};

/// The seed the shells and screenshots use unless told otherwise.
pub const DEFAULT_DEMO_SEED: u64 = 42;

/// Lock the shared machine. A panic while holding the lock cannot leave the simulation
/// half-updated in a way that matters to a demo, so a poisoned lock is simply reused.
fn lock(machine: &Mutex<DemoMachine>) -> MutexGuard<'_, DemoMachine> {
    machine
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The demo's telemetry provider.
pub struct DemoProvider {
    machine: Arc<Mutex<DemoMachine>>,
}

impl TelemetryProvider for DemoProvider {
    fn machine_info(&mut self) -> MachineInfo {
        lock(&self.machine).machine_info()
    }

    fn sample(&mut self, now: Instant) -> Snapshot {
        lock(&self.machine).sample(now)
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }
}

/// The demo's platform services: acts on the simulated machine.
pub struct DemoServices {
    machine: Arc<Mutex<DemoMachine>>,
}

impl PlatformServices for DemoServices {
    fn is_demo(&self) -> bool {
        true
    }

    /// Removes the simulated process (and its descendants); polite and forced are the
    /// same here.
    fn end_process(&self, key: ProcessKey, _force: bool) -> Result<(), ActionError> {
        lock(&self.machine).end_process(key)
    }

    /// Nothing to open in the simulation; the UI tells the user it is a demo.
    fn open_url(&self, _url: &str) -> Result<(), ActionError> {
        Ok(())
    }
}

/// The demo provider and services for the machine drawn from `seed`, sharing one state.
pub fn demo_platform(seed: u64) -> (DemoProvider, DemoServices) {
    let machine = Arc::new(Mutex::new(DemoMachine::seeded(seed)));
    let provider = DemoProvider {
        machine: Arc::clone(&machine),
    };
    let services = DemoServices { machine };
    (provider, services)
}
