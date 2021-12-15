// vim:set et sw=4 ts=4 tw=79 fdm=marker:
//! Rustload persistent state handling routines.
//!
//! Most of the documentation here is adapted from the original thesis of
//! `preload` by Behdad Esfahbod. See [Rustload's documentation][super] for
//! more information.
// TODO: Add more details and explanation.

// use ndarray::{Array1, Array2};
use crate::{
    common::{LogResult, RcCell, RcCellNew},
    proc::MemInfo,
    schema,
};
use anyhow::{Context, Result};
use clap::crate_version;
use diesel::prelude::*;
use indoc::indoc;
use log::Level;
use ordered_float::OrderedFloat;
use semver::Version;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::BufReader,
    marker::PhantomPinned,
    ops::Deref,
    path::{Path, PathBuf},
    pin::Pin,
    rc::Rc,
};
use url::Url;

/// Hosts all the types required to fetch from and insert values into the
/// database.
#[doc(hidden)]
pub(crate) mod models {
    use crate::database::table_creator;
    use crate::schema::*;

    table_creator! {
        State {
            version: String,
            time: i32,
        },
        "states",
        NewState,
    }

    table_creator! {
        BadExe {
            update_time: i32,
            uri: String,
        },
        "badexes",
        NewBadExe,
    }

    table_creator! {
        ExeMap {
            seq: i32,
            map_seq: i32,
            prob: f64,
        },
        "exemaps",
        NewExeMap,
    }

    table_creator! {
        Exe {
            seq: i32,
            update_time: i32,
            time: i32,
            uri: String,
        },
        "exes",
        NewExe,
    }

    table_creator! {
        Map {
            seq: i32,
            update_time: i32,
            offset: i32,
            length: i64,
            uri: String,
        },
        "maps",
        NewMap,
    }

    table_creator! {
        MarkovState {
            a_seq: i32,
            b_seq: i32,
            time: i32,
            time_to_leave: Vec<u8>,
            weight: Vec<u8>,
        },
        "markovstates",
        NewMarkovState,
    }
} /* models */

/// Represents an vector of `f64` with `N` elements. Since default values for
/// const generics are experimental at the time of writing, it must be assumed
/// that `N` is equal to `4`.
pub(crate) type ArrayN<const N: usize> = [OrderedFloat<f64>; N];

/// Represents an `N x N` nested array of `i32`. Since default values for const
/// generics are experimental at the time of writing, it must be assumed that
/// `N` is equal to `4`.
pub(crate) type ArrayNxN<const N: usize> = [[i32; N]; N];

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
fn uri_to_filename(uri: impl AsRef<str>) -> Result<PathBuf> {
    Url::parse(uri.as_ref())?
        .to_file_path()
        .map_err(|_| anyhow::anyhow!("Failed to parse filepath"))
}

/// Used to treat path-like objects as badexes and write them to the database.
pub(crate) trait ReadWriteBadExe: AsRef<Path> {
    /// Writes information about the badexes in the database, along with its
    /// update times.
    ///
    /// The [path][Self] is converted to a [`Url`].
    fn write_badexes(
        badexes_utimes: &[(&Self, &usize)],
        conn: &SqliteConnection,
    ) -> Result<()> {
        let mut db_badexes = vec![];
        db_badexes.reserve_exact(badexes_utimes.len());

        for (badexe, utime) in badexes_utimes {
            db_badexes.push(models::NewBadExe {
                update_time: **utime as i32,
                uri: filename_to_uri(badexe)
                    .log_on_err(Level::Error, "Failed to parse filepath")?
                    .to_string(),
            })
        }

        diesel::insert_into(schema::badexes::table)
            .values(&db_badexes)
            .execute(conn)
            .log_on_err(
                Level::Error,
                "Failed to insert badexe into database",
            )?;

        Ok(())
    }

    fn read_all(conn: &SqliteConnection, state: &mut State) -> Result<()> {
        use schema::badexes::dsl::*;

        let db_badexes: Vec<models::BadExe> = badexes.load(conn)?;
        for db_badexe in db_badexes {
            state.bad_exes.insert(
                uri_to_filename(&db_badexe.uri)?,
                db_badexe.update_time as usize,
            );
        }
        Ok(())
    }
}

