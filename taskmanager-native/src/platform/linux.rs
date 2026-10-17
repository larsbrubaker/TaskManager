//! Linux extras for the native platform: what sysinfo does not give us (PLAN §4.3), read
//! from `/proc` and `/sys` and parsed by the core's pure parsers
//! (`taskmanager_core::platform::parse::{proc_stat, desktop, procfs, sysfs}`).
//!
//! - Process identity: `/proc/<pid>/stat` field 22 `starttime` (clock ticks since boot),
//!   the fine-grained per-boot stamp `ProcessKey` needs (PLAN §4.4); field 20 gives the
//!   thread count in the same read.
//! - Ending a process: a pidfd (`pidfd_open`, Linux 5.3+) is opened first, the stamp is
//!   re-checked, and the signal goes through the pidfd, so a pid recycled between the
//!   check and the signal cannot be hit. Older kernels fall back to `kill`.
//! - Disk I/O per process: only where `/proc/<pid>/io` is readable (`disk_io_readable`);
//!   elsewhere sysinfo's zeros become `Unavailable`.
//! - Kernel CPU split: `/proc/stat` per-CPU system + irq + softirq tick deltas.
//! - Memory composition: `/proc/meminfo` (cached = `Cached` + `Buffers`, committed =
//!   `Committed_AS`, compressed = `Zswap` when the kernel reports it) and PSI
//!   `/proc/pressure/memory` for pressure (`Unsupported` on kernels without PSI, or when
//!   the file is unreadable: noted in health on the first sample only).
//! - System thread count: `/proc/loadavg`'s scheduling-entity total.
//! - Applications: the executable (or, when another user's `exe` link is unreadable, the
//!   process name) is a program some visible `.desktop` launcher runs, from
//!   `/usr/share/applications`, `/usr/local/share/applications`,
//!   `/var/lib/snapd/desktop/applications` and `~/.local/share/applications` (read when
//!   the provider starts). M5 refines this with window ownership.
//! - Machine facts: cache totals from sysfs; interface kind (wireless, loopback, virtual =
//!   no backing device) and link speed from `/sys/class/net`.

use std::collections::{HashMap, HashSet};
use std::ffi::c_int;
use std::fs;
use std::path::{Path, PathBuf};

use taskmanager_core::platform::parse::desktop::{
    launcher_programs, parse_desktop_entry, program_file_name, DesktopEntry,
};
use taskmanager_core::platform::parse::proc_stat::parse_proc_pid_stat;
use taskmanager_core::platform::parse::procfs::{
    parse_loadavg_threads, parse_meminfo, parse_proc_stat_cpus, parse_psi, CpuTimes,
};
use taskmanager_core::platform::parse::sysfs::{
    cache_totals, parse_cache_size, parse_link_speed_bps, CacheEntry,
};
use taskmanager_core::telemetry::{MachineInfo, MemorySample, NetInfo, NetKind};
use taskmanager_core::{ActionError, ProcessKey, Reading};

use super::os_api::{Identity, KernelSplit, StampError, ThreadInfo};
use super::unix;
pub use super::unix::{process_cpu_time, sysinfo_uid};

/// Whether identity reads are cached per (pid, start second) across ticks: `/proc/<pid>/stat` is read every tick anyway: it also gives the thread count.
pub const CACHE_IDENTITY: bool = false;

/// `/proc` is readable for every process, so sysinfo's zeros are measured zeros (kernel
/// threads really have no user memory).
pub const ZERO_USAGE_MEANS_DENIED: bool = false;

