//! The demo machine's processes: who is running, and how that changes.
//!
//! [`Population::new`] builds a machine that has been up for a couple of days: a
//! `kernel_task`-like pid 0 and a `launchd`-like pid 1 (both root), ~500 background
//! daemons under pid 1 (root, the `demo` user and service accounts), and 8–11
//! applications with their helper processes. Pids grow with start time, as on a real
//! boot, and every process has a [`ProcessKey`] whose start stamp is microseconds since
//! boot.
//!
//! [`Population::advance`] moves the simulated clock forward and applies churn in
//! chronological order: short-lived tools (`git`, `mdworker`, …) start every few seconds
//! and exit on their own, and now and then an app's renderer is restarted (exits, and a
//! new one with a new pid takes its place). Events draw from their own [`Rng`] stream in
//! time order, so the result does not depend on how often the sampler asks.
//! [`Population::end`] removes a process (and its descendants) after the identity check.
//!
//! The per-sample measurements (CPU share, memory, rates) are made in [`super::rows`].

use crate::platform::ActionError;
use crate::process::{Pid, ProcessKey, ProcessKind};

use super::catalog::{
    AppSpec, HelperSpec, TransientSpec, APPS, DAEMON_PREFIXES, DAEMON_SUFFIXES, DEMO_USER,
    NOTABLE_DAEMONS, PER_USER_DAEMONS, ROOT_DIRS, ROOT_USER, SERVICE_USERS, TRANSIENTS, USER_DIRS,
};
use super::rng::{hash, Rng};

const MB: f64 = 1024.0 * 1024.0;

/// One simulated process: identity, static traits and running totals.
#[derive(Clone, Debug, PartialEq)]
pub struct SimProcess {
    pub key: ProcessKey,
    pub parent: Option<Pid>,
    pub name: String,
    pub exe: String,
    pub user: String,
    pub kind: ProcessKind,
    /// Start, seconds since boot.
    pub start_s: f64,
    /// Relative CPU appetite.
    pub weight: f64,
    /// Typical memory footprint, bytes.
    pub memory_base: f64,
    pub threads_base: u32,
    pub priority: i32,
    /// Share of the GPU it accounts for.
    pub gpu_share: f64,
    /// Whether it uses the Neural Engine.
    pub uses_npu: bool,
    /// Whether it moves network traffic.
    pub networked: bool,
    /// Disk appetite, bytes per second at its busiest.
    pub disk_bps: f64,
    /// Root-owned: the signed-in user cannot read its task info ("Access denied").
    pub restricted: bool,
    /// A Terminal shell (the parent of command-line tools).
    pub is_shell: bool,
    /// Restarted now and then (app renderers).
    pub restarts: bool,
    /// When it exits on its own, simulated seconds since the first sample.
    pub exit_at: Option<f64>,
    /// Noise channel for its curves.
    pub channel: u64,
    // Running totals.
    pub cpu_time_s: f64,
    pub page_faults: f64,
    pub disk_read_total: f64,
    pub disk_written_total: f64,
    pub peak_memory: u64,
}

/// Where a process's parent is, before pids are assigned.
#[derive(Clone, Copy, Debug)]
enum ParentRef {
    None,
    Init,
    Birth(usize),
}

/// A process of the initial population, before pids are assigned.
struct Birth {
    proc: SimProcess,
    parent: ParentRef,
}

/// Traits shared by the constructors below.
struct Traits<'a> {
    name: &'a str,
    exe: String,
    user: &'a str,
    kind: ProcessKind,
    weight: f64,
    memory_mb: f64,
    threads: u32,
}

fn sim_process(seed: u64, t: Traits<'_>, start_s: f64) -> SimProcess {
    SimProcess {
        key: ProcessKey::default(),
        parent: None,
        name: t.name.to_string(),
        exe: t.exe,
        user: t.user.to_string(),
        kind: t.kind,
        start_s,
        weight: t.weight,
        memory_base: t.memory_mb * MB,
        threads_base: t.threads,
        priority: 0,
        gpu_share: 0.0,
        uses_npu: false,
        networked: false,
        disk_bps: 0.0,
        restricted: t.user == ROOT_USER,
        is_shell: false,
        restarts: false,
        exit_at: None,
        channel: hash(seed, 0x9C0, (start_s * 1e6) as i64),
        cpu_time_s: 0.0,
        page_faults: 0.0,
        disk_read_total: 0.0,
        disk_written_total: 0.0,
        peak_memory: 0,
    }
}

