//! First-party lazy initialization primitives built on top of the
//! standard library.  These types intentionally mirror the subset of the
//! `once_cell` API that the workspace relied on so that production code
//! can migrate away from the third-party crate without churn.

use std::ops::Deref;
use std::sync::OnceLock;

/// Lazily initialised container backed by [`LazyLock`].
///
/// `Lazy` values are constructed with an initializer closure and only
/// evaluated the first time they are dereferenced or [`force`].  The
/// implementation delegates to the standard library to preserve
/// thread-safety guarantees and panics on re-entrancy just like the
/// previous third-party facade.
#[derive(Debug)]
pub struct Lazy<T> {
    init: fn() -> T,
    cell: OnceLock<T>,
}

impl<T> Lazy<T> {
    /// Creates a new lazy value with the supplied initializer.
    pub const fn new(init: fn() -> T) -> Self {
        Self {
            init,
            cell: OnceLock::new(),
        }
    }

    /// Forces the lazy value to initialise and returns a reference to the
    /// stored value.
    pub fn force(this: &Self) -> &T {
        this.cell.get_or_init(this.init)
    }

    /// Returns the underlying value if it has already been initialised.
    pub fn get(this: &Self) -> Option<&T> {
        this.cell.get()
    }

    /// Ensures the lazy value has been initialised and returns a
    /// reference to it.
    pub fn get_or_init(&self) -> &T {
        self.cell.get_or_init(self.init)
    }
}

impl<T> Deref for Lazy<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.get_or_init()
    }
}

/// Interior mutable cell that can be written to once.
#[derive(Debug)]
pub struct OnceCell<T> {
    inner: OnceLock<T>,
}

impl<T> OnceCell<T> {
    /// Creates a new empty cell.
    pub const fn new() -> Self {
        Self {
            inner: OnceLock::new(),
        }
    }

    /// Attempts to set the cell, returning the value if it was already set.
    pub fn set(&self, value: T) -> Result<(), T> {
        self.inner.set(value)
    }

    /// Returns a reference to the stored value if present.
    pub fn get(&self) -> Option<&T> {
        self.inner.get()
    }

    /// Returns a mutable reference to the stored value if present.
    pub fn get_mut(&mut self) -> Option<&mut T> {
        self.inner.get_mut()
    }

    /// Initializes the cell with the provided closure if it has not been
    /// set yet and returns a shared reference to the result.
    pub fn get_or_init<F>(&self, init: F) -> &T
    where
        F: FnOnce() -> T,
    {
        self.inner.get_or_init(init)
    }

    /// Fallible variant of [`get_or_init`] that propagates the error from
    /// the initializer.
    pub fn get_or_try_init<F, E>(&self, init: F) -> Result<&T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        if let Some(value) = self.inner.get() {
            return Ok(value);
        }
        let value = init()?;
        match self.inner.set(value) {
            Ok(()) => Ok(self
                .inner
                .get()
                .expect("value present after successful initialization")),
            Err(original) => {
                drop(original);
                Ok(self
                    .inner
                    .get()
                    .expect("value present after racing initialization"))
            }
        }
    }

    /// Consumes the cell, returning the stored value if it was initialised.
    pub fn into_inner(self) -> Option<T> {
        self.inner.into_inner()
    }

    /// Takes the stored value out of the cell if it has been initialised.
    pub fn take(&mut self) -> Option<T> {
        self.inner.take()
    }
}

impl<T> Default for OnceCell<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Synchronised primitives matching the `once_cell::sync` module layout so
/// existing imports continue to compile with a simple substitution.
pub mod sync {
    pub use super::{Lazy, OnceCell};
}
