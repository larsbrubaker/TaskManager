//! macOS extras for the native platform: what sysinfo does not give us (PLAN §4.3).
//!
//! - Process identity: the kernel's process start time (`p_start`) as microseconds, the
//!   fine-grained per-boot stamp `ProcessKey` needs (PLAN §4.4), with the owner's uid.
//!   `proc_pidinfo(PROC_PIDTBSDINFO)` (`pbi_start_tvsec * 1e6 + pbi_start_tvusec`) is
//!   tried first; it is refused for other users' processes without root, so those fall
//!   back to `sysctl(CTL_KERN, KERN_PROC, KERN_PROC_PID)`, whose `kinfo_proc` carries the
//!   same `p_start` and is readable for every process. Either way every process gets a
//!   real stamp (no whole-second fallback keys on macOS).
//! - Kernel CPU split: `host_processor_info(PROCESSOR_CPU_LOAD_INFO)` per-CPU user /
//!   system / idle / nice ticks; the kernel share is the system-tick delta.
//! - Memory composition: `host_statistics64(HOST_VM_INFO64)` (wired, compressed, cached =
//!   file-backed + purgeable, app = anonymous − purgeable, page ins/outs) and the kernel's
//!   memory-status sysctls for pressure.
//! - System thread count: `processor_set_statistics(PROCESSOR_SET_LOAD_INFO)` on the
//!   default processor set (works without root, unlike per-task thread info).
//! - Applications: the main executable of a top-level `.app` bundle
//!   (`process::kind::is_macos_bundle_main_executable`).
//! - Machine facts: core classes (`hw.perflevelN.*`), cache totals, interface names and
//!   kinds from `networksetup -listallhardwareports` (run once per `machine_info`).
//!
//! Every `unsafe` block is an FFI call into libc / the Mach kernel interface and says why
//! it is sound. Mach port rights we are handed (host port, default processor set) are
//! owned by `PortRight` and released when the sampler drops.

use std::ffi::{c_int, c_void, CString};
use std::mem::{size_of, MaybeUninit};

use taskmanager_core::platform::parse::net::{parse_hardware_ports, HardwarePort};
use taskmanager_core::process::kind::is_macos_bundle_main_executable;
use taskmanager_core::telemetry::{
    CoreClass, MachineInfo, MemorySample, NetInfo, NetKind, PressureLevel,
};
use taskmanager_core::{ActionError, ProcessKey, Reading};

use super::os_api::{Identity, KernelSplit, StampError, ThreadInfo};
use super::rates::delta_between;
use super::unix;
pub use super::unix::{process_cpu_time, sysinfo_uid};

/// Whether identity reads are cached per (pid, start second) across ticks: `proc_pidinfo` / `kinfo_proc` reads are per-process syscalls worth caching.
pub const CACHE_IDENTITY: bool = true;

/// On macOS, sysinfo reports zero memory and CPU time for processes it may not inspect
/// (other users' processes without root), so an all-zero row means "not readable".
pub const ZERO_USAGE_MEANS_DENIED: bool = true;

/// A send right to a Mach port we own, released on drop with `mach_port_deallocate`
/// (`mach_host_self` and `processor_set_default` each hand us a new reference).
struct PortRight(libc::mach_port_t);

impl PortRight {
    /// The host port. `mach_host_self` is declared by libc but deprecated in favour of the
    /// `mach2` crate; it is stable kernel ABI and our only use, so we keep to libc.
    #[allow(deprecated)]
    fn host() -> Self {
        // SAFETY: `mach_host_self` takes no arguments and returns a new send right to the
        // host port (or MACH_PORT_NULL), which this value now owns.
        Self(unsafe { libc::mach_host_self() })
    }

    fn port(&self) -> libc::mach_port_t {
        self.0
    }
}

impl Drop for PortRight {
    fn drop(&mut self) {
        if self.0 != 0 {
            // SAFETY: releases the one user reference this value owns, exactly once; the
            // port name is not used afterwards.
            unsafe {
                mach_port_deallocate(task_port(), self.0);
            }
        }
    }
}

#[allow(deprecated)]
fn task_port() -> libc::mach_port_t {
    // SAFETY: reads the process's own task port, set up by the runtime before `main`.
    unsafe { libc::mach_task_self() }
}

