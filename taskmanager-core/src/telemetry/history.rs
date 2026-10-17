//! `History`: a fixed-capacity ring of readings for one series, plus the compressed view.
//!
//! The [`HistoryStore`](super::HistoryStore) keeps one `History` per graphed series (CPU
//! total, a disk's read rate, …) and pushes one [`Reading`] per sampler tick. Gaps are
//! kept as gaps: an `Unavailable` sample stays `Unavailable`, so graphs break the line
//! there instead of drawing through a made-up value (PLAN §4.1).
//!
//! [`History::compressed`] implements the "Compress older history" graph setting
//! (PLAN §4.2, §5.16): the newest samples are shown one per column, older stretches are
//! averaged into 2x, 4x and 8x buckets so a narrow graph still spans a long time. Buckets
//! are aligned to the absolute sample count (not to the newest sample), so a sample stays
//! in the same bucket as time advances and compressed graphs scroll without shimmering.

use std::collections::VecDeque;

use crate::reading::Reading;

/// Samples kept per series by default: an hour at the default 1 s interval.
pub const DEFAULT_HISTORY_CAPACITY: usize = 3600;

/// The most regions [`History::compressed`] uses: 1x, 2x, 4x and 8x.
pub const MAX_COMPRESSION_REGIONS: usize = 4;

/// A fixed-capacity ring of readings, oldest dropped first.
#[derive(Clone, Debug, PartialEq)]
pub struct History {
    samples: VecDeque<Reading<f32>>,
    capacity: usize,
    /// Samples ever pushed (including padding), i.e. the absolute index of the next one.
    total: u64,
}

impl Default for History {
    fn default() -> Self {
        Self::new(DEFAULT_HISTORY_CAPACITY)
    }
}

