use std::{cmp::Ordering, convert::TryInto};

use crate::{
    common::{LogResult, RcCell},
    config::Config,
    model::SortStrategy,
    state::Map,
};
use anyhow::Result;
use log::Level;

impl Map {
    // TODO: create the actual function
    fn set_block(&mut self, use_inode: bool) -> Result<()> {
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
}

// TODO: implement this
pub(crate) fn readahead(files: &mut [RcCell<Map>]) -> i32 {
    // let files = files.sort_unstable_by_key(|v| v.borrow().block);
    // TODO: sort files
    todo!()
}

fn sort_files(files: &mut [RcCell<Map>], conf: &mut Config) -> Result<()> {
    let sort_strategy = conf
        .system
        .sortstrategy
        .try_into()
        .log_on_err(
            Level::Warn,
            format!(
                "Invalid value for config key system.sortstrategy: {}",
                conf.system.sortstrategy
            ),
        )
        .unwrap_or_else(|_| {
            conf.system.sortstrategy = SortStrategy::Block as u8;
            SortStrategy::Block
        });

    match sort_strategy {
        SortStrategy::None => (),
        SortStrategy::Path => {
            files.sort_unstable_by(|a, b| a.borrow().path_compare(&b.borrow()))
        }
        SortStrategy::Inode | SortStrategy::Block => {
            sort_by_block_or_inode(files, conf)?
        }
    }

    Ok(())
}

fn sort_by_block_or_inode(
    files: &mut [RcCell<Map>],
    conf: &Config,
) -> Result<()> {
    let mut needs_block = false;

    // check if any file doesn't have block/inode info
    for file in files.iter_mut() {
        let file = file.borrow();
        if file.block == -1 {
            needs_block = true;
            break;
        }
    }

    if needs_block {
        // sorting by path to make stat fast
        files.sort_unstable_by(|a, b| a.borrow().path_compare(&b.borrow()));
        for file in files.iter_mut() {
            let mut file = file.borrow_mut();

            if file.block == -1 {
                file.set_block(
                    conf.system.sortstrategy == SortStrategy::Inode as u8,
                )?;
            }
        }
    }

    // sorting by block
    files.sort_unstable_by_key(|v| v.borrow().block);
    Ok(())
}