extern "C" {
    // <mach/mach_host.h> / <mach/processor_set.h> / <mach/mach_port.h>; not in libc.
    fn mach_port_deallocate(
        task: libc::mach_port_t,
        name: libc::mach_port_t,
    ) -> libc::kern_return_t;
    fn processor_set_default(
        host: libc::mach_port_t,
        default_set: *mut libc::mach_port_t,
    ) -> libc::kern_return_t;
    fn processor_set_statistics(
        pset: libc::mach_port_t,
        flavor: c_int,
        info_out: *mut libc::integer_t,
        info_out_count: *mut libc::mach_msg_type_number_t,
    ) -> libc::kern_return_t;
}

/// Read process `pid`'s start stamp (microseconds since the epoch, as the kernel records
/// it at fork) and owner. Thread counts are not read here (per-task info needs the task
/// port, i.e. root for other users' processes).
pub fn read_identity(pid: u32) -> Result<Identity, StampError> {
    let pid = c_int::try_from(pid).map_err(|_| StampError::NotFound)?;
    match bsd_info_identity(pid) {
        // Another user's process (or a zombie proc_pidinfo no longer describes): the
        // kinfo_proc record is readable for every process that still exists.
        Err(StampError::Denied | StampError::NotFound) => kinfo_identity(pid),
        other => other,
    }
}

/// Identity from `proc_pidinfo(PROC_PIDTBSDINFO)`.
fn bsd_info_identity(pid: c_int) -> Result<Identity, StampError> {
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = size_of::<libc::proc_bsdinfo>() as c_int;
    // SAFETY: `info` is a writable, properly aligned `proc_bsdinfo` and we pass its exact
    // size; the kernel writes at most `size` bytes and returns how many it wrote.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast::<c_void>(),
            size,
        )
    };
    if written > 0 && written != size {
        // A short write is not a failed call: errno was not set and must not be read.
        return Err(StampError::Failed(format!(
            "proc_pidinfo returned {written} of {size} bytes"
        )));
    }
    if written <= 0 {
        return Err(match unix::errno() {
            libc::ESRCH => StampError::NotFound,
            libc::EPERM | libc::EACCES => StampError::Denied,
            e => StampError::Failed(std::io::Error::from_raw_os_error(e).to_string()),
        });
    }
    // SAFETY: the kernel filled all `size` bytes (checked above); the struct is plain data.
    let info = unsafe { info.assume_init() };
    Ok(Identity {
        stamp: info
            .pbi_start_tvsec
            .saturating_mul(1_000_000)
            .saturating_add(info.pbi_start_tvusec),
        threads: None,
        uid: Some(info.pbi_ruid),
    })
}

/// `struct kinfo_proc` (<sys/sysctl.h>), which libc does not define: its size and the
/// offsets we read. Identical on arm64 and x86_64 macOS (checked against the SDK headers
/// with `offsetof`); `kinfo_layout_matches_proc_pidinfo` re-checks it at test time.
const KINFO_PROC_SIZE: usize = 648;
/// `kp_proc.p_un.__p_starttime.tv_sec` (`time_t`, 8 bytes).
const KINFO_START_SEC: usize = 0;
/// `kp_proc.p_un.__p_starttime.tv_usec` (`suseconds_t`, 4 bytes).
const KINFO_START_USEC: usize = 8;
/// `kp_eproc.e_pcred.p_ruid` (`uid_t`, 4 bytes).
const KINFO_RUID: usize = 392;

/// Identity from `sysctl(CTL_KERN, KERN_PROC, KERN_PROC_PID, pid)`.
fn kinfo_identity(pid: c_int) -> Result<Identity, StampError> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid];
    // u64 storage keeps the record 8-byte aligned, as the kernel's struct is.
    let mut storage = [0u64; KINFO_PROC_SIZE / 8];
    let mut len = KINFO_PROC_SIZE;
    // SAFETY: `mib` is a valid 4-element name; `storage`/`len` describe a writable buffer
    // of `KINFO_PROC_SIZE` bytes, and the kernel writes at most `len` bytes and stores how
    // many it wrote (0 when no such process exists).
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            storage.as_mut_ptr().cast::<c_void>(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        let e = unix::errno();
        return Err(StampError::Failed(
            std::io::Error::from_raw_os_error(e).to_string(),
        ));
    }
    if len == 0 {
        return Err(StampError::NotFound);
    }
    let bytes: Vec<u8> = storage.iter().flat_map(|w| w.to_ne_bytes()).collect();
    parse_kinfo_proc(&bytes[..len.min(bytes.len())])
        .ok_or_else(|| StampError::Failed("kinfo_proc record too short".into()))
}

