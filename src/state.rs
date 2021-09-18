// vim:set et sw=4 ts=4 tw=79:
//! Rustload persistent state handling routines.
//!
//! Most of the documentation here is adapted from the original thesis of
//! `preload` by Behdad Esfahbod. The thesis is available [here](https://)
// TODO: Add more details and explanation.

// use ndarray::{Array1, Array2};
use crate::{
    ext_impls::{LogResult, RcCell},
    schema,
};
use anyhow::{Context, Result};
use diesel::prelude::*;
use indoc::indoc;
use ordered_float::OrderedFloat;
use std::rc::Rc;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::BufReader,
    marker::{PhantomData, PhantomPinned},
    path::{Path, PathBuf},
    pin::Pin,
};
use url::Url;

/// Hosts all the types required to fetch from and insert values into the
/// database.
/// TODO: Consider using a macro to cut off repeated shit!
pub(crate) mod models {
    use crate::schema::*;

    #[derive(Queryable)]
    pub struct BadExe {
        pub id: i64,
        pub update_time: i32,
        pub uri: String,
    }

    #[derive(Insertable)]
    #[table_name = "badexes"]
    pub struct NewBadExe<'a> {
        pub update_time: &'a i32,
        pub uri: &'a str,
    }

    #[derive(Queryable)]
    pub struct ExeMap {
        pub id: i64,
        pub seq: i32,
        pub map_seq: i32,
        pub prob: f64,
    }

    #[derive(Insertable)]
    #[table_name = "exemaps"]
    pub struct NewExeMap<'a> {
        pub seq: &'a i32,
        pub map_seq: &'a i32,
        pub prob: &'a f64,
    }

    #[derive(Queryable)]
    pub struct Exe {
        pub id: i64,
        pub seq: i32,
        pub update_time: i32,
        pub time: i32,
        pub uri: String,
    }

    #[derive(Insertable)]
    #[table_name = "exes"]
    pub struct NewExe<'a> {
        pub seq: &'a i32,
        pub update_time: &'a i32,
        pub time: &'a i32,
        pub uri: &'a str,
    }

    #[derive(Queryable)]
    pub struct Map {
        pub id: i64,
        pub seq: i32,
        pub update_time: i32,
        pub offset: i32,
        pub uri: String,
    }

    #[derive(Insertable)]
    #[table_name = "maps"]
    pub struct NewMap<'a> {
        pub seq: &'a i32,
        pub update_time: &'a i32,
        pub offset: &'a i32,
        pub uri: &'a str,
    }

    #[derive(Queryable)]
    pub struct Markov {
        pub id: i64,
        pub a_seq: i32,
        pub b_seq: i32,
        pub time: i32,
        pub time_to_leave: Vec<u8>,
        pub weight: Vec<u8>,
    }

    #[derive(Insertable)]
    #[table_name = "markovs"]
    pub struct NewMarkov<'a> {
        pub a_seq: &'a i32,
        pub b_seq: &'a i32,
        pub time: &'a i32,
        pub time_to_leave: &'a [u8],
        pub weight: &'a [u8],
    }
} /* models */

/// Represents an vector of `i32` with `N` elements. Since default values for
/// const generics are experimental at the time of writing, it must be assumed
/// that `N` is equal to `4`.
pub(crate) type ArrayN<const N: usize> = [i32; N];

/// Represents an `N x N` nested array of `i32`. Since default values for const
/// generics are experimental at the time of writing, it must be assumed that
/// `N` is equal to `4`.
pub(crate) type ArrayNxN<const N: usize> = [[i32; N]; N];

/// Calculates the `state` of the markov chain based on the running state of
/// two exes.
///
/// Read `RustloadMarkov`'s documentation for more information.
#[inline]
pub(crate) fn markov_state(
    a: &RustloadExe,
    b: &RustloadExe,
    state: &RustloadState,
) -> i32 {
    (if a.is_running(state) { 1 } else { 0 })
        + (if b.is_running(state) { 2 } else { 0 })
}

/// Convert a file name as `std::path::Path` into an URL in the `file` scheme.
///
/// Difference between `filename_to_uri` and `Url::from_file_path` is, this
/// function returns an `anyhow::Result` type, whereas the latter doesn't.
#[inline]
fn filename_to_uri(path: impl AsRef<Path>) -> Result<Url> {
    Url::from_file_path(path)
        .map_err(|_| anyhow::anyhow!("Failed to parse filepath"))
}