impl ReadWriteBadExe for Path {}
impl ReadWriteBadExe for PathBuf {}

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
pub(crate) struct Map {
    /// absolute path of the mapped file.
    pub(crate) path: PathBuf,

    /// offset in bytes
    pub(crate) offset: usize,

    /// length in bytes
    pub(crate) length: usize,

    /// last time it was probed
    #[derivative(PartialEq = "ignore")]
    update_time: i32,

    // runtime section:
    // number of exes linking to this.
    // TODO: Can `Rc<...>` or `Arc<...>` work here instead of `refcount`
    // refcount: i32,
    /// log-probability of NOT being needed in next period.
    #[derivative(PartialEq = "ignore")]
    pub(crate) lnprob: OrderedFloat<f64>,

    /// unique map sequence number.
    #[derivative(PartialEq = "ignore")]
    seq: i32,

    /// on-disk location of the start of the map.
    #[derivative(PartialEq = "ignore")]
    pub(crate) block: i64,

    /// for private local use of functions.
    #[derivative(PartialEq = "ignore")]
    private: i32,
    // The state TODO:
    // state: State,
}

impl Map {
    fn read_all(
        conn: &SqliteConnection,
        state: &mut State,
    ) -> Result<BTreeMap<i32, RcCell<Map>>> {
        use schema::maps::dsl::*;

        let db_maps: Vec<models::Map> = maps.load(conn)?;
        let mut map_seqs = BTreeMap::new();

        for db_map in db_maps {
            let mut map = Map::new(
                uri_to_filename(db_map.uri)?,
                db_map.offset as usize,
                db_map.length as usize,
            );
            map.update_time = db_map.update_time;

            let map = RcCell::new_cell(map);

            // this solves our map lookup in exemaps!
            // TODO: what about duplicate objects as in original?
            map_seqs.insert(db_map.seq, Rc::clone(&map));

            state.register_map(map);
        }

        Ok(map_seqs)
    }