/// Start stamp and real uid from a raw `kinfo_proc`.
fn parse_kinfo_proc(bytes: &[u8]) -> Option<Identity> {
    let field = |at: usize, n: usize| bytes.get(at..at + n);
    let sec = i64::from_ne_bytes(field(KINFO_START_SEC, 8)?.try_into().ok()?);
    let usec = i32::from_ne_bytes(field(KINFO_START_USEC, 4)?.try_into().ok()?);
    let uid = u32::from_ne_bytes(field(KINFO_RUID, 4)?.try_into().ok()?);
    let stamp = u64::try_from(sec)
        .ok()?
        .saturating_mul(1_000_000)
        .saturating_add(u64::try_from(usec).ok()?);
    Some(Identity {
        stamp,
        threads: None,
        uid: Some(uid),
    })
}

/// Re-read `key`'s start stamp and, if it still matches, signal it.
pub fn end_verified(key: ProcessKey, force: bool) -> Result<(), ActionError> {
    let current = read_identity(key.pid.0).map_err(StampError::into_action_error)?;
    if current.stamp != key.start_time {
        return Err(ActionError::IdentityChanged);
    }
    // macOS has no pidfd: between this check and `kill` the process could exit and its pid
    // be reused. The window is a few microseconds and pids are handed out sequentially
    // (wrapping at 99999), so a reuse inside it would need ~100k process launches in that
    // time; we accept this unavoidable gap.
    unix::signal(key.pid.0, force)
}

/// Whether process `pid`'s disk I/O counters are readable: sysinfo's macOS I/O counters come with the task info it reads; processes it may not
/// inspect are already blanked through `ZERO_USAGE_MEANS_DENIED`.
pub fn disk_io_readable(_pid: u32) -> bool {
    true
}

/// Whether `mount` is the volume macOS boots from.
pub fn is_system_mount(mount: &std::path::Path) -> bool {
    mount == std::path::Path::new("/")
}

/// Per-CPU kernel share from `host_processor_info` tick deltas.
pub struct KernelCpu {
    host: PortRight,
    previous: Option<Vec<[u32; 4]>>,
}

impl KernelCpu {
    pub fn new() -> Self {
        Self {
            host: PortRight::host(),
            previous: None,
        }
    }

    /// The kernel share since the previous call (`Ok(None)` on the first).
    pub fn sample(&mut self) -> Result<Option<KernelSplit>, String> {
        let ticks = cpu_ticks(self.host.port())?;
        let split = self
            .previous
            .as_ref()
            .map(|prev| split_between(prev, &ticks));
        self.previous = Some(ticks);
        Ok(split)
    }
}

/// Kernel share per CPU and overall between two tick readings (Mach ticks wrap at u32).
fn split_between(prev: &[[u32; 4]], now: &[[u32; 4]]) -> KernelSplit {
    let user = libc::CPU_STATE_USER as usize;
    let system = libc::CPU_STATE_SYSTEM as usize;
    let idle = libc::CPU_STATE_IDLE as usize;
    let nice = libc::CPU_STATE_NICE as usize;
    let mut per_cpu = Vec::with_capacity(now.len());
    let (mut all_system, mut all_total) = (0u64, 0u64);
    for (i, cur) in now.iter().enumerate() {
        let Some(old) = prev.get(i) else {
            per_cpu.push(None);
            continue;
        };
        let d = |state: usize| u64::from(cur[state].wrapping_sub(old[state]));
        let total = d(user) + d(system) + d(idle) + d(nice);
        all_system += d(system);
        all_total += total;
        per_cpu.push((total > 0).then(|| (d(system) as f64 / total as f64 * 100.0) as f32));
    }
    KernelSplit {
        total: (all_total > 0).then(|| (all_system as f64 / all_total as f64 * 100.0) as f32),
        per_cpu,
    }
}

