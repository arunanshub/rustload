//! Inference and prediction routines.
// TODO: Add docs

use anyhow::Result;

use crate::{
    common::{kb, RcCell},
    model::SortStrategy,
    proc, readahead,
    state::{Exe, ExeMap, Map, MarkovState, State},
};

impl MarkovState {
    /// Computes the $P(Y \text{ runs in next period} | \text{current state})$
    /// and bids in for the $Y$. $Y$ should not be running.
    ///
    /// $Y = 1$ if it's needed in next period, 0 otherwise.
    /// Probability inference follows:
    ///
    /// $$P(Y=1) = 1 - P(Y=0)$$
    /// $$P(Y=0) = \prod P(Y = 1 | X\_i)$$
    /// $$P(Y=0|X\_i) = 1 - P(Y=1|X\_i)$$
    /// $$
    /// P(Y=1|X\_i) = P(\text{state change of } Y, X) \cdot P(\text{next state
    /// has } Y=1) \cdot \text{corr}(Y, X)
    /// $$
    /// $$\text{corr}(Y=X) = \text{regularized} |\text{correlation}(Y, X)|$$
    ///
    /// So:
    ///
    /// $$
    /// \text{lnprob}(Y) = \log(P(Y=0)) = \sum \log(P(Y=0|X\_i)) = \sum \log(1
    /// \- P(Y=1|X\_i))
    /// $$
    pub(crate) fn bid_for_exe(
        &self,
        y: &mut Exe,
        ystate: i32,
        correlation: f64,
    ) {
        let state = self.state as usize;

        if self.weight[state][state] == 0
            || self.time_to_leave[state] <= 1.0.into()
        {
            return;
        }

        let p_state_change = -(self.cycle as f64 * 1.5
            / f64::from(self.time_to_leave[state]))
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
    pub(crate) fn bid_in_exes(&self, usecorrelation: bool, state: &State) {
        if self.weight[self.state as usize][self.state as usize] == 0 {
            return;
        }

        let correlation = if usecorrelation {
            self.correlation(state)
        } else {
            1.0
        };

        let a_mut = self.a.upgrade().unwrap();
        let mut a_mut = a_mut.borrow_mut();

        let b_mut = self.a.upgrade().unwrap();
        let b_mut = b_mut.borrow_mut();

        self.bid_for_exe(&mut a_mut, 1, correlation);
        self.bid_for_exe(&mut a_mut, 2, correlation);
    }
}

impl Map {
    /// Set probability of [self][Self] to 0.0.
    #[inline]
    pub(crate) fn zero_prob(&mut self) {
        self.lnprob = 0.0.into();
    }

    /// Perform a three way comparison with a [`Map`]'s `lnprob` and
    /// returns the result as a signed integer.
    #[inline]
    pub(crate) fn prob_compare(&self, other: &Self) -> i32 {
        self.lnprob.cmp(&other.lnprob) as i32
    }

    #[inline]
    pub(crate) fn prob_print(&self) {
        log::warn!("ln(prob(~MAP)) = {}    {:?}", self.lnprob, self.path);
    }
}

impl Exe {
    /// Set probability of [self][Self] to 0.0.
    #[inline]
    pub(crate) fn zero_prob(&mut self) {
        self.lnprob = 0.0.into();
    }

