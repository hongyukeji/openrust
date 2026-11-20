use crate::database::{Connection, DatabaseResult, Value};

use super::super::casts::date::format_for_storage;

pub(in crate::database::eloquent) fn timestamp(
    connection: &Connection,
    format: &str,
) -> DatabaseResult<Value> {
    let owner = Connection::model_container(Some(connection));
    format_for_storage(
        crate::support::utc_now_for(owner.as_deref()).to_zoned(connection.model_timezone.clone()),
        format,
    )
}
