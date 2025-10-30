use foundation_metrics::{label_key, label_value, LabelPair};

/// Collection of telemetry labels accumulated while recording structured data.
#[derive(Clone, Debug, Default)]
pub struct Labels {
    entries: Vec<LabelPair>,
}

impl Labels {
    /// Construct an empty label set.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Insert a telemetry label using the provided key/value pair.
    pub fn insert(&mut self, key: impl ToString, value: impl ToString) {
        self.entries.push((label_key(key), label_value(value)));
    }

    /// Extend the current label set with another [`Labels`] instance.
    pub fn extend(&mut self, other: Self) {
        self.entries.extend(other.entries);
    }

    /// Borrow the accumulated labels as a slice.
    pub fn as_slice(&self) -> &[(String, String)] {
        &self.entries
    }

    /// Consume the label set and return the owned label pairs.
    pub fn into_pairs(self) -> Vec<LabelPair> {
        self.entries
    }
}

impl IntoIterator for Labels {
    type Item = LabelPair;
    type IntoIter = std::vec::IntoIter<LabelPair>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

/// Trait implemented by types that can record themselves into telemetry labels.
pub trait Recordable {
    /// Record telemetry labels for the receiver into the provided [`Labels`] set.
    fn record(&self, labels: &mut Labels);
}

/// Render the provided [`Recordable`] into a vector of telemetry label pairs.
pub fn record<R: Recordable>(value: &R) -> Vec<LabelPair> {
    let mut labels = Labels::new();
    value.record(&mut labels);
    labels.into_pairs()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyRecorder<'a> {
        key: &'a str,
        value: &'a str,
    }

    impl<'a> Recordable for DummyRecorder<'a> {
        fn record(&self, labels: &mut Labels) {
            labels.insert(self.key, self.value);
        }
    }

    #[test]
    fn record_trait_collects_labels() {
        let recorder = DummyRecorder {
            key: "component",
            value: "crypto",
        };
        let labels = record(&recorder);
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].0, "component");
        assert_eq!(labels[0].1, "crypto");
    }

    #[test]
    fn labels_extend_merges_pairs() {
        let mut base = Labels::new();
        base.insert("scope", "base");
        let mut extra = Labels::new();
        extra.insert("scope", "extra");
        base.extend(extra);
        assert_eq!(base.into_pairs().len(), 2);
    }
}
