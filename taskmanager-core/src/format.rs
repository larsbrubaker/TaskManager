//! Human-readable formatting for every number the UI shows: byte sizes, transfer rates,
//! percentages, temperatures and durations. Pure functions, unit-tested here; the widgets
//! and pages call them with values taken out of a `Reading` (a missing reading is never
//! formatted as zero; see `reading.rs`).
//!
//! The forms follow the primary reference (the macOS captures in `reference/app/`):
//!
//! - Sizes use base-1024 units (`KB` = 1024 bytes): whole bytes below one kilobyte
//!   (`512 B`, `0 B`), otherwise exactly one decimal (`176.0 KB`, `521.5 MB`, `2.2 GB`).
//! - Rates end in `/sec`: whole bytes below one kilobyte (`0 B/sec`), otherwise three
//!   significant figures (`9.93 KB/sec`, `35.8 KB/sec`, `142 KB/sec`, `0.98 MB/sec`).
//! - Durations read `16 h 41 min`; uptimes `5:14:51:19` (days:hours:minutes:seconds).

use std::time::Duration;

/// Size units above bytes, each 1024× the previous.
const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];

/// Bytes in one kilobyte.
const KB: f64 = 1024.0;

/// Printed rate values stay below this; a value that would round up to it moves to the
/// next unit (`1000 KB/sec` reads `0.98 MB/sec`), keeping three significant figures.
const RATE_UNIT_LIMIT: f64 = 1000.0;

/// Text shown for a number that cannot be printed (NaN, infinite or negative input).
pub const NOT_A_NUMBER: &str = "—";

/// Round `value` to `decimals` places, as `format!` would print it.
fn round_to(value: f64, decimals: usize) -> f64 {
    let scale = 10f64.powi(decimals as i32);
    (value * scale).round() / scale
}

/// `value`, or exactly `0.0` when it prints as zero at one decimal, so tiny negative
/// noise never shows as `-0.0`.
fn zero_if_rounds_to_zero(value: f64) -> f64 {
    if round_to(value, 1) == 0.0 {
        0.0
    } else {
        value
    }
}

/// Decimal places that print `value` with three significant figures: two below 10, one
/// below 100, none from 100 up (judged on the rounded value, so 9.996 prints `10.0`).
fn significant3_decimals(value: f64) -> usize {
    if round_to(value, 2) < 10.0 {
        2
    } else if round_to(value, 1) < 100.0 {
        1
    } else {
        0
    }
}

/// A byte count: `"0 B"`, `"512 B"`, `"176.0 KB"`, `"521.5 MB"`, `"2.2 GB"`. A value that
/// would round up to `1024.0` of a unit is shown in the next one; terabytes are the
/// largest unit.
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / KB;
    let mut unit = 0;
    while unit + 1 < UNITS.len() && round_to(value, 1) >= KB {
        value /= KB;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// A transfer rate in bytes per second: whole bytes below one kilobyte (`"0 B/sec"`,
/// `"512 B/sec"`), then three significant figures (`"9.93 KB/sec"`, `"35.8 KB/sec"`,
/// `"142 KB/sec"`, `"73.4 MB/sec"`). A value that would print as 1000 or more moves to the
/// next unit (`"0.98 MB/sec"`); terabytes are the largest unit. Negative rates (a counter
/// that went backwards) and non-finite ones print [`NOT_A_NUMBER`]: the caller should have
/// dropped such a sample.
pub fn format_rate(bytes_per_sec: f64) -> String {
    if !bytes_per_sec.is_finite() || bytes_per_sec < 0.0 {
        return NOT_A_NUMBER.to_string();
    }
    if bytes_per_sec.round() < RATE_UNIT_LIMIT {
        return format!("{:.0} B/sec", bytes_per_sec.round());
    }
    let mut value = bytes_per_sec / KB;
    let mut unit = 0;
    while unit + 1 < UNITS.len() && round_to(value, significant3_decimals(value)) >= RATE_UNIT_LIMIT
    {
        value /= KB;
        unit += 1;
    }
    let decimals = significant3_decimals(value);
    format!("{value:.decimals$} {}/sec", UNITS[unit])
}

/// A percentage with one decimal: `15.8` → `"15.8%"`. Non-finite input prints
/// [`NOT_A_NUMBER`].
pub fn format_percent(percent: f64) -> String {
    if !percent.is_finite() {
        return NOT_A_NUMBER.to_string();
    }
    format!("{:.1}%", zero_if_rounds_to_zero(percent))
}

/// The unit temperatures are shown in (a setting; sensors always report Celsius).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TemperatureUnit {
    #[default]
    Celsius,
    Fahrenheit,
}

