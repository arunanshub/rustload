//! This module holds the modifications done to external types.

use anyhow::Result;
use log::error;
use std::{
    borrow::Cow,
    fmt::{Debug, Display},
    path::{Path, PathBuf},
};

pub(crate) trait ToPathBuf {
    fn to_pathbuf(&self) -> Vec<PathBuf>;
}

/// Convert `&str`s to `PathBuf`s.
impl<'a, X> ToPathBuf for Vec<X>
where
    X: AsRef<Path>,
{
    fn to_pathbuf(&self) -> Vec<PathBuf> {
        self.iter().map(|x| x.as_ref().to_owned()).collect()
    }
}

pub(crate) trait LogOnError<T, U: Display + Debug> {
    fn log_on_err<'a>(self, msg: impl Into<Cow<'a, str>>) -> Result<T, U>;
}

impl<T, U: Display + Debug> LogOnError<T, U> for Result<T, U> {
    fn log_on_err<'a>(self, msg: impl Into<Cow<'a, str>>) -> Result<T, U> {
        self.map_err(|e| {
            error!("{}: {}", msg.into(), e);
            e
        })
    }
}
