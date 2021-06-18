use std::path::PathBuf;

use structopt::StructOpt;

/// rustload is an adaptive readahead daemon that prefetches files mapped by
/// applications from the disk to reduce application startup time.
#[derive(Debug, StructOpt)]
#[structopt(
    name = clap::crate_name!(),
    version = clap::crate_version!(),
    max_term_width = 100,
    global_settings = &[
        clap::AppSettings::ColoredHelp,
        clap::AppSettings::UnifiedHelpMessage,
    ],
    after_help = "\
    Note: `-h` prints a short and concise overview while `--help` gives all \
    details.",
)]
pub(crate) struct Opt {
    /// Set configuration file. Empty string means no conf file.
    #[structopt(
        short,
        long,
        default_value = "/etc/rustload.conf",
        parse(from_os_str)
    )]
    pub(crate) conffile: PathBuf,

    /// Set state file to load/save. Empty string means no state.
    #[structopt(
        short,
        long,
        default_value = "/var/lib/rustload/rustload.state",
        parse(from_os_str)
    )]
    pub(crate) statefile: PathBuf,

    /// Set log file. Empty string means log to stderr.
    #[structopt(
        short,
        long,
        default_value = "/var/log/rustload.log",
        parse(from_os_str)
    )]
    pub(crate) logfile: PathBuf,

    /// Run in foreground, do not daemonize.
    #[structopt(short, long)]
    pub(crate) foreground: bool,

    /// Nice level.
    #[structopt(short, long, default_value = "15")]
    pub(crate) nice: i32,

    /// Set the verbosity level.
    ///
    /// Verbosity ranges from 0 to 5+. Values greater than or equal to 5 will
    /// be treated as highest verbosity level. 0 turns off logging, which is
    /// the same as using `--quiet` flag.
    ///
    /// This option conflicts with both `--quiet` and `--debug`.
    #[structopt(short = "V", long, default_value = "2")]
    pub(crate) verbosity: i32,

    /// Turns off logging. It is same as setting `--verbosity 0`
    ///
    /// This option conflicts with both `--verbosity` and `--debug`.
    #[structopt(short, long, conflicts_with = "verbosity")]
    pub(crate) quiet: bool,

    /// Debug mode.
    /// Shortcut for `--logfile '' --foreground --verbose 9`
    ///
    /// This option conflicts with both `--quiet` and `--verbosity`.
    #[structopt(
        short,
        long,
        conflicts_with = "verbosity",
        conflicts_with = "quiet"
    )]
    pub(crate) debug: bool,
}
