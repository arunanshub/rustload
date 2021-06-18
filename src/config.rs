use std::path::{Path, PathBuf};
use crate::impls::ToPathBuf;

use confy::load_path;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Config {
    pub(crate) cycle: u32,
    pub(crate) usecorrelation: bool,
    pub(crate) minsize: u32,
    pub(crate) memtotal: i32,
    pub(crate) memfree: i32,
    pub(crate) memcached: i32,

    pub(crate) doscan: bool,
    pub(crate) dopredict: bool,
    pub(crate) autosave: u32,
    pub(crate) mapprefix: Vec<PathBuf>,
    pub(crate) exeprefix: Vec<PathBuf>,
    pub(crate) processes: u32,
    pub(crate) sortstrategy: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cycle: 20,
            usecorrelation: true,
            minsize: 2000000,
            memtotal: -10,
            memfree: 50,
            memcached: 0,
            doscan: true,
            dopredict: true,
            autosave: 3600,
            mapprefix: vec![
                "/opt",
                "!/usr/sbin/",
                "!/usr/local/sbin/",
                "/usr/",
                "!/",
            ].to_pathbuf(),
            exeprefix: vec![
                "/opt",
                "!/usr/sbin/",
                "!/usr/local/sbin/",
                "/usr/",
                "!/",
            ].to_pathbuf(),
            processes: 30,
            sortstrategy: 3,
        }
    }
}

pub(crate) fn store_config(path: &Path) -> anyhow::Result<()> {
    dbg!(&path);
    let cf: Config = load_path(path)?;
    dbg!(&cf);
    Ok(())
}
