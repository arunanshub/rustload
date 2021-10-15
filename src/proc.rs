//! Process listing routines.

use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use crate::{
    ext_impls::{LogResult, RcCell, RcCellNew},
    state::{ExeMap, Map},
};
use anyhow::{anyhow, Result};
use procfs::process::MMapPath;

/// Holds all information about memory conditions of the system.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct MemInfo {
    /// Total memory of the system.
    total: u64,

    /// Free memory of the system.
    free: u64,

    /// Buffer memory.
    buffers: u64,

    /// Page-cache memory.
    cached: u64,

    /// Total data paged (read) in since boot.
    pagein: u64,

    /// Total data paged (written) in since boot.
    pageout: u64,
}

impl MemInfo {
    pub(crate) fn new() -> Result<Self> {
        let mut this = Self::default();
        this.update()?;
        Ok(this)
    }

    /// Updates the memory information.
    pub(crate) fn update(&mut self) -> Result<()> {
        let mem = procfs::Meminfo::new()
            .log_on_err("Failed to fetch memory info. Is /proc mounted?")?;

        self.total = mem.mem_total;
        self.free = mem.mem_free;
        self.buffers = mem.buffers;
        self.cached = mem.cached;

        // let pagesize = procfs::page_size()
        //     .log_on_err("Failed to fetch pagesize value")? as u64;

        let vm = procfs::vmstat().log_on_err("Failed to fetch vmstat info")?;

        self.pagein = *vm
            .get("pgpgin")
            .ok_or_else(|| anyhow!("Failed to fetch vmstat.pgpgin value"))
            .log_on_err("Failed to fetch vmstat.pgpgin value")?
            as u64;

        self.pageout = *vm
            .get("pgpgout")
            .ok_or_else(|| anyhow!("Failed to fetch vmstat.pgpgin value"))
            .log_on_err("Failed to fetch vmstat.pgpgin value")?
            as u64;

        Ok(())
    }
}

/// TODO:
pub(crate) fn get_maps(
    pid: libc::pid_t,
    maps: Option<&BTreeMap<RcCell<Map>, usize>>,
    mut exemaps: Option<&mut BTreeSet<ExeMap>>,
) -> Result<u64> {
    let procmaps = procfs::process::Process::new(pid)
        .log_on_err("Failed to fetch process info")?
        .maps()
        .log_on_err("Failed to fetch process map info")?;

    let mut size = 0;

    for procmap in &procmaps {
        // we only accept actual paths
        if let MMapPath::Path(ref path) = procmap.pathname {
            let length = procmap.address.1 - procmap.address.0;
            size += length;

            if maps != None || exemaps != None {
                let mut newmap = RcCell::new_cell(Map::new(
                    path.clone(),
                    procmap.offset as usize,
                    length as usize,
                ));

                // if (maps) { ... }
                if let Some(maps) = maps {
                    if let Some((key, _)) = maps.get_key_value(&newmap) {
                        newmap = Rc::clone(key);
                    }
                }

                // if (exemaps) { ... }
                if let Some(ref mut exemaps) = exemaps {
                    exemaps.insert(ExeMap::new(newmap));
                }
            }
        }
    }

    Ok(size)
}
