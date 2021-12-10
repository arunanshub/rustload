use std::cmp::Ordering;

use crate::state::Map;
use anyhow::Result;

impl Map {
    fn set_block(&mut self) -> Result<()> {
        let fd = -1;
        let block = 0;

        let buf: libc::stat;
        self.block = 0;

        let fd = unsafe {
            libc::open(
                self.path
                    .to_str()
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Failed to parse filepath: {:?}",
                            self.path
                        )
                    })?
                    .as_ptr() as *const libc::c_char,
                libc::O_RDONLY,
            )
        };

        unsafe { libc::close(fd) };

        Ok(())
    }

    fn path_compare(&self, other: &Self) -> Ordering {
        self.path.cmp(&other.path)
    }

    fn block_compare(&self, other: &Self) -> Ordering {
        self.block.cmp(&other.block)
    }
}
