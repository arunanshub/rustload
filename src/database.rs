//! Functions related to connecting to the `sqlite` database.

use std::path::Path;

use crate::ext_impls::LogResult;
use anyhow::{anyhow, Context, Result};
use diesel::prelude::*;

embed_migrations!();

/// Creates two Diesel-compatible structs (or tables) that can be used for
/// query and insertion.
///
/// All the fields in the `itable` (the insertion struct) have a lifetime of
/// `'a` associated with them.
///
/// # Arguments
///
/// * `qtable_name` - The name of the struct that will be used for query
///   purposes.
///
/// * `dbtable_name` - The name of the table in the database.
///
/// * `itable_name` - The name of the struct that will be used for insertion.
///   It is conventional to use `New<qtable_name>` as the struct name.
///
/// ## Important
///
/// It must be noted that this macro assumes that a primary key field named
/// `id` already exists in the table. Hence, you should not provide a field
/// with the name `id` to this macro.
///
/// # Drawbacks
///
/// Since the fields of `itable` are created as the `&'a` version of the fields
/// in `qtable`, it is impossible to optimize the cases in which referenced
/// version of the field exists.
///
/// If we have a field in the `qtable` called `name: String`, then this macro
/// will generate an `itable` where the field is `name: &'a String`, instead of
/// `name: &'a str`.
///
/// ```
/// table_creator {
///     Qtable {
///         name: String,
///         array: Vec<i32>,
///     },
///     "qtables",
///     NewQtable,
/// }
/// ```
///
/// The code above will generate this `itable` struct:
///
/// ```
/// struct NewQtable {
///     name: &'a String,
///     array: &'a Vec<i32>,
/// }
/// ```
///
/// instead of:
///
/// ```
/// struct NewQtable {
///     name: &'a str,
///     array: &'a [i32],
/// }
/// ```
///
/// # Example
///
/// ```
/// table_creator! {
///     Data {
///         name: String,
///         value: i32,
///     },
///     "datas",
///     NewData,
/// }
/// ```
#[macro_export]
macro_rules! table_creator {
    {
        $qtable_name:ident {
            $( $field:ident : $field_type:ty, )+ $(,)?
        },
        $dbtable_name:literal,
        $itable_name:ident $(,)?
    } => {
        #[derive(Queryable)]
        pub struct $qtable_name {
            pub id: i64,
            $( $field: $field_type, )+
        }

        #[derive(Insertable)]
        #[table_name = $dbtable_name]
        pub struct $itable_name<'a> {
            $( pub $field: &'a $field_type, )+
        }
    };
}

pub(crate) use table_creator;

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
