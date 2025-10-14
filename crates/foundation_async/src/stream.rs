use std::pin::Pin;
use std::task::{Context, Poll};

/// Minimal stream trait mirroring the subset used by runtime primitives.
pub trait Stream {
    type Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>>;
}