/// Cumulative user / system / idle / nice ticks per logical CPU.
fn cpu_ticks(host: libc::mach_port_t) -> Result<Vec<[u32; 4]>, String> {
    let mut cpu_count: libc::natural_t = 0;
    let mut info: libc::processor_info_array_t = std::ptr::null_mut();
    let mut info_count: libc::mach_msg_type_number_t = 0;
    // SAFETY: all three out-pointers are valid locals; on success the kernel allocates
    // `info_count` integers in our address space and stores the address in `info`.
    let kr = unsafe {
        libc::host_processor_info(
            host,
            libc::PROCESSOR_CPU_LOAD_INFO,
            &mut cpu_count,
            &mut info,
            &mut info_count,
        )
    };
    if kr != libc::KERN_SUCCESS || info.is_null() {
        return Err(format!("host_processor_info failed ({kr})"));
    }
    // SAFETY: on success `info` points to `info_count` initialised `integer_t`s that stay
    // valid until we deallocate them below.
    let raw = unsafe { std::slice::from_raw_parts(info, info_count as usize) };
    let states = libc::CPU_STATE_MAX as usize;
    let ticks = raw
        .chunks_exact(states)
        .take(cpu_count as usize)
        .map(|c| [c[0] as u32, c[1] as u32, c[2] as u32, c[3] as u32])
        .collect();
    // SAFETY: returns the kernel's allocation (address and byte size exactly as handed to
    // us) to our own task; `raw` is not used after this.
    unsafe {
        libc::vm_deallocate(
            task_port(),
            info as libc::vm_address_t,
            info_count as usize * size_of::<libc::integer_t>(),
        );
    }
    Ok(ticks)
}

/// Memory composition from the VM statistics and memory-status sysctls.
pub struct MemoryExtras {
    host: PortRight,
    page_size: Option<u64>,
    previous_pages: Option<(u64, u64)>,
}

impl MemoryExtras {
    pub fn new() -> Self {
        Self {
            host: PortRight::host(),
            page_size: sysctl_u64("hw.pagesize"),
            previous_pages: None,
        }
    }

    /// Fill wired, compressed, cached, app, page ins/outs and pressure into `memory`.
    pub fn fill(&mut self, memory: &mut MemorySample) -> Result<(), String> {
        memory.committed = Reading::Unsupported;
        if let Some(level) = sysctl_u64("kern.memorystatus_vm_pressure_level") {
            memory.pressure_level = match level {
                1 => Reading::Value(PressureLevel::Normal),
                2 => Reading::Value(PressureLevel::Elevated),
                4 => Reading::Value(PressureLevel::Critical),
                _ => Reading::Unavailable,
            };
        }
        // `kern.memorystatus_level` is the share of memory the kernel considers free;
        // pressure is the rest.
        if let Some(free_pct) = sysctl_u64("kern.memorystatus_level").filter(|p| *p <= 100) {
            memory.pressure_pct = Reading::Value((100 - free_pct) as f32);
        }
        let page = self.page_size.ok_or("hw.pagesize unreadable")?;
        let stats = vm_statistics(self.host.port())?;
        let pages = |count: u32| Reading::Value(u64::from(count) * page);
        memory.wired = pages(stats.wire_count);
        memory.compressed = pages(stats.compressor_page_count);
        memory.cached = pages(
            stats
                .external_page_count
                .saturating_add(stats.purgeable_count),
        );
        memory.app = pages(
            stats
                .internal_page_count
                .saturating_sub(stats.purgeable_count),
        );
        let (ins, outs) = (stats.pageins, stats.pageouts);
        let previous = self.previous_pages.replace((ins, outs));
        memory.page_ins = delta_between(previous.map(|p| p.0), ins);
        memory.page_outs = delta_between(previous.map(|p| p.1), outs);
        Ok(())
    }
}

