//! `TASKMANAGER_STATS=1`: per-sample timing of the native provider (CLAUDE.md
//! "Performance": measure first). When enabled, each `NativeProvider::sample` prints one
//! line to stderr with the wall time of every phase and the process CPU time the whole
//! sample consumed, e.g.
//!
//! ```text
//! sampler: 61.2 ms wall, 7.9 ms CPU | cpu 0.1  memory 0.0  threads 0.1  processes 9.8  disks 1.5  networks 0.3  sensors 49.4
//! ```
//!
//! The CPU figure is the process's CPU time (`os::process_cpu_time`) across the sample;
//! in the windowed app other threads' work during those milliseconds is included.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use super::os;

/// The environment variable that turns the timing on.
pub const STATS_ENV: &str = "TASKMANAGER_STATS";

/// Whether `TASKMANAGER_STATS` asks for timing (`1`, `true`, `yes`, `on`).
pub fn stats_enabled(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// Phase timer for one sample; inert when disabled.
pub struct SampleTimer {
    started: Option<(Instant, Option<Duration>)>,
    last: Option<Instant>,
    phases: Vec<(&'static str, Duration)>,
}

impl SampleTimer {
    /// Start timing a sample (`enabled == false`: every call is a no-op).
    pub fn start(enabled: bool) -> Self {
        let now = Instant::now();
        Self {
            started: enabled.then(|| (now, os::process_cpu_time())),
            last: enabled.then_some(now),
            phases: Vec::new(),
        }
    }

    /// The phase called `name` ended now.
    pub fn mark(&mut self, name: &'static str) {
        if let Some(last) = self.last.as_mut() {
            let now = Instant::now();
            self.phases.push((name, now - *last));
            *last = now;
        }
    }

    /// Print the line, if enabled.
    pub fn finish(self) {
        if let Some(line) = self.line() {
            eprintln!("{line}");
        }
    }

    fn line(&self) -> Option<String> {
        let (started, cpu_at_start) = self.started?;
        let wall = started.elapsed();
        let cpu = cpu_at_start
            .zip(os::process_cpu_time())
            .map(|(a, b)| b.saturating_sub(a));
        Some(format_line(wall, cpu, &self.phases))
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// The stats line for a sample.
pub fn format_line(wall: Duration, cpu: Option<Duration>, phases: &[(&str, Duration)]) -> String {
    let mut line = format!("sampler: {:.1} ms wall, ", ms(wall));
    match cpu {
        Some(cpu) => {
            let _ = write!(line, "{:.1} ms CPU |", ms(cpu));
        }
        None => line.push_str("CPU unavailable |"),
    }
    for (name, took) in phases {
        let _ = write!(line, " {name} {:.1}", ms(*took));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_values_turn_it_on() {
        assert!(stats_enabled(Some("1")) && stats_enabled(Some(" Yes ")));
        assert!(!stats_enabled(Some("0")) && !stats_enabled(Some("")) && !stats_enabled(None));
    }

    #[test]
    fn the_line_lists_wall_cpu_and_phases() {
        let line = format_line(
            Duration::from_micros(61_240),
            Some(Duration::from_micros(7_900)),
            &[
                ("processes", Duration::from_micros(9_800)),
                ("sensors", Duration::from_millis(49)),
            ],
        );
        assert_eq!(
            line,
            "sampler: 61.2 ms wall, 7.9 ms CPU | processes 9.8 sensors 49.0"
        );
        assert!(format_line(Duration::ZERO, None, &[]).contains("CPU unavailable"));
    }

    #[test]
    fn a_disabled_timer_prints_nothing() {
        let mut timer = SampleTimer::start(false);
        timer.mark("cpu");
        assert_eq!(timer.line(), None);
        let mut timer = SampleTimer::start(true);
        timer.mark("cpu");
        assert!(timer.line().is_some_and(|l| l.contains(" cpu ")));
    }
}