/// Convert a URI as `url::Url` into a `std::path::PathBuf`, assuming the URL
/// is in a file scheme.
///
/// Difference between `uri_to_filename` and `Url::to_file_path` is, this
/// function returns an `anyhow::Result` type, whereas the latter doesn't.
#[inline]
fn uri_to_filename(uri: impl AsRef<Url>) -> Result<PathBuf> {
    uri.as_ref()
        .to_file_path()
        .map_err(|_| anyhow::anyhow!("Failed to parse filepath"))
}

/// A Map object corresponds to a single map that may be used by one or more
/// applications. A Map is identified by the path of its file, a start offset,
/// and a length. The size of a Map is its length.
///
/// A map is a contiguous part of the shared object that a process maps into
/// its address space. This is identified by and offset and length; in
/// practice, both of them are multiples of the page-size of the system, `4kb`
/// on 32-bit preocessors and `8kb` on 64-bit preocessors.
///
/// A process may use multiple maps of the same shared object. The list of the
/// maps of a process can be accessed through the file `/proc/<pid>/maps`. This
/// contains a list of address ranges, access permissions, offsets, and
/// file-names of all maps of the process.  When the shared object file of a
/// map is unlinked from the file-system, the string " (deleted)" will appear
/// after the file-name of the map in the maps file, so this can be detected
/// easily.
#[derive(Derivative)]
#[derivative(Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct RustloadMap {
    /// absolute path of the mapped file.
    path: PathBuf,

    /// offset in bytes
    offset: usize,

    /// length in bytes
    length: usize,

    /// last time it was probed
    #[derivative(PartialEq = "ignore")]
    update_time: i32,

    // runtime section:
    // number of exes linking to this.
    // TODO: Can `Rc<...>` or `Arc<...>` work here instead of `refcount`
    // refcount: i32,
    /// log-probability of NOT being needed in next period.
    #[derivative(PartialEq = "ignore")]
    lnprob: OrderedFloat<f64>,

    /// unique map sequence number.
    #[derivative(PartialEq = "ignore")]
    seq: i32,

    /// on-disk location of the start of the map.
    #[derivative(PartialEq = "ignore")]
    block: i32,

    /// for private local use of functions.
    #[derivative(PartialEq = "ignore")]
    private: i32,
    // The state TODO:
    // state: RustloadState,
}

impl RustloadMap {
    #[inline]
    pub(crate) fn prob_print(&self) {
        log::warn!("ln(prob(~EXE)) = {}    {:?}", self.lnprob, self.path);
    }

    /// Perform a three way comparison with a [`RustloadMap`]'s `lnprob` and
    /// returns the result as a signed integer.
    #[inline]
    pub(crate) fn prob_compare(&self, other: &Self) -> i32 {
        self.lnprob.cmp(&other.lnprob) as i32
    }

    #[inline]
    pub(crate) fn zero_prob(&mut self) {
        self.lnprob = 0.0.into();
    }

    #[inline]
    pub(crate) fn get_size(&self) -> usize {
        self.length
    }

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

    // TODO: Do I require a `WriteContext` type? Although I don't want to.
    /// Write the map values to the database. see TODO.
    pub(crate) fn write_map(
        &self,
        conn: &SqliteConnection, // TODO: Should this be kept in struct?
    ) -> Result<()> {
        let uri = filename_to_uri(&self.path)
            .log_on_err("Failed to parse filepath")?;

        let new_map = models::NewMap {
            seq: &self.seq,
            update_time: &self.update_time,
            offset: &(self.offset as i32),
            uri: uri.as_str(),
        };

        diesel::insert_into(schema::maps::table)
            .values(&new_map)
            .execute(conn)
            .log_on_err("Failed to insert map into database")?;

        Ok(())
    }

    /*
     * // TODO: is this the correct way...
     * pub(crate) fn increase_ref(&mut self) {
     *     self.refcount += 1;
     * }
     */
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
    // TODO: add docs
    pub(crate) fn bid_in_maps(
        &mut self,
        exe: &RustloadExe,
        state: &RustloadState,
    ) {
        // FIXME: (original author) use exemap->prob, needs some theory work.
        let mut map = self.map.borrow_mut();
        if exe.is_running(state) {
            map.lnprob = 1.0.into();
        } else {
            map.lnprob += exe.lnprob;
        }
    }