    #[inline]
    pub(crate) fn prob_print(&self, state: &State) {
        if !self.is_running(state) {
            log::debug!("ln(prob(~EXE)) = {}    {:?}", self.lnprob, self.path);
        }
    }
}

impl ExeMap {
    // TODO: add docs
    pub(crate) fn bid_in_maps(&mut self, exe: &Exe, state: &State) {
        // FIXME: (original author) use exemap->prob, needs some theory work.
        let mut map = self.map.borrow_mut();
        if exe.is_running(state) {
            map.lnprob = 1.0.into();
        } else {
            map.lnprob += exe.lnprob;
        }
    }
}

// TODO: Yet to implement preload_prophet_(predict, readahead)
pub(crate) fn predict(
    state: &mut State,
    use_correlation: bool,
    sort_strategy: SortStrategy,
    memtotal: i32,
    memfree: i32,
    memcached: i32,
) -> Result<()> {
    // prevent logic error by collecting everything into a vec
    let maps = std::mem::take(&mut state.maps)
        .into_iter()
        .collect::<Vec<_>>();

    maps.iter()
        .for_each(|(map, _)| map.borrow_mut().zero_prob());

    // ...and then fill it again
    state.maps = maps.into_iter().collect();

    state.exes.values().for_each(|exe| {
        let mut exe_mut = exe.borrow_mut();

        // reset probabilities that we are going to compute
        exe_mut.zero_prob();

        // `preload_markov_foreach`
        // prevent logic error by collecting markovs into vec
        let markovs = std::mem::take(&mut exe_mut.markovs)
            .into_iter()
            .collect::<Vec<_>>();

        markovs.iter().for_each(|markov| {
            let markov = markov.borrow_mut();
            // markov bid in exes
            markov.bid_in_exes(use_correlation, state);
        });

        // ...and fill it back again
        exe_mut.markovs = markovs.into_iter().collect();

        exe_mut.prob_print(state);

        // Elements inside a `BTreeMap` cannot be mutated. Thus we take a
        // longcut. First we move all elements into a `Vec`, leaving the set
        // empty.
        let mut exemaps = std::mem::take(&mut exe_mut.exemaps)
            .into_iter()
            .collect::<Vec<_>>();

        // Then we mutate each element (here, exemap).
        exemaps
            .iter_mut()
            .for_each(|exemap| exemap.bid_in_maps(&exe_mut, state));

        // at last, we put them back again into the set.
        exe_mut.exemaps = exemaps.into_iter().collect();
    });

    let mut maps_on_prob = state.maps.keys().cloned().collect::<Vec<_>>();

    // TODO: what about sort_unstable_by?
    // sort maps by probabilities
    maps_on_prob.sort_unstable_by_key(|a| a.borrow().lnprob);
    // .sort_unstable_by(|a, b| a.borrow().lnprob.cmp(&b.borrow().lnprob));

    // TODO: preload_prophet_readahead
    // readahead(&mut maps_on_prob, state, conf)?;
    readahead(
        &mut maps_on_prob,
        state,
        sort_strategy,
        memtotal,
        memfree,
        memcached,
    )?;

    Ok(())
}

pub(crate) fn readahead(
    maps_arr: &mut [RcCell<Map>],
    state: &mut State,
    sort_strategy: SortStrategy,
    memtotal: i32,
    memfree: i32,
    memcached: i32,
) -> Result<()> {
    let memstat = proc::MemInfo::new()?;

    // memory we are allowed to use (in kilobytes)
    let mut memavail = (memtotal.clamp(-100, 100)
        * (memstat.total as i32 / 100)
        * memfree.clamp(-100, 100)
        * (memstat.free as i32 / 100))
        .max(0)
        + (memcached.clamp(-100, 100) * (memstat.cached as i32 / 100));

    let memavailtotal = memavail;

    state.memstat = memstat;
    state.memstat_timestamp = state.time;

    let mut is_available = false;
    maps_arr.iter().for_each(|map| {
        let map = map.borrow();

        if !(map.lnprob < 0.0.into() && kb(map.length as i32) <= memavail) {
            memavail -= kb(map.length as i32);
            map.prob_print();
            is_available = true;
        }
    });

    log::info!(
        "{} kb available for preloading, using {} kb of it.",
        memavail,
        memavailtotal - memavail,
    );

    if is_available {
        // TODO: perform actual readahead
        let num_processed = readahead::readahead(maps_arr, sort_strategy)?;
        log::debug!("Readahead {} files.", num_processed);
    } else {
        log::debug!("Nothing to readahead.");
    }

    Ok(())
}