    #[inline]
    pub(crate) const fn get_size(&self) -> usize {
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

    pub(crate) fn write_maps(
        maps: &[&RcCell<Self>],
        conn: &SqliteConnection,
    ) -> Result<()> {
        let mut db_maps = vec![];
        db_maps.reserve_exact(maps.len());

        for each in maps {
            let each = each.borrow();

            db_maps.push(models::NewMap {
                seq: each.seq,
                update_time: each.update_time,
                offset: each.offset as i32,
                length: each.length as i64,
                uri: filename_to_uri(&each.path)
                    .log_on_err(Level::Error, "Failed to parse filepath")?
                    .to_string(),
            })
        }

        diesel::insert_into(schema::maps::table)
            .values(&db_maps)
            .execute(conn)
            .log_on_err(Level::Error, "Failed to insert map into database")?;

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
pub(crate) struct ExeMap {
    /// TODO: ...or can we use a Rc/Arc<Map> here?
    pub(crate) map: RcCell<Map>,

    /// Probability that this map will be used when an exe is running.
    prob: OrderedFloat<f64>,
}

impl ExeMap {
    fn add_map_size(&self, exe: &RcCell<Exe>) {
        exe.borrow_mut().size += self.map.borrow().get_size();
    }

    fn new_exe_map(exe: RcCell<Exe>, map: RcCell<Map>, prob: f64) {
        let mut this = Self::new(Rc::clone(&map));
        this.add_map_size(&exe);
        this.prob = OrderedFloat(prob);
        exe.borrow_mut().exemaps.insert(this);
    }

    fn read_all(
        conn: &SqliteConnection,
        state: &mut State,
        exe_seqs: &BTreeMap<i32, RcCell<Exe>>,
        map_seqs: &BTreeMap<i32, RcCell<Map>>,
    ) -> Result<()> {
        use schema::exemaps::dsl::*;

        let db_exemaps: Vec<models::ExeMap> = exemaps.load(conn)?;

        for db_exemap in db_exemaps {
            let exe = exe_seqs.get(&db_exemap.seq);
            let map = map_seqs.get(&db_exemap.map_seq);

            if exe != None || map != None {
                anyhow::bail!("invalid index for exemap's exe and/or map")
            }

            // and thus we insert the exemap while simutaneously creating it.
            Self::new_exe_map(
                Rc::clone(exe.unwrap()),
                Rc::clone(map.unwrap()),
                db_exemap.prob,
            );
        }

        Ok(())
    }

    /// Add new `map` using `Rc::clone(&map)`.
    pub(crate) fn new(map: RcCell<Map>) -> Self {
        Self {
            map,
            prob: 1.0.into(),
        }
    }

    /// Write exemaps data into the database.
    pub(crate) fn write_exemaps(
        exemaps: &[&Self],
        exe: &Exe,
        conn: &SqliteConnection,
    ) -> Result<()> {
        let mut db_exemaps = vec![];
        db_exemaps.reserve_exact(exemaps.len());

        for each in exemaps {
            let map = each.map.borrow();
            db_exemaps.push(models::NewExeMap {
                seq: exe.seq,
                map_seq: map.seq,
                prob: *each.prob,
            })
        }

        diesel::insert_into(schema::exemaps::table)
            .values(&db_exemaps)
            .execute(conn)
            .log_on_err(
                Level::Error,
                "Failed to insert exemap into database",
            )?;

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
pub(crate) struct Exe {
    /// Absolute path of the executable.
    pub(crate) path: PathBuf,

    /// Total running time of the executable.
    pub(crate) time: i32,

    /// Last time it was probed.
    update_time: i32,

    /// Set of markov chain with other exes.
    pub(crate) markovs: BTreeSet<MarkovStateWrapper>,

    /// Set of [`ExeMap`] structures.
    pub(crate) exemaps: BTreeSet<ExeMap>,

    /// sum of the size of maps.
    size: usize,

    /// Last time it was running.
    pub(crate) running_timestamp: i32,

    /// Time when exe stopped/started running.
    pub(crate) change_timestamp: i32,

    /// log-probability of NOT being needed in the next period.
    pub(crate) lnprob: OrderedFloat<f64>,

    /// Unique exe sequence number.
    seq: i32,
}

impl Exe {
    pub(crate) fn read_all(
        conn: &SqliteConnection,
        state: &mut State,
        cycle: u32,
    ) -> Result<BTreeMap<i32, RcCell<Exe>>> {
        use schema::exes::dsl::*;
        let db_exes: Vec<models::Exe> = exes.load(conn)?;
        let mut exe_seqs = BTreeMap::new();

        for db_exe in db_exes {
            let mut exe =
                Exe::new(uri_to_filename(db_exe.uri)?, false, None, state);
            exe.change_timestamp = -1;
            exe.update_time = db_exe.update_time;
            exe.time = db_exe.time;

            let exe = RcCell::new_cell(exe);
            state.register_exe(Rc::clone(&exe), false, cycle)?;

            // this solves our lookup in exemap!
            exe_seqs.insert(db_exe.seq, exe);
        }

        Ok(exe_seqs)
    }

    /// Add an exemap state to the set of exemaps.
    pub(crate) fn add_exemap(&mut self, value: ExeMap) {
        self.exemaps.insert(value);
    }

    /// Add a markov state to the set of markovs.
    ///
    /// This is an unsafe function.
    pub(crate) unsafe fn add_markov_unsafe(
        &mut self,
        value: MarkovStateWrapper,
    ) {
        self.markovs.insert(value);
    }

    /// Add a markov state to the set of markovs.
    // pub(crate) fn add_markov(&mut self, value: &mut MarkovState) {
    //     self.markovs.insert(value);
    // }

    pub(crate) const fn is_running(&self, state: &State) -> bool {
        self.running_timestamp >= state.last_running_timestamp
    }

    pub(crate) fn new(
        path: impl Into<PathBuf>,
        is_running: bool,
        exemaps: Option<BTreeSet<ExeMap>>,
        state: &State,
    ) -> Self {
        let path = path.into();
        let mut size = 0;
        let time = 0;
        let change_timestamp = state.time;

        let (update_time, running_timestamp);
        if is_running {
            update_time = state.last_running_timestamp;
            running_timestamp = state.last_running_timestamp;
        } else {
            update_time = -1;
            running_timestamp = update_time;
        }

        // TODO: think about `*mut ExeMap`
        // looks like we are creating `exemaps` in one place. I hope this means
        // I can own the value, instead of shitting references all over the
        // place.
        let exemaps = match exemaps {
            Some(exemaps) => {
                exemaps
                    .iter()
                    .for_each(|em| size += em.map.borrow().get_size());
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
        }
    }

    /// Write exes data into the database.
    pub(crate) fn write_exes(
        exes: &[&RcCell<Self>],
        conn: &SqliteConnection,
    ) -> Result<()> {
        let mut db_exes = vec![];
        db_exes.reserve_exact(exes.len());

        for each in exes {
            let each = each.borrow();

            db_exes.push(models::NewExe {
                seq: each.seq,
                update_time: each.update_time,
                time: each.time,
                uri: filename_to_uri(&each.path)
                    .log_on_err(Level::Error, "Failed to parse filepath")?
                    .to_string(),
            })
        }

        diesel::insert_into(schema::exes::table)
            .values(&db_exes)
            .execute(conn)
            .log_on_err(Level::Error, "Failed to insert exe into database")?;

        Ok(())
    }
}

/// A Markov object corresponds to the four-state continuous-time Markov chain
/// constructed for two applications $A$ and $B$. The states are numbered 0 to
/// 3 and respectively mean:
///
/// - 0 if none of $A$ or $B$ is running,
/// - 1 if only $A$ is running,
/// - 2 if only $B$ is running,
/// - 3 if both are running.
///
/// A Markov object is identified by its links to the Exes $A$ and $B$, and has
/// as its persistent data the (exponentially-fading mean of) transition time
/// for each state, timestamp of when the last transition from that state
/// happened, and probability that each outgoing transition edge is taken when
/// a transition happens.
///
/// The runtime property of a Markov is its current state and the timestamp of
/// when it entered the current state. Upon construction, the current state is
/// computed based on the `running` member of the two Exe objects referenced,
/// and transition time is set to the current timestamp.
#[derive(Derivative)]
#[derivative(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct MarkovState {
    /// Involved exe `a`.
    #[derivative(PartialOrd = "ignore")]
    #[derivative(PartialEq = "ignore")]
    #[derivative(Ord = "ignore")]
    pub(crate) a: RcCell<Exe>,

    /// Involved exe `b`.
    #[derivative(PartialOrd = "ignore")]
    #[derivative(PartialEq = "ignore")]
    #[derivative(Ord = "ignore")]
    pub(crate) b: RcCell<Exe>,

    /// Current state
    pub(crate) state: i32,

    /// Total time both exes have been running simultaneously (state 3).
    pub(crate) time: i32,

    /// Mean time to leave each state
    pub(crate) time_to_leave: ArrayN<4>,

    /// Number of times we've got from state $i$ to state $j$.
    /// $\text{weight}\_{ij}$ is the number of times we have left state $i$
    /// (sum over $\text{weight}\_{ij}$).
    pub(crate) weight: ArrayNxN<4>,

    /// The time we entered the current state.
    change_timestamp: i32,

    pub(crate) cycle: u32,

    _marker: PhantomPinned,
}

// MarkovStateWrapper {{{1 //

/// Wraps a raw mutable pointer to [`MarkovState`] as a workaround for lifetime
/// checker's limitations. It provides access to the internal object via
/// [`Deref`] and [`DerefMut`], but provides no guarantees whatsoever for
/// memory safety. As a result, `MarkovState` implements [`Drop`] in such a way
/// that the raw pointer is removed from [`Exe`] once it goes out of scope.
///
/// # Rationale
///
/// We need to store a mutable reference to `MarkovState` in `Exe` during its
/// initialization. But, doing so is problematic since Rust's normal borrowing
/// rules don't allow building such things.
///
/// One alternative was to use a combination of [`Rc`] and
/// [`Weak`](std::rc::Weak) to build a self-referential struct and thus avoid
/// numerous `unsafe`s altogether.  However, **it is difficult to construct a
/// `Weak` or and `Rc` from a reference.** This is crucial because
/// `MarkovState` needs to construct the `Weak` type to search and remove it
/// from `Exe`. Not doing so leads to a opening of the gates to nasty
/// dereference errors.
///
/// Using a raw pointer, we are able to not only circumvent the borrow
/// checker's limitations, but also safely remove the raw pointers once the
/// parent (ie, `MarkovState`) has been dropped. However, the burden of
/// watching out for dangling pointers lies on us too, althogh it is rare to
/// face one, given that the wrapper will hardly be cherry-picked and stored
/// aside.
///
/// In essence, the benefits of being able to perform the tasks mentioned above
/// far outweighs the associated risks.
#[repr(transparent)]
#[derive(Derivative)]
#[derivative(Debug = "transparent")]
pub(crate) struct MarkovStateWrapper(pub(crate) *mut MarkovState);

impl Deref for MarkovStateWrapper {
    type Target = MarkovState;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.0 }
    }
}

impl From<*mut MarkovState> for MarkovStateWrapper {
    fn from(value: *mut MarkovState) -> Self {
        Self(value)
    }
}

impl Eq for MarkovStateWrapper {}

impl PartialEq for MarkovStateWrapper {
    fn eq(&self, other: &Self) -> bool {
        let this = &**self;
        let other = &**other;
        this == other
    }
}

impl Ord for MarkovStateWrapper {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let this = &**self;
        let other = &**other;
        this.cmp(other)
    }
}

impl PartialOrd for MarkovStateWrapper {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let this = &**self;
        let other = &**other;
        this.partial_cmp(other)
    }
}
// 1}}} //

impl MarkovState {
    fn read_all(
        conn: &SqliteConnection,
        state: &State,
        exe_seqs: &BTreeMap<i32, RcCell<Exe>>,
        cycle: u32,
    ) -> Result<Vec<Pin<Box<MarkovState>>>> {
        use schema::markovstates::dsl::markovstates;

        let db_markovs: Vec<models::MarkovState> = markovstates.load(conn)?;
        let mut all_markovstates = vec![];

        for db_markov in db_markovs {
            let a = exe_seqs.get(&db_markov.a_seq);
            let b = exe_seqs.get(&db_markov.a_seq);

            if a != None || b != None {
                anyhow::bail!("invalid index for exes in markov states")
            }

            let mut markov_state = Self::new(
                Rc::clone(a.unwrap()),
                Rc::clone(b.unwrap()),
                cycle,
                false,
                state,
            );

            let time_to_leave: ArrayN<4> =
                rmp_serde::from_read_ref(&db_markov.time_to_leave)?;
            let weight: ArrayNxN<4> =
                rmp_serde::from_read_ref(&db_markov.weight)?;

            unsafe {
                let mut_markov = markov_state.as_mut().get_unchecked_mut();
                mut_markov.time_to_leave = time_to_leave;
                mut_markov.weight = weight;
            }

            // we have to keep the markovs alive
            all_markovstates.push(markov_state);
        }

        Ok(all_markovstates)
    }

