//! Parser for Linux `/proc/<pid>/stat` (proc(5)), the source of a process's fine-grained
//! creation stamp on Linux (PLAN §4.4).
//!
//! The native Linux platform (`taskmanager-native/src/platform/linux.rs`) reads the file and
//! hands the text here; the parser is pure so it is tested on every OS. Field 22,
//! `starttime`, is the process's start in clock ticks since boot: monotonic within a boot
//! and finer than sysinfo's whole seconds, so it becomes `ProcessKey::start_time`.
//!
//! The awkward part is field 2, `comm`: the executable name in parentheses, which may
//! itself contain spaces and parentheses (`(my (odd) prog)`). The kernel never escapes it,
//! so the only reliable split is at the *last* `)` in the line.
//!
//! Field 20, `num_threads`, comes along for free (the platform reads this file anyway), so
//! the platform need not walk `/proc/<pid>/task/` to count threads.

/// The fields of `/proc/<pid>/stat` TaskManager uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcPidStat {
    /// Field 1.
    pub pid: u32,
    /// Field 2, without the surrounding parentheses (at most 15 bytes of the name).
    pub comm: String,
    /// Field 3: `R`, `S`, `D`, `Z`, `T`, `t`, `X`, `I`, …
    pub state: char,
    /// Field 4.
    pub ppid: u32,
    /// Field 20.
    pub num_threads: u32,
    /// Field 22: start time in clock ticks after boot.
    pub start_ticks: u64,
}

/// Parse one `/proc/<pid>/stat` line. `None` if it is malformed or truncated.
pub fn parse_proc_pid_stat(text: &str) -> Option<ProcPidStat> {
    let text = text.trim_end();
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close < open {
        return None;
    }
    let pid = text[..open].trim().parse().ok()?;
    let comm = text[open + 1..close].to_string();
    // Fields after the comm: index 0 is field 3 (state), so field N is index N - 3.
    let rest: Vec<&str> = text[close + 1..].split_whitespace().collect();
    let mut state_chars = rest.first()?.chars();
    let state = state_chars.next()?;
    if state_chars.next().is_some() {
        return None;
    }
    let ppid = rest.get(4 - 3)?.parse().ok()?;
    let num_threads = rest.get(20 - 3)?.parse().ok()?;
    let start_ticks = rest.get(22 - 3)?.parse().ok()?;
    Some(ProcPidStat {
        pid,
        comm,
        state,
        ppid,
        num_threads,
        start_ticks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real line (systemd on a desktop), fields 1–52.
    const SYSTEMD: &str = "1 (systemd) S 0 1 1 0 -1 4194560 85938 9273546 104 4185 \
        350 410 22013 6447 20 0 1 0 12 172032000 3301 18446744073709551615 1 1 0 0 0 0 \
        671173123 4096 1260 0 0 0 17 3 0 0 0 0 0 0 0 0 0 0 0 0 0\n";

    #[test]
    fn reads_pid_comm_state_parent_and_start_ticks() {
        let stat = parse_proc_pid_stat(SYSTEMD).expect("parses");
        assert_eq!(
            stat,
            ProcPidStat {
                pid: 1,
                comm: "systemd".into(),
                state: 'S',
                ppid: 0,
                num_threads: 1,
                start_ticks: 12,
            }
        );
    }

    #[test]
    fn comm_with_spaces_and_parentheses_splits_at_the_last_paren() {
        let line = "4242 (Web (Content) 2) R 4100 4100 4100 0 -1 4194304 10 0 0 0 \
            5 3 0 0 20 0 27 0 987654321 100 200 300";
        let stat = parse_proc_pid_stat(line).expect("parses");
        assert_eq!(stat.pid, 4242);
        assert_eq!(stat.comm, "Web (Content) 2");
        assert_eq!(stat.state, 'R');
        assert_eq!(stat.ppid, 4100);
        assert_eq!(stat.num_threads, 27);
        assert_eq!(stat.start_ticks, 987_654_321);
    }

    #[test]
    fn a_comm_that_looks_like_fields_does_not_shift_them() {
        let line = "77 (x) S 1 2 3) Z 9 9 9 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 555 0 0";
        let stat = parse_proc_pid_stat(line).expect("parses");
        assert_eq!(stat.comm, "x) S 1 2 3");
        assert_eq!(stat.state, 'Z');
        assert_eq!(stat.ppid, 9);
        assert_eq!(stat.start_ticks, 555);
    }

    #[test]
    fn malformed_or_truncated_lines_are_rejected() {
        assert_eq!(parse_proc_pid_stat(""), None);
        assert_eq!(parse_proc_pid_stat("12 sleep S 1"), None);
        assert_eq!(parse_proc_pid_stat("12 (sleep) S 1 2 3"), None);
        assert_eq!(
            parse_proc_pid_stat("x (sleep) S 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9"),
            None
        );
        assert_eq!(parse_proc_pid_stat(") 12 (sleep"), None);
        let bad_state = SYSTEMD.replacen(") S ", ") SS ", 1);
        assert_eq!(parse_proc_pid_stat(&bad_state), None);
    }
}
