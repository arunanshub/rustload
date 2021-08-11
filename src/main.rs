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
use signal_hook::{
    consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM, SIGUSR1, SIGUSR2},
    iterator::Signals,
    low_level::signal_name,
};

mod cli;
mod config;
mod ext_impls;
mod logging;
mod model;
// mod state;

use crate::ext_impls::LogOnError;

lazy_static! {
    // this will be change to `/var/run` folder.
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
        .log_on_err("Failed to daemonize")
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
            .log_on_err("Failed to install signal handler")
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

fn main() -> Result<()> {
    let opt = cli::Opt::from_args();
    crate::logging::enable_logging(&opt)?;
    log::debug!("Enabled logging");

    let cfg = config::load_config(&opt.conffile)
        .log_on_err(format!("Cannot open {:?}", opt.conffile))?;
    log::info!("Configuration = {:#?}", cfg);

    if !opt.foreground {
        daemonize()?;
    }

    handle_signals()?;

    // test function
    log::warn!("Sleeping");
    sleep(Duration::from_secs(10));

    // TODO: begin work here and clean up
    Ok(())
}