    /// Calculates the correlation coefficient of the two random variable of
    /// the exes in this markov been running.
    ///
    /// The returned value is a number in the range $-1$ to $1$ that is a
    /// numeric measure of the strength of linear relationship between two
    /// random variables.  the correlation is $1$ in the case of an increasing
    /// linear relationship, $−1$ in the case of a decreasing linear
    /// relationship, and some value in between in all other cases, indicating
    /// the degree of linear dependence between the variables.  the closer the
    /// coefficient is to either $−1$ or $1$, the stronger the correlation
    /// between the variables.
    ///
    /// See [Correlation](https://en.wikipedia.org/wiki/Correlation) for more
    /// information.
    ///
    /// We calculate the Pearson product-moment correlation coefficient, which
    /// is found by dividing the covariance of the two variables by the product
    /// of their standard deviations. That is:
    ///
    /// $$
    /// \rho(a, b) = \frac{E(A \cdot B) - E(A) \cdot  E(B)} {\sqrt{E(A^2) -
    /// E^2(A)} \cdot \sqrt{E(B^2) - E^2(B)}}
    /// $$
    ///
    /// Where $A$ and $B$ are the random variables of exes `a` and `b` being
    /// run, with a value of `1` when running, and `0` when not. It's obvious
    /// to compute the above then, since:
    ///
    /// $$E(AB) = \frac {\text{markov.time}} {\text{state.time}}$$
    /// $$E(A) = \frac {\text{markov.a.time}} {\text{state.time}}$$
    /// $$E(A^2) = E(A)$$
    /// $$E^2(A) = E(A)^2$$
    /// same for $B$.
    pub(crate) fn correlation(self: Pin<&Self>, state: &State) -> f64 {
        let t = state.time;
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

    /// Calculates the `state` of the markov chain based on the running state
    /// of two exes.
    ///
    /// Read [`MarkovState`]'s documentation for more information.
    pub(crate) const fn get_markov_state(
        a: &Exe,
        b: &Exe,
        state: &State,
    ) -> i32 {
        (if a.is_running(state) { 1 } else { 0 })
            + (if b.is_running(state) { 2 } else { 0 })
    }

    pub(crate) fn new(
        a: RcCell<Exe>,
        b: RcCell<Exe>,
        cycle: u32,
        initialize: bool,
        state: &State,
    ) -> Pin<Box<Self>> {
        let mut markov_state = 0;
        let mut change_timestamp = 0;

        if initialize {
            let a_ref = a.borrow();
            let b_ref = b.borrow();

            markov_state = Self::get_markov_state(&a_ref, &b_ref, state);
            change_timestamp = state.time;

            if a_ref.change_timestamp > 0 && b_ref.change_timestamp > 0 {
                if a_ref.change_timestamp < state.time {
                    change_timestamp = a_ref.change_timestamp
                }
                if b_ref.change_timestamp < state.time
                    && b_ref.change_timestamp > change_timestamp
                {
                    change_timestamp = a_ref.change_timestamp
                }
                if a_ref.change_timestamp > change_timestamp {
                    markov_state ^= 1
                }
                if b_ref.change_timestamp > change_timestamp {
                    markov_state ^= 2
                }
            }
        }

        let mut markov = Box::pin(Self {
            a,
            b,
            state: markov_state,
            // state_ref: state,
            change_timestamp,
            cycle,
            time: 0,
            time_to_leave: Default::default(),
            weight: Default::default(),
            _marker: Default::default(),
        });

        if initialize {
            markov.as_mut().state_changed(state);
        }

        let value: *mut Self = unsafe { markov.as_mut().get_unchecked_mut() };
        unsafe {
            markov.a.borrow_mut().add_markov_unsafe(value.into());
            markov.b.borrow_mut().add_markov_unsafe(value.into());
        }

        markov
    }

    pub(crate) fn read_markov() {
        // TODO:
    }

    // FIXME: Find some other way to use `self`.
    /// The markov update algorithm.
    pub(crate) fn state_changed(self: Pin<&mut Self>, state: &State) {
        if self.change_timestamp == state.time {
            return;
        }

        let old_state = self.state as usize;
        let new_state =
            Self::get_markov_state(&self.a.borrow(), &self.b.borrow(), state)
                as usize;

        if old_state == new_state {
            log::warn!("old_state is equal to new_state");
            return;
        }

        let this = unsafe { self.get_unchecked_mut() };

        this.weight[old_state][old_state] += 1;
        // workaround: Reverse the subtraction as a workaround for no
        // `std::ops::Sub<OrderedFloat<T>>` for f64
        this.time_to_leave[old_state] += -(this.time_to_leave[old_state]
            - (state.time - this.change_timestamp) as f64)
            / this.weight[old_state][new_state] as f64;

        this.weight[old_state][new_state] += 1;
        this.state = new_state as i32;
        this.change_timestamp = state.time;
    }

    /// Write the markov data to the database.
    pub(crate) fn write_markovs(
        markovs: &[Pin<&Self>],
        conn: &SqliteConnection,
    ) -> Result<()> {
        let mut db_markovs = vec![];
        db_markovs.reserve_exact(markovs.len());

        for each in markovs {
            let a = each.a.borrow();
            let b = each.b.borrow();

            let v_ttl = rmp_serde::to_vec(&each.time_to_leave)
                .log_on_err(Level::Error, "Failed to serialize ttl array")
                .with_context(|| "Failed to serialize ttl array")?;

            let v_weight = rmp_serde::to_vec(&each.weight)
                .log_on_err(Level::Error, "Failed to serialize weight matrix")
                .with_context(|| "Failed to serialize weight matrix")?;

            db_markovs.push(models::NewMarkovState {
                a_seq: a.seq,
                b_seq: b.seq,
                time: each.time,
                time_to_leave: v_ttl,
                weight: v_weight,
            })
        }

        diesel::insert_into(schema::markovstates::table)
            .values(&db_markovs)
            .execute(conn)
            .log_on_err(
                Level::Error,
                "Failed to insert markov to the database",
            )?;

        Ok(())
    }
}

impl Drop for MarkovState {
    fn drop(&mut self) {
        // Remove self from the set to prevent errors.
        let this = (self as *mut Self).into();
        for i in [&self.a, &self.b] {
            i.borrow_mut().markovs.remove(&this);
        }
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
#[derive(PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) struct State {
    /// Total seconds that we have been running, from the beginning of the
    /// persistent state.
    pub(crate) time: i32,

    /// Map of known applications, indexed by exe name.
    pub(crate) exes: BTreeMap<PathBuf, RcCell<Exe>>,

    /// Set of applications that we are not interested in. Typically it is the
    /// case that these applications are too small to be a candidate for
    /// preloading.
    /// Mapped value is the size of the binary (sum of the length of the maps).
    pub(crate) bad_exes: BTreeMap<PathBuf, usize>,

    /// Set of maps used by known executables, indexed by `Map`
    /// structure.
    // TODO: Making them `RcCell` since they will be shared often, but is that
    // a good idea?
    pub(crate) maps: BTreeMap<RcCell<Map>, usize>,

    // runtime section:
    /// Set of exe structs currently running.
    pub(crate) running_exes: Vec<RcCell<Exe>>,

    // TODO: What to do with `GPtrArray* maps_arr`?
    // Looks like we can utilize `maps`'s keys, since all we want is a sorted
    // array of paths
    // maps_arr: Vec<PathBuf>,
    /// Increasing sequence of unique numbers to assign to maps.
    map_seq: i32,

    /// Increasing sequence of unique numbers to assign to exes.
    exe_seq: i32,

    /// Last time we checked for process' running.
    pub(crate) last_running_timestamp: i32,

    /// Last time we did accounting on running times, etc.
    pub(crate) last_accounting_timestamp: i32,

    /// Whether new scan has been performed since last save.
    dirty: bool,

    /// Whether new scan has been performed but no model update yet.
    model_dirty: bool,

    /// System memory stats.
    pub(crate) memstat: MemInfo,

    /// Last time we updated the memory stats.
    pub(crate) memstat_timestamp: i32,

    // TODO:
    pub(crate) state_changed_exes: Vec<RcCell<Exe>>,

    // TODO:
    pub(crate) new_running_exes: Vec<RcCell<Exe>>,

    /// Stores exes we've never seen before
    pub(crate) new_exes: BTreeMap<PathBuf, libc::pid_t>,
}

impl State {
    fn write_self(&self, conn: &SqliteConnection) -> Result<()> {
        diesel::replace_into(schema::states::table)
            .values(models::NewState {
                version: crate_version!().to_string(),
                time: self.time,
            })
            .execute(conn)
            .log_on_err(
                Level::Error,
                "Failed to insert state into database",
            )?;

        Ok(())
    }

