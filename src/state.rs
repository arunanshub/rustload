use ndarray::{Array1, Array2};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// Holds information about a mapped section.
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

    pub(crate) fn register_map(map: PreloadMap) {
        // ...
    }

    pub(crate) fn unregister_map(map: PreloadMap) {
        // ...
    }
}

/// Holds information about a mapped section in an exe.
/// TODO: Describe in details.
pub(crate) struct PreloadExeMap<'a> {
    /// TODO:
    map: &'a PreloadMap,

    /// Probability that this map will be used when an exe is running.
    prob: f64,
}

impl<'a> PreloadExeMap<'a> {
    // TODO:
}

/// Holds information about and executable.
pub(crate) struct PreloadExe<'a> {
    path: PathBuf,
    time: i32,
    update_time: i32,
    markovs: HashSet<PreloadMarkov<'a>>,
    exemaps: HashSet<PreloadExeMap<'a>>,
    size: usize,
    running_timestamp: i32,
    change_timestamp: i32,
    lnprob: i32,
    seq: i32,
}

impl<'a> PreloadExe<'a> {}

pub(crate) struct PreloadMarkov<'a> {
    a: PreloadExe<'a>,
    b: PreloadExe<'a>,
    time: i32,
    time_to_leave: Array1<f64>,
    weight: Array2<i32>,
}

impl<'a> PreloadMarkov<'a> {}

/// Persistent state TODO: Add more details and description
pub(crate) struct PreloadState {
    /// Total seconds that rustload has been running, from the beginning of the
    /// persistent state.
    time: i32,

    /// Map of known applications, indexed by exe name.
    exes: HashMap<PathBuf, usize>,

    /// Set of applications that rustload is not interested in. Typically it is
    /// the case that these applications are too small to be a candidate for
    /// preloading.
    /// Mapped value is the size of the binary (sum of the length of the maps).
    bad_exes: HashMap<PathBuf, usize>,

    /// Set of maps used by known executables, indexed by `PreloadMap`
    /// structure.
    maps: HashMap<PathBuf, usize>,

    // runtime section:
    /// Set of exe structs currently running.
    running_exes: Vec<PathBuf>,

    // TODO: What to do with `GPtrArray* maps_arr`?
    /// Increasing sequence of unique numbers to assign to maps.
    map_seq: i32,

    /// Increasing sequence of unique numbers to assign to exes.
    exe_seq: i32,

    /// Last time we checked for preocesses running.
    last_running_timestamp: i32,

    /// Last time we did accounting on running times, etc.
    last_accounting_timestamp: i32,

    /// Whether new scan has been performed since last save.
    dirty: bool,

    /// Whether new scan has been performed but no model update yet.
    model_dirty: bool,

    // System memory stats.
    // TODO: memstat: PreloadMemory,
    /// Last time we updated the memory stats.
    memstat_timestamp: i32,
}

impl PreloadState {
    pub(crate) fn load(statefile: impl AsRef<Path>) {
        let statefile = statefile.as_ref();
        // ...
    }
}
