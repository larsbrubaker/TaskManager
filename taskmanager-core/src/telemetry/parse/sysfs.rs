//! Parsers for Linux `/sys` text: CPU cache descriptions
//! (`/sys/devices/system/cpu/cpu*/cache/index*/{level,type,size,shared_cpu_list}`) and
//! network interface link speed (`/sys/class/net/<if>/speed`).
//!
//! The native Linux platform reads the files; the parsing and the package totals
//! (`CpuInfo::l1`/`l2`/`l3` are totals across the package, and every CPU sharing a cache
//! lists it, so instances must be de-duplicated) are pure and tested here.

use std::collections::HashSet;

/// One `cache/indexN` directory as read for one CPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheEntry {
    /// `level`: 1, 2, 3.
    pub level: u8,
    /// `type`: `Data`, `Instruction` or `Unified`.
    pub kind: String,
    /// `size` in bytes.
    pub size: u64,
    /// `shared_cpu_list` (`0-3`, `0,8`): identifies the cache instance.
    pub shared_cpu_list: String,
}

/// Total cache per level across the package, bytes (`None`: no such level).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheTotals {
    pub l1: Option<u64>,
    pub l2: Option<u64>,
    pub l3: Option<u64>,
}

/// Parse a sysfs cache `size` (`32K`, `1024K`, `16M`, `512`) to bytes.
pub fn parse_cache_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let (digits, multiplier) = match text.chars().last()? {
        'K' | 'k' => (&text[..text.len() - 1], 1024),
        'M' | 'm' => (&text[..text.len() - 1], 1024 * 1024),
        'G' | 'g' => (&text[..text.len() - 1], 1024 * 1024 * 1024),
        _ => (text, 1),
    };
    digits.trim().parse::<u64>().ok()?.checked_mul(multiplier)
}

/// Sum each cache instance once (instances are identified by level, type and the CPUs that
/// share them). L1 adds data and instruction caches.
pub fn cache_totals(entries: &[CacheEntry]) -> CacheTotals {
    let mut seen = HashSet::new();
    let mut totals = CacheTotals::default();
    for entry in entries {
        if !seen.insert((
            entry.level,
            entry.kind.as_str(),
            entry.shared_cpu_list.as_str(),
        )) {
            continue;
        }
        let slot = match entry.level {
            1 => &mut totals.l1,
            2 => &mut totals.l2,
            3 => &mut totals.l3,
            _ => continue,
        };
        *slot = Some(slot.unwrap_or(0) + entry.size);
    }
    totals
}

/// Parse `/sys/class/net/<if>/speed` (megabits per second) to bits per second. The kernel
/// reports `-1` (or fails the read) when the link is down or the driver cannot tell.
pub fn parse_link_speed_bps(text: &str) -> Option<u64> {
    let mbps: i64 = text.trim().parse().ok()?;
    (mbps > 0).then(|| mbps as u64 * 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(level: u8, kind: &str, size: u64, shared: &str) -> CacheEntry {
        CacheEntry {
            level,
            kind: kind.into(),
            size,
            shared_cpu_list: shared.into(),
        }
    }

    #[test]
    fn sizes_parse_with_units() {
        assert_eq!(parse_cache_size("32K\n"), Some(32 * 1024));
        assert_eq!(parse_cache_size("16M"), Some(16 * 1024 * 1024));
        assert_eq!(parse_cache_size("1G"), Some(1 << 30));
        assert_eq!(parse_cache_size("512"), Some(512));
        assert_eq!(parse_cache_size("K"), None);
        assert_eq!(parse_cache_size(""), None);
        assert_eq!(parse_cache_size("lots"), None);
    }

    #[test]
    fn shared_caches_count_once() {
        // Two cores with SMT siblings (0,2) and (1,3): private L1d/L1i/L2, one shared L3.
        let mut entries = Vec::new();
        for cpu in 0..4 {
            let core = if cpu % 2 == 0 { "0,2" } else { "1,3" };
            entries.push(entry(1, "Data", 48 * 1024, core));
            entries.push(entry(1, "Instruction", 32 * 1024, core));
            entries.push(entry(2, "Unified", 1280 * 1024, core));
            entries.push(entry(3, "Unified", 24 << 20, "0-3"));
        }
        let totals = cache_totals(&entries);
        assert_eq!(totals.l1, Some(2 * (48 + 32) * 1024));
        assert_eq!(totals.l2, Some(2 * 1280 * 1024));
        assert_eq!(totals.l3, Some(24 << 20));
        assert_eq!(cache_totals(&[]), CacheTotals::default());
    }

    #[test]
    fn link_speed_is_megabits_or_unknown() {
        assert_eq!(parse_link_speed_bps("1000\n"), Some(1_000_000_000));
        assert_eq!(parse_link_speed_bps("-1\n"), None);
        assert_eq!(parse_link_speed_bps("0"), None);
        assert_eq!(parse_link_speed_bps(""), None);
    }
}