fn vm_statistics(host: libc::mach_port_t) -> Result<libc::vm_statistics64, String> {
    let mut stats = MaybeUninit::<libc::vm_statistics64>::zeroed();
    let mut count = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: `stats` is a writable `vm_statistics64` and `count` says how many
    // `integer_t`s it holds; the kernel writes at most that many and updates `count`.
    let kr = unsafe {
        libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            stats.as_mut_ptr().cast::<libc::integer_t>(),
            &mut count,
        )
    };
    if kr != libc::KERN_SUCCESS {
        return Err(format!("host_statistics64 failed ({kr})"));
    }
    // SAFETY: zero-initialised plain data, (partly) overwritten by the kernel.
    Ok(unsafe { stats.assume_init() })
}

/// Thread totals: system-wide from the default processor set; no per-process counts.
pub struct Threads {
    pset: Option<PortRight>,
}

impl Threads {
    pub fn new() -> Self {
        let host = PortRight::host();
        let mut pset: libc::mach_port_t = 0;
        // SAFETY: `pset` is a valid out-pointer; on success it receives a send right to the
        // default processor set's name port, which we keep for our lifetime.
        let kr = unsafe { processor_set_default(host.port(), &mut pset) };
        Self {
            pset: (kr == libc::KERN_SUCCESS).then_some(PortRight(pset)),
        }
    }

    pub fn sample(&mut self) -> Result<ThreadInfo, String> {
        let pset = self
            .pset
            .as_ref()
            .ok_or("processor_set_default failed")?
            .port();
        let mut info = MaybeUninit::<libc::processor_set_load_info>::zeroed();
        let mut count = (size_of::<libc::processor_set_load_info>() / size_of::<libc::natural_t>())
            as libc::mach_msg_type_number_t;
        // SAFETY: `info` is a writable `processor_set_load_info` of `count` integers.
        let kr = unsafe {
            processor_set_statistics(
                pset,
                libc::PROCESSOR_SET_LOAD_INFO,
                info.as_mut_ptr().cast::<libc::integer_t>(),
                &mut count,
            )
        };
        if kr != libc::KERN_SUCCESS {
            return Err(format!("processor_set_statistics failed ({kr})"));
        }
        // SAFETY: zero-initialised plain data, filled by the kernel.
        let info = unsafe { info.assume_init() };
        Ok(ThreadInfo {
            total: u64::try_from(info.thread_count).map_or(Reading::Unavailable, Reading::Value),
            per_pid: None,
        })
    }
}

/// Applications are the main executables of top-level `.app` bundles.
pub struct AppClassifier;

impl AppClassifier {
    pub fn new() -> Self {
        Self
    }

    pub fn begin_tick(&mut self) -> Result<(), String> {
        Ok(())
    }

    pub fn is_application(&self, _pid: u32, exe: Option<&str>, _name: &str) -> bool {
        exe.is_some_and(is_macos_bundle_main_executable)
    }
}

/// Fill what macOS knows beyond sysinfo: the OS name, core classes, caches, interface names and kinds.
pub fn machine_extras(info: &mut MachineInfo) {
    // sysinfo names the kernel ("Darwin"); the product is macOS.
    info.os_name = Reading::Value("macOS".into());
    info.cpu.core_classes = core_classes(info.cpu.logical_cores);
    let (l1, l2, l3) = cache_totals();
    info.cpu.l1 = l1;
    info.cpu.l2 = l2;
    info.cpu.l3 = l3;
    info.cpu.is_vm =
        sysctl_u64("kern.hv_vmm_present").map_or(Reading::Unavailable, |v| Reading::Value(v != 0));
    refine_networks(&mut info.networks, &hardware_ports());
}

/// Core classes from `hw.perflevelN` (level 0 is the fastest). Assumes the kernel numbers
/// logical CPUs cluster by cluster from the slowest level up, as Apple silicon does (the
/// efficiency cores are cpu0…); empty when there is a single class or the counts do not
/// add up to `logical`.
fn core_classes(logical: usize) -> Vec<CoreClass> {
    let levels = sysctl_u64("hw.nperflevels").unwrap_or(0) as usize;
    if levels < 2 {
        return Vec::new();
    }
    let mut classes = Vec::new();
    let mut next = 0usize;
    for level in (0..levels).rev() {
        let Some(count) = sysctl_u64(&format!("hw.perflevel{level}.logicalcpu")) else {
            return Vec::new();
        };
        let name = sysctl_string(&format!("hw.perflevel{level}.name"))
            .unwrap_or_else(|| format!("Level {level}"));
        let count = count as usize;
        classes.push(CoreClass {
            name,
            logical_indices: (next..next + count).collect(),
        });
        next += count;
    }
    classes.reverse();
    if next == logical {
        classes
    } else {
        Vec::new()
    }
}

