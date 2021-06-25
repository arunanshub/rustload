use anyhow::Result;
use confy::load_path;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::model::{Model, System};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Config {
    pub(crate) model: Model,
    pub(crate) system: System,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: Default::default(),
            system: Default::default(),
        }
    }
}

pub(crate) fn load_config(path: &Path) -> Result<Config> {
    if !path.exists() {
        log::info!(
            "File {:?} does not exist. Will try to create a new file.",
            path
        );
    }
    let x = load_path(path)?;
    Ok(x)
}
