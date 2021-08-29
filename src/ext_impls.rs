//! This module holds the modifications done to external types.

use anyhow::Result;
use std::cell::RefCell;
use std::rc::Rc;
use std::{
    fmt::Display,
    path::{Path, PathBuf},
};

pub(crate) type RcCell<T> = Rc<RefCell<T>>;

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
    /// Logs an `Error` level message only if an error value `Err` is received.
    fn log_on_err(self, msg: impl AsRef<str>) -> Result<T, U>;

    /// Logs an `Info` level message only if no error value is received.
    fn log_on_ok<'a>(self, msg: impl AsRef<str>) -> Result<T, U>;
}

impl<T, U: Display> LogResult<T, U> for Result<T, U> {
    fn log_on_err(self, msg: impl AsRef<str>) -> Result<T, U> {
        self.map_err(|e| {
            log::error!("{}: {}", msg.as_ref(), e);
            e
        })
    }

    fn log_on_ok(self, msg: impl AsRef<str>) -> Result<T, U> {
        self.map(|v| {
            log::info!("{}", msg.as_ref());
            v
        })
    }
}
