use anyhow::Result;
use structopt::StructOpt;

mod cli;
mod config;
mod ext_impls;
mod logging;
mod model;

use crate::ext_impls::LogOnError;

fn main() -> Result<()> {
    let opt = cli::Opt::from_args();
    logging::enable_logging(&opt)?;
    log::debug!("Enabled logging");

    // a small experiment
    log::info!("Starting logging with {:#?}", opt);

    let _cfg = config::load_config(&opt.conffile)
        .log_on_err(format!("Cannot open {:?}", opt.conffile))?;

    Ok(())
}