impl TemperatureUnit {
    /// The unit's symbol, with the degree sign.
    pub fn symbol(self) -> &'static str {
        match self {
            TemperatureUnit::Celsius => "°C",
            TemperatureUnit::Fahrenheit => "°F",
        }
    }
}

/// A temperature measured in degrees Celsius, shown in `unit` with one decimal:
/// `"37.5 °C"`, `"99.5 °F"`. Non-finite input prints [`NOT_A_NUMBER`].
pub fn format_temperature(celsius: f64, unit: TemperatureUnit) -> String {
    if !celsius.is_finite() {
        return NOT_A_NUMBER.to_string();
    }
    let value = match unit {
        TemperatureUnit::Celsius => celsius,
        TemperatureUnit::Fahrenheit => celsius * 9.0 / 5.0 + 32.0,
    };
    format!("{:.1} {}", zero_if_rounds_to_zero(value), unit.symbol())
}

/// An uptime as `days:hours:minutes:seconds`, the fields after days zero-padded:
/// `"5:14:51:19"`, `"0:00:03:07"`. Sub-second parts are dropped.
pub fn format_uptime(uptime: Duration) -> String {
    let secs = uptime.as_secs();
    let (d, h, m, s) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    format!("{d}:{h:02}:{m:02}:{s:02}")
}