/// The simulated process list and its churn schedule.
#[derive(Clone, Debug, PartialEq)]
pub struct Population {
    seed: u64,
    procs: Vec<SimProcess>,
    next_pid: u32,
    /// Machine uptime at the first sample, seconds.
    boot_uptime_s: f64,
    rng: Rng,
    next_spawn_at: f64,
    next_restart_at: f64,
}

impl Population {
    /// A machine that has been up `boot_uptime_s` seconds when sampling starts.
    pub fn new(seed: u64, boot_uptime_s: f64) -> Self {
        let mut rng = Rng::new(seed, 1);
        let mut births = Vec::new();
        add_daemons(seed, &mut rng, boot_uptime_s, &mut births);
        add_apps(seed, &mut rng, boot_uptime_s, &mut births);
        let (procs, next_pid) = assign_pids(&mut rng, births, boot_uptime_s);
        let mut churn = Rng::new(seed, 2);
        let next_spawn_at = churn.range(0.5, 3.0);
        let next_restart_at = churn.range(20.0, 90.0);
        Self {
            seed,
            procs,
            next_pid,
            boot_uptime_s,
            rng: churn,
            next_spawn_at,
            next_restart_at,
        }
    }

    /// The live processes.
    pub fn procs(&self) -> &[SimProcess] {
        &self.procs
    }

    pub fn procs_mut(&mut self) -> &mut [SimProcess] {
        &mut self.procs
    }

    /// Machine uptime at the first sample, seconds.
    pub fn boot_uptime_s(&self) -> f64 {
        self.boot_uptime_s
    }

    /// Apply every start and exit up to simulated time `to` (seconds since the first
    /// sample), in chronological order.
    pub fn advance(&mut self, to: f64) {
        loop {
            let spawn_first = self.next_spawn_at <= self.next_restart_at;
            let next = self.next_spawn_at.min(self.next_restart_at);
            if next > to {
                break;
            }
            if spawn_first {
                self.spawn_transient(next);
                self.next_spawn_at = next + self.rng.range(1.0, 5.0);
            } else {
                self.restart_helper(next);
                self.next_restart_at = next + self.rng.range(60.0, 240.0);
            }
        }
        self.procs.retain(|p| p.exit_at.is_none_or(|at| at > to));
    }

    fn take_pid(&mut self) -> u32 {
        self.next_pid += self.rng.int(1, 3) as u32;
        self.next_pid
    }

    fn spawn_transient(&mut self, at: f64) {
        let Some(&spec) = self.rng.pick(&TRANSIENTS) else {
            return;
        };
        let TransientSpec {
            name,
            user,
            from_shell,
            lifetime: (min_life, max_life),
            weight,
            memory_mb: mb,
        } = spec;
        let parent_pid = if from_shell {
            let shells: Vec<Pid> = self
                .procs
                .iter()
                .filter(|p| p.is_shell)
                .map(|p| p.key.pid)
                .collect();
            self.rng.pick(&shells).copied().unwrap_or(Pid(1))
        } else {
            Pid(1)
        };
        let dir = if user == ROOT_USER {
            "/usr/libexec/"
        } else {
            "/usr/bin/"
        };
        let start_s = self.boot_uptime_s + at;
        let traits = Traits {
            name,
            exe: format!("{dir}{name}"),
            user,
            kind: ProcessKind::Background,
            weight,
            memory_mb: mb * self.rng.range(0.7, 1.4),
            threads: self.rng.int(1, 4) as u32,
        };
        let mut p = sim_process(self.seed, traits, start_s);
        p.disk_bps = if name == "mdworker" { 4e6 } else { 2e5 };
        p.priority = if name == "mdworker" { 20 } else { 0 };
        p.exit_at = Some(at + self.rng.range(min_life, max_life));
        p.parent = Some(parent_pid);
        p.key = ProcessKey::new(self.take_pid(), (start_s * 1e6) as u64);
        self.procs.push(p);
    }