/// Read process `pid`'s start stamp (clock ticks since boot) and thread count.
pub fn read_identity(pid: u32) -> Result<Identity, StampError> {
    match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(text) => parse_proc_pid_stat(&text)
            .map(|stat| Identity {
                stamp: stat.start_ticks,
                threads: Some(stat.num_threads),
                uid: None,
            })
            .ok_or_else(|| StampError::Failed(format!("unparseable /proc/{pid}/stat"))),
        Err(e) => Err(match e.kind() {
            std::io::ErrorKind::NotFound => StampError::NotFound,
            std::io::ErrorKind::PermissionDenied => StampError::Denied,
            _ if e.raw_os_error() == Some(libc::ESRCH) => StampError::NotFound,
            _ => StampError::Failed(e.to_string()),
        }),
    }
}

/// Check that `key`'s stamp still matches.
fn verify(key: ProcessKey) -> Result<(), ActionError> {
    let current = read_identity(key.pid.0).map_err(StampError::into_action_error)?;
    if current.stamp == key.start_time {
        Ok(())
    } else {
        Err(ActionError::IdentityChanged)
    }
}

/// An open pidfd, closed on drop.
struct Pidfd(c_int);

impl Pidfd {
    /// `pidfd_open(pid, 0)`, or the errno.
    fn open(pid: libc::pid_t) -> Result<Self, i32> {
        // SAFETY: `pidfd_open` takes a pid and flags by value and returns a new file
        // descriptor or -1; no memory is passed.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if fd < 0 {
            Err(unix::errno())
        } else {
            c_int::try_from(fd).map(Pidfd).map_err(|_| libc::EBADF)
        }
    }

    /// `pidfd_send_signal(fd, sig, NULL, 0)`.
    fn send_signal(&self, signal: c_int) -> Result<(), ActionError> {
        // SAFETY: `self.0` is an open pidfd we own; a null `siginfo` is documented as
        // "behave like kill"; flags must be 0.
        let rc = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.0,
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(unix::signal_error(unix::errno()))
        }
    }
}

impl Drop for Pidfd {
    fn drop(&mut self) {
        // SAFETY: closes the descriptor `open` returned, exactly once.
        unsafe {
            libc::close(self.0);
        }
    }
}

/// Re-check `key`'s stamp and signal it, through a pidfd when the kernel has them.
pub fn end_verified(key: ProcessKey, force: bool) -> Result<(), ActionError> {
    let pid = unix::target_pid(key.pid.0)?;
    match Pidfd::open(pid) {
        Ok(pidfd) => {
            verify(key)?;
            pidfd.send_signal(unix::termination_signal(force))
        }
        Err(libc::ESRCH) => Err(ActionError::NotFound),
        // No pidfds (pre-5.3 kernel, seccomp) or out of descriptors: check, then kill.
        Err(_) => {
            verify(key)?;
            unix::signal(key.pid.0, force)
        }
    }
}

/// Whether we may read process `pid`'s I/O counters (`/proc/<pid>/io` is mode 0400,
/// owned by the process's user, and the kernel also requires ptrace-read access, so other
/// users' processes are unreadable unless we are root). sysinfo reports 0 bytes when the
/// read fails, which must not be shown as a measured zero.
pub fn disk_io_readable(pid: u32) -> bool {
    let Ok(path) = std::ffi::CString::new(format!("/proc/{pid}/io")) else {
        return false;
    };
    // SAFETY: `path` is a valid NUL-terminated string; `access` only reads it.
    unsafe { libc::access(path.as_ptr(), libc::R_OK) == 0 }
}

/// Whether `mount` is the root file system.
pub fn is_system_mount(mount: &Path) -> bool {
    mount == Path::new("/")
}

/// Per-CPU kernel share from `/proc/stat` deltas.
pub struct KernelCpu {
    previous: Option<(Option<CpuTimes>, HashMap<usize, CpuTimes>)>,
}

impl KernelCpu {
    pub fn new() -> Self {
        Self { previous: None }
    }

