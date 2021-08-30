//! Functions related to connecting to the `sqlite` database.

use std::path::Path;

use crate::ext_impls::LogResult;
use anyhow::{anyhow, Context, Result};
use diesel::prelude::*;

embed_migrations!();

/// Connect to an `sqlite` database located at `path`.
fn establish_connection(path: impl AsRef<Path>) -> Result<SqliteConnection> {
    SqliteConnection::establish(&path.as_ref().to_string_lossy())
        .map_err(|e| anyhow!("{}", e))
}

/// Connect to an `sqlite` database located at `path`, run all migrations and
/// return a connection result.
pub(crate) fn conn_and_migrate(
    path: impl AsRef<Path>,
) -> Result<SqliteConnection> {
    let path = path.as_ref();
    let conn = establish_connection(path)
        .log_on_ok(format!(
            "Established connection with the database at {:?}",
            path
        ))
        .log_on_err(format!("Failed to connect to the database at {:?}", path))
        .with_context(|| "Failed to connect to the database")?;

    embedded_migrations::run(&conn)
        .log_on_ok("Successfully ran migrations!")
        .log_on_err("Failed to run migrations")
        .with_context(|| "Failed to run migrations")?;

    Ok(conn)
}
