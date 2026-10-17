//! Command-line parsing for the `taskmanager` binary.
//!
//! ```text
//! taskmanager                      open the app
//! taskmanager --demo               open the app on the simulated demo machine
//! taskmanager --dump-snapshot [N]  sample the real machine N times (default 2) at 1 s
//!                                  without a window, print a summary, exit
//! taskmanager --help
//! ```
//!
//! `--dump-snapshot` is the headless check of the native platform ([`crate::dump`]).
//! `--demo` is accepted now so the screenshot recipe in CLAUDE.md keeps working; the
//! window shows no telemetry from either platform until the UI is wired to them (M1.4).

/// Text printed for `--help` and after a usage error.
pub const USAGE: &str = "\
Usage:
  taskmanager                       Open TaskManager
  taskmanager --demo                Open TaskManager on the simulated demo machine
  taskmanager --dump-snapshot [N]   Sample this machine N times (default 2) one second
                                    apart without a window, print a summary and exit
  taskmanager --help                Show this help";

/// Samples `--dump-snapshot` takes without a count: two, so rates are measured.
pub const DEFAULT_DUMP_SAMPLES: u32 = 2;

/// What the command line asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// The windowed app.
    App,
    /// Sample headless `samples` times and print a summary.
    DumpSnapshot {
        samples: u32,
    },
    Help,
}

/// Parse the arguments after the program name.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let mut command = Command::App;
    let mut demo = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "--demo" => demo = true,
            "--dump-snapshot" => {
                let count = args.get(i + 1).filter(|next| !next.starts_with('-'));
                let samples = match count {
                    Some(text) => {
                        i += 1;
                        parse_count(text)?
                    }
                    None => DEFAULT_DUMP_SAMPLES,
                };
                command = Command::DumpSnapshot { samples };
            }
            // Finder may pass a process serial number to a launched app bundle.
            psn if psn.starts_with("-psn_") => {}
            other => return Err(format!("unknown argument {other:?}")),
        }
        i += 1;
    }
    if demo && command != Command::App {
        return Err(
            "--dump-snapshot samples this machine; it cannot be combined with --demo".into(),
        );
    }
    Ok(command)
}

/// A `--dump-snapshot` sample count: a whole number, at least 1.
fn parse_count(text: &str) -> Result<u32, String> {
    match text.parse::<u32>() {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err(format!(
            "--dump-snapshot takes a sample count of 1 or more, not {text:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_arguments_or_demo_open_the_app() {
        assert_eq!(parse(&args(&[])), Ok(Command::App));
        assert_eq!(parse(&args(&["--demo"])), Ok(Command::App));
        assert_eq!(parse(&args(&["-psn_0_1234567"])), Ok(Command::App));
    }

    #[test]
    fn dump_snapshot_takes_an_optional_count() {
        assert_eq!(
            parse(&args(&["--dump-snapshot"])),
            Ok(Command::DumpSnapshot {
                samples: DEFAULT_DUMP_SAMPLES
            })
        );
        assert_eq!(
            parse(&args(&["--dump-snapshot", "3"])),
            Ok(Command::DumpSnapshot { samples: 3 })
        );
        assert!(parse(&args(&["--dump-snapshot", "--demo"])).is_err());
        assert!(parse(&args(&["--dump-snapshot", "0"])).is_err());
        assert!(parse(&args(&["--dump-snapshot", "many"])).is_err());
    }

    #[test]
    fn help_and_unknown_arguments() {
        assert_eq!(parse(&args(&["--help"])), Ok(Command::Help));
        assert_eq!(
            parse(&args(&["--dump-snapshot", "2", "-h"])),
            Ok(Command::Help)
        );
        let err = parse(&args(&["--pro"])).expect_err("unknown");
        assert!(err.contains("--pro"));
    }
}
