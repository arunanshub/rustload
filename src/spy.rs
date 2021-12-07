use std::{path::Path, pin::Pin, rc::Rc};

use crate::{
    ext_impls::RcCell,
    state::{Exe, MarkovState, State},
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
    fn exe_already_running(&mut self, exe: RcCell<Exe>) {
        if exe.borrow().is_running(self) {
            self.new_running_exes.push(exe);
        } else {
            self.state_changed_exes.push(exe);
        }
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
