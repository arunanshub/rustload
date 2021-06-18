use std::path::{Path, PathBuf};

pub(crate) trait ToPathBuf {
    fn to_pathbuf(&self) -> Vec<PathBuf>;
}

impl ToPathBuf for Vec<&str> {
    fn to_pathbuf(&self) -> Vec<PathBuf> {
        self.iter().map(|x| Path::new(x).to_owned()).collect()
    }
}
