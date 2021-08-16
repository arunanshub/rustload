//! Rustload persistent state handling routines
//! TODO: Add more details and explaination.

// use ndarray::{Array1, Array2};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    rc::Rc,
    str::FromStr,
};

use ordered_float::OrderedFloat;

pub(crate) type RcCell<T> = Rc<RefCell<T>>;

#[inline]
pub(crate) fn exe_is_running(
    exe: &RustloadExe,
    state: &RustloadState,
) -> bool {
    exe.running_timestamp >= state.last_running_timestamp
}

#[inline]
pub(crate) fn markov_state(
    a: &RustloadExe,
    b: &RustloadExe,
    state: &RustloadState,
) -> i32 {
    (if exe_is_running(a, state) { 1 } else { 0 })
        + (if exe_is_running(b, state) { 2 } else { 0 })
}

/// Holds information about a mapped section.
#[derive(Eq, PartialOrd, Ord)]
pub(crate) struct RustloadMap {
    /// absolute path of the mapped file.
    path: PathBuf,

    /// offset in bytes
    offset: usize,

    /// length in bytes
    length: usize,

    /// last time it was probed
    update_time: i32,

    // runtime section:
    // number of exes linking to this.
    // TODO: Can `Rc<...>` or `Arc<...>` work here instead of `refcount`
    // refcount: i32,
    /// log-probability of NOT being needed in next period.
    lnprob: OrderedFloat<f64>,

    /// unique map sequence number.
    seq: i32,

    /// on-disk location of the start of the map.
    block: i32,

    /// for private local use of functions.
    private: i32,
    // The state TODO:
    // state: RustloadState,
}

impl RustloadMap {
    pub(crate) fn new(
        path: impl Into<PathBuf>,
        offset: usize,
        length: usize,
    ) -> Self {
        Self {
            path: path.into(),
            offset,
            length,
            // refcount: 0,
            update_time: 0,
            block: -1,
            lnprob: 0.0.into(),
            seq: 0,
            private: 0,
        }
    }

    pub(crate) fn register_map(map: RustloadMap) {
        // ...
    }

    pub(crate) fn unregister_map(map: RustloadMap) {
        // ...
    }

    pub(crate) fn get_size(&self) -> usize {
        self.length
    }

    /*
     * // TODO: is this the correct way...
     * pub(crate) fn increase_ref(&mut self) {
     *     self.refcount += 1;
     * }
     */
}

impl PartialEq for RustloadMap {
    fn eq(&self, other: &Self) -> bool {
        self.offset == other.offset
            && self.length == other.length
            && self.path == other.path
    }
}

/// Holds information about a mapped section in an exe.
/// TODO: Describe in details.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RustloadExeMap {
    /// TODO: ...or can we use a Rc/Arc<RustloadMap> here?
    map: RcCell<RustloadMap>,

    /// Probability that this map will be used when an exe is running.
    prob: OrderedFloat<f64>,
}

impl RustloadExeMap {
    /// Add new `map` using `Rc::clone(&map)`.
    pub(crate) fn new(map: RcCell<RustloadMap>) -> Self {
        Self {
            map,
            prob: 1.0.into(),
        }
    }
}

/// Holds information about and executable.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RustloadExe<'a> {
    /// Absolute path of the executable.
    path: PathBuf,

    /// Total running time of the executable.
    time: i32,

    /// Last time it was probed.
    update_time: i32,

    /// Set of markov chain with other exes.
    markovs: BTreeSet<RustloadMarkov<'a>>,

    /// Set of `RustloadExeMap` structures.
    exemaps: BTreeSet<RustloadExeMap>,

    /// sum of the size of maps.
    size: usize,

    /// Last time it was running.
    running_timestamp: i32,

    /// Time when exe stopped/started running.
    change_timestamp: i32,

    /// log-probability of NOT being needed in the next period.
    lnprob: i32,

    /// Unique exe sequence number.
    seq: i32,
}

impl<'a> RustloadExe<'a> {}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RustloadMarkov<'a> {
    /// Involved exes.
    a: &'a RustloadExe<'a>,
    b: &'a RustloadExe<'a>,

    /// Current state
    state: i32,

    rustload_state: &'a RustloadState,

    /// Total time both exes have been running simultaneously (state 3).
    time: i32,

    /// Mean time to leave each state
    time_to_leave: [i32; 4],

    /// Number of times we've got from state `i` to state `j`. `weight[i][j]`
    /// is the number of times we have left state `i` (sum over `weight[i][j]`)
    /// for `j<>i` essentially.
    weight: [[i32; 4]; 4],
}

