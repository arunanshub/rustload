use anyhow::Result;
use confy::load_path;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::model::{Model, System};

#[derive(Debug, Serialize, Deserialize, Default)]
pub(crate) struct Config {
    pub(crate) model: Model,
    pub(crate) system: System,
}

pub(crate) fn load_config(path: impl AsRef<Path>) -> Result<Config> {
    let path = path.as_ref();

    if path == Path::new("") {
        log::info!("No config file provided. Using default params.");
        return Ok(Config::default());
    }

    if !path.exists() {
        log::info!(
            "File {:?} does not exist. Will try to create a new file.",
            path
        );
    }
    Ok(load_path(path)?)
}
