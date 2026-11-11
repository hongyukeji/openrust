use crate::database::{Connection, DatabaseResult, Value};

use super::super::casts::date::format_for_storage;

pub(in crate::database::eloquent) fn timestamp(
    connection: &Connection,
    format: &str,
) -> DatabaseResult<Value> {
    format_for_storage(
        chrono::Utc::now().with_timezone(&connection.model_timezone),
        format,
    )
}