impl History {
    /// An empty history holding at most `capacity` samples (at least one).
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            samples: VecDeque::with_capacity(capacity),
            capacity,
            total: 0,
        }
    }

    /// Append the newest sample, dropping the oldest when full.
    pub fn push(&mut self, reading: Reading<f32>) {
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(reading);
        self.total += 1;
    }

    /// Append `count` `Unavailable` samples: the time before a series existed, so it lines
    /// up (and buckets align) with series that were recorded all along.
    pub fn pad_unavailable(&mut self, count: u64) {
        let stored = count.min(self.capacity as u64);
        for _ in 0..stored {
            self.push(Reading::Unavailable);
        }
        self.total += count - stored;
    }

    /// Samples held.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether nothing has been pushed yet.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The most samples held.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Samples ever pushed, including ones since dropped.
    pub fn total_pushed(&self) -> u64 {
        self.total
    }

    /// The newest sample, or `None` before the first push.
    pub fn latest(&self) -> Option<Reading<f32>> {
        self.samples.back().copied()
    }

    /// The samples, oldest to newest.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = Reading<f32>> + ExactSizeIterator + '_ {
        self.samples.iter().copied()
    }

    /// The measured values, oldest to newest, skipping gaps.
    pub fn values(&self) -> impl DoubleEndedIterator<Item = f32> + '_ {
        self.samples.iter().filter_map(|r| r.value().copied())
    }

    /// The smallest measured value held, or `None` if there is none.
    pub fn min(&self) -> Option<f32> {
        self.values().reduce(f32::min)
    }

    /// The largest measured value held, or `None` if there is none.
    pub fn max(&self) -> Option<f32> {
        self.values().reduce(f32::max)
    }

    /// The compressed view for a graph `width` columns wide, oldest to newest.
    ///
    /// `width` is split into `regions` equal parts (clamped to
    /// 1..=[`MAX_COMPRESSION_REGIONS`]; any remainder goes to the newest region). Counting
    /// back from the newest sample, region 0 shows one sample per column, region 1 two,
    /// region 2 four and region 3 eight. Each bucket covers absolute sample indices
    /// `[k·f, (k+1)·f)` for its factor `f`; the newest bucket of a region may be partial
    /// where it meets the finer region in front of it.
    ///
    /// A bucket is the mean of its measured values; with none it is `Unsupported` if every
    /// sample was `Unsupported`, otherwise `Unavailable`, so gaps survive compression.
    /// Columns before the oldest sample held are omitted, so the result can be shorter
    /// than `width`; graphs right-align it. `regions == 1` is the plain newest-`width`
    /// samples.
    pub fn compressed(&self, width: usize, regions: usize) -> Vec<Reading<f32>> {
        if width == 0 || self.samples.is_empty() {
            return Vec::new();
        }
        let regions = regions.clamp(1, MAX_COMPRESSION_REGIONS);
        let per_region = width / regions;
        let remainder = width % regions;
        let first = self.total - self.samples.len() as u64;

        let mut newest_first = Vec::with_capacity(width);
        let mut end = self.total;
        'regions: for region in 0..regions {
            let factor = 1u64 << region;
            let columns = per_region + if region == 0 { remainder } else { 0 };
            for _ in 0..columns {
                if end <= first {
                    break 'regions;
                }
                let start = ((end - 1) / factor) * factor;
                newest_first.push(self.bucket(start.max(first), end));
                end = start;
            }
        }
        newest_first.reverse();
        newest_first
    }

    /// The bucket value over absolute indices `[from, to)`, all of which are held.
    fn bucket(&self, from: u64, to: u64) -> Reading<f32> {
        let first = self.total - self.samples.len() as u64;
        let lo = (from - first) as usize;
        let hi = (to - first) as usize;
        let mut sum = 0.0f64;
        let mut count = 0u32;
        let mut all_unsupported = true;
        for reading in self.samples.range(lo..hi) {
            match reading {
                Reading::Value(v) => {
                    sum += f64::from(*v);
                    count += 1;
                    all_unsupported = false;
                }
                Reading::Unavailable => all_unsupported = false,
                Reading::Unsupported => {}
            }
        }
        if count > 0 {
            Reading::Value((sum / f64::from(count)) as f32)
        } else if all_unsupported {
            Reading::Unsupported
        } else {
            Reading::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(capacity: usize, values: impl IntoIterator<Item = f32>) -> History {
        let mut h = History::new(capacity);
        for v in values {
            h.push(Reading::Value(v));
        }
        h
    }

    fn vals(readings: &[Reading<f32>]) -> Vec<Option<f32>> {
        readings.iter().map(|r| r.value().copied()).collect()
    }

    fn range(n: u32) -> impl Iterator<Item = f32> {
        (0..n).map(|v| v as f32)
    }

    #[test]
    fn empty_history_has_nothing() {
        let h = History::default();
        assert_eq!(h.capacity(), DEFAULT_HISTORY_CAPACITY);
        assert!(h.is_empty());
        assert_eq!(h.latest(), None);
        assert_eq!(h.min(), None);
        assert_eq!(h.max(), None);
        assert!(h.compressed(100, 4).is_empty());
    }

    #[test]
    fn zero_capacity_holds_one() {
        let mut h = History::new(0);
        h.push(Reading::Value(1.0));
        h.push(Reading::Value(2.0));
        assert_eq!(h.len(), 1);
        assert_eq!(h.latest(), Some(Reading::Value(2.0)));
    }

    #[test]
    fn wraparound_drops_the_oldest() {
        let h = filled(4, range(7));
        assert_eq!(h.len(), 4);
        assert_eq!(h.total_pushed(), 7);
        assert_eq!(h.latest(), Some(Reading::Value(6.0)));
        let order: Vec<f32> = h.values().collect();
        assert_eq!(order, vec![3.0, 4.0, 5.0, 6.0]);
        assert_eq!(h.iter().next_back(), Some(Reading::Value(6.0)));
        assert_eq!(h.iter().len(), 4);
    }

    #[test]
    fn min_max_ignore_gaps() {
        let mut h = History::new(8);
        h.push(Reading::Unavailable);
        assert_eq!(h.min(), None);
        h.push(Reading::Value(5.0));
        h.push(Reading::Unsupported);
        h.push(Reading::Value(-2.0));
        h.push(Reading::Unavailable);
        h.push(Reading::Value(9.5));
        assert_eq!(h.min(), Some(-2.0));
        assert_eq!(h.max(), Some(9.5));
        assert_eq!(h.latest(), Some(Reading::Value(9.5)));
    }

    #[test]
    fn min_max_forget_dropped_samples() {
        let h = filled(3, [100.0, 1.0, 2.0, 3.0]);
        assert_eq!(h.max(), Some(3.0));
        assert_eq!(h.min(), Some(1.0));
    }

    #[test]
    fn padding_counts_beyond_capacity() {
        let mut h = History::new(3);
        h.pad_unavailable(10);
        assert_eq!(h.len(), 3);
        assert_eq!(h.total_pushed(), 10);
        assert!(h.iter().all(|r| r.is_unavailable()));
        h.push(Reading::Value(1.0));
        assert_eq!(h.total_pushed(), 11);
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn one_region_is_the_newest_samples() {
        let h = filled(100, range(10));
        assert_eq!(
            vals(&h.compressed(4, 1)),
            vec![Some(6.0), Some(7.0), Some(8.0), Some(9.0)]
        );
        // Wider than the history: only the columns that have samples.
        assert_eq!(h.compressed(50, 1).len(), 10);
        assert!(h.compressed(0, 1).is_empty());
    }

    #[test]
    fn two_regions_average_older_pairs() {
        let h = filled(100, range(10));
        assert_eq!(
            vals(&h.compressed(4, 2)),
            vec![Some(4.5), Some(6.5), Some(8.0), Some(9.0)]
        );
    }

    #[test]
    fn buckets_are_aligned_so_samples_stay_put() {
        // One more sample: the 2x region's newest bucket is partial ([8]) and the pair
        // [6, 7] is the same bucket it was before.
        let h = filled(100, range(11));
        assert_eq!(
            vals(&h.compressed(4, 2)),
            vec![Some(6.5), Some(8.0), Some(9.0), Some(10.0)]
        );
        let h = filled(100, range(12));
        assert_eq!(
            vals(&h.compressed(4, 2)),
            vec![Some(6.5), Some(8.5), Some(10.0), Some(11.0)]
        );
    }

    #[test]
    fn four_regions_reach_8x() {
        let h = filled(100, range(64));
        assert_eq!(
            vals(&h.compressed(8, 4)),
            vec![
                Some(43.5), // 8x [40, 48)
                Some(49.5), // 8x [48, 52) partial
                Some(53.5), // 4x [52, 56)
                Some(56.5), // 4x [56, 58) partial
                Some(58.5), // 2x [58, 60)
                Some(60.5), // 2x [60, 62)
                Some(62.0),
                Some(63.0),
            ]
        );
    }

    #[test]
    fn remainder_columns_go_to_the_newest_region() {
        let h = filled(100, range(20));
        // width 5, 2 regions: 3 columns at 1x (19, 18, 17), 2 at 2x: the partial
        // bucket [16, 17) and the pair [14, 16).
        assert_eq!(
            vals(&h.compressed(5, 2)),
            vec![Some(14.5), Some(16.0), Some(17.0), Some(18.0), Some(19.0)]
        );
        // Narrower than the region count: the newest region takes everything.
        assert_eq!(vals(&h.compressed(2, 4)), vec![Some(18.0), Some(19.0)]);
    }

    #[test]
    fn region_count_is_clamped() {
        let h = filled(1000, range(200));
        assert_eq!(h.compressed(8, 0), h.compressed(8, 1));
        assert_eq!(h.compressed(8, 9), h.compressed(8, MAX_COMPRESSION_REGIONS));
    }

    #[test]
    fn compression_respects_wraparound() {
        // Holds 7..=11; the oldest 2x bucket [6, 8) only has 7 left.
        let h = filled(5, range(12));
        assert_eq!(
            vals(&h.compressed(4, 2)),
            vec![Some(7.0), Some(8.5), Some(10.0), Some(11.0)]
        );
        assert_eq!(
            vals(&h.compressed(10, 1)),
            vec![Some(7.0), Some(8.0), Some(9.0), Some(10.0), Some(11.0)]
        );
    }

    #[test]
    fn gaps_survive_compression() {
        let mut h = History::new(100);
        for r in [
            Reading::Value(1.0),  // 0 ┐ 2x bucket [0, 2): one value
            Reading::Unavailable, // 1 ┘
            Reading::Unavailable, // 2 ┐ 2x bucket [2, 4): no values
            Reading::Unavailable, // 3 ┘
            Reading::Value(4.0),  // 4
            Reading::Unavailable, // 5
        ] {
            h.push(r);
        }
        let c = h.compressed(4, 2);
        assert_eq!(
            c,
            vec![
                Reading::Value(1.0),
                Reading::Unavailable,
                Reading::Value(4.0),
                Reading::Unavailable,
            ]
        );
    }

    #[test]
    fn unsupported_passes_through_and_survives_buckets() {
        let mut h = History::new(100);
        for _ in 0..4 {
            h.push(Reading::Unsupported);
        }
        h.push(Reading::Unsupported);
        h.push(Reading::Unsupported);
        let c = h.compressed(4, 2);
        assert!(c.iter().all(|r| r.is_unsupported()), "{c:?}");

        // A bucket mixing Unsupported and Unavailable is a gap, not "unsupported".
        let mut h = History::new(100);
        h.push(Reading::Unsupported);
        h.push(Reading::Unavailable);
        h.push(Reading::Value(1.0));
        h.push(Reading::Value(2.0));
        assert_eq!(
            h.compressed(3, 2),
            vec![
                Reading::Unavailable,
                Reading::Value(1.0),
                Reading::Value(2.0)
            ]
        );
    }

    #[test]
    fn padded_series_align_with_full_ones() {
        let full = filled(100, range(10));
        let mut late = History::new(100);
        late.pad_unavailable(6);
        for v in 6..10 {
            late.push(Reading::Value(v as f32));
        }
        let a = full.compressed(4, 2);
        let b = late.compressed(4, 2);
        assert_eq!(a.len(), b.len());
        assert_eq!(b[0], Reading::Unavailable);
        assert_eq!(&a[1..], &b[1..]);
    }
}
