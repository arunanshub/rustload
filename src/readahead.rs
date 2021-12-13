use std::{
    cmp::Ordering,
    convert::TryInto,
    fs::OpenOptions,
    os::unix::{
        fs::MetadataExt,
        prelude::{AsRawFd, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::atomic::{self, AtomicI32},
};

use crate::{
    common::{LogResult, RcCell},
    config::Config,
    model::SortStrategy,
    state::Map,
};
use anyhow::Result;
use log::Level;
use nix::fcntl::{self, PosixFadviseAdvice};
use rayon::iter::{IntoParallelIterator, ParallelIterator};

impl Map {
    /// Sets the inode number for the file by reading the metadata of the file.
    /// If the metadata is not available, error is returned.
    ///
    /// Currently `_use_inode` is not used.
    fn set_block(&mut self, _use_inode: bool) -> Result<()> {
        // in case we can get block, set to 0 to not retry
        self.block = 0;

        let stat = self.path.metadata()?;
        // TODO: Can we somehow use inode?
        // fall back to inode
        self.block = stat.ino() as i64;

        Ok(())
    }

    fn path_compare(&self, other: &Self) -> Ordering {
        self.path.cmp(&other.path)
    }
}

/// Performs readahead on files based on the map information and configuration,
/// esp. `sortstrategy`.
///
/// # Returns
///
/// Number of files processed.
///
/// # Error
///
/// Error is returned if sorting of files failed.
pub(crate) fn readahead(
    maps: &mut [RcCell<Map>],
    conf: &mut Config,
) -> Result<i32> {
    sort_files(maps, conf)?;

    let mut path: PathBuf = Default::default();
    let mut length = 0;
    let mut offset = 0;

    let mut to_process = vec![];

    for file in maps {
        let file = file.borrow();

        if !path.as_os_str().is_empty()
            && offset <= file.offset
            && offset + length >= file.offset
            && file.path == path
        {
            // merge requests
            length = file.offset + file.length - offset;
            continue;
        }

        if !path.as_os_str().is_empty() {
            to_process.push((path, offset as i64, length as i64));
        }

        path = file.path.clone();
        offset = file.offset;
        length = file.length;
    }

    // parallelize the readahead calls via threads. Btw, `AtomicI32` is
    // supported only on platforms tht support atomic ops on `i32`.
    let processed = AtomicI32::new(0);
    to_process
        .into_par_iter()
        .for_each(|(path, offset, length)| {
            process_file(&path, offset, length)
                .log_on_err(
                    Level::Warn,
                    format!("Could not readahead file {:?}", path),
                )
                .map_or((), |_| {
                    processed.fetch_add(1, atomic::Ordering::SeqCst);
                });
        });

    Ok(processed.into_inner())
}

/// Acutal workhorse of the entire program. This function opens a file in
/// readonly mode and uses portable `posix_fadvise` to perform readahead.
/// `POSIX_FADV_WILLNEED` is used as the advice value.
///
/// Note that the access time of the file is not changed.
///
/// # Error
///
/// Returns error if file cannot be accessed or call to `posix_fadvise` failed.
#[inline]
fn process_file(
    path: impl AsRef<Path>,
    offset: i64,
    length: i64,
) -> Result<()> {
    // do not update the access time and don't make it the controlling terminal
    // for the process.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOCTTY | libc::O_NOATIME)
        .open(path.as_ref())?;

    // the raw file descriptor is alive as long as the `file` variable is in
    // scope.
    // We use `posix_fadvise` instead of `readahead` because the former is
    // portable and also provides the appropriate error message.
    fcntl::posix_fadvise(
        file.as_raw_fd(),
        offset,
        length,
        PosixFadviseAdvice::POSIX_FADV_WILLNEED,
    )?;

    Ok(())
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
