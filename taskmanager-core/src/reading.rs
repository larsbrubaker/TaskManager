//! `Reading<T>`: one measurement, or the honest reason there is none.
//!
//! The charter's rule is that a measuring instrument never makes up an answer, so every
//! value a provider reports (CPU load, a temperature, a disk rate) is a `Reading`. Widgets
//! show a value only for [`Reading::Value`]; an [`Reading::Unavailable`] reading is a gap
//! (no line drawn through it, no zero shown) and [`Reading::Unsupported`] means this machine
//! or OS cannot provide the metric at all, so the UI may hide it. See PLAN §4.1.

/// A measured value, or why there is none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Reading<T> {
    /// The metric was measured.
    Value(T),
    /// The metric exists here but could not be measured this time (permission denied,
    /// sensor busy, first sample of a rate). The default: nothing measured yet.
    #[default]
    Unavailable,
    /// This machine or OS has no such metric.
    Unsupported,
}

impl<T> Reading<T> {
    /// Transform the value, keeping `Unavailable` / `Unsupported` as they are.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Reading<U> {
        match self {
            Reading::Value(v) => Reading::Value(f(v)),
            Reading::Unavailable => Reading::Unavailable,
            Reading::Unsupported => Reading::Unsupported,
        }
    }

    /// Chain a computation that may itself fail to produce a value.
    pub fn and_then<U>(self, f: impl FnOnce(T) -> Reading<U>) -> Reading<U> {
        match self {
            Reading::Value(v) => f(v),
            Reading::Unavailable => Reading::Unavailable,
            Reading::Unsupported => Reading::Unsupported,
        }
    }

    /// Borrow the value: `Reading<&T>`.
    pub fn as_ref(&self) -> Reading<&T> {
        match self {
            Reading::Value(v) => Reading::Value(v),
            Reading::Unavailable => Reading::Unavailable,
            Reading::Unsupported => Reading::Unsupported,
        }
    }

    /// The value, if one was measured.
    pub fn value(&self) -> Option<&T> {
        match self {
            Reading::Value(v) => Some(v),
            Reading::Unavailable | Reading::Unsupported => None,
        }
    }

    /// The value by move, if one was measured.
    pub fn into_value(self) -> Option<T> {
        match self {
            Reading::Value(v) => Some(v),
            Reading::Unavailable | Reading::Unsupported => None,
        }
    }

    /// Whether a value was measured.
    pub fn is_value(&self) -> bool {
        matches!(self, Reading::Value(_))
    }

    /// Whether the metric exists but was not measured this time.
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Reading::Unavailable)
    }

    /// Whether the metric does not exist on this machine.
    pub fn is_unsupported(&self) -> bool {
        matches!(self, Reading::Unsupported)
    }
}

impl<T> From<Option<T>> for Reading<T> {
    /// `Some(v)` is a value; `None` is `Unavailable` (never `Unsupported`: a missing sample
    /// does not prove the metric cannot exist).
    fn from(value: Option<T>) -> Self {
        match value {
            Some(v) => Reading::Value(v),
            None => Reading::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_unavailable_never_a_zero() {
        let r: Reading<f64> = Reading::default();
        assert!(r.is_unavailable());
        assert_eq!(r.value(), None);
    }

    #[test]
    fn map_transforms_only_values() {
        assert_eq!(Reading::Value(2).map(|v| v * 10), Reading::Value(20));
        assert_eq!(
            Reading::<i32>::Unavailable.map(|v| v * 10),
            Reading::Unavailable
        );
        assert_eq!(
            Reading::<i32>::Unsupported.map(|v| v * 10),
            Reading::Unsupported
        );
    }

    #[test]
    fn and_then_can_turn_a_value_into_a_gap() {
        let ratio = |v: f64| {
            if v > 0.0 {
                Reading::Value(1.0 / v)
            } else {
                Reading::Unavailable
            }
        };
        assert_eq!(Reading::Value(4.0).and_then(ratio), Reading::Value(0.25));
        assert_eq!(Reading::Value(0.0).and_then(ratio), Reading::Unavailable);
        assert_eq!(Reading::Unsupported.and_then(ratio), Reading::Unsupported);
    }

    #[test]
    fn accessors_report_the_variant() {
        let v = Reading::Value(String::from("37.5"));
        assert!(v.is_value() && !v.is_unavailable() && !v.is_unsupported());
        assert_eq!(v.value().map(String::as_str), Some("37.5"));
        assert_eq!(v.as_ref().map(|s| s.len()), Reading::Value(4));
        assert_eq!(v.into_value(), Some(String::from("37.5")));

        let u: Reading<u8> = Reading::Unsupported;
        assert!(u.is_unsupported() && !u.is_value() && !u.is_unavailable());
        assert_eq!(u.into_value(), None);
    }

    #[test]
    fn option_none_is_unavailable_not_unsupported() {
        assert_eq!(Reading::from(Some(3)), Reading::Value(3));
        assert_eq!(Reading::<i32>::from(None), Reading::Unavailable);
    }
}
