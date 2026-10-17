//! Unix pieces macOS and Linux share: sending a termination signal, reading `errno` and
//! turning a raw uid into sysinfo's `Uid`.
//!
//! `macos.rs` and `linux.rs` verify a process's identity (its start stamp, PLAN §4.4) and
//! then call [`signal`] here; Linux prefers a pidfd when the kernel has one (see
//! `linux.rs`) and falls back to this.

use taskmanager_core::ActionError;

/// A raw uid as sysinfo's `Uid` (for `Users::get_user_by_id`).
pub fn sysinfo_uid(uid: u32) -> Option<sysinfo::Uid> {
    sysinfo::Uid::try_from(uid as usize).ok()
}

/// The calling thread's `errno` after a failed libc call.
pub fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// The signal for a polite (`SIGTERM`) or forced (`SIGKILL`) end.
pub fn termination_signal(force: bool) -> libc::c_int {
    if force {
        libc::SIGKILL
    } else {
        libc::SIGTERM
    }
}

/// Map a `kill`-style errno to an action error.
pub fn signal_error(errno: i32) -> ActionError {
    match errno {
        libc::ESRCH => ActionError::NotFound,
        libc::EPERM | libc::EACCES => ActionError::PermissionDenied,
        other => ActionError::Failed(std::io::Error::from_raw_os_error(other).to_string()),
    }
}

/// Send `SIGTERM` (or `SIGKILL` when `force`) to exactly one process, `pid`. The caller
/// has already checked the process's identity.
pub fn signal(pid: u32, force: bool) -> Result<(), ActionError> {
    let pid = target_pid(pid)?;
    // SAFETY: `kill` takes plain integers and has no memory-safety preconditions;
    // `target_pid` guarantees `pid > 0`, so this addresses one process, never a process
    // group (0 or negative) or every process (-1).
    let result = unsafe { libc::kill(pid, termination_signal(force)) };
    if result == 0 {
        Ok(())
    } else {
        Err(signal_error(errno()))
    }
}

/// `pid` as a `pid_t` that names exactly one process. Pid 0 (the kernel / swapper) and
/// values that do not fit would make `kill` address a group or fail, so they are refused.
pub fn target_pid(pid: u32) -> Result<libc::pid_t, ActionError> {
    match libc::pid_t::try_from(pid) {
        Ok(pid) if pid > 0 => Ok(pid),
        _ => Err(ActionError::Failed(format!(
            "pid {pid} does not name a single process"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_zero_and_huge_pids_are_never_signalled() {
        assert!(matches!(target_pid(0), Err(ActionError::Failed(_))));
        assert!(matches!(target_pid(u32::MAX), Err(ActionError::Failed(_))));
        assert_eq!(target_pid(1), Ok(1));
        assert!(matches!(signal(0, false), Err(ActionError::Failed(_))));
    }

    #[test]
    fn errnos_map_to_action_errors() {
        assert_eq!(signal_error(libc::ESRCH), ActionError::NotFound);
        assert_eq!(signal_error(libc::EPERM), ActionError::PermissionDenied);
        assert!(matches!(signal_error(libc::EINVAL), ActionError::Failed(_)));
        assert_eq!(termination_signal(true), libc::SIGKILL);
        assert_eq!(termination_signal(false), libc::SIGTERM);
    }
}
