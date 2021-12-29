// vim:set et sw=4 ts=4 tw=79:
//! Rustload is a daemon process that prefetches binary files and shared
//! libraries from the hard disc to the main memory of the computer system to
//! achieve faster application startup time. Rustload is adaptive: it monitors
//! the application that the user runs, and by analyzing this data, predicts
//! what applications he might run in the near future, and fetches those
//! binaries and their dependencies into memory.
//!
//! It builds a Markov-based probabilistic model capturing the correlation
//! between every two applications on the system. The model is then used to
//! infer the probability that each application may be started in the near
//! future. These probabilities are used to choose files to prefetch into the
//! main memory. Special care is taken to not degrade system performance and
//! only prefetch when enough resources are available.
//!
//! ## Citation
//!
//! Esfahbod, B. (2006). Preload — an adaptive prefetching daemon. Retrieved
//! September 18, 2021, from
//! <https://citeseerx.ist.psu.edu/viewdoc/download?doi=10.1.1.138.2940&rep=rep1&type=pdf>.

#![deny(unused_imports)]
// Allow some checks during development, but warn about them when releasing.
#![cfg_attr(debug_assertions, allow(unused_variables, dead_code))]

#[macro_use]
extern crate diesel_migrations;

#[macro_use]
extern crate diesel;

#[macro_use]
extern crate derivative;

use std::{env::temp_dir, path::PathBuf};

use anyhow::{Context, Result};
use calloop::{
    signals::{
        Signal::{SIGHUP, SIGINT, SIGQUIT, SIGTERM, SIGUSR1, SIGUSR2},
        Signals,
    },
    EventLoop, LoopHandle,
};
use daemonize::Daemonize;
use lazy_static::lazy_static;
use log::Level;

mod cli;
mod common;
mod config;
mod database;
mod event;
mod logging;
mod model;
mod proc;
mod prophet;
mod readahead;
mod spy;
mod state;

#[doc(hidden)]
mod schema;

use common::LogResult;
use event::SharedData;

use crate::state::State;

lazy_static! {
    // TODO: this will be change to `/var/run` folder.
    static ref PIDFILE: PathBuf = temp_dir().join("rustload.pid");
}

/// Create a PID file, change the umask to `0o077` and daemonize.
///
/// If daemonization fails, log it as Error and return an `anyhow::Error`
/// instance.
fn daemonize() -> Result<()> {
    Daemonize::new()
        .pid_file(&*PIDFILE)
        .umask(0o007)
        .start()
        .log_on_err(Level::Error, "Failed to daemonize")
        .with_context(|| "Failed to daemonize")?;

    log::debug!("Daemonized: PID file = {:?}", PIDFILE.display());
    Ok(())
}

/// Install signal handlers to manipulate [`State`][state::State].
///
/// 1. If SIGTERM is received, shut down the daemon and exit cleanly.
/// 2. If SIGHUP is received, reload the configuration files, if this
///    applies.
fn set_signal_handlers(event_handle: &LoopHandle<SharedData>) -> Result<()> {
    let signals =
        Signals::new(&[SIGINT, SIGQUIT, SIGTERM, SIGHUP, SIGUSR1, SIGUSR2])
            .log_on_err(Level::Error, "Failed to install signal handler")?;

    log::info!("Installed signal handler.");

    event_handle.insert_source(signals, |event, _, shared| {
        match event.signal() {
            // Reload conf
            sig @ SIGHUP => {
                log::warn!("Recieved {}, reloading configuration.", sig);

                if let Ok(conf) = config::load_config(&shared.opt.conffile)
                    .log_on_err(
                        Level::Warn,
                        "Failed to load configuration. \
                        Using old configuration.",
                    )
                {
                    shared.conf = conf;
                    log::info!("Reloading config done!");
                }
            }

            // Dump statelog and conflog
            sig @ SIGUSR1 => {
                log::warn!("Caught {}. Dumping statelog and conflog", sig);
                shared.state.borrow().dump_log();
                log::warn!("Configuration = {:#?}", shared.conf);
            }

            // save statefile and exit
            sig @ SIGUSR2 => {
                log::warn!("Caught {}. Saving statefile and exiting", sig);
                shared
                    .state
                    .borrow_mut()
                    // TODO: change the stuff here
                    .save(&shared.conn)
                    .log_on_err(
                        Level::Error,
                        "Failed to write to the database",
                    )
                    .ok();
                shared.signal.stop();
            }

            // default case: exit
            sig => {
                log::warn!("Caught: {}. Exit requested.", sig);
                shared.signal.stop();
            }
        }
    })?;

    Ok(())
}

#[doc(hidden)]
fn main() -> Result<()> {
    // Parse the CLI.
    let opt = cli::Opt::from_args();

    // Enable logging for this app.
    crate::logging::enable_logging(&opt)
        .log_on_ok(Level::Info, "Enabled logging!")?;

    // Fetch or create configuration file.
    let conf = config::load_config(&opt.conffile)
        .log_on_err(Level::Error, format!("Cannot open {:?}", opt.conffile))?;
    log::info!("Configuration = {:#?}", conf);

    // Connect and migrate to the database.
    let conn = database::conn_and_migrate(&opt.statefile)?;

    // load state from db
    let state = state::State::load(
        conf.model.cycle,
        Some(&conf.system.exeprefix),
        &conn,
    )?;

    let mut event_loop = EventLoop::<SharedData>::try_new()?;
    let handle = event_loop.handle();

    set_signal_handlers(&handle)?;

    // optionally daemonize
    if !opt.foreground {
        daemonize()?;
    }

    let signal = event_loop.get_signal();
    let mut shared = SharedData::new(signal, state, conf, opt, conn);

    State::run(handle, &mut shared)?;

    event_loop.run(None, &mut shared, |_| {})?;

    log::debug!("Exiting");
    Ok(())
}
