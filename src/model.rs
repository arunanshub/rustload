use crate::ext_impls::ToPathBuf;
use anyhow::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{convert::TryFrom, path::PathBuf};

/// Configuration for model which will be used to make predictions.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Model {
    pub(crate) cycle: u32,
    pub(crate) usecorrelation: bool,
    pub(crate) minsize: u32,
    pub(crate) memtotal: i32,
    pub(crate) memfree: i32,
    pub(crate) memcached: i32,
}

// this is the default as seen in original preload, a separate function
// will be used to compute the optimal values(?)
impl Default for Model {
    fn default() -> Self {
        // we can perform some calculation before setting values.
        Self {
            cycle: 20,
            usecorrelation: true,
            minsize: 2000000,
            memtotal: -10,
            memfree: 50,
            memcached: 0,
        }
    }
}

// TODO: Add functions for generation of optimized defaults.
impl Model {}

#[derive(Debug, Serialize, Deserialize)]
/// How rustload will interact with the system.
pub(crate) struct System {
    pub(crate) doscan: bool,
    pub(crate) dopredict: bool,
    pub(crate) autosave: u32,
    pub(crate) mapprefix: Vec<PathBuf>,
    pub(crate) exeprefix: Vec<PathBuf>,
    pub(crate) processes: u32,
    pub(crate) sortstrategy: u8, // we need an enum
}

// TODO: Add functions for generation of optimized defaults.
impl System {}

// this is the default as seen in original preload, a separate function
// will be used to compute the optimal values(?)
impl Default for System {
    fn default() -> Self {
        // we can perform some calculation before setting values.
        Self {
            doscan: true,
            dopredict: true,
            autosave: 3600,
            mapprefix: vec![
                "/opt",
                "!/usr/sbin/",
                "!/usr/local/sbin/",
                "/usr/",
                "!/",
            ]
            .to_pathbuf(),
            exeprefix: vec![
                "/opt",
                "!/usr/sbin/",
                "!/usr/local/sbin/",
                "/usr/",
                "!/",
            ]
            .to_pathbuf(),
            processes: 30,
            sortstrategy: 3,
        }
    }
}

/// Sort strategy for System.sortstrategy; I/O sorting strategy.
#[derive(Copy, Clone, Debug)]
pub(crate) enum SortStrategy {
    None = 0,
    Path = 1,
    Inode = 2,
    Block = 3,
}

// For easy conversion from u8 to SortStrategy.
impl TryFrom<u8> for SortStrategy {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        let strat = match value {
            0 => Self::None,
            1 => Self::Path,
            2 => Self::Inode,
            3 => Self::Block,
            _ => {
                return Err(anyhow::format_err!(
                    "Invalid value for SortStrategy: {:?}",
                    value
                ));
            }
        };
        Ok(strat)
    }
}
