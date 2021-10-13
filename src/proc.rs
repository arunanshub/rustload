//! Process listing routines.

use crate::ext_impls::LogResult;
use anyhow::{anyhow, Result};

/// Holds all information about memory conditions of the system.
#[derive(Clone, Copy, Debug, Default)]
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

    /// Total data paged (read) in since boot.
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
