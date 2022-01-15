// vim:set et sw=4 ts=4 tw=79 fdm=marker:
//! Rustload persistent state handling routines.
//!
//! Most of the documentation here is adapted from the original thesis of
//! `preload` by Behdad Esfahbod. See [Rustload's documentation][super] for
//! more information.
// TODO: Add more details and explanation.

// use ndarray::{Array1, Array2};
use crate::{
    common::{DropperCell, LogResult, RcCell, RcCellNew, WeakCell},
    proc::{self, MemInfo},
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
    cmp::Ordering,
    collections::{btree_map::Entry, BTreeMap, BTreeSet},
    ops::Deref,
    path::{Path, PathBuf},
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
    fn write_all(
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

    /// Reads all the `BadExe` info from the database and inserts it into the
    /// [`State::bad_exes`] map, indexed by the update time.
    fn read_all(conn: &SqliteConnection, state: &mut State) -> Result<()> {
        use schema::badexes::dsl::*;

        // `optional` will handle the case where no data is present
        if let Some(db_badexes) =
            badexes.load::<models::BadExe>(conn).optional()?
        {
            for db_badexe in db_badexes {
                state.bad_exes.insert(
                    uri_to_filename(&db_badexe.uri)?,
                    db_badexe.update_time as usize,
                );
            }
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
#[derivative(Eq, PartialEq, Ord, PartialOrd, Debug)]
pub(crate) struct Map {
    /// absolute path of the mapped file.
    pub(crate) path: PathBuf,

    /// offset in bytes
    pub(crate) offset: usize,

    /// length in bytes
    pub(crate) length: usize,

    /// last time it was probed
    #[derivative(
        PartialEq = "ignore",
        PartialOrd = "ignore",
        Ord = "ignore",
        Debug = "ignore"
    )]
    update_time: i32,

    /// The state object.
    #[derivative(
        PartialEq = "ignore",
        PartialOrd = "ignore",
        Ord = "ignore",
        Debug = "ignore"
    )]
    state: WeakCell<State>,

    /// log-probability of NOT being needed in next period.
    #[derivative(
        PartialEq = "ignore",
        PartialOrd = "ignore",
        Ord = "ignore",
        Debug = "ignore"
    )]
    pub(crate) lnprob: OrderedFloat<f64>,

    /// unique map sequence number.
    #[derivative(
        PartialEq = "ignore",
        PartialOrd = "ignore",
        Ord = "ignore",
        Debug = "ignore"
    )]
    seq: i32,

    /// on-disk location of the start of the map.
    #[derivative(
        PartialEq = "ignore",
        PartialOrd = "ignore",
        Ord = "ignore",
        Debug = "ignore"
    )]
    pub(crate) block: i64,
}

impl Map {
    /// Reads the [`Map`] info from the database and returns a map of `Map`s
    /// indexed by its sequence number.
    fn read_all(
        conn: &SqliteConnection,
        state: &RcCell<State>,
    ) -> Result<BTreeMap<i32, RcCell<Map>>> {
        use schema::maps::dsl::*;

        let mut map_seqs = BTreeMap::new();

        // handle the case where no value is present, probably during first run
        if let Some(db_maps) = maps.load::<models::Map>(conn).optional()? {
            for db_map in db_maps {
                let map = Map::new(
                    uri_to_filename(db_map.uri)?,
                    db_map.offset as usize,
                    db_map.length as usize,
                    Rc::downgrade(state),
                );
                map.borrow_mut().update_time = db_map.update_time;

                if let Entry::Vacant(e) = map_seqs.entry(db_map.seq) {
                    e.insert(Rc::clone(&map));
                } else {
                    anyhow::bail!("Map index error")
                }

                state
                    .borrow_mut()
                    .register_map(Rc::clone(&map))
                    .log_on_err(Level::Warn, "Failed to register map")
                    .ok();
            }
        }

        Ok(map_seqs)
    }

    /// Returns the length of the [`Map`] in bytes.
    pub(crate) const fn get_size(&self) -> usize {
        self.length
    }