    /// The kernel share since the previous call (`Ok(None)` on the first). `per_cpu` is
    /// in `/proc/stat` order, which is sysinfo's CPU order.
    pub fn sample(&mut self) -> Result<Option<KernelSplit>, String> {
        let text = fs::read_to_string("/proc/stat").map_err(|e| format!("/proc/stat: {e}"))?;
        let cpus = parse_proc_stat_cpus(&text);
        if cpus.total.is_none() {
            return Err("/proc/stat has no cpu line".into());
        }
        let split = self
            .previous
            .as_ref()
            .map(|(prev_total, prev_cpus)| KernelSplit {
                total: cpus
                    .total
                    .zip(*prev_total)
                    .and_then(|(now, prev)| now.kernel_pct_since(&prev)),
                per_cpu: cpus
                    .per_cpu
                    .iter()
                    .map(|(index, now)| {
                        prev_cpus
                            .get(index)
                            .and_then(|prev| now.kernel_pct_since(prev))
                    })
                    .collect(),
            });
        self.previous = Some((cpus.total, cpus.per_cpu.into_iter().collect()));
        Ok(split)
    }
}

/// Memory composition from `/proc/meminfo` and PSI.
pub struct MemoryExtras {
    /// Whether an unreadable PSI file has been reported in health already.
    psi_unreadable_reported: bool,
}

impl MemoryExtras {
    pub fn new() -> Self {
        Self {
            psi_unreadable_reported: false,
        }
    }

    /// Fill cached, committed, compressed and pressure. Wired and app memory have no Linux
    /// counterpart, and we do not sample page ins/outs here, so those are `Unsupported`.
    /// Pressure is `Unsupported` without PSI (an old kernel, or a present but unreadable
    /// file: the latter is reported once, via `Err`, on the first sample that sees it).
    pub fn fill(&mut self, memory: &mut MemorySample) -> Result<(), String> {
        memory.wired = Reading::Unsupported;
        memory.app = Reading::Unsupported;
        memory.page_ins = Reading::Unsupported;
        memory.page_outs = Reading::Unsupported;
        let text =
            fs::read_to_string("/proc/meminfo").map_err(|e| format!("/proc/meminfo: {e}"))?;
        let info = parse_meminfo(&text);
        let field = |name: &str| info.get(name).copied();
        memory.cached = match (field("Cached"), field("Buffers")) {
            (Some(cached), buffers) => Reading::Value(cached + buffers.unwrap_or(0)),
            (None, _) => Reading::Unavailable,
        };
        memory.committed = field("Committed_AS").into();
        memory.compressed = field("Zswap").into();

        match fs::read_to_string("/proc/pressure/memory") {
            Ok(text) => {
                if let Some(psi) = parse_psi(&text) {
                    memory.pressure_pct = Reading::Value(psi.some_avg10.clamp(0.0, 100.0));
                    memory.pressure_level = Reading::Value(psi.level());
                }
                Ok(())
            }
            Err(e) => {
                memory.pressure_pct = Reading::Unsupported;
                memory.pressure_level = Reading::Unsupported;
                let first_report = !self.psi_unreadable_reported;
                if e.kind() == std::io::ErrorKind::NotFound || !first_report {
                    return Ok(());
                }
                self.psi_unreadable_reported = true;
                Err(format!("memory pressure (PSI) unreadable: {e}"))
            }
        }
    }
}

/// The system thread count from `/proc/loadavg` (per-process counts come with identity).
pub struct Threads;

impl Threads {
    pub fn new() -> Self {
        Self
    }

    pub fn sample(&mut self) -> Result<ThreadInfo, String> {
        let text =
            fs::read_to_string("/proc/loadavg").map_err(|e| format!("/proc/loadavg: {e}"))?;
        let total = parse_loadavg_threads(&text).ok_or("unparseable /proc/loadavg")?;
        Ok(ThreadInfo {
            total: Reading::Value(total),
            per_pid: None,
        })
    }
}

/// Applications are processes some visible desktop launcher starts.
pub struct AppClassifier {
    launchers: HashSet<String>,
}