    /// Add new `map` using `Rc::clone(&map)`.
    pub(crate) fn new(map: RcCell<RustloadMap>) -> Self {
        Self {
            map,
            prob: 1.0.into(),
        }
    }

    /// Write exemap data into the database.
    pub(crate) fn write_exemap(
        &self,
        exe: &RustloadExe,
        conn: &SqliteConnection,
    ) -> Result<()> {
        let new_exemap = models::NewExeMap {
            seq: &exe.seq,
            map_seq: &self.map.borrow().seq,
            prob: &*self.prob,
        };

        diesel::insert_into(schema::exemaps::table)
            .values(&new_exemap)
            .execute(conn)
            .log_on_err("Failed to insert exemap into database")?;

        Ok(())
    }
}

/// An Exe object corresponds to an application. An Exe is identified by the
/// path of its executable binary, and as its persistent data it contains the
/// set of maps it uses and the set of Markov chains it builds with every other
/// application.
///
/// The runtime property of the Exe is its running state which is a boolean
/// variable represented as an integer with value one if the application is
/// running, and zero otherwise. The running member is initialized upon
/// construction of the object, based on information from `/proc`.
///
/// The size of an Exe is the sum of the size of its Map objects.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RustloadExe<'a, T: 'a = RustloadMarkov<'a>> {
    /// Absolute path of the executable.
    path: PathBuf,

    /// Total running time of the executable.
    time: i32,

    /// Last time it was probed.
    update_time: i32,

    /// Set of markov chain with other exes.
    markovs: BTreeSet<*const T>,

    /// Set of `RustloadExeMap` structures.
    exemaps: BTreeSet<RustloadExeMap>,

    /// sum of the size of maps.
    size: usize,

    /// Last time it was running.
    running_timestamp: i32,

    /// Time when exe stopped/started running.
    change_timestamp: i32,

    /// log-probability of NOT being needed in the next period.
    lnprob: OrderedFloat<f64>,

    /// Unique exe sequence number.
    seq: i32,

    /// Tells the compiler that `T` will have a lifetime of `'a`
    phantom: PhantomData<&'a T>,
}

impl<'a> RustloadExe<'a> {
    #[inline]
    pub(crate) fn prob_print(&self) {
        log::warn!("ln(prob(~EXE)) = {}    {:?}", self.lnprob, self.path);
    }

    pub(crate) fn zero_prob(&mut self) {
        self.lnprob = 0.0.into();
    }

    pub(crate) fn read_exe(
        state: &mut RustloadState,
        conn: &SqliteConnection,
    ) {
        use schema::exes::dsl::*;
        // TODO: Implement this
        let exe: models::Exe = exes.filter(id.eq(5)).first(conn).unwrap();
    }

    /// Add an exemap state to the set of exemaps.
    pub(crate) fn add_exemap(&mut self, value: RustloadExeMap) {
        self.exemaps.insert(value);
    }

    /// Add a markov state to the set of markovs.
    ///
    /// This is an unsafe function. Use `add_markov` to achieve the same
    /// result, safely.
    pub(crate) unsafe fn add_markov_unsafe(
        &mut self,
        value: *const RustloadMarkov<'a>,
    ) {
        self.markovs.insert(value);
    }

    /// Add a markov state to the set of markovs.
    pub(crate) fn add_markov(&mut self, value: &RustloadMarkov<'a>) {
        self.markovs.insert(value);
    }

    #[inline]
    pub(crate) fn is_running(&self, state: &RustloadState) -> bool {
        self.running_timestamp >= state.last_running_timestamp
    }

    pub(crate) fn new(
        path: impl Into<PathBuf>,
        running: bool,
        exemaps: Option<BTreeSet<RustloadExeMap>>,
        state: &RustloadState,
    ) -> Self {
        let path = path.into();
        let mut size = 0;
        let time = 0;
        let change_timestamp = state.time;

        let (update_time, running_timestamp);
        if running {
            update_time = state.last_running_timestamp;
            running_timestamp = state.last_running_timestamp;
        } else {
            update_time = -1;
            running_timestamp = update_time;
        }

        // TODO: think about `*mut RustloadExeMap`
        // looks like we are creating `exemaps` in one place. I hope this means
        // I can own the value, instead of shitting references all over the
        // place.
        let exemaps = match exemaps {
            Some(exemaps) => {
                exemaps.iter().map(|em| size += em.map.borrow().get_size());
                exemaps
            }
            None => Default::default(),
        };

        Self {
            path,
            size,
            time,
            change_timestamp,
            update_time,
            running_timestamp,
            exemaps,
            lnprob: 0.0.into(),
            seq: 0,
            markovs: Default::default(),
            phantom: PhantomData,
        }
    }