    pub(crate) fn write_state(&self, conn: &SqliteConnection) -> Result<()> {
        // write my details first. If this fails, it means any further
        // validation in the future won't be possible, hence it would be
        // futile.
        self.write_self(conn)?;

        let mut is_error = Ok(());

        let maps: Vec<_> = self.maps.keys().collect();
        Map::write_maps(&maps, conn).unwrap_or_else(|v| is_error = Err(v));

        if is_error.is_ok() {
            let bad_exes_updtimes: Vec<_> = self.bad_exes.iter().collect();
            ReadWriteBadExe::write_badexes(&bad_exes_updtimes, conn)
                .unwrap_or_else(|e| is_error = Err(e));
        }

        if is_error.is_ok() {
            // NOTE: Several things are happening to exes at a time.
            let exes_to_write = self.exes.values().collect::<Vec<_>>();
            Exe::write_exes(&exes_to_write, conn)
                .unwrap_or_else(|e| is_error = Err(e));

            self.exes.values().for_each(|exe| {
                let exe = exe.borrow();

                // `preload_exemap_foreach`
                let exemaps: Vec<_> = exe.exemaps.iter().collect();
                ExeMap::write_exemaps(&exemaps, &exe, conn)
                    .unwrap_or_else(|e| is_error = Err(e));

                let markovs: Vec<_> = exe
                    .markovs
                    .iter()
                    .map(|v| unsafe { Pin::new_unchecked(&**v) })
                    .collect();
                MarkovState::write_markovs(&markovs, conn)
                    .unwrap_or_else(|e| is_error = Err(e));
            });
        }

        is_error
    }

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
        let file = File::open(statefile).log_on_err(
            Level::Error,
            format!("Error opening file: {:?}", statefile),
        )?;
        let buffer = BufReader::new(file);

