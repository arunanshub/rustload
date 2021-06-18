use crate::cli::Opt;
use env_logger::Builder;
use log::LevelFilter;

fn level_from_verbosity(verbosity: i32) -> LevelFilter {
    match verbosity {
        0 => LevelFilter::Off,
        1 => LevelFilter::Error,
        2 => LevelFilter::Warn,
        3 => LevelFilter::Info,
        4 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    }
}

pub(crate) fn enable_logging(opt: &Opt) {
    let loglevel = level_from_verbosity(if opt.quiet {
        0
    } else if opt.debug {
        9 // as a compat since anything greater than 4 is accepted as Trace
    } else {
        opt.verbosity
    });
    Builder::new().filter(None, loglevel).init();
}