    fn restart_helper(&mut self, at: f64) {
        let candidates: Vec<usize> = (0..self.procs.len())
            .filter(|&i| self.procs[i].restarts && self.procs[i].exit_at.is_none())
            .collect();
        let Some(&i) = self.rng.pick(&candidates) else {
            return;
        };
        self.procs[i].exit_at = Some(at);
        let mut fresh = self.procs[i].clone();
        fresh.exit_at = None;
        fresh.start_s = self.boot_uptime_s + at + 0.3;
        fresh.channel = hash(self.seed, 0x9C0, (fresh.start_s * 1e6) as i64);
        fresh.cpu_time_s = 0.0;
        fresh.page_faults = 0.0;
        fresh.disk_read_total = 0.0;
        fresh.disk_written_total = 0.0;
        fresh.peak_memory = 0;
        fresh.key = ProcessKey::new(self.take_pid(), (fresh.start_s * 1e6) as u64);
        self.procs.push(fresh);
    }

    /// End the process `key` names, with its descendants (as quitting an app takes its
    /// helpers with it). Root-owned processes refuse: the demo user may not signal them.
    pub fn end(&mut self, key: ProcessKey) -> Result<(), ActionError> {
        let Some(target) = self.procs.iter().find(|p| p.key.pid == key.pid) else {
            return Err(ActionError::NotFound);
        };
        if target.key != key {
            return Err(ActionError::IdentityChanged);
        }
        if target.user == ROOT_USER {
            return Err(ActionError::PermissionDenied);
        }
        let mut doomed = vec![key.pid];
        let mut i = 0;
        while i < doomed.len() {
            let parent = doomed[i];
            doomed.extend(
                self.procs
                    .iter()
                    .filter(|p| p.parent == Some(parent) && p.key.pid != parent)
                    .map(|p| p.key.pid),
            );
            i += 1;
        }
        self.procs.retain(|p| !doomed.contains(&p.key.pid));
        Ok(())
    }
}

fn daemon_user(rng: &mut Rng) -> &'static str {
    let roll = rng.unit();
    if roll < 0.22 {
        ROOT_USER
    } else if roll < 0.85 {
        DEMO_USER
    } else {
        rng.pick(&SERVICE_USERS).copied().unwrap_or(DEMO_USER)
    }
}

fn daemon_exe(rng: &mut Rng, user: &str, name: &str) -> String {
    let dirs = if user == ROOT_USER {
        &ROOT_DIRS
    } else {
        &USER_DIRS
    };
    let dir = rng.pick(dirs).copied().unwrap_or("/usr/libexec/");
    format!("{dir}{name}")
}

/// Appetite of an ordinary daemon: almost always idle, a few lively.
fn daemon_weight(rng: &mut Rng) -> f64 {
    let roll = rng.unit();
    if roll < 0.9 {
        rng.range(0.0005, 0.004)
    } else if roll < 0.99 {
        rng.range(0.01, 0.06)
    } else {
        rng.range(0.1, 0.3)
    }
}

fn boot_time(rng: &mut Rng, uptime: f64) -> f64 {
    if rng.chance(0.7) {
        rng.range(0.5, 120.0)
    } else {
        rng.range(120.0, uptime * 0.98)
    }
}

fn push_daemon(
    seed: u64,
    rng: &mut Rng,
    uptime: f64,
    out: &mut Vec<Birth>,
    name: &str,
    user: &str,
) {
    let start = boot_time(rng, uptime);
    let traits = Traits {
        name,
        exe: daemon_exe(rng, user, name),
        user,
        kind: ProcessKind::Background,
        weight: daemon_weight(rng),
        memory_mb: rng.log_range(1.5, 60.0),
        threads: rng.int(2, 8) as u32,
    };
    let mut p = sim_process(seed, traits, start);
    p.priority = if rng.chance(0.15) { 4 } else { 0 };
    p.disk_bps = if rng.chance(0.08) {
        rng.range(2e4, 3e5)
    } else {
        0.0
    };
    out.push(Birth {
        proc: p,
        parent: ParentRef::Init,
    });
}

