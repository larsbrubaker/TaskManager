//! Windows extras for the native platform: what sysinfo does not give us (PLAN §4.3),
//! through `windows-sys`.
//!
//! - Process identity: the creation `FILETIME` from `GetProcessTimes` (100 ns units), the
//!   fine-grained per-boot stamp `ProcessKey` needs (PLAN §4.4). Protected processes
//!   (and the Idle process, pid 0) refuse even `PROCESS_QUERY_LIMITED_INFORMATION`; their
//!   keys fall back to sysinfo's whole-second start time (see `processes.rs`).
//! - Ending a process: one handle opened with terminate + query rights; the creation time
//!   is re-checked *on that handle* and `TerminateProcess` called on it, so a recycled pid
//!   can never be hit. A polite end (`force == false`) also terminates for now: a WM_CLOSE
//!   path for windowed apps arrives with the Processes page (M5).
//! - Kernel CPU split: `GetSystemTimes` deltas (kernel time includes idle, so kernel share
//!   = (kernel − idle) / (kernel + user)). Windows has no cheap per-CPU split, so per-core
//!   kernel shares stay `Unavailable` (PDH counters, M4).
//! - Memory composition: `GetPerformanceInfo` (committed = commit total, cached = system
//!   cache, wired = non-paged kernel pool).
//! - Thread counts: one Toolhelp32 process snapshot per tick gives every process's thread
//!   count and the system total.
//! - Applications: processes that own a visible, unowned top-level window with a title
//!   (`EnumWindows`), collected once per tick.
//! - Machine facts: cache totals from `GetLogicalProcessorInformationEx(RelationCache)`
//!   (parsed by the core); interface kinds come from their names.
//!
//! Every `unsafe` block is a Win32 FFI call and says why it is sound.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use taskmanager_core::platform::parse::logical_processor::parse_logical_processor_caches;
use taskmanager_core::telemetry::{MachineInfo, MemorySample};
use taskmanager_core::{ActionError, ProcessKey, Reading};
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, HANDLE,
    HWND, INVALID_HANDLE_VALUE, LPARAM, STILL_ACTIVE,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows_sys::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, RelationCache, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, GetSystemTimes, OpenProcess, TerminateProcess,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
    GW_OWNER,
};

use super::os_api::{Identity, KernelSplit, StampError, ThreadInfo};

/// sysinfo reports zero memory and CPU time for processes it cannot open (protected
/// processes), so an all-zero row means "not readable".
pub const ZERO_USAGE_MEANS_DENIED: bool = true;

/// A kernel handle (a process, or a Toolhelp snapshot), closed on drop.
struct OwnedHandle(HANDLE);

impl OwnedHandle {
    /// `OpenProcess(access, FALSE, pid)`, or why not.
    fn open(pid: u32, access: u32) -> Result<Self, StampError> {
        if pid == 0 {
            // The Idle pseudo-process cannot be opened; it is listed with a fallback key.
            return Err(StampError::Denied);
        }
        // SAFETY: plain-value arguments; returns a new handle or null.
        let handle = unsafe { OpenProcess(access, 0, pid) };
        if handle.is_null() {
            // SAFETY: reads this thread's last-error value, set by the failed call above.
            let error = unsafe { GetLastError() };
            return Err(match error {
                ERROR_INVALID_PARAMETER => StampError::NotFound,
                ERROR_ACCESS_DENIED => StampError::Denied,
                other => {
                    StampError::Failed(std::io::Error::from_raw_os_error(other as i32).to_string())
                }
            });
        }
        Ok(Self(handle))
    }