/// A duration in its two largest units, as the process details show it: `"2 d 3 h"`,
/// `"16 h 41 min"`, `"8 min 5 s"`, `"42 s"`. Sub-second parts are dropped.
pub fn format_duration_short(duration: Duration) -> String {
    let secs = duration.as_secs();
    let (d, h, m, s) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 {
        format!("{d} d {h} h")
    } else if h > 0 {
        format!("{h} h {m} min")
    } else if m > 0 {
        format!("{m} min {s} s")
    } else {
        format!("{s} s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * MB;

    #[test]
    fn sizes_below_a_kilobyte_are_whole_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn sizes_have_exactly_one_decimal() {
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(176 * 1024), "176.0 KB");
        assert_eq!(format_bytes(546_832_384), "521.5 MB");
        assert_eq!(format_bytes(2_362_232_013), "2.2 GB");
        assert_eq!(format_bytes(24 * GB), "24.0 GB");
    }

    #[test]
    fn sizes_that_round_to_1024_move_to_the_next_unit() {
        assert_eq!(format_bytes(MB - 1), "1.0 MB");
        assert_eq!(format_bytes(1023 * MB + 1000 * 1024), "1.0 GB");
        assert_eq!(format_bytes(1023 * MB), "1023.0 MB");
    }

    #[test]
    fn terabytes_are_the_largest_unit() {
        assert_eq!(format_bytes(2 * 1024 * GB), "2.0 TB");
        assert_eq!(format_bytes(2048 * 1024 * GB), "2048.0 TB");
    }

    #[test]
    fn rates_below_a_kilobyte_are_whole_bytes() {
        assert_eq!(format_rate(0.0), "0 B/sec");
        assert_eq!(format_rate(0.2), "0 B/sec");
        assert_eq!(format_rate(0.6), "1 B/sec");
        assert_eq!(format_rate(512.0), "512 B/sec");
        assert_eq!(format_rate(999.4), "999 B/sec");
    }

    #[test]
    fn rates_have_three_significant_figures() {
        assert_eq!(format_rate(9.93 * 1024.0), "9.93 KB/sec");
        assert_eq!(format_rate(35.8 * 1024.0), "35.8 KB/sec");
        assert_eq!(format_rate(142.0 * 1024.0), "142 KB/sec");
        assert_eq!(format_rate(73.4 * MB as f64), "73.4 MB/sec");
        assert_eq!(format_rate(222.0 * MB as f64), "222 MB/sec");
        assert_eq!(format_rate(1.5 * GB as f64), "1.50 GB/sec");
    }

    #[test]
    fn rates_that_round_up_a_place_drop_a_decimal() {
        assert_eq!(format_rate(9.996 * 1024.0), "10.0 KB/sec");
        assert_eq!(format_rate(99.96 * 1024.0), "100 KB/sec");
    }

    #[test]
    fn rates_move_up_a_unit_before_printing_1000() {
        assert_eq!(format_rate(999.4 * 1024.0), "999 KB/sec");
        assert_eq!(format_rate(1000.0 * 1024.0), "0.98 MB/sec");
        assert_eq!(format_rate(1023.7 * 1024.0), "1.00 MB/sec");
        // Bytes follow the same rule: 1000 B/sec never prints four digits.
        assert_eq!(format_rate(1000.0), "0.98 KB/sec");
        assert_eq!(format_rate(5000.0 * GB as f64), "4.88 TB/sec");
        assert_eq!(format_rate(5000.0 * 1024.0 * GB as f64), "5000 TB/sec");
    }

    #[test]
    fn impossible_rates_are_not_printed_as_numbers() {
        assert_eq!(format_rate(-1.0), "—");
        assert_eq!(format_rate(f64::NAN), "—");
        assert_eq!(format_rate(f64::INFINITY), "—");
    }

    #[test]
    fn percentages_have_one_decimal() {
        assert_eq!(format_percent(15.8), "15.8%");
        assert_eq!(format_percent(100.0), "100.0%");
        assert_eq!(format_percent(0.04), "0.0%");
        assert_eq!(format_percent(-0.01), "0.0%");
        assert_eq!(format_percent(f64::NAN), "—");
    }

    #[test]
    fn temperatures_in_celsius_and_fahrenheit() {
        assert_eq!(
            format_temperature(37.5, TemperatureUnit::Celsius),
            "37.5 °C"
        );
        assert_eq!(
            format_temperature(37.5, TemperatureUnit::Fahrenheit),
            "99.5 °F"
        );
        assert_eq!(
            format_temperature(-40.0, TemperatureUnit::Fahrenheit),
            "-40.0 °F"
        );
        assert_eq!(
            format_temperature(-0.01, TemperatureUnit::Celsius),
            "0.0 °C"
        );
        assert_eq!(
            format_temperature(f64::INFINITY, TemperatureUnit::Celsius),
            "—"
        );
    }

    #[test]
    fn uptime_reads_days_hours_minutes_seconds() {
        let uptime = Duration::from_secs(5 * 86_400 + 14 * 3600 + 51 * 60 + 19);
        assert_eq!(format_uptime(uptime), "5:14:51:19");
        assert_eq!(format_uptime(Duration::from_millis(187_900)), "0:00:03:07");
        assert_eq!(
            format_uptime(Duration::from_secs(400 * 86_400)),
            "400:00:00:00"
        );
    }

    #[test]
    fn short_durations_show_the_two_largest_units() {
        assert_eq!(
            format_duration_short(Duration::from_secs(16 * 3600 + 41 * 60 + 30)),
            "16 h 41 min"
        );
        assert_eq!(
            format_duration_short(Duration::from_secs(19 * 3600 + 8 * 60 + 59)),
            "19 h 8 min"
        );
        assert_eq!(
            format_duration_short(Duration::from_secs(2 * 86_400 + 3 * 3600 + 7)),
            "2 d 3 h"
        );
        assert_eq!(format_duration_short(Duration::from_secs(485)), "8 min 5 s");
        assert_eq!(format_duration_short(Duration::from_millis(42_900)), "42 s");
        assert_eq!(format_duration_short(Duration::ZERO), "0 s");
    }
}