impl<'a> RustloadMarkov<'a> {
    pub(crate) fn new(
        a: &'a RustloadExe<'_>,
        b: &'a RustloadExe<'_>,
        rustload_state: &'a RustloadState,
    ) -> Self {
        let mut state = markov_state(a, b, rustload_state);
        let mut change_timestamp = rustload_state.time;

        if a.change_timestamp > 0 && b.change_timestamp > 0 {
            if a.change_timestamp < rustload_state.time {
                change_timestamp = a.change_timestamp
            }
            if b.change_timestamp < rustload_state.time
                && b.change_timestamp > change_timestamp
            {
                change_timestamp = a.change_timestamp
            }
            if a.change_timestamp > change_timestamp {
                state ^= 1
            }
            if b.change_timestamp > change_timestamp {
                state ^= 2
            }
        }
        let markov = Self {
            a,
            b,
            state,
            rustload_state,
            time: 0,
            time_to_leave: Default::default(),
            weight: Default::default(),
        };

        // TODO: Fix markov insertion stuff
        // markov.a.markovs.insert(markov);
        markov
    }

    /// Calculates the correlation coefficient of the two random variable of
    /// the exes in this markov been running.
    ///
    /// The returned value is a number in the range `-1` to `1` that is a
    /// numeric measure of the strength of linear relationship between two
    /// random variables.  the correlation is `1` in the case of an increasing
    /// linear relationship, `−1` in the case of a decreasing linear
    /// relationship, and some value in between in all other cases, indicating
    /// the degree of linear dependence between the variables.  the closer the
    /// coefficient is to either `−1` or `1`, the stronger the correlation
    /// between the variables.
    ///
    /// See [Correlation](https://en.wikipedia.org/wiki/Correlation) for more
    /// information.
    ///
    /// We calculate the Pearson product-moment correlation coefficient, which
    /// is found by dividing the covariance of the two variables by the product
    /// of their standard deviations. That is:
    ///
    /// ```none
    ///                E(AB) - E(A)E(B)
    /// ρ(a,b) = ___________________________
    ///           ____________  ____________
    ///          √ E(A²)-E²(A) √ E(B²)-E²(B)
    /// ```
    ///
    /// Where `A` and `B` are the random variables of exes `a` and `b` being
    /// run, with a value of `1` when running, and `0` when not. It's obvious
    /// to compute the above then, since:
    ///
    /// ```none
    /// E(AB) = markov.time / state.time
    /// E(A) = markov.a.time / state.time
    /// E(A²) = E(A)
    /// E²(A) = E(A)²
    /// (same for B)
    /// ```
    pub(crate) fn corellation(&self) -> f64 {
        // TODO: Fix the `state` object
        let t = self.state;
        let (a, b) = (self.a.time, self.b.time);
        let ab = self.time;

        let (corellation, numerator, denominator2);

        if (a == 0 || a == t || b == 0 || b == t) {
            corellation = 0.0;
        } else {
            numerator = (t * ab) - (a * b);
            denominator2 = (a * b) * ((t - a) * (t - b));
            corellation = numerator as f64 / f64::sqrt(denominator2 as f64)
        }
        corellation
    }
}

/// Persistent state
/// TODO: Add more details and description
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RustloadState {
    /// Total seconds that rustload has been running, from the beginning of the
    /// persistent state.
    time: i32,

    /// Map of known applications, indexed by exe name.
    exes: BTreeMap<PathBuf, usize>,

    /// Set of applications that rustload is not interested in. Typically it is
    /// the case that these applications are too small to be a candidate for
    /// preloading.
    /// Mapped value is the size of the binary (sum of the length of the maps).
    bad_exes: BTreeMap<PathBuf, usize>,

    /// Set of maps used by known executables, indexed by `RustloadMap`
    /// structure.
    maps: BTreeMap<PathBuf, usize>,

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
    // TODO: memstat: RustloadMemory,
    /// Last time we updated the memory stats.
    memstat_timestamp: i32,
}

impl RustloadState {
    pub(crate) fn load(&self, statefile: impl AsRef<Path>) {
        let statefile = statefile.as_ref();
        // TODO:
    }

    pub(crate) fn save(&self, statefile: impl AsRef<Path>) {
        let statefile = statefile.as_ref();
        // TODO:
    }

    pub(crate) fn dump_log(&self) {
        // TODO:
    }

    pub(crate) fn run(&self, statefile: impl AsRef<Path>) {
        let statefile = statefile.as_ref();
        // TODO:
    }

    pub(crate) fn register_exe<'a>(
        &self,
        exe: &'a RustloadExe,
        create_markov: bool,
    ) {
        // TODO:
    }

    pub(crate) fn unregister_exe<'a>(exe: &'a RustloadExe) {
        // TODO:
    }
}