        log::info!("Loading state from {:?}", statefile);

        // TODO: Fix this up

        Ok(())
    }

    // TODO: implement this!
    pub(crate) fn read_state(
        cycle: u32,
        conn: &SqliteConnection,
    ) -> Result<Self> {
        // load our state information
        use schema::states::dsl::states;
        let db_state: models::State = states.first(conn).log_on_err(
            Level::Error,
            "Failed to load state info from database",
        )?;

        // check versions
        let read_version = Version::parse(&db_state.version)?;
        let my_version = Version::parse(crate_version!())?;

        if my_version < read_version {
            log::warn!("State file is of a newer version, ignoring it.");
        } else {
            log::warn!("State file is of an older version.")
        }

        // last checked time
        let time = db_state.time;

        // create the state and keep it for further updates
        let mut this = Self::default();

        // update the timestamps
        this.time = time;
        this.last_accounting_timestamp = this.time;

        // fetch the maps keyed by their seq numbers.
        let map_seqs = Map::read_all(conn, &mut this)
            .log_on_err(Level::Error, "Failed to load maps from database.")?;

        // fetch the badexes
        Path::read_all(conn, &mut this).log_on_err(
            Level::Error,
            "Failed to load badexes from database.",
        )?;

        // fetch the exes keyed by their seq numbers.
        let exe_seqs = Exe::read_all(conn, &mut this, cycle)
            .log_on_err(Level::Error, "Failed to load exes from database.")?;

        ExeMap::read_all(conn, &mut this, &exe_seqs, &map_seqs).log_on_err(
            Level::Error,
            "Failed to load exes from the database.",
        )?;

        let markov_states =
            MarkovState::read_all(conn, &this, &exe_seqs, cycle).log_on_err(
                Level::Error,
                "Failed to load markov states from database.",
            )?;

        // let this = Self {
        //
        // }
        Ok(this)
    }

