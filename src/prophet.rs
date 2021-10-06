//! Inference and prediction routines.
// TODO: Add docs

use crate::state::{Exe, ExeMap, Map, MarkovState, State};
use std::pin::Pin;

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
        self: Pin<&Self>,
        y: &mut Exe,
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
    pub(crate) fn bid_in_exes(
        self: Pin<&mut Self>,
        usecorrelation: bool,
        state: &State,
    ) {
        if self.weight[self.state as usize][self.state as usize] == 0 {
            return;
        }

        let correlation = if usecorrelation {
            self.as_ref().correlation(state)
        } else {
            1.0
        };

        self.as_ref()
            .bid_for_exe(&mut self.a.borrow_mut(), 1, correlation);
        self.as_ref()
            .bid_for_exe(&mut self.b.borrow_mut(), 2, correlation);
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
        log::warn!("ln(prob(~EXE)) = {}    {:?}", self.lnprob, self.path);
    }
}

impl Exe {
    /// Set probability of [self][Self] to 0.0.
    pub(crate) fn zero_prob(&mut self) {
        self.lnprob = 0.0.into();
    }

    #[inline]
    pub(crate) fn prob_print(&self) {
        log::info!("ln(prob(~EXE)) = {}    {:?}", self.lnprob, self.path);
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