    pub(crate) fn new(
        path: impl Into<PathBuf>,
        offset: usize,
        length: usize,
        state: WeakCell<State>,
    ) -> DropperCell<Self> {
        DropperCell::new(
            Self {
                path: path.into(),
                offset,
                length,
                state,
                update_time: 0,
                block: -1,
                lnprob: 0.0.into(),
                seq: 0,
            },
            Some(|v| {
                let state = &v.borrow().state;
                if let Some(state) = state.upgrade() {
                    state.borrow_mut().unregister_map(v);
                }
            }),
        )
    }

    /// Writes [`Map`] info to the database.
    pub(crate) fn write_all(
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
}

/// Holds information about a mapped section in an exe.
/// TODO: Describe in details.
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct ExeMap {
    pub(crate) map: RcCell<Map>,

    /// Probability that this map will be used when an exe is running.
    prob: OrderedFloat<f64>,
}

impl ExeMap {
    /// Adds the size of the [`Map`] to the total size of the maps in an
    /// [`Exe`].
    #[inline]
    fn add_map_size(&self, exe: &mut Exe) {
        exe.size += self.map.borrow().get_size();
    }

    /// Creates an [`ExeMap`], registers a [`Map`] with itself and registers
    /// itself with an [`Exe`] in one go.
    fn new_exe_map(
        exe: &mut Exe,
        map: RcCell<Map>,
        prob: f64,
        state: &mut State,
    ) -> Result<()> {
        let mut this = Self::new(map, state)?;
        this.add_map_size(exe);
        this.prob = prob.into();
        exe.add_exemap(this);
        Ok(())
    }

    /// Reads from the database and registers the [`ExeMap`] with [`Exe`]s and
    /// [`Map`]s.
    fn read_all(
        conn: &SqliteConnection,
        state: &mut State,
        exe_seqs: &BTreeMap<i32, RcCell<Exe>>,
        map_seqs: &BTreeMap<i32, RcCell<Map>>,
    ) -> Result<()> {
        use schema::exemaps::dsl::*;

        // handle case where no data is found
        if let Some(db_exemaps) =
            exemaps.load::<models::ExeMap>(conn).optional()?
        {
            for db_exemap in db_exemaps {
                let exe = exe_seqs.get(&db_exemap.seq);
                let map = map_seqs.get(&db_exemap.map_seq);

                if exe == None || map == None {
                    continue;
                }

                // and thus we insert the exemap while simutaneously creating
                // it.
                Self::new_exe_map(
                    &mut exe.unwrap().borrow_mut(),
                    Rc::clone(map.unwrap()),
                    db_exemap.prob,
                    state,
                )?;
            }
        }
        Ok(())
    }

    /// Add new `map` using `Rc::clone(&map)`.
    pub(crate) fn new(map: RcCell<Map>, state: &mut State) -> Result<Self> {
        state.register_map(Rc::clone(&map))?;
        Ok(Self {
            map,
            prob: 1.0.into(),
        })
    }

