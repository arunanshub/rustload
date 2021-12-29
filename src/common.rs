//! This module holds items common to everyone.

use anyhow::Result;
use derive_more::Deref;
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::{
    fmt::Display,
    path::{Path, PathBuf},
};

/// A shorthand way to write `Rc<RefCell<T>>`.
pub(crate) type RcCell<T> = Rc<RefCell<T>>;

/// A shorthand way to write `Weak<RefCell<T>>`.
pub(crate) type WeakCell<T> = Weak<RefCell<T>>;

/// A custom [`RcCell`] with custom drop function.
#[derive(Deref, Derivative)]
#[derivative(Debug = "transparent")]
pub(crate) struct DropperCell<T, F: Fn(&RcCell<T>) = fn(&RcCell<T>)>(
    Rc<RefCell<T>>,
    #[deref(ignore)]
    #[derivative(Debug = "ignore")]
    Option<F>,
);

impl<T, F: Fn(&RcCell<T>)> DropperCell<T, F> {
    pub fn new(value: T, dropper: Option<F>) -> Self {
        Self(Rc::new(RefCell::new(value)), dropper)
    }
}

impl<T, F: Fn(&RcCell<T>)> Drop for DropperCell<T, F> {
    fn drop(&mut self) {
        if let Some(func) = &self.1 {
            func(&self.0)
        }
    }
}

/// Adds a `.new(...)` to [`RcCell<T>`] type.
pub(crate) trait RcCellNew<T> {
    /// Create a [`RefCell<T>`] enclosed in a [`Rc<T>`].
    fn new_cell(value: T) -> Self;
}

impl<T> RcCellNew<T> for RcCell<T> {
    fn new_cell(value: T) -> Self {
        Rc::new(RefCell::new(value))
    }
}

pub(crate) trait ToPathBuf {
    fn to_pathbuf(&self) -> Vec<PathBuf>;
}

/// Convert `Vec<&str>`s to `PathBuf`s.
impl<X> ToPathBuf for Vec<X>
where
    X: AsRef<Path>,
{
    fn to_pathbuf(&self) -> Vec<PathBuf> {
        // X: Into<PathBuf> requires more and more stupid stuff. Better to use
        // simple `as_ref().into()` here.
        self.iter().map(|x| x.as_ref().into()).collect()
    }
}

/// Trait that logs a message depending on `Result` variant.
pub(crate) trait LogResult<T, U: Display> {
    /// Logs a message only if an error value `Err` is received.
    fn log_on_err(
        self,
        level: log::Level,
        msg: impl AsRef<str>,
    ) -> Result<T, U>;

    /// Logs a message only if no error value is received.
    fn log_on_ok(
        self,
        level: log::Level,
        msg: impl AsRef<str>,
    ) -> Result<T, U>;
}

impl<T, U: Display> LogResult<T, U> for Result<T, U> {
    fn log_on_err(
        self,
        level: log::Level,
        msg: impl AsRef<str>,
    ) -> Result<T, U> {
        self.map_err(|e| {
            log::log!(level, "{}: {}", msg.as_ref(), e);
            e
        })
    }

    fn log_on_ok(
        self,
        level: log::Level,
        msg: impl AsRef<str>,
    ) -> Result<T, U> {
        self.map(|v| {
            log::log!(level, "{}", msg.as_ref());
            v
        })
    }
}

/// Convert bytes to kibibytes.
pub(crate) const fn kb(v: u64) -> u64 {
    v / 1024
}
