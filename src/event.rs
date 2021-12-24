use calloop::LoopSignal;
use diesel::SqliteConnection;

use crate::{cli, config, state};

/// Holds the data that will be shared across our event loop. Notably, it also
/// contains a [`LoopSignal`] object that will allow us to stop the event loop
/// from anywhere.
pub(crate) struct SharedData {
    pub(crate) signal: LoopSignal,
    pub(crate) state: state::State,
    pub(crate) conf: config::Config,
    pub(crate) opt: cli::Opt,
    pub(crate) conn: SqliteConnection,
}

impl SharedData {
    pub(crate) fn new(
        signal: LoopSignal,
        state: state::State,
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
