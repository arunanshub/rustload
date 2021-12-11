use std::{collections::BTreeSet, path::Path, pin::Pin, rc::Rc};

use anyhow::Result;

use crate::{
    config::Config,
    common::{RcCell, RcCellNew},
    proc,
    state::{Exe, ExeMap, MarkovState, State},
};

impl State {
    fn running_process_callback(
        &mut self,
        pid: libc::pid_t,
        path: impl AsRef<Path>,
    ) {
        let path = path.as_ref();

        if let Some(exe) = self.exes.get(path) {
            // has the exe been running already?
            if !exe.borrow().is_running(self) {
                self.new_running_exes.push(Rc::clone(exe));
                self.state_changed_exes.push(Rc::clone(exe));
            }

            // update timestamp
            exe.borrow_mut().running_timestamp = self.time;
        } else if self.bad_exes.get(path) == None {
            // we have never seen the exe before
            self.new_exes.insert(path.to_owned(), pid);
        }
    }

    /// for every exe that has been running, check whether it's still running
    /// and take proper action.
    ///
    /// Originally, this was associated with [`Exe`], but it has been
    /// reassigned as the member of [`State`].
    // TODO: should it be associated with Exe?
    fn exe_already_running_callback(&mut self, exe: RcCell<Exe>) {
        if exe.borrow().is_running(self) {
            self.new_running_exes.push(exe);
        } else {
            self.state_changed_exes.push(exe);
        }
    }

    fn new_exe_callback(
        &mut self,
        path: impl AsRef<Path>,
        pid: libc::pid_t,
        conf: &Config,
    ) -> Result<Vec<Pin<Box<MarkovState>>>> {
        let path = path.as_ref();
        let mut size = proc::get_maps(pid, None, None, conf)?;
        let want_it = size >= conf.model.minsize as u64;

        if want_it {
            let mut exemaps: BTreeSet<ExeMap> = Default::default();
            size = proc::get_maps(
                pid,
                Some(&self.maps),
                Some(&mut exemaps),
                conf,
            )?;

            if size == 0 {
                // TODO: Should this return an error? Since the original code
                // uses this as a cleanup point.
                anyhow::bail!("The process died")
            }

            let exe =
                RcCell::new_cell(Exe::new(path, true, Some(exemaps), self));

            // TODO: We currently return the markovs. But what are the
            // implications?
            let markovs =
                self.register_exe(Rc::clone(&exe), true, conf.model.cycle)?;

            self.running_exes.push(exe);
            return Ok(markovs);
        } else {
            self.bad_exes.insert(path.to_owned(), size as usize);
        }

        Ok(Default::default())
    }
}

impl MarkovState {
    #[inline]
    fn running_inc_time(self: Pin<&mut Self>, time: i32) {
        if self.state == 3 {
            unsafe {
                self.get_unchecked_mut().time += time;
            }
        }
    }
}

impl Exe {
    /// Adjust states on exes that change state (running/not-running).
    fn changed_callback(&mut self, state: &State) {
        self.change_timestamp = state.time;
        self.markovs.iter().for_each(|markov| {
            let markov = unsafe { Pin::new_unchecked(&mut *markov.0) };
            markov.state_changed(state);
        });
    }

    #[inline]
    fn running_inc_time(&mut self, time: i32, state: &State) {
        if self.is_running(state) {
            self.time += time;
        }
    }
}

/// Scan processes and see which exes started running, which are not running
/// anymore, and what new exes are around.
pub(crate) fn scan(
    state: &mut State,
    prefixes: Option<&[impl AsRef<Path>]>,
) -> Result<()> {
    state.state_changed_exes.clear();
    state.new_running_exes.clear();

    // mark each exe with fresh timestamp
    proc::proc_foreach(
        |pid, exe| state.running_process_callback(pid, exe),
        prefixes,
    )?;
    state.last_running_timestamp = state.time;

    // hack to prevent mutable-immutable issue
    let running_exes = std::mem::take(&mut state.running_exes);
    // figure out who's not running by checking their timestamp
    running_exes.iter().for_each(|e| {
        state.exe_already_running_callback(Rc::clone(e));
    });

    // update our running exes info
    state.running_exes = state.new_running_exes.clone();

    Ok(())
}

pub(crate) fn update_model(
    state: &mut State,
    conf: &Config,
) -> Result<Vec<Pin<Box<MarkovState>>>> {
    let mut is_error = Ok(Default::default());
    let mut markovs = vec![];

    // register new discovered exes
    let new_exes = std::mem::take(&mut state.new_exes);
    new_exes.iter().for_each(|(path, &pid)| {
        markovs.push(
            state
                .new_exe_callback(path, pid as libc::pid_t, conf)
                .unwrap_or_else(|e| {
                    is_error = Err(e);
                    Default::default()
                }),
        );
    });

    if is_error.is_err() {
        return is_error;
    }

    // adjust states for those changing
    let state_changed_exes = std::mem::take(&mut state.state_changed_exes);
    state_changed_exes.iter().for_each(|v| {
        v.borrow_mut().changed_callback(state);
    });

    // accounting
    let period = state.time - state.last_accounting_timestamp;
    state.exes.iter().for_each(|(_, exe)| {
        let mut exe_mut = exe.borrow_mut();
        exe_mut.running_inc_time(period, state);

        // `preload_markov_foreach`
        exe_mut.markovs.iter().for_each(|markov| {
            // `exe_markov_callback`
            if exe == &markov.a {
                let markov = unsafe { Pin::new_unchecked(&mut *markov.0) };
                markov.running_inc_time(period);
            }
        })
    });

    state.last_accounting_timestamp = state.time;
    Ok(markovs.into_iter().flatten().collect())
}