    /// Write exe data into the database.
    pub(crate) fn write_exe(&self, conn: &SqliteConnection) -> Result<()> {
        let uri = filename_to_uri(&self.path)
            .log_on_err("Failed to parse filepath")?;

        let new_exe = models::NewExe {
            seq: &self.seq,
            update_time: &self.update_time,
            time: &self.time,
            uri: uri.as_str(),
        };

        diesel::insert_into(schema::exes::table)
            .values(&new_exe)
            .execute(conn)
            .log_on_err("Failed to insert exe into database")?;

        Ok(())
    }
}

/// A Markov object corresponds to the four-state continuous-time Markov chain
/// constructed for two applications _A_ and _B_. The states are numbered 0 to
/// 3 and respectively mean:
///
/// - 0 if none of _A_ or _B_ is running,
/// - 1 if only _A_ is running,
/// - 2 if only _B_ is running,
/// - 3 if both are running.
///
/// A Markov object is identified by its links to the Exes _A_ and _B_, and has
/// as its persistent data the (exponentially-fading mean of) transition time
/// for each state, timestamp of when the last transition from that state
/// happened, and probability that each outgoing transition edge is taken when
/// a transition happens.
///
/// The runtime property of a Markov is its current state and the timestamp of
/// when it entered the current state. Upon construction, the current state is
/// computed based on the `running` member of the two Exe objects referenced,
/// and transition time is set to the current timestamp.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RustloadMarkov<'a> {
    /// Involved exe `a`.
    a: RcCell<RustloadExe<'a>>,

    /// Involved exe `b`.
    b: RcCell<RustloadExe<'a>>,

    /// Current state
    state: i32,

    // TODO: Should this be passed or kept as a ref?
    rustload_state: &'a RustloadState<'a>,

    /// Total time both exes have been running simultaneously (state 3).
    time: i32,

    /// Mean time to leave each state
    time_to_leave: ArrayN<4>,

    /// Number of times we've got from state `i` to state `j`. `weight[i][j]`
    /// is the number of times we have left state `i` (sum over `weight[i][j]`)
    /// for `j<>i` essentially.
    weight: ArrayNxN<4>,

    /// The time we entered the current state.
    change_timestamp: i32,

    cycle: u32,

    _marker: PhantomPinned,
}

impl<'a> RustloadMarkov<'a> {
    /// Computes the _P(Y runs in next period | current state)_
    /// and bids in for the _Y_. _Y_ should not be running.
    ///
    /// _Y = 1_ if it's needed in next period, 0 otherwise.
    /// Probability inference follows:
    ///
    /// ```none
    /// P(Y=1) = 1 - P(Y=0)
    /// P(Y=0) = Π P(Y=0|Xi)
    /// P(Y=0|Xi) = 1 - P(Y=1|Xi)
    /// P(Y=1|Xi) = P(state change of Y,X) * P(next state has Y=1) * corr(Y,X)
    /// corr(Y=X) = regularized |correlation(Y,X)|
    /// ```
    ///
    /// So:
    ///
    /// ```none
    /// lnprob(Y) = log(P(Y=0)) = Σ log(P(Y=0|Xi)) = Σ log(1 - P(Y=1|Xi))
    /// ```
    pub(crate) fn bid_for_exe(
        self: Pin<&Self>,
        y: &mut RustloadExe,
        ystate: i32,
        correlation: f64,
    ) {
        let state = self.state as usize;

        if self.weight[state][state] == 0 || !self.time_to_leave[state] > 1 {
            return;
        }

        let p_state_change = -(self.cycle as f64 * 1.5
            / self.time_to_leave[state] as f64)
            .exp_m1();

        let mut p_y_runs_next = self.weight[state][ystate as usize] as f64
            + self.weight[state][3] as f64;
        p_y_runs_next /= self.weight[state][state] as f64 + 0.01;

        // putting a fixme here until I figure out the author's purpose
        // FIXME: what should we do we correlation w.r.t. state?
        let correlation = correlation.abs();
        let p_runs = correlation * p_state_change * p_y_runs_next;

        y.lnprob += (1.0 - p_runs).log(std::f64::consts::E);
    }

