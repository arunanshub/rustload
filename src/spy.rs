use std::{collections::BTreeSet, path::Path, rc::Rc};

use anyhow::Result;

use crate::{
    common::RcCell,
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
        mapprefix: &[impl AsRef<Path>],
        minsize: u64,
        cycle: u32,
    ) -> Result<()> {
        let path = path.as_ref();
        let mut size = proc::get_maps(pid, None, None, mapprefix)?;
        let want_it = size >= minsize;

        if want_it {
            let mut exemaps: BTreeSet<ExeMap> = Default::default();
            size = proc::get_maps(
                pid,
                Some(&self.maps),
                Some(&mut exemaps),
                mapprefix,
            )?;

            // TODO: Should this return an error? Since the original code
            // uses this as a cleanup point.
            anyhow::ensure!(size != 0, "The process died");

            let exe = Exe::new(path, true, Some(exemaps), self);

            self.register_exe(Rc::clone(&exe), true, cycle)?;

            self.running_exes.push(exe);
            return Ok(());
        } else {
            self.bad_exes.insert(path.to_owned(), size as usize);
        }

        Ok(())
    }
}

impl MarkovState {
    #[inline]
    fn running_inc_time(&mut self, time: i32) {
        if self.state == 3 {
            self.time += time;
        }
    }
}

impl Exe {
    /// Adjust states on exes that change state (running/not-running).
    fn changed_callback(&mut self, state: &State) {
        self.change_timestamp = state.time;

        // This solution prevents logic error.
        // See: https://doc.rust-lang.org/stable/std/collections/struct.BTreeSet.html
        self.markovs = std::mem::take(&mut self.markovs)
            .into_iter()
            .map(|markov| {
                markov.borrow_mut().state_changed(state);
                markov
            })
            .collect();
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

    // figure out who's not running by checking their timestamp
    std::mem::take(&mut state.running_exes)
        .iter()
        .for_each(|exe| {
            state.exe_already_running_callback(Rc::clone(exe));
        });

    // update our running exes info
    state.running_exes = state.new_running_exes.clone();

    Ok(())
}

pub(crate) fn update_model(
    state: &mut State,
    mapprefix: &[impl AsRef<Path>],
    minsize: u64,
    cycle: u32,
) -> Result<()> {
    let mut is_error = Ok(());

    // register new discovered exes
    std::mem::take(&mut state.new_exes)
        .into_iter()
        .for_each(|(path, pid)| {
            state
                .new_exe_callback(
                    &path,
                    pid as libc::pid_t,
                    mapprefix,
                    minsize,
                    cycle,
                )
                .unwrap_or_else(|e| {
                    is_error = Err(e);
                    Default::default()
                });
        });

    if is_error.is_err() {
        return is_error;
    }

    // adjust states for those changing
    std::mem::take(&mut state.state_changed_exes)
        .into_iter()
        .for_each(|exe| exe.borrow_mut().changed_callback(state));

    // accounting
    let period = state.time - state.last_accounting_timestamp;
    state
        .exes
        .values()
        .for_each(|exe| exe.borrow_mut().running_inc_time(period, state));
    state.markov_foreach(|markov| markov.running_inc_time(period));
    state.last_accounting_timestamp = state.time;
    Ok(())
}
