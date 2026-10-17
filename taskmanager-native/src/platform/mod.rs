//! `NativePlatform`: the real machine behind TaskManager's platform seam (PLAN §4.3).
//!
//! [`native_platform`] builds the two halves `taskmanager-core` asks for:
//! - [`NativeProvider`] (`TelemetryProvider`, runs on the sampler's worker thread):
//!   sysinfo for the cross-platform base (CPU load and clocks, memory and swap, processes,
//!   volumes, interfaces, temperatures, users, uptime) plus the per-OS module for what
//!   sysinfo lacks: fine-grained process start stamps, the kernel CPU split, memory
//!   composition and pressure, thread counts, Application detection, core classes and
//!   caches. Rates are `Unavailable` on the first sample and computed over the time since
//!   the previous one after that. Any subsystem that fails a tick makes the provider's
//!   health `Degraded` with a description; a failed process listing returns
//!   `processes: Unavailable` (the producer carries the last table forward and the health
//!   tells the UI it is stale).
//! - [`NativeServices`] (`PlatformServices`, `is_demo() == false`): end a process after
//!   re-checking its identity, open URLs.
//!
//! The halves share an `Arc<Mutex<Shared>>` (see `taskmanager_core::platform`): the
//! provider records which published keys carry a fallback start stamp, and the services
//! refuse to act on those keys (we could not read the process's identity, so we cannot
//! verify it before acting).
//!
//! `Instant` is `std::time::Instant`: on native targets the core's `web_time::Instant` is
//! a re-export of it.
//!
//! Per-OS code lives in `macos.rs`, `windows.rs` and `linux.rs` (the items each provides
//! are listed in [`os_api`]); `unix.rs` holds what macOS and Linux share.

mod devices;
mod machine;
mod os_api;
mod processes;
mod rates;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use linux as os;
#[cfg(target_os = "macos")]
use macos as os;
#[cfg(windows)]
use windows as os;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
compile_error!("TaskManager's native platform supports macOS, Windows and Linux only");

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use sysinfo::{Components, CpuRefreshKind, DiskRefreshKind, Disks, Networks, System, Users};
use taskmanager_core::telemetry::{CoreSample, CpuSample, MemorySample};
use taskmanager_core::{
    ActionError, MachineInfo, PlatformServices, ProcessKey, ProviderHealth, Reading, Snapshot,
    TelemetryProvider,
};

use devices::DeviceSampler;
use processes::ProcessSampler;

/// Build the native provider and services over one shared state.
pub fn native_platform() -> (NativeProvider, NativeServices) {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let provider = NativeProvider::new(Arc::clone(&shared));
    (provider, NativeServices { shared })
}

/// What the provider and services share.
#[derive(Debug, Default)]
struct Shared {
    /// Keys in the latest table whose start stamp is a fallback (see `processes`).
    fallback_keys: HashSet<ProcessKey>,
}

/// Lock the shared state; a panic elsewhere while holding it leaves plain data behind, so
/// a poisoned lock is still usable.
fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The native telemetry provider.
pub struct NativeProvider {
    shared: Arc<Mutex<Shared>>,
    sys: System,
    disks: Disks,
    networks: Networks,
    components: Components,
    users: Users,
    kernel: os::KernelCpu,
    memory: os::MemoryExtras,
    threads: os::Threads,
    processes: ProcessSampler,
    devices: DeviceSampler,
    previous_at: Option<Instant>,
    health: ProviderHealth,
}

impl NativeProvider {
    fn new(shared: Arc<Mutex<Shared>>) -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_list(CpuRefreshKind::nothing().with_frequency());
        sys.refresh_memory();
        Self {
            shared,
            sys,
            disks: Disks::new_with_refreshed_list_specifics(DiskRefreshKind::everything()),
            networks: Networks::new_with_refreshed_list(),
            components: Components::new_with_refreshed_list(),
            users: Users::new_with_refreshed_list(),
            kernel: os::KernelCpu::new(),
            memory: os::MemoryExtras::new(),
            threads: os::Threads::new(),
            processes: ProcessSampler::new(),
            devices: DeviceSampler::default(),
            previous_at: None,
            health: ProviderHealth::Healthy,
        }
    }

    /// CPU load (sysinfo), clocks and the kernel split (OS module). Load needs two
    /// readings, so it is `Unavailable` on the first sample.
    fn sample_cpu(&mut self, measured: bool, issues: &mut Vec<String>) -> CpuSample {
        self.sys
            .refresh_cpu_specifics(CpuRefreshKind::nothing().with_cpu_usage().with_frequency());
        let kernel = self.kernel.sample().unwrap_or_else(|why| {
            issues.push(format!("kernel CPU time unavailable: {why}"));
            None
        });
        let cpus = self.sys.cpus();
        let clocks: Vec<f32> = cpus
            .iter()
            .map(|c| c.frequency() as f32)
            .filter(|f| *f > 0.0)
            .collect();
        let mut sample = CpuSample {
            frequency_mhz: if clocks.is_empty() {
                Reading::Unavailable
            } else {
                Reading::Value(clocks.iter().sum::<f32>() / clocks.len() as f32)
            },
            ..CpuSample::default()
        };
        if !measured {
            sample.per_cpu = vec![CoreSample::default(); cpus.len()];
            return sample;
        }
        let total = self.sys.global_cpu_usage().clamp(0.0, 100.0);
        sample.total_pct = Reading::Value(total);
        // Kernel time is part of utilisation; the two come from different counters, so
        // clamp the kernel share to the load it belongs to.
        sample.kernel_pct = kernel
            .as_ref()
            .and_then(|k| k.total)
            .map(|k| k.min(total))
            .into();
        sample.per_cpu = cpus
            .iter()
            .enumerate()
            .map(|(i, cpu)| {
                let util = cpu.cpu_usage().clamp(0.0, 100.0);
                let kernel_pct = kernel
                    .as_ref()
                    .and_then(|k| k.per_cpu.get(i).copied().flatten())
                    .map(|k| k.min(util));
                CoreSample {
                    util_pct: Reading::Value(util),
                    kernel_pct: kernel_pct.into(),
                }
            })
            .collect();
        sample
    }

    /// Totals and swap (sysinfo), composition and pressure (OS module).
    fn sample_memory(&mut self, issues: &mut Vec<String>) -> MemorySample {
        self.sys.refresh_memory();
        let total = self.sys.total_memory();
        if total == 0 {
            issues.push("memory counters unavailable".into());
            return MemorySample::default();
        }
        let mut memory = MemorySample {
            total: Reading::Value(total),
            used: Reading::Value(self.sys.used_memory().min(total)),
            available: Reading::Value(self.sys.available_memory()),
            swap_total: Reading::Value(self.sys.total_swap()),
            swap_used: Reading::Value(self.sys.used_swap()),
            ..MemorySample::default()
        };
        if let Err(why) = self.memory.fill(&mut memory) {
            issues.push(format!("memory composition unavailable: {why}"));
        }
        memory
    }
}