    /// The process's creation time, 100 ns units since 1601.
    fn creation_time(&self) -> Result<u64, StampError> {
        let mut times = [FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        }; 4];
        let [creation, exit, kernel, user] = &mut times;
        // SAFETY: `self.0` is an open process handle with query rights; the four
        // out-pointers are distinct, writable FILETIMEs.
        let ok = unsafe { GetProcessTimes(self.0, creation, exit, kernel, user) };
        if ok == 0 {
            return Err(StampError::Failed(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        Ok(filetime(times[0]))
    }

    /// Whether the process is still running (its handle can outlive it).
    fn is_running(&self) -> bool {
        let mut code = 0u32;
        // SAFETY: open handle with query rights; `code` is a writable u32.
        let ok = unsafe { GetExitCodeProcess(self.0, &mut code) };
        ok != 0 && code == STILL_ACTIVE as u32
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: closes the handle `open` returned, exactly once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// A FILETIME as one 64-bit count.
fn filetime(t: FILETIME) -> u64 {
    (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
}

/// Read process `pid`'s creation stamp.
pub fn read_identity(pid: u32) -> Result<Identity, StampError> {
    let handle = OwnedHandle::open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    Ok(Identity {
        stamp: handle.creation_time()?,
        threads: None,
        uid: None,
    })
}

/// Open `key`'s process, re-check its creation time on that handle and terminate it.
pub fn end_verified(key: ProcessKey, _force: bool) -> Result<(), ActionError> {
    let handle = OwnedHandle::open(
        key.pid.0,
        PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
    )
    .map_err(StampError::into_action_error)?;
    if !handle.is_running() {
        return Err(ActionError::NotFound);
    }
    let stamp = handle
        .creation_time()
        .map_err(StampError::into_action_error)?;
    if stamp != key.start_time {
        return Err(ActionError::IdentityChanged);
    }
    terminate(&handle)
}

fn terminate(handle: &OwnedHandle) -> Result<(), ActionError> {
    // SAFETY: open handle with PROCESS_TERMINATE; exit code 1.
    let ok = unsafe { TerminateProcess(handle.0, 1) };
    if ok != 0 {
        return Ok(());
    }
    // SAFETY: reads this thread's last-error value, set by the failed call above.
    match unsafe { GetLastError() } {
        ERROR_ACCESS_DENIED => Err(ActionError::PermissionDenied),
        other => Err(ActionError::Failed(
            std::io::Error::from_raw_os_error(other as i32).to_string(),
        )),
    }
}

/// Raw uids never occur on Windows (owners are SIDs, and identity reads report none).
pub fn sysinfo_uid(_uid: u32) -> Option<sysinfo::Uid> {
    None
}

/// Whether `mount` is the system drive (`%SystemDrive%\`, normally `C:\`).
pub fn is_system_mount(mount: &Path) -> bool {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let mount = mount.to_string_lossy();
    mount
        .trim_end_matches('\\')
        .eq_ignore_ascii_case(drive.trim_end_matches('\\'))
}

/// Machine-wide kernel share from `GetSystemTimes` deltas; no per-CPU split.
pub struct KernelCpu {
    previous: Option<(u64, u64, u64)>,
}

impl KernelCpu {
    pub fn new() -> Self {
        Self { previous: None }
    }

    pub fn sample(&mut self) -> Result<Option<KernelSplit>, String> {
        let mut idle = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut kernel = idle;
        let mut user = idle;
        // SAFETY: three distinct, writable FILETIMEs.
        let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
        if ok == 0 {
            return Err(format!(
                "GetSystemTimes failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let now = (filetime(idle), filetime(kernel), filetime(user));
        let split = self.previous.map(|(idle0, kernel0, user0)| {
            let idle = now.0.saturating_sub(idle0);
            let kernel = now.1.saturating_sub(kernel0);
            let user = now.2.saturating_sub(user0);
            let total = kernel + user;
            KernelSplit {
                total: (total > 0)
                    .then(|| (kernel.saturating_sub(idle) as f64 / total as f64 * 100.0) as f32),
                per_cpu: Vec::new(),
            }
        });
        self.previous = Some(now);
        Ok(split)
    }
}

/// Memory composition from `GetPerformanceInfo`.
pub struct MemoryExtras;

impl MemoryExtras {
    pub fn new() -> Self {
        Self
    }

    pub fn fill(&mut self, memory: &mut MemorySample) -> Result<(), String> {
        memory.app = Reading::Unsupported;
        let size = std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32;
        // SAFETY: PERFORMANCE_INFORMATION is plain data; all-zero is a valid value.
        let mut info: PERFORMANCE_INFORMATION = unsafe { std::mem::zeroed() };
        info.cb = size;
        // SAFETY: `info` is writable and `size` is its exact size.
        let ok = unsafe { GetPerformanceInfo(&mut info, size) };
        if ok == 0 {
            return Err(format!(
                "GetPerformanceInfo failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let page = info.PageSize as u64;
        memory.committed = Reading::Value(info.CommitTotal as u64 * page);
        memory.cached = Reading::Value(info.SystemCache as u64 * page);
        memory.wired = Reading::Value(info.KernelNonpaged as u64 * page);
        Ok(())
    }
}

/// Thread counts per process and in total from a Toolhelp32 snapshot.
pub struct Threads;

impl Threads {
    pub fn new() -> Self {
        Self
    }

    pub fn sample(&mut self) -> Result<ThreadInfo, String> {
        // SAFETY: plain-value arguments; returns a snapshot handle or INVALID_HANDLE_VALUE.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(format!(
                "process snapshot failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let snapshot = OwnedHandle(snapshot);
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..PROCESSENTRY32W::default()
        };
        let mut per_pid = HashMap::new();
        let mut total = 0u64;
        // SAFETY: `snapshot.0` is an open Toolhelp snapshot; `entry` is writable with
        // `dwSize` set as the API requires.
        let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
        while more {
            per_pid.insert(entry.th32ProcessID, entry.cntThreads);
            total += u64::from(entry.cntThreads);
            // SAFETY: as above.
            more = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
        }
        if per_pid.is_empty() {
            return Err("process snapshot was empty".into());
        }
        Ok(ThreadInfo {
            total: Reading::Value(total),
            per_pid: Some(per_pid),
        })
    }
}

/// Applications are processes owning a visible, unowned, titled top-level window.
pub struct AppClassifier {
    windowed: HashSet<u32>,
}

/// `EnumWindows` callback: adds the owner pid of each app-like window to the set behind
/// `lparam`.
unsafe extern "system" fn collect_window_owner(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the `&mut HashSet<u32>` `begin_tick` passed, valid for the whole
    // synchronous `EnumWindows` call; `hwnd` is a window handle the system just gave us.
    unsafe {
        let pids = &mut *(lparam as *mut HashSet<u32>);
        let app_like = IsWindowVisible(hwnd) != 0
            && GetWindow(hwnd, GW_OWNER).is_null()
            && GetWindowTextLengthW(hwnd) > 0;
        if app_like {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid != 0 {
                pids.insert(pid);
            }
        }
    }
    1
}

impl AppClassifier {
    pub fn new() -> Self {
        Self {
            windowed: HashSet::new(),
        }
    }

    /// Collect this tick's window owners.
    pub fn begin_tick(&mut self) -> Result<(), String> {
        let mut pids = HashSet::new();
        // SAFETY: the callback only touches `pids` through the pointer we pass, and
        // EnumWindows returns before `pids` goes out of scope.
        let ok = unsafe {
            EnumWindows(
                Some(collect_window_owner),
                &mut pids as *mut HashSet<u32> as LPARAM,
            )
        };
        if ok == 0 {
            return Err(format!(
                "EnumWindows failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        self.windowed = pids;
        Ok(())
    }

    pub fn is_application(&self, pid: u32, _exe: Option<&str>, _name: &str) -> bool {
        self.windowed.contains(&pid)
    }
}

/// Fill cache totals; interface kinds come from their names (set by `devices`).
pub fn machine_extras(info: &mut MachineInfo) {
    let totals = parse_logical_processor_caches(&cache_records());
    info.cpu.l1 = totals.l1.into();
    info.cpu.l2 = totals.l2.into();
    info.cpu.l3 = totals.l3.into();
}

/// The raw `GetLogicalProcessorInformationEx(RelationCache)` buffer (empty on failure).
fn cache_records() -> Vec<u8> {
    let mut len = 0u32;
    // SAFETY: a null buffer with length 0 asks only for the required length.
    unsafe {
        GetLogicalProcessorInformationEx(RelationCache, std::ptr::null_mut(), &mut len);
    }
    if len == 0 {
        return Vec::new();
    }
    // u64 storage keeps the records 8-byte aligned, as the API expects.
    let mut storage = vec![0u64; (len as usize).div_ceil(8)];
    // SAFETY: `storage` holds at least `len` writable, aligned bytes.
    let ok = unsafe {
        GetLogicalProcessorInformationEx(
            RelationCache,
            storage
                .as_mut_ptr()
                .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>(),
            &mut len,
        )
    };
    if ok == 0 {
        return Vec::new();
    }
    storage
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .take(len as usize)
        .collect()
}
