//! Parser for the buffer Windows' `GetLogicalProcessorInformationEx(RelationCache)` fills:
//! a packed run of variable-size `SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX` records, one per
//! cache instance.
//!
//! The native Windows platform makes the call and hands the bytes here, so the walk (which
//! must trust each record's `Size` and never read past the buffer) is pure and tested on
//! every OS. Layout (little-endian, from the Windows SDK):
//!
//! ```text
//! +0  Relationship: i32   (RelationCache = 2)
//! +4  Size: u32           (bytes in this record, header included)
//! +8  CACHE_RELATIONSHIP: Level u8, Associativity u8, LineSize u16,
//! +12                     CacheSize u32,
//! +16                     Type i32 (0 unified, 1 instruction, 2 data, 3 trace)
//! ```

use super::sysfs::CacheTotals;

/// `LOGICAL_PROCESSOR_RELATIONSHIP::RelationCache`.
const RELATION_CACHE: i32 = 2;
/// `PROCESSOR_CACHE_TYPE::CacheTrace`: not a data cache; not counted.
const CACHE_TRACE: i32 = 3;
/// Bytes up to and including the cache `Type` field.
const MIN_CACHE_RECORD: usize = 20;

/// Sum the cache records in `buf` per level (L1 adds data and instruction caches). Stops
/// at the first malformed record (a `Size` too small or running past the buffer).
pub fn parse_logical_processor_caches(buf: &[u8]) -> CacheTotals {
    let read_i32 = |at: usize| -> Option<i32> {
        Some(i32::from_le_bytes(buf.get(at..at + 4)?.try_into().ok()?))
    };
    let read_u32 = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(buf.get(at..at + 4)?.try_into().ok()?))
    };
    let mut totals = CacheTotals::default();
    let mut offset = 0usize;
    while offset
        .checked_add(8)
        .is_some_and(|header_end| header_end <= buf.len())
    {
        let (Some(relationship), Some(size)) = (read_i32(offset), read_u32(offset + 4)) else {
            break;
        };
        let size = size as usize;
        let record_end = offset.checked_add(size);
        if size < 8 || record_end.is_none_or(|end| end > buf.len()) {
            break;
        }
        if relationship == RELATION_CACHE && size >= MIN_CACHE_RECORD {
            let level = buf[offset + 8];
            let cache_size = read_u32(offset + 12).unwrap_or(0) as u64;
            let kind = read_i32(offset + 16).unwrap_or(CACHE_TRACE);
            let slot = match level {
                1 => Some(&mut totals.l1),
                2 => Some(&mut totals.l2),
                3 => Some(&mut totals.l3),
                _ => None,
            };
            if let Some(slot) = slot.filter(|_| kind != CACHE_TRACE) {
                *slot = Some(slot.unwrap_or(0) + cache_size);
            }
        }
        offset += size;
    }
    totals
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One cache record as Windows lays it out (48 bytes, like the real thing).
    fn record(level: u8, size: u32, kind: i32) -> Vec<u8> {
        let mut r = Vec::new();
        r.extend_from_slice(&RELATION_CACHE.to_le_bytes());
        r.extend_from_slice(&48u32.to_le_bytes());
        r.push(level);
        r.push(8); // associativity
        r.extend_from_slice(&64u16.to_le_bytes()); // line size
        r.extend_from_slice(&size.to_le_bytes());
        r.extend_from_slice(&kind.to_le_bytes());
        r.resize(48, 0);
        r
    }

    #[test]
    fn sums_each_cache_instance_by_level() {
        let mut buf = Vec::new();
        for _core in 0..2 {
            buf.extend(record(1, 48 * 1024, 2));
            buf.extend(record(1, 32 * 1024, 1));
            buf.extend(record(2, 1280 * 1024, 0));
        }
        buf.extend(record(3, 24 << 20, 0));
        buf.extend(record(1, 12 * 1024, CACHE_TRACE));
        let totals = parse_logical_processor_caches(&buf);
        assert_eq!(totals.l1, Some(2 * (48 + 32) * 1024));
        assert_eq!(totals.l2, Some(2 * 1280 * 1024));
        assert_eq!(totals.l3, Some(24 << 20));
    }

    #[test]
    fn malformed_records_stop_the_walk_without_overrunning() {
        let mut buf = record(2, 4 << 20, 0);
        let mut bad = record(3, 8 << 20, 0);
        bad[4..8].copy_from_slice(&4000u32.to_le_bytes()); // runs past the end
        buf.extend(bad);
        let totals = parse_logical_processor_caches(&buf);
        assert_eq!(totals.l2, Some(4 << 20));
        assert_eq!(totals.l3, None);
        assert_eq!(parse_logical_processor_caches(&[]), CacheTotals::default());
        assert_eq!(
            parse_logical_processor_caches(&[2, 0, 0]),
            CacheTotals::default()
        );
        let mut zero = record(1, 1, 2);
        zero[4..8].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            parse_logical_processor_caches(&zero),
            CacheTotals::default()
        );
    }
}
