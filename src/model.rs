// TODO: Explain self and add doc source.

use crate::common::ToPathBuf;
use anyhow::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{convert::TryFrom, path::PathBuf};

/// Configuration for model which will be used to make predictions.
#[derive(Derivative, Serialize, Deserialize, Debug)]
#[derivative(Default)]
pub(crate) struct Model {
    /// This is the quantum of time for preload. Preload performs data
    /// gathering and predictions every cycle. Use an even number.
    ///
    /// # Note
    ///
    /// Setting this parameter too low may reduce system performance and
    /// stability.
    #[derivative(Default(value = "20"))]
    pub(crate) cycle: u32,

    /// Whether correlation coefficient should be used in the prediction
    /// algorithm. There are arguments both for and against using it.
    /// Currently it's believed that using it results in more accurate
    /// prediction. The option may be removed in the future.
    #[derivative(Default(value = "true"))]
    pub(crate) usecorrelation: bool,

    /// Minimum sum of the length of maps of the process for preload to
    /// consider tracking the application.
    ///
    /// # Note
    ///
    /// Setting this parameter too high will make preload less effective,
    /// while setting it too low will make it eat quadratically more resources,
    /// as it tracks more processes.
    #[derivative(Default(value = "2000000"))]
    pub(crate) minsize: u32,

    /// The following control how much memory preload is allowed to use for
    /// preloading in each cycle. All values are percentages and are clamped
    /// to -100 to 100.
    ///
    /// The total memory preload uses for prefetching is then computed using
    /// the following formulae:
    ///
    /// ```
    /// max(0, TOTAL * memtotal + FREE * memfree) + CACHED * memcached
    /// ```
    ///
    /// where TOTAL, FREE, and CACHED are the respective values read at runtime
    /// from `/proc/meminfo`.
    #[derivative(Default(value = "-10"))]
    pub(crate) memtotal: i32,

    /// Percentage of free memory.
    #[derivative(Default(value = "50"))]
    pub(crate) memfree: i32,

    /// Percentage of cached memory.
    #[derivative(Default(value = "0"))]
    pub(crate) memcached: i32,
}

// TODO: Add functions for generation of optimized defaults.
impl Model {}

/// How rustload will interact with the system.
#[derive(Derivative, Debug, Serialize, Deserialize)]
#[derivative(Default)]
pub(crate) struct System {
    /// Whether preload should monitor running processes and update its model
    /// state. Normally you do want that, that's all preload is about, but you
    /// may want to temporarily turn it off for various reasons like testing
    /// and only make predictions.
    ///
    /// # Note
    ///
    /// If scanning is off, predictions are made based on whatever processes
    /// have been running when preload started and the list of running
    /// processes is not updated at all.
    #[derivative(Default(value = "true"))]
    pub(crate) doscan: bool,

    /// Whether preload should make prediction and prefetch anything off the
    /// disk. Quite like doscan, you normally want that, that's the other half
    /// of what preload is about, but you may want to temporarily turn it off,
    /// to only train the model for example.
    ///
    /// # Note
    ///
    /// This allows you to turn scan/predict or or off on the fly, by modifying
    /// the config file and signalling the daemon.
    #[derivative(Default(value = "true"))]
    pub(crate) dopredict: bool,

    /// Preload will automatically save the state to disk every autosave
    /// period. This is only relevant if doscan is set to true.
    ///
    /// # Note
    ///
    /// Some janitory work on the model, like removing entries for files that
    /// no longer exist happen at state save time. So, turning off autosave
    /// completely is not advised.
    #[derivative(Default(value = "3600"))]
    pub(crate) autosave: u32,

    /// A list of path prefixes that control which mapped file are to be
    /// considered by preload and which not. The list items are separated by
    /// semicolons. Matching will be stopped as soon as the first item is
    /// matched. For each item, if item appears at the beginning of the path
    /// of the file, then a match occurs, and the file is accepted. If on the
    /// other hand, the item has a exclamation mark as its first character,
    /// then the rest of the item is considered, and if a match happens, the
    /// file is rejected. For example a value of !/lib/modules;/ means that
    /// every file other than those in /lib/modules should be accepted. In
    /// this case, the trailing item can be removed, since if no match occurs,
    /// the file is accepted. It's advised to make sure /dev is rejected,
    /// since preload doesn't special-handle device files internally.
    ///
    /// # Note
    ///
    /// If /lib matches all of /lib, /lib64, and even /libexec if there was
    /// one. If one really meant /lib only, they should use /lib/ instead.
    #[derivative(Default(value = r#"vec![
        "/opt",
        "!/usr/sbin/",
        "!/usr/local/sbin/",
        "/usr/",
        "!/",
    ].to_pathbuf()"#))]
    pub(crate) mapprefix: Vec<PathBuf>,

    /// The syntax for this is exactly the same as for mapprefix. The only
    /// difference is that this is used to accept or reject binary exectuable
    /// files instead of maps.
    #[derivative(Default(value = r#"vec![
        "/opt",
        "!/usr/sbin/",
        "!/usr/local/sbin/",
        "/usr/",
        "!/",
    ].to_pathbuf()"#))]
    pub(crate) exeprefix: Vec<PathBuf>,

    /// Maximum number of processes to use to do parallel readahead. If
    /// equal to 0, no parallel processing is done and all readahead is
    /// done in-process. Parallel readahead supposedly gives a better I/O
    /// performance as it allows the kernel to batch several I/O requests
    /// of nearby blocks.
    #[derivative(Default(value = "30"))]
    pub(crate) processes: u32,

    /// The I/O sorting strategy. Ideally this should be automatically
    /// decided, but it's not currently.
    ///
    /// See [`SortStrategy`] for possible values.
    #[derivative(Default(value = "SortStrategy::Block as u8"))]
    pub(crate) sortstrategy: u8, // we need an enum
}

// TODO: Add functions for generation of optimized defaults.
impl System {}

/// The I/O sorting strategy.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum SortStrategy {
    /// No I/O sorting. Useful on Flash memory for example.
    None = 0,

    /// Sort based on file path only. Useful for network filesystems.
    Path = 1,

    /// Sort based on inode number. Does less house-keeping I/O than the next
    /// option.
    Inode = 2,

    /// Sort I/O based on disk block. Most sophisticated. And useful for most
    /// Linux filesystems.
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