fn add_daemons(seed: u64, rng: &mut Rng, uptime: f64, out: &mut Vec<Birth>) {
    fn root_traits(name: &str, threads: u32, mb: f64) -> Traits<'_> {
        Traits {
            name,
            exe: String::new(),
            user: ROOT_USER,
            kind: ProcessKind::Background,
            weight: 0.0,
            memory_mb: mb,
            threads,
        }
    }
    let mut kernel = sim_process(seed, root_traits("kernel_task", 600, 40.0), 0.0);
    kernel.weight = 0.8;
    out.push(Birth {
        proc: kernel,
        parent: ParentRef::None,
    });
    let mut launchd = root_traits("launchd", 4, 25.0);
    launchd.exe = "/sbin/launchd".to_string();
    launchd.weight = 0.02;
    out.push(Birth {
        proc: sim_process(seed, launchd, 0.000_1),
        parent: ParentRef::None,
    });

    for &(name, user, weight, mb, gpu) in &NOTABLE_DAEMONS {
        let user = if user.is_empty() { DEMO_USER } else { user };
        let start = rng.range(1.0, 60.0);
        let traits = Traits {
            name,
            exe: daemon_exe(rng, user, name),
            user,
            kind: ProcessKind::Background,
            weight,
            memory_mb: mb * rng.range(0.8, 1.25),
            threads: rng.int(4, 16) as u32,
        };
        let mut p = sim_process(seed, traits, start);
        p.gpu_share = gpu;
        p.uses_npu = name == "photoanalysisd";
        p.networked = name == "cloudsyncd" || name == "networkd";
        p.disk_bps = match name {
            "searchindexd" => 3e6,
            "cloudsyncd" | "photoanalysisd" => 1e6,
            "logd" => 2e5,
            _ => 0.0,
        };
        out.push(Birth {
            proc: p,
            parent: ParentRef::Init,
        });
    }
    for name in PER_USER_DAEMONS {
        for user in [ROOT_USER, DEMO_USER] {
            push_daemon(seed, rng, uptime, out, name, user);
        }
        let service = rng.pick(&SERVICE_USERS).copied().unwrap_or(DEMO_USER);
        push_daemon(seed, rng, uptime, out, name, service);
    }
    let mut composed: Vec<String> = DAEMON_PREFIXES
        .iter()
        .flat_map(|p| DAEMON_SUFFIXES.iter().map(move |s| format!("{p}{s}")))
        .collect();
    rng.shuffle(&mut composed);
    let total_daemons = 498 + rng.int(0, 16) as usize;
    let have = out.len() - 2;
    for name in composed.iter().take(total_daemons.saturating_sub(have)) {
        let user = daemon_user(rng);
        push_daemon(seed, rng, uptime, out, name, user);
    }
}

fn helper_births(
    seed: u64,
    rng: &mut Rng,
    uptime: f64,
    app: &AppSpec,
    app_index: usize,
    app_start: f64,
    out: &mut Vec<Birth>,
) {
    for h in app.helpers {
        let HelperSpec {
            name,
            count: (lo, hi),
            weight,
            memory_mb: (mb_lo, mb_hi),
            restarts,
            gpu_share,
        } = *h;
        for _ in 0..rng.int(lo, hi) {
            let start = if restarts {
                rng.range(app_start + 1.0, uptime - 1.0)
            } else {
                app_start + rng.range(0.2, 3.0)
            };
            let exe = if name == "zsh" {
                "/bin/zsh".to_string()
            } else {
                format!(
                    "{}/Contents/Frameworks/{name}.app/Contents/MacOS/{name}",
                    app.bundle
                )
            };
            let traits = Traits {
                name,
                exe,
                user: DEMO_USER,
                kind: ProcessKind::Application,
                weight: weight * rng.range(0.3, 1.6),
                memory_mb: rng.log_range(mb_lo, mb_hi),
                threads: rng.int(6, 24) as u32,
            };
            let mut p = sim_process(seed, traits, start);
            p.restarts = restarts;
            p.gpu_share = gpu_share;
            p.is_shell = name == "zsh";
            p.networked = app.networked && name.contains("Renderer");
            out.push(Birth {
                proc: p,
                parent: ParentRef::Birth(app_index),
            });
        }
    }
}

