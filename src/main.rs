use std::{env::temp_dir, path::PathBuf};

use anyhow::{Context, Result};
use daemonize::Daemonize;
use lazy_static::lazy_static;

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

fn main() -> Result<()> {
    let opt = cli::Opt::from_args();
    crate::logging::enable_logging(&opt)?;
    log::debug!("Enabled logging");

    let cfg = config::load_config(&opt.conffile)
        .log_on_err(format!("Cannot open {:?}", opt.conffile))?;
    log::info!("Configuration = {:#?}", cfg);

    // TODO: add signal handlers:
    // 1. If SIGTERM is received, shut down the daemon and exit cleanly.
    // 2. If SIGHUP is received, reload the configuration files, if this applies.

    if !opt.foreground {
        daemonize()?;
    }

    // TODO: begin work here and clean up
    Ok(())
}
