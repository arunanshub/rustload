use crate::ext_impls::ToPathBuf;
use anyhow::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{convert::TryFrom, path::PathBuf};

/// Configuration for model which will be used to make predictions.
#[derive(Derivative, Serialize, Deserialize, Debug)]
#[derivative(Default)]
pub(crate) struct Model {
    #[derivative(Default(value = "20"))]
    pub(crate) cycle: u32,

    #[derivative(Default(value = "true"))]
    pub(crate) usecorrelation: bool,

    #[derivative(Default(value = "2000000"))]
    pub(crate) minsize: u32,

    #[derivative(Default(value = "-10"))]
    pub(crate) memtotal: i32,

    #[derivative(Default(value = "50"))]
    pub(crate) memfree: i32,

    #[derivative(Default(value = "0"))]
    pub(crate) memcached: i32,
}

// TODO: Add functions for generation of optimized defaults.
impl Model {}

/// How rustload will interact with the system.
#[derive(Derivative, Debug, Serialize, Deserialize)]
#[derivative(Default)]
pub(crate) struct System {
    #[derivative(Default(value = "true"))]
    pub(crate) doscan: bool,

    #[derivative(Default(value = "true"))]
    pub(crate) dopredict: bool,

    #[derivative(Default(value = "3600"))]
    pub(crate) autosave: u32,

    #[derivative(Default(value = r#"vec![
        "/opt",
        "!/usr/sbin/",
        "!/usr/local/sbin/",
        "/usr/",
        "!/",
    ].to_pathbuf()"#))]
    pub(crate) mapprefix: Vec<PathBuf>,

    #[derivative(Default(value = r#"vec![
        "/opt",
        "!/usr/sbin/",
        "!/usr/local/sbin/",
        "/usr/",
        "!/",
    ].to_pathbuf()"#))]
    pub(crate) exeprefix: Vec<PathBuf>,

    #[derivative(Default(value = "30"))]
    pub(crate) processes: u32,

    #[derivative(Default(value = "SortStrategy::Block as u8"))]
    pub(crate) sortstrategy: u8, // we need an enum
}

// TODO: Add functions for generation of optimized defaults.
impl System {}

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
            _ => anyhow::bail!("Invalid value for SortStrategy: {:?}", value),
        };
        Ok(strat)
    }
}
