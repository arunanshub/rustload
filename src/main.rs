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

use std::{
    env::temp_dir,
    path::PathBuf,
    process::exit,
    thread::{self, sleep},
    time::Duration,
};

use anyhow::{Context, Result};
use daemonize::Daemonize;
use lazy_static::lazy_static;
use log::Level;
use signal_hook::{
    consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM, SIGUSR1, SIGUSR2},
    iterator::Signals,
    low_level::signal_name,
};

mod cli;
mod config;
mod database;
mod ext_impls;
mod logging;
mod model;
mod proc;
mod prophet;
mod readahead;
mod spy;
mod state;

#[doc(hidden)]
mod schema;

use crate::ext_impls::LogResult;

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

/// Install signal handlers and spawn a thread to handle them.
///
/// TODO: add signal handlers:
/// 1. If SIGTERM is received, shut down the daemon and exit cleanly.
/// 2. If SIGHUP is received, reload the configuration files, if this
///    applies.
fn handle_signals() -> Result<()> {
    let mut signals =
        Signals::new(&[SIGINT, SIGQUIT, SIGTERM, SIGHUP, SIGUSR1, SIGUSR2])
            .log_on_err(Level::Error, "Failed to install signal handler")
            .with_context(|| "Failed to install signal handler")?;

    log::info!("Installed signal handler.");

    thread::spawn(move || {
        for sig in signals.forever() {
            match sig {
                // TODO: Reload conf and log
                SIGHUP => {
                    log::warn!(
                        r#"Caught "SIGHUP". Reloading configs and logs"#
                    );
                    // ...
                }
                // TODO: dump statelog and conflog
                SIGUSR1 => {
                    log::warn!(
                        r#"Caught "SIGUSR1". Dumping statelog and conflog"#
                    );
                    // ...
                }
                // TODO: save statefile and exit
                SIGUSR2 => {
                    log::warn!(
                        r#"Caught "SIGUSR2". Saving statefile and exiting"#
                    );
                    // ...
                    exit(sig);
                }
                // default case: exit
                _ => {
                    log::warn!(
                        "Caught: {:?} (as integer: {}). Exit requested.",
                        signal_name(sig).unwrap(),
                        sig,
                    );
                    exit(sig);
                }
            }
        }
    });
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
    let cfg = config::load_config(&opt.conffile)
        .log_on_err(Level::Error, format!("Cannot open {:?}", opt.conffile))?;
    log::info!("Configuration = {:#?}", cfg);

    // Connect and migrate to the database.
    let _conn = database::conn_and_migrate(&opt.statefile)?;

    handle_signals()?;

    if !opt.foreground {
        daemonize()?;
    }

    // test function
    log::warn!("Testing MemInfo");
    let mut mem = proc::MemInfo::new()?;
    for i in 0..10 {
        log::info!("{:#?}", mem);
        sleep(Duration::from_secs_f32(0.5));
        mem.update()?;
    }

    // TODO: begin work here and clean up
    log::debug!("Exiting");
    Ok(())
}
