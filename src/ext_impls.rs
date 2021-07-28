//! This module holds the modifications done to external types.

use anyhow::Result;
use log::error;
use std::{
    borrow::Cow,
    fmt::Display,
    path::{Path, PathBuf},
};

pub(crate) trait ToPathBuf {
    fn to_pathbuf(&self) -> Vec<PathBuf>;
}

/// Convert `&str`s to `PathBuf`s.
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

pub(crate) trait LogOnError<T, U: Display> {
    fn log_on_err<'a>(self, msg: impl Into<Cow<'a, str>>) -> Result<T, U>;
}

impl<T, U: Display> LogOnError<T, U> for Result<T, U> {
    fn log_on_err<'a>(self, msg: impl Into<Cow<'a, str>>) -> Result<T, U> {
        self.map_err(|e| {
            error!("{}: {}", msg.into(), e);
            e
        })
    }
}