use anyhow::Result;
use daemonize::Daemonize;

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

    let _cfg = config::load_config(&opt.conffile)
        .log_on_err(format!("Cannot open {:?}", opt.conffile))?;

    if !opt.foreground {
        let d = Daemonize::new().pid_file("/tmp/rustload.pid");
        match d.start() {
            Ok(_) => println!("Success, daemonized"),
            Err(e) => println!("Error: {}", e),
        }
    }

    // begin work here
    // cleap up
    Ok(())
}