    // TODO: implement this
    pub(crate) fn register_exe(
        &mut self,
        exe: RcCell<Exe>,
        create_markovs: bool,
        cycle: u32,
    ) -> Result<Vec<Pin<Box<MarkovState>>>> {
        self.exes
            .get(&exe.borrow().path)
            .with_context(|| "exe not in state.exes")?;

        self.exe_seq += 1;
        exe.borrow_mut().seq = self.exe_seq;

        let mut markovs = vec![];

        if create_markovs {
            // TODO: Understand the author's intentions
            self.exes.values().for_each(|v| {
                // `shift_preload_markov_new(...)`
                if v != &exe {
                    markovs.push(MarkovState::new(
                        Rc::clone(v),
                        Rc::clone(&exe),
                        cycle,
                        true,
                        self,
                    ));
                }
            });
        }
        self.exes.insert(exe.borrow().path.clone(), Rc::clone(&exe));
        Ok(markovs)
    }

    pub(crate) fn unregister_exe(&mut self, exe: &Exe) -> Result<()> {
        self.exes.remove(&exe.path);
        Ok(())
    }

    pub(crate) fn run(&self, statefile: impl AsRef<Path>) {
        let statefile = statefile.as_ref();
        // TODO:
    }

    pub(crate) fn save(&mut self, statefile: impl AsRef<Path>) {
        let statefile = statefile.as_ref();
        // TODO:

        // clean once in a while
        self.bad_exes.clear();
    }

    // TODO: think about this later and write the docs
    pub(crate) fn register_map(&mut self, map: RcCell<Map>) -> Option<usize> {
        self.map_seq += 1;
        map.borrow_mut().seq += self.map_seq;
        self.maps.insert(map, 1)
    }

    // TODO: think about this later and write the docs
    pub(crate) fn unregister_map(
        &mut self,
        map: &RcCell<Map>,
    ) -> Option<usize> {
        self.maps.remove(map)
    }
}