    // TODO: Write doc
    pub(crate) fn bid_in_exes(self: Pin<&mut Self>, usecorrelation: bool) {
        if self.weight[self.state as usize][self.state as usize] == 0 {
            return;
        }

        let correlation = if usecorrelation {
            self.as_ref().correlation()
        } else {
            1.0
        };

        self.as_ref()
            .bid_for_exe(&mut self.a.borrow_mut(), 1, correlation);
        self.as_ref()
            .bid_for_exe(&mut self.b.borrow_mut(), 2, correlation);
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
    pub(crate) fn correlation(self: Pin<&Self>) -> f64 {
        let t = self.rustload_state.time;
        let (a, b) = (self.a.borrow().time, self.b.borrow().time);
        let ab = self.time;

        let (correlation, numerator, denominator2);

        if a == 0 || a == t || b == 0 || b == t {
            correlation = 0.0;
        } else {
            numerator = (t * ab) - (a * b);
            denominator2 = (a * b) * ((t - a) * (t - b));
            correlation = numerator as f64 / f64::sqrt(denominator2 as f64)
        }
        correlation
    }

    // TODO: yet to implement `initialize` var
    pub(crate) fn new(
        a: RcCell<RustloadExe<'a>>,
        b: RcCell<RustloadExe<'a>>,
        cycle: u32,
        rustload_state: &'a RustloadState<'a>,
    ) -> Pin<Box<Self>> {
        let mut state = markov_state(&a.borrow(), &b.borrow(), rustload_state);
        let mut change_timestamp = rustload_state.time;

        {
            let a_ref = a.borrow();
            let b_ref = b.borrow();

            if a_ref.change_timestamp > 0 && b_ref.change_timestamp > 0 {
                if a_ref.change_timestamp < rustload_state.time {
                    change_timestamp = a_ref.change_timestamp
                }
                if b_ref.change_timestamp < rustload_state.time
                    && b_ref.change_timestamp > change_timestamp
                {
                    change_timestamp = a_ref.change_timestamp
                }
                if a_ref.change_timestamp > change_timestamp {
                    state ^= 1
                }
                if b_ref.change_timestamp > change_timestamp {
                    state ^= 2
                }
            }
        }

        let mut markov = Box::pin(Self {
            a,
            b,
            state,
            rustload_state,
            change_timestamp,
            cycle,
            time: 0,
            time_to_leave: Default::default(),
            weight: Default::default(),
            _marker: Default::default(),
        });

        Self::state_changed(markov.as_mut());

        let value: *const Self = &*markov;
        unsafe {
            markov.a.borrow_mut().add_markov_unsafe(value);
            markov.b.borrow_mut().add_markov_unsafe(value);
        }

        markov
    }

    // FIXME: Find some other way to use `self`.
    /// The markov update algorithm.
    pub(crate) fn state_changed(self: Pin<&mut Self>) {
        if self.change_timestamp == self.rustload_state.time {
            return;
        }

        let old_state = self.state as usize;
        let new_state = markov_state(
            &self.a.borrow(),
            &self.b.borrow(),
            self.rustload_state,
        ) as usize;

        if old_state == new_state {
            log::warn!("old_state is equal to new_state");
            return;
        }

        let this = unsafe { self.get_unchecked_mut() };

        this.weight[old_state][old_state] += 1;
        this.time_to_leave[old_state] += ((this.rustload_state.time
            - this.change_timestamp)
            - this.time_to_leave[old_state])
            / this.weight[old_state][old_state];

        this.weight[old_state][new_state] += 1;
        this.state = new_state as i32;
        this.change_timestamp = this.rustload_state.time;
    }

    /// Write the markov data to the database.
    pub(crate) fn write_markov(
        self: Pin<&Self>,
        conn: &SqliteConnection,
    ) -> Result<()> {
        let v_weight = rmp_serde::to_vec(&self.weight)
            .log_on_err("Failed to serialize weight matrix")
            .with_context(|| "Failed to serialize weight matrix")?;

        let v_ttl = rmp_serde::to_vec(&self.time_to_leave)
            .log_on_err("Failed to serialize ttl array")
            .with_context(|| "Failed to serialize ttl array")?;

        let new_markov = models::NewMarkov {
            a_seq: &self.a.borrow().seq,
            b_seq: &self.b.borrow().seq,
            time: &self.time,
            time_to_leave: &*v_ttl,
            weight: &*v_weight,
        };

        diesel::insert_into(schema::markovs::table)
            .values(&new_markov)
            .execute(conn)
            .log_on_err("Failed to insert markov to the database")?;

        Ok(())
    }
}

/// The State object holds all the information about the model except for
/// configuration parameters. It contains the set of all applications and maps
/// known, and also a runtime list of running applications and memory
/// statistics which are populated from `/proc` when a State object is
/// constructed.
///
/// There is a singleton instance of this object at runtime that is trained by
/// the data gathering component, and used by the predictor. It has methods to
/// read its persistent state from a file and to dump them into a file. This
/// will load/save all referenced Markov, Exe, and Map objects recursively.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RustloadState<'a> {
    /// Total seconds that rustload has been running, from the beginning of the
    /// persistent state.
    time: i32,

