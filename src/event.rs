use std::{convert::TryInto, rc::Rc, time::Duration};

use anyhow::Result;
use calloop::{timer::Timer, LoopHandle, LoopSignal};
use diesel::SqliteConnection;
use log::Level;

use crate::{
    cli,
    common::{LogResult, RcCell},
    config,
    model::SortStrategy,
    prophet, spy,
    state::{self, State},
};

/// Holds the data that will be shared across our event loop. Notably, it also
/// contains a [`LoopSignal`] object that will allow us to stop the event loop
/// from anywhere.
pub(crate) struct SharedData {
    pub(crate) signal: LoopSignal,
    pub(crate) state: RcCell<state::State>,
    pub(crate) conf: config::Config,
    pub(crate) opt: cli::Opt,
    pub(crate) conn: SqliteConnection,
}

impl SharedData {
    pub(crate) fn new(
        signal: LoopSignal,
        state: RcCell<state::State>,
        conf: config::Config,
        opt: cli::Opt,
        conn: SqliteConnection,
    ) -> Self {
        Self {
            signal,
            state,
            conf,
            opt,
            conn,
        }
    }
}

impl State {
    /// Autosaves the state file after a fixed period of time. The time is
    /// governed by the parameter
    /// [`System::autosave`](crate::model::System::autosave).
    fn autosave(
        handle: LoopHandle<SharedData>,
        shared: &mut SharedData,
    ) -> Result<()> {
        let timer = Timer::new()?;
        let delay_from_now =
            Duration::from_secs(shared.conf.system.autosave as u64);
        timer.handle().add_timeout(delay_from_now, ());

        handle.insert_source(timer, move |_, meta, shared| {
            if shared.state.borrow_mut().save(&shared.conn).is_err() {
                shared.signal.stop()
            }
            meta.add_timeout(delay_from_now, ());
        })?;
        Ok(())
    }

    pub(crate) fn run(
        handle: LoopHandle<SharedData>,
        shared: &mut SharedData,
    ) -> Result<()> {
        // set up ticker
        Self::autosave(handle.clone(), shared)?;
        Self::tick(handle.clone(), shared)?;
        Self::tick2(handle.clone(), shared)?;
        Ok(())
    }

    fn tick(
        handle: LoopHandle<SharedData>,
        _shared: &mut SharedData,
    ) -> Result<()> {
        let timer = Timer::new()?;
        let delay_from_now = Duration::from_secs(0);
        timer.handle().add_timeout(delay_from_now, ());

        handle.insert_source(timer, |_, meta, shared| {
            let conf = &shared.conf;
            let state = &shared.state;

            if conf.system.doscan {
                log::debug!("State scanning begin");
                spy::scan(
                    &mut state.borrow_mut(),
                    Some(&conf.system.mapprefix),
                )
                .log_on_err(Level::Warn, "Failed to scan")
                .ok();
                {
                    let mut state = state.borrow_mut();
                    state.dump_log();
                    state.dirty = true;
                    state.model_dirty = true;
                }
                log::debug!("State scanning end")
            }
            if conf.system.dopredict {
                prophet::predict(
                    &mut state.borrow_mut(),
                    conf.model.usecorrelation,
                    shared
                        .conf
                        .system
                        .sortstrategy
                        .try_into()
                        .unwrap_or(SortStrategy::Block),
                    conf.model.memtotal,
                    conf.model.memfree,
                    conf.model.memcached,
                )
                .log_on_err(Level::Warn, "Failed to predict")
                .ok();
            }

            state.borrow_mut().time += conf.model.cycle as i32 / 2;
            meta.add_timeout(
                Duration::from_secs((conf.model.cycle as u64 + 1) / 2),
                (),
            );
        })?;
        Ok(())
    }

    fn tick2(
        handle: LoopHandle<SharedData>,
        _shared: &mut SharedData,
    ) -> Result<()> {
        let timer = Timer::new()?;
        let delay_from_now = Duration::from_secs(0);
        timer.handle().add_timeout(delay_from_now, ());

        handle.insert_source(timer, |_, meta, shared| {
            let conf = &shared.conf;
            let state = &shared.state;

            let model_dirty = state.borrow().model_dirty;
            if model_dirty
                && spy::update_model(
                    Rc::clone(state),
                    &conf.system.mapprefix,
                    conf.model.minsize as u64,
                    conf.model.cycle,
                )
                .log_on_err(Level::Error, "Failed to update model")
                .is_err()
            {
                shared.signal.stop()
            }

            state.borrow_mut().time += conf.model.cycle as i32 / 2;
            meta.add_timeout(
                Duration::from_secs(conf.model.cycle as u64 / 2),
                (),
            );
        })?;
        Ok(())
    }
}