    /// Write exemaps data into the database.
    pub(crate) fn write_all(
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
#[derive(Derivative)]
#[derivative(PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Exe {
    /// Absolute path of the executable.
    pub(crate) path: PathBuf,

    /// Total running time of the executable.
    pub(crate) time: i32,

    /// Last time it was probed.
    update_time: i32,

    /// Set of markov chain with other exes.
    pub(crate) markovs: BTreeSet<RcCell<MarkovState>>,

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

// ExeWrapper {{{1 //
#[repr(transparent)]
#[derive(Derivative)]
#[derivative(Debug = "transparent")]
pub(crate) struct ExeWrapper(WeakCell<Exe>);

impl Deref for ExeWrapper {
    type Target = WeakCell<Exe>;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<WeakCell<Exe>> for ExeWrapper {
    fn from(value: WeakCell<Exe>) -> Self {
        Self(value)
    }
}

impl Eq for ExeWrapper {}

impl PartialEq for ExeWrapper {
    fn eq(&self, other: &Self) -> bool {
        let this = self.upgrade().unwrap();
        let other = other.upgrade().unwrap();
        this == other
    }
}
// 1}}} //

impl Exe {
    pub(crate) fn read_all(
        conn: &SqliteConnection,
        state: &mut State,
        cycle: u32,
    ) -> Result<BTreeMap<i32, RcCell<Exe>>> {
        use schema::exes::dsl::*;

        let mut exe_seqs = BTreeMap::new();

        // handle the case where no value is present
        if let Some(db_exes) = exes.load::<models::Exe>(conn).optional()? {
            for db_exe in db_exes {
                let exe =
                    Exe::new(uri_to_filename(db_exe.uri)?, false, None, state);

                {
                    let mut exe = exe.borrow_mut();
                    exe.change_timestamp = -1;
                    exe.update_time = db_exe.update_time;
                    exe.time = db_exe.time;
                }

                // this solves our lookup in exemap!
                anyhow::ensure!(
                    exe_seqs.insert(db_exe.seq, Rc::clone(&exe)) == None,
                    "Duplicate index for Exe {:#?}",
                    exe.borrow(),
                );

                state.register_exe(exe, false, cycle)?;
            }
        }
        Ok(exe_seqs)
    }

    /// Add an exemap state to the set of exemaps.
    pub(crate) fn add_exemap(&mut self, value: ExeMap) {
        self.exemaps.insert(value);
    }

    /// Add a markov state to the set of markovs.
    pub(crate) fn add_markov(&mut self, value: RcCell<MarkovState>) {
        self.markovs.insert(value);
    }

    /// Checks whether the current [`Exe`] is running or not depending on the
    /// timestamp of the last scan for running processes.
    pub(crate) const fn is_running(&self, state: &State) -> bool {
        self.running_timestamp >= state.last_running_timestamp
    }

    pub(crate) fn new(
        path: impl Into<PathBuf>,
        is_running: bool,
        exemaps: Option<BTreeSet<ExeMap>>,
        state: &State,
    ) -> RcCell<Self> {
        let path = path.into();

        let (update_time, running_timestamp);
        if is_running {
            update_time = state.last_running_timestamp;
            running_timestamp = state.last_running_timestamp;
        } else {
            update_time = -1;
            running_timestamp = update_time;
        }

        // calculate the total sizes
        let mut size = 0;
        let exemaps = exemaps.map_or_else(Default::default, |exemap| {
            exemap
                .into_iter()
                .map(|exemap| {
                    size += exemap.map.borrow().get_size();
                    exemap
                })
                .collect()
        });

        Rc::new_cell(Self {
            path,
            size,
            time: 0,
            change_timestamp: state.time,
            update_time,
            running_timestamp,
            exemaps,
            lnprob: 0.0.into(),
            seq: 0,
            markovs: Default::default(),
        })
    }

    /// Write exes data into the database.
    pub(crate) fn write_all(
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

impl Drop for Exe {
    fn drop(&mut self) {
        std::mem::take(&mut self.markovs)
            .iter()
            .for_each(MarkovState::remove_from_exe);
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
#[derivative(PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct MarkovState {
    /// Involved exe `a`.
    ///
    /// We prevent any `Ord` and `PartialOrd` checks to prevent a stack
    /// overflow.
    #[derivative(Ord = "ignore", PartialOrd = "ignore")]
    pub(crate) a: ExeWrapper,

    /// Involved exe `b`.
    ///
    /// We prevent any `Ord` and `PartialOrd` checks to prevent a stack
    /// overflow.
    #[derivative(Ord = "ignore", PartialOrd = "ignore")]
    pub(crate) b: ExeWrapper,

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
}

impl MarkovState {
    fn remove_from_exe(this: &RcCell<Self>) {
        let this_borrow = this.borrow();

        let a = this_borrow.a.upgrade();
        let b = this_borrow.b.upgrade();

        if let Some(a) = a {
            a.borrow_mut().markovs.remove(this);
        } else if let Some(b) = b {
            b.borrow_mut().markovs.remove(this);
        }
    }

    /// Reads and loads the [`MarkovState`] information from the database. It
    /// should be noted that the markov objects are loaded into their
    /// corresponding [`Exe`]s.
    fn read_all(
        conn: &SqliteConnection,
        state: &State,
        exe_seqs: &BTreeMap<i32, RcCell<Exe>>,
        cycle: u32,
    ) -> Result<()> {
        use schema::markovstates::dsl::markovstates;

        // handle case where data is absent
        if let Some(db_markovs) =
            markovstates.load::<models::MarkovState>(conn).optional()?
        {
            for db_markov in db_markovs {
                let a = exe_seqs.get(&db_markov.a_seq);
                let b = exe_seqs.get(&db_markov.b_seq);

                if a == None || b == None {
                    continue;
                }

                let markov_state = Self::new(
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

                let mut mut_markov = markov_state.borrow_mut();
                mut_markov.time_to_leave = time_to_leave;
                mut_markov.weight = weight;
            }
        }
        Ok(())
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
    pub(crate) fn correlation(&self, state: &State) -> f64 {
        let t = state.time;
        let (a, b) = (
            self.a.upgrade().unwrap().borrow().time,
            self.b.upgrade().unwrap().borrow().time,
        );
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
    ) -> RcCell<Self> {
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

        let this = Rc::new_cell(Self {
            a: Rc::downgrade(&a).into(),
            b: Rc::downgrade(&b).into(),
            state: markov_state,
            change_timestamp,
            cycle,
            time: 0,
            time_to_leave: Default::default(),
            weight: Default::default(),
        });

        if initialize {
            this.borrow_mut().state_changed(state);
        }

        a.borrow_mut().add_markov(Rc::clone(&this));
        b.borrow_mut().add_markov(Rc::clone(&this));

        this
    }

    /// The markov update algorithm.
    pub(crate) fn state_changed(&mut self, state: &State) {
        if self.change_timestamp == state.time {
            return;
        }

        let a = self.a.upgrade().unwrap();
        let b = self.b.upgrade().unwrap();

        let old_state = self.state as usize;
        let new_state =
            Self::get_markov_state(&a.borrow(), &b.borrow(), state) as usize;

        if old_state == new_state {
            log::warn!("old_state is equal to new_state");
            return;
        }

        self.weight[old_state][old_state] += 1;
        // workaround: Reverse the subtraction as a workaround for no
        // `std::ops::Sub<OrderedFloat<T>>` for f64
        self.time_to_leave[old_state] += -(self.time_to_leave[old_state]
            - (state.time - self.change_timestamp) as f64)
            / self.weight[old_state][new_state] as f64;

        self.weight[old_state][new_state] += 1;
        self.state = new_state as i32;
        self.change_timestamp = state.time;
    }

    /// Write the markov data to the database.
    pub(crate) fn write_all(
        markovs: &[&RcCell<Self>],
        conn: &SqliteConnection,
    ) -> Result<()> {
        let mut db_markovs = vec![];
        db_markovs.reserve_exact(markovs.len());

        for each in markovs {
            let each = each.borrow();

            let v_ttl = rmp_serde::to_vec(&each.time_to_leave)
                .log_on_err(Level::Error, "Failed to serialize ttl array")
                .with_context(|| "Failed to serialize ttl array")?;

            let v_weight = rmp_serde::to_vec(&each.weight)
                .log_on_err(Level::Error, "Failed to serialize weight matrix")
                .with_context(|| "Failed to serialize weight matrix")?;

            let a = each.a.upgrade().unwrap();
            let a_seq = a.borrow().seq;

            let b = each.b.upgrade().unwrap();
            let b_seq = b.borrow().seq;

            db_markovs.push(models::NewMarkovState {
                a_seq,
                b_seq,
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

    /// Set of maps used by known executables, indexed by `Map` structure.
    pub(crate) maps: BTreeSet<RcCell<Map>>,

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
    pub(crate) dirty: bool,

    /// Whether new scan has been performed but no model update yet.
    pub(crate) model_dirty: bool,

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
    /// Calls a closure on each [`MarkovState`] of an [`Exe`], given that the
    /// `Exe` in question is the same as [`MarkovState::a`].
    pub(crate) fn markov_foreach(&self, func: impl Fn(&mut MarkovState)) {
        self.exes.values().for_each(|exe| {
            // prevent logic error
            let markovs =
                std::mem::take(&mut exe.borrow_mut().markovs).into_iter();

            // `exe_markov_foreach`
            exe.borrow_mut().markovs = markovs
                .map(|markov| {
                    {
                        let mut mut_markov = markov.borrow_mut();
                        let a = mut_markov.a.upgrade().unwrap();

                        // `exe_markov_callback`
                        if exe == &a {
                            func(&mut mut_markov)
                        }
                    }
                    markov
                })
                .collect();
        })
    }

    /// Writes the metadata of state to the database. If the data is already
    /// present, it is replaced with the updated one.
    ///
    /// It must be noted that in the database, the `id` column has a constraint
    /// of only one row.
    pub(crate) fn write_self(&self, conn: &SqliteConnection) -> Result<()> {
        use schema::states::dsl::*;

        diesel::replace_into(schema::states::table)
            .values((
                id.eq(1),
                models::NewState {
                    version: crate_version!().to_string(),
                    time: self.time,
                },
            ))
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

        let maps = self.maps.iter().collect::<Vec<_>>();
        Map::write_all(&maps, conn).unwrap_or_else(|v| is_error = Err(v));

        if is_error.is_ok() {
            let bad_exes_updtimes: Vec<_> = self.bad_exes.iter().collect();
            ReadWriteBadExe::write_all(&bad_exes_updtimes, conn)
                .unwrap_or_else(|e| is_error = Err(e));
        }

        if is_error.is_ok() {
            // NOTE: Several things are happening to exes at a time.
            let exes_to_write = self.exes.values().collect::<Vec<_>>();
            Exe::write_all(&exes_to_write, conn)
                .unwrap_or_else(|e| is_error = Err(e));

            self.exes.values().for_each(|exe| {
                let exe = exe.borrow();

                // `preload_exemap_foreach`
                let exemaps: Vec<_> = exe.exemaps.iter().collect();
                ExeMap::write_all(&exemaps, &exe, conn)
                    .unwrap_or_else(|e| is_error = Err(e));

                let markovs = exe.markovs.iter().collect::<Vec<_>>();
                MarkovState::write_all(&markovs, conn)
                    .unwrap_or_else(|e| is_error = Err(e));
            });
        }

        is_error
    }

    /// Logs various statistics about the state.
    pub(crate) fn dump_log(&self) {
        log::debug!("Dump log requested!");
        log::info!(
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
        log::debug!("state dump log done!")
    }

    pub(crate) fn load(
        cycle: u32,
        exeprefix: Option<&[impl AsRef<Path>]>,
        conn: &SqliteConnection,
    ) -> Result<RcCell<Self>> {
        // creation
        let this = RcCell::new_cell(Self::default());

        // TODO: how should the data be processed?
        Self::read_state(&this, cycle, exeprefix, conn)?;

        // happens at last just before returning
        {
            let mut this = this.borrow_mut();
            this.memstat.update()?;
            this.memstat_timestamp = this.time;
        }

        Ok(this)
    }

    /// Reads the information about [`State`]'s metadata from the database.
    fn read_self(&mut self, conn: &SqliteConnection) -> Result<()> {
        // load our state information
        use schema::states::dsl::states;
        if let Some(db_state) =
            states.first::<models::State>(conn).optional().log_on_err(
                Level::Error,
                "Failed to load state info from database",
            )?
        {
            // check versions
            let read_version = Version::parse(&db_state.version)?;
            let my_version = Version::parse(crate_version!())?;

            match my_version.major.cmp(&read_version.major) {
                Ordering::Less => log::warn!(
                    "State file is of a newer version, ignoring it."
                ),
                Ordering::Greater => {
                    log::warn!("State file is of an older version.")
                }
                _ => (),
            }

            // last checked time
            let time = db_state.time;

            // update the timestamps
            self.time = time;
            self.last_accounting_timestamp = self.time;
        }

        Ok(())
    }

    /// Read everything from the database and fill the [`State`] info.
    fn read_state(
        this: &RcCell<Self>,
        cycle: u32,
        exeprefix: Option<&[impl AsRef<Path>]>,
        conn: &SqliteConnection,
    ) -> Result<()> {
        this.borrow_mut().read_self(conn)?;

        // fetch the maps keyed by their seq numbers.
        let map_seqs = Map::read_all(conn, this)
            .log_on_err(Level::Error, "Failed to load maps from database")?;

        // fetch the badexes
        Path::read_all(conn, &mut this.borrow_mut()).log_on_err(
            Level::Error,
            "Failed to load badexes from database",
        )?;

        // fetch the exes keyed by their seq numbers.
        let exe_seqs = Exe::read_all(conn, &mut this.borrow_mut(), cycle)
            .log_on_err(Level::Error, "Failed to load exes from database")?;

        ExeMap::read_all(conn, &mut this.borrow_mut(), &exe_seqs, &map_seqs).log_on_err(
            Level::Error,
            "Failed to load exes from the database",
        )?;

        MarkovState::read_all(conn, &this.borrow(), &exe_seqs, cycle)
            .log_on_err(
                Level::Error,
                "Failed to load markov states from database",
            )?;

        proc::proc_foreach(
            |_, path| {
                let mut this = this.borrow_mut();
                let time = this.time;
                this.set_running_process_callback(path, time)
            },
            exeprefix,
        )?;

        {
            let mut this = this.borrow_mut();
            this.last_running_timestamp = this.time;
        }

        this.borrow().markov_foreach(|markov| {
            let a = markov.a.upgrade().unwrap();
            let b = markov.a.upgrade().unwrap();

            // `set_markov_state_callback`
            markov.state = MarkovState::get_markov_state(
                &a.borrow(),
                &b.borrow(),
                &this.borrow(),
            );
        });

        Ok(())
    }

    /// Updates running exe list based on the given path and time.
    fn set_running_process_callback(
        &mut self,
        path: impl AsRef<Path>,
        time: i32,
    ) {
        let path = path.as_ref();
        if let Some(exe) = self.exes.get(path) {
            exe.borrow_mut().running_timestamp = time;
            self.running_exes.push(Rc::clone(exe));
        }
    }

    // TODO: implement this
    pub(crate) fn register_exe(
        &mut self,
        exe: RcCell<Exe>,
        create_markovs: bool,
        cycle: u32,
    ) -> Result<()> {
        // don't allow duplicates!
        anyhow::ensure!(
            !self.exes.contains_key(&exe.borrow().path),
            "Exe is already present",
        );

        if create_markovs {
            // TODO: Understand the author's intentions
            self.exes.values().for_each(|v| {
                // `shift_preload_markov_new(...)`
                if v != &exe {
                    MarkovState::new(
                        Rc::clone(v),
                        Rc::clone(&exe),
                        cycle,
                        true,
                        self,
                    );
                }
            });
        }
        self.exes.insert(exe.borrow().path.clone(), Rc::clone(&exe));
        self.exe_seq += 1;
        exe.borrow_mut().seq = self.exe_seq;

        Ok(())
    }

    pub(crate) fn save(&mut self, conn: &SqliteConnection) -> Result<()> {
        log::debug!("Begin saving state.");
        self.write_state(conn)?;
        self.dirty = false;
        // clean once in a while
        self.bad_exes.clear();
        log::debug!("Saving state done.");
        Ok(())
    }

    /// Adds the given [`Map`] to the registry of maps. It returns error value
    /// if the map was already present.
    pub(crate) fn register_map(&mut self, map: RcCell<Map>) -> Result<()> {
        // don't allow duplicate maps
        // TODO: We can remove this bit.
        anyhow::ensure!(!self.maps.contains(&map), "Map is already present");

        self.map_seq += 1;
        // updating the sequence is safe. The `seq` field does not contribute
        // to comparison.
        map.borrow_mut().seq += self.map_seq;
        self.maps.insert(map);
        Ok(())
    }

    /// Removes the given [`Map`] from the registry of maps.
    pub(crate) fn unregister_map(&mut self, map: &RcCell<Map>) {
        self.maps.remove(map);
    }
}