impl TelemetryProvider for NativeProvider {
    fn machine_info(&mut self) -> MachineInfo {
        // Brand, vendor and core count come from the CPU list read at start-up. Re-listing
        // CPUs here would recreate sysinfo's per-CPU state and turn the next load reading
        // into an average since boot.
        self.sys.refresh_memory();
        self.disks
            .refresh_specifics(true, DiskRefreshKind::everything());
        self.networks.refresh(true);
        machine::read_machine_info(&self.sys, &self.disks, &self.networks)
    }

    fn sample(&mut self, now: Instant) -> Snapshot {
        let elapsed = self.previous_at.map(|at| now.saturating_duration_since(at));
        self.previous_at = Some(now);
        let mut snapshot = Snapshot::new(now);
        snapshot.elapsed = elapsed.unwrap_or_default();
        let mut issues = Vec::new();

        snapshot.cpu = self.sample_cpu(elapsed.is_some(), &mut issues);
        snapshot.memory = self.sample_memory(&mut issues);
        let threads = self.threads.sample().map_err(|why| {
            issues.push(format!("thread count unavailable: {why}"));
        });
        let threads = threads.ok();
        let thread_total = threads.as_ref().map_or(Reading::Unavailable, |t| t.total);
        match self.processes.sample(
            &mut self.sys,
            &mut self.users,
            threads.as_ref(),
            elapsed,
            &mut issues,
        ) {
            Ok(listing) => {
                snapshot.thread_count = if thread_total.is_value() {
                    thread_total
                } else {
                    listing.thread_sum.into()
                };
                lock(&self.shared).fallback_keys = listing.fallback_keys;
                snapshot.processes = Reading::Value(listing.table);
            }
            Err(why) => {
                issues.push(why);
                snapshot.thread_count = thread_total;
                snapshot.processes = Reading::Unavailable;
            }
        }
        snapshot.disks = self.devices.disks(&mut self.disks, elapsed);
        snapshot.networks = self.devices.networks(&mut self.networks, elapsed);
        snapshot.sensors = devices::sensors(&mut self.components);
        snapshot.uptime = match System::uptime() {
            0 => Reading::Unavailable,
            secs => Reading::Value(Duration::from_secs(secs)),
        };

        self.health = if issues.is_empty() {
            ProviderHealth::Healthy
        } else {
            ProviderHealth::Degraded(issues.join("; "))
        };
        snapshot
    }

    fn health(&self) -> ProviderHealth {
        self.health.clone()
    }
}

/// The native platform services.
pub struct NativeServices {
    shared: Arc<Mutex<Shared>>,
}

impl PlatformServices for NativeServices {
    fn is_demo(&self) -> bool {
        false
    }

    /// Re-read the target's start stamp, refuse if it changed (or the process is gone),
    /// then end it: SIGTERM / SIGKILL on macOS and Linux; on Windows both a polite and a
    /// forced end call `TerminateProcess` for now (a polite WM_CLOSE path arrives with the
    /// Processes page, M5).
    ///
    /// A key whose stamp is a fallback (the OS refused to tell us the process's creation
    /// time: Windows protected processes) is refused with `PermissionDenied`: we cannot
    /// verify its identity, and the OS would not let us end it anyway.
    fn end_process(&self, key: ProcessKey, force: bool) -> Result<(), ActionError> {
        if lock(&self.shared).fallback_keys.contains(&key) {
            return Err(ActionError::PermissionDenied);
        }
        os::end_verified(key, force)
    }

    /// Open an `http`/`https` URL in the user's browser; other schemes are refused so
    /// this cannot be used to launch files or programs.
    fn open_url(&self, url: &str) -> Result<(), ActionError> {
        let lower = url.trim_start().to_ascii_lowercase();
        if !(lower.starts_with("https://") || lower.starts_with("http://")) {
            return Err(ActionError::Unsupported);
        }
        opener::open_browser(url).map_err(|e| ActionError::Failed(e.to_string()))
    }
}

#[cfg(test)]
mod tests;