impl AppClassifier {
    pub fn new() -> Self {
        let mut dirs = vec![
            PathBuf::from("/usr/share/applications"),
            PathBuf::from("/usr/local/share/applications"),
            PathBuf::from("/var/lib/snapd/desktop/applications"),
        ];
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
        if let Some(data_home) = data_home {
            dirs.push(data_home.join("applications"));
        }
        let mut entries = Vec::new();
        for dir in &dirs {
            collect_desktop_entries(dir, 2, &mut entries);
        }
        Self {
            launchers: launcher_programs(&entries),
        }
    }

    pub fn begin_tick(&mut self) -> Result<(), String> {
        Ok(())
    }

    pub fn is_application(&self, _pid: u32, exe: Option<&str>, name: &str) -> bool {
        let exe_name = exe.and_then(program_file_name);
        match exe_name {
            Some(exe_name) => self.launchers.contains(&exe_name),
            None => self.launchers.contains(name),
        }
    }
}

/// Parse every `*.desktop` file under `dir` (descending `depth` levels).
fn collect_desktop_entries(dir: &Path, depth: u32, out: &mut Vec<DesktopEntry>) {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth > 0 {
                collect_desktop_entries(&path, depth - 1, out);
            }
        } else if path.extension().is_some_and(|e| e == "desktop") {
            if let Some(parsed) = fs::read_to_string(&path)
                .ok()
                .and_then(|t| parse_desktop_entry(&t))
            {
                out.push(parsed);
            }
        }
    }
}

/// Fill cache totals and interface kinds / link speeds.
pub fn machine_extras(info: &mut MachineInfo) {
    let totals = cache_totals(&sysfs_caches());
    info.cpu.l1 = totals.l1.into();
    info.cpu.l2 = totals.l2.into();
    info.cpu.l3 = totals.l3.into();
    for net in &mut info.networks {
        refine_network(net);
    }
}

/// Every CPU's cache descriptions.
fn sysfs_caches() -> Vec<CacheEntry> {
    let mut entries = Vec::new();
    let Ok(cpus) = fs::read_dir("/sys/devices/system/cpu") else {
        return entries;
    };
    for cpu in cpus.flatten() {
        let name = cpu.file_name();
        let name = name.to_string_lossy();
        let is_cpu = name
            .strip_prefix("cpu")
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if !is_cpu {
            continue;
        }
        let Ok(indices) = fs::read_dir(cpu.path().join("cache")) else {
            continue;
        };
        for index in indices.flatten() {
            let dir = index.path();
            let read = |file: &str| fs::read_to_string(dir.join(file)).ok();
            let level = read("level").and_then(|l| l.trim().parse().ok());
            let size = read("size").as_deref().and_then(parse_cache_size);
            if let (Some(level), Some(size)) = (level, size) {
                entries.push(CacheEntry {
                    level,
                    kind: read("type").unwrap_or_default().trim().to_string(),
                    size,
                    shared_cpu_list: read("shared_cpu_list")
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                });
            }
        }
    }
    entries
}

/// Interface kind and link speed from `/sys/class/net/<if>`.
fn refine_network(net: &mut NetInfo) {
    let dir = Path::new("/sys/class/net").join(&net.id);
    if !dir.exists() {
        return;
    }
    let arp_type = fs::read_to_string(dir.join("type")).ok();
    if arp_type.as_deref().map(str::trim) == Some("772") {
        net.kind = NetKind::Loopback;
    } else if dir.join("wireless").exists() || dir.join("phy80211").exists() {
        net.kind = NetKind::WiFi;
    } else if !dir.join("device").exists() {
        net.kind = NetKind::Virtual;
    } else if net.kind == NetKind::Other {
        net.kind = NetKind::Ethernet;
    }
    net.link_speed_bps = fs::read_to_string(dir.join("speed"))
        .ok()
        .as_deref()
        .and_then(parse_link_speed_bps)
        .into();
}
