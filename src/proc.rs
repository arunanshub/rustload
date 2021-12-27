// vim:set et sw=4 ts=4 tw=79 fdm=marker:
//! Process listing routines.

use std::{
    collections::BTreeSet,
    path::Path,
    rc::Rc,
};

use crate::{
    common::{kb, LogResult, RcCell},
    state::{ExeMap, Map, State},
};
use anyhow::{anyhow, Result};
use log::Level;
use procfs::process::MMapPath;

/// Holds all information about memory conditions of the system.
///
/// All memory information is represented in
/// [**Kibibytes**](https://en.wikipedia.org/wiki/Kilobyte)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct MemInfo {
    /// Total memory of the system.
    pub(crate) total: u32,

    /// Free memory of the system.
    pub(crate) free: u32,

    /// Buffer memory.
    pub(crate) buffers: u32,

    /// Page-cache memory.
    pub(crate) cached: u32,

    /// Total data paged (read) in since boot.
    pub(crate) pagein: u32,

    /// Total data paged (written) in since boot.
    pub(crate) pageout: u32,
}

impl MemInfo {
    pub(crate) fn new() -> Result<Self> {
        let mut this = Self::default();
        this.update()?;
        Ok(this)
    }

    /// Updates the memory information.
    pub(crate) fn update(&mut self) -> Result<()> {
        let mem = procfs::Meminfo::new().log_on_err(
            Level::Error,
            "Failed to fetch memory info. Is /proc mounted?",
        )?;

        self.total = kb(mem.mem_total) as u32;
        self.free = kb(mem.mem_free) as u32;
        self.buffers = kb(mem.buffers) as u32;
        self.cached = kb(mem.cached) as u32;

        let pagesize = kb(procfs::page_size()
            .log_on_err(Level::Error, "Failed to fetch pagesize value")?
            as u64) as u32;

        let vm = procfs::vmstat()
            .log_on_err(Level::Error, "Failed to fetch vmstat info")?;

        self.pagein = *vm
            .get("pgpgin")
            .ok_or_else(|| anyhow!("Failed to fetch vmstat.pgpgin value"))
            .log_on_err(Level::Error, "Failed to fetch vmstat.pgpgin value")?
            as u32;

        self.pageout = *vm
            .get("pgpgout")
            .ok_or_else(|| anyhow!("Failed to fetch vmstat.pgpgin value"))
            .log_on_err(Level::Error, "Failed to fetch vmstat.pgpgin value")?
            as u32;

        self.pagein *= pagesize;
        self.pageout *= pagesize;

        Ok(())
    }
}

/// Checks if the given file (`file`) is acceptable by comparing against a list
/// of prefixes (`prefixes`), if provided; otherwise it recognises the file as
/// acceptable.
///
/// # Steps
///
/// 1. If `prefixes` is [`None`], the file is acceptable.
/// 2. If a prefix starts with `!` **AND** the `file` starts with the prefix
///    (excluding the `!`), it is marked as unacceptable. Otherwise it is
///    acceptable.
///
/// # Example
///
/// ```
/// # fn main() {
/// let file = "/bin/ls";
/// let prefixes = [
///     "/sbin",
///     "/lib",
///     "/bin",
/// ]
/// assert!(accept_file(file, Some(&prefixes)));
/// # }
/// ```
fn accept_file(
    file: impl AsRef<Path>,
    prefixes: Option<&[impl AsRef<Path>]>,
) -> bool {
    if let Some(prefixes) = prefixes {
        for prefix in prefixes {
            let mut is_accepted = true;
            let mut prefix = &*prefix.as_ref().to_string_lossy();

            if prefix.starts_with('!') {
                prefix = &prefix[1..];
                is_accepted = false;
            }

            if file.as_ref().starts_with(prefix) {
                return is_accepted;
            }
        }
    }

    // accept if no match
    true
}

/// TODO:
pub(crate) fn get_maps(
    pid: libc::pid_t,
    maps: Option<&BTreeSet<RcCell<Map>>>,
    mut exemaps: Option<&mut BTreeSet<ExeMap>>,
    mapprefix: &[impl AsRef<Path>],
    state: &mut State,
) -> Result<u64> {
    let procmaps = procfs::process::Process::new(pid)
        .log_on_err(Level::Error, "Failed to fetch process info")?
        .maps()
        .log_on_err(Level::Error, "Failed to fetch process map info")?;

    let mut size = 0;

    for procmap in &procmaps {
        // we only accept actual paths
        if let MMapPath::Path(ref path) = procmap.pathname {
            let length = procmap.address.1 - procmap.address.0;
            size += length;

            // also check if the file is "acceptable" using "conf"
            if !accept_file(path, Some(mapprefix)) {
                continue;
            }

            if maps != None || exemaps != None {
                let mut newmap = Map::new(
                    path.clone(),
                    procmap.offset as usize,
                    length as usize,
                );

                // if (maps) { ... }
                if let Some(maps) = maps {
                    if let Some(key) = maps.get(&newmap) {
                        newmap = Rc::clone(key);
                    }
                }

                // if (exemaps) { ... }
                if let Some(ref mut exemaps) = exemaps {
                    exemaps.insert(ExeMap::new(newmap, state)?);
                }
            }
        }
    }

    Ok(size)
}

pub(crate) fn proc_foreach(
    mut func: impl FnMut(libc::pid_t, &Path),
    exeprefix: Option<&[impl AsRef<Path>]>,
) -> Result<()> {
    let procs = procfs::process::all_processes()
        .log_on_err(Level::Error, "Failed to get process details")?;

    for proc in procs {
        if proc.pid == std::process::id() as i32 {
            continue;
        }

        if let Ok(exe_name) = proc.exe() {
            if !accept_file(&exe_name, exeprefix) {
                continue;
            }
            func(proc.pid, &exe_name);
        }
    }

    Ok(())
}

// tests {{{1 //
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_file_test() {
        let file = "/bin/ls";
        let prefixes = ["/sbin", "/lib", "/bin"];

        assert!(accept_file(file, None::<&[&str]>));
        assert!(accept_file(file, Some(&prefixes)));
        assert!(!accept_file(file, Some(&["/sbin", "/lib", "!/bin"])));
    }
}
// 1}}} //