    /// Map of known applications, indexed by exe name.
    exes: BTreeMap<PathBuf, RcCell<RustloadExe<'a>>>,

    /// Set of applications that rustload is not interested in. Typically it is
    /// the case that these applications are too small to be a candidate for
    /// preloading.
    /// Mapped value is the size of the binary (sum of the length of the maps).
    bad_exes: BTreeMap<PathBuf, usize>,

    /// Set of maps used by known executables, indexed by `RustloadMap`
    /// structure.
    maps: BTreeMap<RustloadMap, usize>,

    // runtime section:
    /// Set of exe structs currently running.
    running_exes: Vec<PathBuf>,

    // TODO: What to do with `GPtrArray* maps_arr`?
    // Looks like we can utilize `maps`'s keys, since all we want is a sorted
    // array of paths
    // maps_arr: Vec<PathBuf>,
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
    // TODO: memstat: RustloadMemory
    // We can use some crate...
    /// Last time we updated the memory stats.
    memstat_timestamp: i32,
}

impl<'a> RustloadState<'a> {
    pub(crate) fn dump_log(&self) {
        log::info!("Dump log requested!");
        log::warn!(
            indoc! {"Dump log:
            Persistent state stats:
                preload time = {}
                num exes = {}
                num bad exes = {}
                num maps = {}

            Runtime state stats:
                num running exes = {}"},
            self.time,
            self.exes.len(),
            self.bad_exes.len(),
            self.maps.len(),
            self.running_exes.len()
        );
        log::info!("state dump log done!")
    }

    pub(crate) fn load(statefile: impl AsRef<Path>) -> std::io::Result<()> {
        let statefile = statefile.as_ref();

        let exes: BTreeMap<PathBuf, usize> = Default::default();
        let bad_exes: BTreeMap<PathBuf, usize> = Default::default();
        let maps: BTreeMap<PathBuf, usize> = Default::default();

        // TODO: Add some file handling
        let file = File::open(statefile)
            .log_on_err(format!("Error opening file: {:?}", statefile))?;
        let buffer = BufReader::new(file);

        log::info!("Loading state from {:?}", statefile);

        // TODO: Fix this up

        Ok(())
    }

    pub(crate) fn read_state(&mut self, conn: &SqliteConnection) {
        // TODO:
    }

    // TODO: implement this
    pub(crate) fn register_exe(
        &self,
        exe: RcCell<RustloadExe<'a>>,
        state: &mut RustloadState<'a>,
        create_markovs: bool,
    ) -> Result<()> {
        state
            .exes
            .get(&exe.borrow().path)
            .with_context(|| "exe not in state.exes")?;

        state.exe_seq += 1;
        exe.borrow_mut().seq = state.exe_seq;

        if create_markovs {
            // TODO: Understand the author's intentions
            state.exes.iter_mut().map(|(k, v)| {
                // NOTE: As far as I understand, in the original C code, the
                // author wanted a mutable ref to RustloadExe
            });
        }
        state
            .exes
            .insert(exe.borrow().path.clone(), Rc::clone(&exe));
        Ok(())
    }

    pub(crate) fn run(&self, statefile: impl AsRef<Path>) {
        let statefile = statefile.as_ref();
        // TODO:
    }

    pub(crate) fn save(&self, statefile: impl AsRef<Path>) {
        let statefile = statefile.as_ref();
        // TODO:
    }

    pub(crate) fn unregister_exe(&self, exe: &RustloadExe) {
        // TODO:
    }

    pub(crate) fn register_map(&mut self, map: RustloadMap) {
        // TODO:
    }

    pub(crate) fn unregister_map(&mut self, map: RustloadMap) {
        // TODO:
    }
}
