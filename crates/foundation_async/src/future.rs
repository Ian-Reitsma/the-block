use std::cell::{Cell, RefCell};
use std::future::Future;
use std::panic::{self, AssertUnwindSafe, UnwindSafe};
use std::pin::Pin;
use std::task::{Context, Poll};

/// Result of selecting over two futures.
#[derive(Debug)]
pub enum Either<A, B> {
    First(A),
    Second(B),
}

/// Drive all provided futures to completion, returning their outputs in order.
pub fn join_all<F>(futures: Vec<F>) -> JoinAll<F>
where
    F: Future,
{
    let len = futures.len();
    let mut outputs = Vec::with_capacity(len);
    outputs.resize_with(len, || None);

    JoinAll {
        futures: RefCell::new(futures.into_iter().map(|f| Some(Box::pin(f))).collect()),
        outputs: RefCell::new(outputs),
        remaining: Cell::new(len),
    }
}

pub struct JoinAll<F: Future> {
    futures: RefCell<Vec<Option<Pin<Box<F>>>>>,
    outputs: RefCell<Vec<Option<F::Output>>>,
    remaining: Cell<usize>,
}

impl<F> Future for JoinAll<F>
where
    F: Future,
{
    type Output = Vec<F::Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.as_ref().get_ref();
        let mut futures = this.futures.borrow_mut();
        let mut outputs = this.outputs.borrow_mut();
        let mut remaining = this.remaining.get();

        for (idx, slot) in futures.iter_mut().enumerate() {
            if let Some(fut) = slot.as_mut() {
                match fut.as_mut().poll(cx) {
                    Poll::Ready(value) => {
                        outputs[idx] = Some(value);
                        *slot = None;
                        remaining -= 1;
                    }
                    Poll::Pending => {}
                }
            }
        }

        this.remaining.set(remaining);
        drop(futures);

        if remaining == 0 {
            let mut results = Vec::with_capacity(outputs.len());
            for slot in outputs.iter_mut() {
                results.push(slot.take().expect("join_all result missing"));
            }
            drop(outputs);
            Poll::Ready(results)
        } else {
            drop(outputs);
            Poll::Pending
        }
    }
}

/// Returns a future that resolves when either `first` or `second` completes.
pub fn select2<A, B>(first: A, second: B) -> Select2<A, B>
where
    A: Future,
    B: Future,
{
    Select2 {
        first: Some(Box::pin(first)),
        second: Some(Box::pin(second)),
    }
}

pub struct Select2<A: Future, B: Future> {
    first: Option<Pin<Box<A>>>,
    second: Option<Pin<Box<B>>>,
}

impl<A, B> Future for Select2<A, B>
where
    A: Future,
    B: Future,
{
    type Output = Either<A::Output, B::Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(fut) = this.first.as_mut() {
            match fut.as_mut().poll(cx) {
                Poll::Ready(value) => {
                    this.first = None;
                    this.second = None;
                    return Poll::Ready(Either::First(value));
                }
                Poll::Pending => {}
            }
        }

        if let Some(fut) = this.second.as_mut() {
            match fut.as_mut().poll(cx) {
                Poll::Ready(value) => {
                    this.first = None;
                    this.second = None;
                    Poll::Ready(Either::Second(value))
                }
                Poll::Pending => Poll::Pending,
            }
        } else {
            Poll::Pending
        }
    }
}

/// Future that catches unwinds during polling.
pub fn catch_unwind<F>(future: F) -> CatchUnwind<F>
where
    F: Future + UnwindSafe + 'static,
{
    CatchUnwind {
        future: Box::pin(future),
    }
}

pub struct CatchUnwind<F: Future + UnwindSafe + 'static> {
    future: Pin<Box<F>>,
}

impl<F> Future for CatchUnwind<F>
where
    F: Future + UnwindSafe + 'static,
{
    type Output = Result<F::Output, Box<dyn std::any::Any + Send>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let mut future = this.future.as_mut();
        match panic::catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(cx))) {
            Ok(Poll::Ready(value)) => Poll::Ready(Ok(value)),
            Ok(Poll::Pending) => Poll::Pending,
            Err(err) => Poll::Ready(Err(err)),
        }
    }
}
