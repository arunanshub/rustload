use std::path::Path;

use anyhow::Result;
use structopt::StructOpt;

mod cli;
mod config;
mod logging;
mod impls;

fn main() -> Result<()> {
    let opt = cli::Opt::from_args();
    logging::enable_logging(&opt);

    // a small experiment
    log::info!("Starting logging with {:#?}", opt);

    config::store_config(Path::new("clip.conf"))?;
    Ok(())
}
