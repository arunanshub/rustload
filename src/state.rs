use std::{collections::HashMap, path::PathBuf};

pub(crate) struct PreloadMap {
    /// absolute path of the mapped file.
    path: PathBuf,
    /// offset in bytes
    offset: usize,
    /// length in bytes
    length: usize,
    /// last time it was probed
    update_time: i32,

    // runtime section:
    /// number of exes linking to this.
    refcount: i32,
    /// log-probability of NOT being needed in next period.
    lnprob: f64,
    /// unique map sequence number.
    seq: i32,
    /// on-disk location of the start of the map.
    block: i32,
    /// for private local use of functions.
    private: i32,
    // The state TODO:
    // state: PreloadState,
}

pub(crate) struct PreloadState {
    time: i32,
    exes: HashMap<PathBuf, usize>,
    bad_exes: HashMap<PathBuf, usize>,
    maps: HashMap<PathBuf, usize>,
    running_exes: Vec<PathBuf>,
}

impl PreloadMap {
    pub(crate) fn new(
        path: impl Into<PathBuf>,
        offset: usize,
        length: usize,
    ) -> Self {
        Self {
            path: path.into(),
            offset,
            length,
            refcount: 0,
            update_time: 0,
            block: -1,
            lnprob: 0.0,
            seq: 0,
            private: 0,
        }
    }
}
