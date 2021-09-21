use std::path::Path;

use crate::cli::Opt;
use anyhow::{Context, Result};
use log::LevelFilter;

use log4rs::append::console::{ConsoleAppender, Target};
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Config, Root};
use log4rs::encode::pattern::PatternEncoder;

/// Set logging level from `verbosity`. Anything greater than or equal to 5 is
/// considered as `Trace` level of verbosity.
// NOTE: `from_usize` is not public 🥲
const fn level_from_verbosity(verbosity: i32) -> LevelFilter {
    match verbosity {
        0 => LevelFilter::Off,
        1 => LevelFilter::Error,
        2 => LevelFilter::Warn,
        3 => LevelFilter::Info,
        4 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    }
}

/// Build a logger from `Opt` options and start logging.
///
/// Two types of loggers can be built, a console appender and a file appender,
/// and loglevel depends on `opt.verbosity`.
pub(crate) fn enable_logging(opt: &Opt) -> Result<()> {
    let loglevel = level_from_verbosity(if opt.quiet {
        0
    } else if opt.debug {
        9 // as a compat since anything greater than 4 is accepted as Trace
    } else {
        opt.verbosity
    });

    let encoder =
        PatternEncoder::new("[{d(%Y-%m-%d %H:%M:%S %Z)} {h({l}):<5}] {m}{n}");

    let mut config = Config::builder();
    let appender: &str; // only one appender name is possible in this case

    // user wants to log to stderr
    if opt.logfile == Path::new("") || opt.debug {
        let stderr = ConsoleAppender::builder()
            .target(Target::Stderr)
            .encoder(Box::new(encoder))
            .build();

        appender = "stderr";
        config = config
            .appender(Appender::builder().build(appender, Box::new(stderr)));

    // normal logging to file
    } else {
        let logfile = FileAppender::builder()
            .encoder(Box::new(encoder))
            .build(&opt.logfile)
            .with_context(|| "Failed to create log file.")?;

        appender = "logfile";
        config = config
            .appender(Appender::builder().build(appender, Box::new(logfile)));
    }

    let config = config
        .build(Root::builder().appender(appender).build(loglevel))
        .with_context(|| "Failed to configure the logger")?;

    log4rs::init_config(config)
        .with_context(|| "Error occured while initializing logger.")?;
    Ok(())
}
