#![allow(clippy::len_without_is_empty)]
#![forbid(unsafe_code)]
use std::error::Error;
use std::fmt;

/// First-party histogram implementation that keeps a bounded sample window.
#[derive(Clone, Debug)]
pub struct Histogram {
    min: u64,
    max: u64,
    samples: Vec<u64>,
    sorted: bool,
}

impl Histogram {
    pub fn new_with_bounds(min: u64, max: u64, _sigfig: u8) -> Result<Self, HistogramError> {
        if min > max {
            return Err(HistogramError::invalid_bounds(min, max));
        }
        Ok(Self {
            min,
            max,
            samples: Vec::new(),
            sorted: true,
        })
    }

    pub fn record(&mut self, value: u64) -> Result<(), HistogramError> {
        if value < self.min || value > self.max {
            return Err(HistogramError::out_of_range(value, self.min, self.max));
        }
        self.samples.push(value);
        self.sorted = false;
        Ok(())
    }

    pub fn reset(&mut self) {
        self.samples.clear();
        self.sorted = true;
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn value_at_percentile(&mut self, percentile: f64) -> u64 {
        if self.samples.is_empty() {
            return self.min;
        }

        if !self.sorted {
            self.samples.sort_unstable();
            self.sorted = true;
        }

        let percentile = percentile.clamp(0.0, 100.0);
        let rank = if self.samples.len() == 1 {
            0
        } else {
            let max_index = self.samples.len() - 1;
            let position = percentile / 100.0 * max_index as f64;
            position.round() as usize
        };
        self.samples.get(rank).cloned().unwrap_or(self.samples[0])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistogramError {
    kind: HistogramErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HistogramErrorKind {
    InvalidBounds { min: u64, max: u64 },
    OutOfRange { value: u64, min: u64, max: u64 },
}

impl HistogramError {
    fn invalid_bounds(min: u64, max: u64) -> Self {
        Self {
            kind: HistogramErrorKind::InvalidBounds { min, max },
        }
    }

    fn out_of_range(value: u64, min: u64, max: u64) -> Self {
        Self {
            kind: HistogramErrorKind::OutOfRange { value, min, max },
        }
    }
}

impl fmt::Display for HistogramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            HistogramErrorKind::InvalidBounds { min, max } => {
                write!(f, "invalid histogram bounds: min={min} max={max}")
            }
            HistogramErrorKind::OutOfRange { value, min, max } => {
                write!(f, "value {value} outside histogram range [{min}, {max}]")
            }
        }
    }
}

impl Error for HistogramError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_bounds() {
        let err = Histogram::new_with_bounds(10, 5, 3).unwrap_err();
        assert_eq!(err.to_string(), "invalid histogram bounds: min=10 max=5");
    }

    #[test]
    fn records_and_reports_percentiles() {
        let mut hist = Histogram::new_with_bounds(1, 10_000, 3).unwrap();
        hist.record(10).unwrap();
        hist.record(20).unwrap();
        hist.record(30).unwrap();

        assert_eq!(hist.len(), 3);
        assert_eq!(hist.value_at_percentile(0.0), 10);
        assert_eq!(hist.value_at_percentile(50.0), 20);
        assert_eq!(hist.value_at_percentile(100.0), 30);
    }

    #[test]
    fn out_of_range_rejected() {
        let mut hist = Histogram::new_with_bounds(5, 10, 2).unwrap();
        let err = hist.record(4).unwrap_err();
        assert_eq!(err.to_string(), "value 4 outside histogram range [5, 10]");
    }

    #[test]
    fn reset_clears_samples() {
        let mut hist = Histogram::new_with_bounds(1, 100, 2).unwrap();
        hist.record(50).unwrap();
        hist.reset();
        assert_eq!(hist.len(), 0);
        assert_eq!(hist.value_at_percentile(50.0), 1);
    }
}