/// Total L1 (data + instruction), L2 and L3 bytes across the package.
fn cache_totals() -> (Reading<u64>, Reading<u64>, Reading<u64>) {
    let read = |name: &str| sysctl_u64(name).filter(|v| *v > 0);
    let levels = read("hw.nperflevels").unwrap_or(0);
    if levels > 0 {
        let (mut l1, mut l2) = (0u64, 0u64);
        for level in 0..levels {
            let key = |field: &str| read(&format!("hw.perflevel{level}.{field}"));
            let cores = key("physicalcpu").unwrap_or(0);
            let logical = key("logicalcpu").unwrap_or(0);
            l1 += (key("l1dcachesize").unwrap_or(0) + key("l1icachesize").unwrap_or(0)) * cores;
            if let (Some(size), Some(per)) = (key("l2cachesize"), key("cpusperl2")) {
                l2 += size * logical.div_ceil(per);
            }
        }
        let l3 = read("hw.l3cachesize");
        let value = |v: u64| {
            if v > 0 {
                Reading::Value(v)
            } else {
                Reading::Unavailable
            }
        };
        return (value(l1), value(l2), l3.into());
    }
    // Intel Macs: per-core L1/L2, one shared L3 per package.
    let cores = read("hw.physicalcpu");
    let l1 = match (read("hw.l1dcachesize"), read("hw.l1icachesize"), cores) {
        (Some(d), Some(i), Some(c)) => Some((d + i) * c),
        _ => None,
    };
    let l2 = read("hw.l2cachesize").zip(cores).map(|(s, c)| s * c);
    let l3 = read("hw.l3cachesize").map(|s| s * read("hw.packages").unwrap_or(1));
    (l1.into(), l2.into(), l3.into())
}

/// `networksetup -listallhardwareports`, or nothing if it cannot run.
fn hardware_ports() -> Vec<HardwarePort> {
    std::process::Command::new("/usr/sbin/networksetup")
        .arg("-listallhardwareports")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| parse_hardware_ports(&String::from_utf8_lossy(&out.stdout)))
        .unwrap_or_default()
}

/// Name and classify interfaces by their hardware port. `en*` without a port is left
/// `Other`: on a Mac it may be Wi-Fi or Ethernet and the name cannot tell.
fn refine_networks(networks: &mut [NetInfo], ports: &[HardwarePort]) {
    for net in networks.iter_mut() {
        if let Some(port) = ports.iter().find(|p| p.device == net.id) {
            net.name = port.port.clone();
            net.kind = port.kind();
        } else if net.id.starts_with("en") && net.kind == NetKind::Ethernet {
            net.kind = NetKind::Other;
        }
    }
}

/// An integer sysctl (4- or 8-byte).
fn sysctl_u64(name: &str) -> Option<u64> {
    let name = CString::new(name).ok()?;
    let mut buf = [0u8; 8];
    let mut len = buf.len();
    // SAFETY: `name` is NUL-terminated; `buf`/`len` describe a writable 8-byte buffer and
    // the kernel writes at most `len` bytes and stores the length it wrote.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast::<c_void>(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    match (rc, len) {
        (0, 4) => Some(u64::from(u32::from_ne_bytes([
            buf[0], buf[1], buf[2], buf[3],
        ]))),
        (0, 8) => Some(u64::from_ne_bytes(buf)),
        _ => None,
    }
}