fn add_apps(seed: u64, rng: &mut Rng, uptime: f64, out: &mut Vec<Birth>) {
    let mut optional: Vec<&AppSpec> = APPS.iter().filter(|a| !a.always).collect();
    rng.shuffle(&mut optional);
    let extra = rng.int(4, 7) as usize;
    let chosen = APPS
        .iter()
        .filter(|a| a.always)
        .chain(optional.into_iter().take(extra));
    for app in chosen {
        let start = rng.range(uptime * 0.2, uptime * 0.97);
        let traits = Traits {
            name: app.name,
            exe: format!("{}/Contents/MacOS/{}", app.bundle, app.name),
            user: DEMO_USER,
            kind: ProcessKind::Application,
            weight: app.weight,
            memory_mb: rng.range(app.memory_mb.0, app.memory_mb.1),
            threads: rng.int(app.threads.0, app.threads.1) as u32,
        };
        let mut p = sim_process(seed, traits, start);
        p.networked = app.networked;
        p.gpu_share = app.gpu_share;
        p.disk_bps = 3e5;
        let index = out.len();
        out.push(Birth {
            proc: p,
            parent: ParentRef::Init,
        });
        helper_births(seed, rng, uptime, app, index, start, out);
    }
}

/// Order the births by start time, give out pids in that order (pid 0 and 1 fixed) at a
/// realistic pace, resolve parents, and seed each process's running totals from its age.
fn assign_pids(rng: &mut Rng, births: Vec<Birth>, uptime: f64) -> (Vec<SimProcess>, u32) {
    let mut order: Vec<usize> = (0..births.len()).collect();
    order.sort_by(|&a, &b| births[a].proc.start_s.total_cmp(&births[b].proc.start_s));
    let mut pids = vec![0u32; births.len()];
    let mut next = 80u32;
    let mut last_start = 0.0;
    for &i in &order {
        let p = &births[i].proc;
        // `add_daemons` pushes the kernel and launchd first: they own pids 0 and 1.
        pids[i] = match i {
            0 => 0,
            1 => 1,
            _ => {
                let gap = (p.start_s - last_start).max(0.0);
                next += 1 + (gap * 0.2 * rng.range(0.5, 1.5)) as u32 + rng.int(0, 2) as u32;
                last_start = p.start_s;
                next
            }
        };
    }
    let procs = order
        .iter()
        .map(|&i| {
            let birth = &births[i];
            let mut p = birth.proc.clone();
            p.key = ProcessKey::new(pids[i], (p.start_s * 1e6) as u64);
            p.parent = match birth.parent {
                ParentRef::None => None,
                ParentRef::Init => Some(Pid(1)),
                ParentRef::Birth(j) => Some(Pid(pids[j])),
            };
            let age = (uptime - p.start_s).max(0.0);
            p.cpu_time_s = age * p.weight * 0.02;
            p.page_faults = age * (p.memory_base / MB) * 0.3;
            p.disk_read_total = age * p.disk_bps * 0.01;
            p.disk_written_total = age * p.disk_bps * 0.004;
            p
        })
        .collect();
    (procs, next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn population_has_the_planned_shape() {
        let pop = Population::new(42, 170_000.0);
        let n = pop.procs().len();
        assert!((500..=580).contains(&n), "{n} processes");
        let apps = pop
            .procs()
            .iter()
            .filter(|p| p.kind == ProcessKind::Application && p.parent == Some(Pid(1)))
            .count();
        assert!((8..=11).contains(&apps), "{apps} apps");
        let launchd = pop.procs().iter().find(|p| p.key.pid == Pid(1));
        assert_eq!(launchd.map(|p| p.user.as_str()), Some(ROOT_USER));
        let mut pids: Vec<u32> = pop.procs().iter().map(|p| p.key.pid.0).collect();
        pids.sort_unstable();
        pids.dedup();
        assert_eq!(pids.len(), n, "pids are unique");
    }

    #[test]
    fn churn_starts_and_ends_processes() {
        let mut pop = Population::new(3, 100_000.0);
        let before: Vec<ProcessKey> = pop.procs().iter().map(|p| p.key).collect();
        pop.advance(600.0);
        let after: Vec<ProcessKey> = pop.procs().iter().map(|p| p.key).collect();
        assert!(
            after.iter().any(|k| !before.contains(k)),
            "something started"
        );
        assert!(
            before.iter().any(|k| !after.contains(k)),
            "something exited"
        );
        assert!((after.len() as i64 - before.len() as i64).abs() < 30);
    }

    #[test]
    fn churn_is_independent_of_step_size() {
        let mut coarse = Population::new(9, 50_000.0);
        let mut fine = Population::new(9, 50_000.0);
        coarse.advance(300.0);
        for i in 1..=600 {
            fine.advance(f64::from(i) * 0.5);
        }
        assert_eq!(coarse, fine);
    }
}
