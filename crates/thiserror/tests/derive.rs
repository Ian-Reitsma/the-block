use std::error::Error as StdError;

use thiserror::Error;

#[derive(Debug)]
struct DummyError(&'static str);

impl core::fmt::Display for DummyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl StdError for DummyError {}

#[test]
fn unit_variant_display() {
    #[derive(Debug, Error)]
    enum UnitError {
        #[error("unit failure")]
        Failure,
    }

    let err = UnitError::Failure;
    assert_eq!(err.to_string(), "unit failure");
    assert!(err.source().is_none());
}

#[test]
fn tuple_variant_from_and_source() {
    #[derive(Debug, Error)]
    enum Wrapper {
        #[error("wrapped {0}")]
        Inner(#[from] DummyError),
    }

    let inner = DummyError("problem");
    let err = Wrapper::from(inner);
    assert_eq!(err.to_string(), "wrapped problem");
    let source = err.source().expect("source present");
    assert_eq!(source.to_string(), "problem");
}

#[test]
fn struct_variant_named_fields() {
    #[derive(Debug, Error)]
    enum ParseError {
        #[error("invalid token '{value}'")]
        Invalid {
            value: String,
            #[source]
            source: DummyError,
        },
    }

    let err = ParseError::Invalid {
        value: "abc".to_string(),
        source: DummyError("bad"),
    };
    assert_eq!(err.to_string(), "invalid token 'abc'");
    assert_eq!(err.source().unwrap().to_string(), "bad");
}

#[test]
fn optional_boxed_source() {
    #[derive(Debug, Error)]
    enum MaybeError {
        #[error("maybe source")]
        WithSource {
            #[source]
            source: Option<Box<DummyError>>,
        },
    }

    let err_with = MaybeError::WithSource {
        source: Some(Box::new(DummyError("inner"))),
    };
    assert_eq!(err_with.source().unwrap().to_string(), "inner");

    let err_without = MaybeError::WithSource { source: None };
    assert!(err_without.source().is_none());
}

#[test]
fn transparent_tuple_variant() {
    #[derive(Debug, Error)]
    enum TransparentError {
        #[error(transparent)]
        Inner(#[from] DummyError),
    }

    let err = TransparentError::from(DummyError("inner"));
    assert_eq!(err.to_string(), "inner");
    assert_eq!(err.source().unwrap().to_string(), "inner");
}

#[test]
fn lifetime_struct_variant() {
    #[derive(Debug, Error)]
    enum LifetimeError<'a> {
        #[error("missing field {name}")]
        Missing {
            name: &'a str,
            #[source]
            source: DummyError,
        },
    }

    let err = LifetimeError::Missing {
        name: "alpha",
        source: DummyError("inner"),
    };
    assert_eq!(err.to_string(), "missing field alpha");
    assert_eq!(err.source().unwrap().to_string(), "inner");
}

#[test]
fn multiple_sources_chain() {
    #[derive(Debug, Error)]
    enum MultiSource {
        #[error("primary={primary:?}; secondary={secondary}")]
        Both {
            #[source]
            primary: Option<Box<DummyError>>,
            #[source]
            secondary: DummyError,
        },
    }

    let err_with_primary = MultiSource::Both {
        primary: Some(Box::new(DummyError("first"))),
        secondary: DummyError("second"),
    };
    assert!(err_with_primary
        .to_string()
        .contains("primary=Some(DummyError(\"first\"))"));
    assert_eq!(err_with_primary.source().unwrap().to_string(), "first");

    let err_without_primary = MultiSource::Both {
        primary: None,
        secondary: DummyError("second"),
    };
    assert!(err_without_primary.to_string().contains("primary=None"));
    assert_eq!(err_without_primary.source().unwrap().to_string(), "second");
}