/// A string sysctl.
fn sysctl_string(name: &str) -> Option<String> {
    let name = CString::new(name).ok()?;
    let mut buf = [0u8; 256];
    let mut len = buf.len();
    // SAFETY: as in `sysctl_u64`, with a 256-byte buffer.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast::<c_void>(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    let text = &buf[..len.min(buf.len())];
    let text = text.split(|b| *b == 0).next().unwrap_or(text);
    let text = String::from_utf8_lossy(text).trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_split_uses_system_ticks_and_survives_wrap() {
        let prev = [[100, 50, 850, 0], [u32::MAX - 9, 0, 0, 0]];
        let now = [[160, 80, 910, 0], [10, 0, 0, 0]];
        let split = split_between(&prev, &now);
        // CPU 0: 30 of 150 ticks in the kernel; CPU 1: 20 user ticks across the wrap.
        assert_eq!(split.per_cpu, vec![Some(20.0), Some(0.0)]);
        let total = split.total.expect("time passed");
        assert!((total - 30.0 / 170.0 * 100.0).abs() < 1e-4, "{total}");
        let idle = split_between(&now, &now);
        assert_eq!(idle.per_cpu, vec![None, None]);
        assert_eq!(idle.total, None);
    }

    #[test]
    fn kinfo_layout_matches_proc_pidinfo() {
        let me = std::process::id() as c_int;
        let bsd = bsd_info_identity(me).expect("our own bsdinfo");
        let kinfo = kinfo_identity(me).expect("our own kinfo_proc");
        assert_eq!(kinfo.stamp, bsd.stamp);
        // SAFETY: `getuid` has no preconditions.
        let uid = unsafe { libc::getuid() };
        assert_eq!(kinfo.uid, Some(uid));
        assert_eq!(bsd.uid, Some(uid));
    }

    #[test]
    fn root_processes_get_a_real_stamp_and_owner() {
        // launchd (pid 1) belongs to root: proc_pidinfo refuses it unless we are root.
        let launchd = read_identity(1).expect("launchd's identity");
        assert_eq!(launchd.uid, Some(0));
        assert!(
            launchd.stamp > 1_000_000_000 * 1_000_000,
            "microseconds since 1970"
        );
        assert_eq!(kinfo_identity(i32::MAX), Err(StampError::NotFound));
        assert_eq!(parse_kinfo_proc(&[0u8; 16]), None);
    }

    #[test]
    fn samplers_can_be_made_and_dropped_repeatedly() {
        // Each round takes and releases host / processor-set rights.
        for _ in 0..50 {
            let mut kernel = KernelCpu::new();
            assert!(kernel.sample().is_ok());
            let mut memory = MemoryExtras::new();
            let mut sample = MemorySample::default();
            assert!(memory.fill(&mut sample).is_ok());
            assert!(sample.wired.is_value());
            assert!(Threads::new().sample().is_ok_and(|t| t.total.is_value()));
        }
    }

    #[test]
    fn sysctls_read_this_mac() {
        assert!(sysctl_u64("hw.pagesize").is_some_and(|p| p >= 4096));
        assert!(sysctl_u64("hw.ncpu").is_some_and(|n| n >= 1));
        assert!(sysctl_string("kern.ostype").is_some_and(|s| s == "Darwin"));
        assert_eq!(sysctl_u64("no.such.sysctl"), None);
    }

    #[test]
    fn classes_cover_every_logical_cpu_or_none() {
        let logical = sysctl_u64("hw.logicalcpu").unwrap_or(0) as usize;
        let classes = core_classes(logical);
        let covered: usize = classes.iter().map(|c| c.logical_indices.len()).sum();
        assert!(classes.is_empty() || covered == logical);
        assert!(core_classes(logical + 1).is_empty());
    }

    #[test]
    fn ports_rename_and_classify_interfaces() {
        let mut nets = vec![
            NetInfo {
                id: "en0".into(),
                name: "en0".into(),
                kind: NetKind::Ethernet,
                ..NetInfo::default()
            },
            NetInfo {
                id: "en7".into(),
                name: "en7".into(),
                kind: NetKind::Ethernet,
                ..NetInfo::default()
            },
            NetInfo {
                id: "lo0".into(),
                name: "lo0".into(),
                kind: NetKind::Loopback,
                ..NetInfo::default()
            },
        ];
        let ports = [HardwarePort {
            port: "Wi-Fi".into(),
            device: "en0".into(),
            mac: None,
        }];
        refine_networks(&mut nets, &ports);
        assert_eq!(
            (nets[0].name.as_str(), nets[0].kind),
            ("Wi-Fi", NetKind::WiFi)
        );
        assert_eq!(nets[1].kind, NetKind::Other);
        assert_eq!(nets[2].kind, NetKind::Loopback);
    }
}
