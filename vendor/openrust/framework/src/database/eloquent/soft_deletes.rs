use crate::database::{DatabaseError, Value};

use super::{Attributes, ModelMetadata, concerns::has_timestamps::timestamp};

pub(super) fn unsupported() -> DatabaseError {
    DatabaseError::InvalidQuery("soft deletes are disabled for this model")
}

pub(super) fn deletion_attributes(metadata: &ModelMetadata, column: &str) -> Attributes {
    let time = timestamp();
    let mut attributes = Attributes::from([(column.to_owned(), time.clone())]);
    if let Some(column) = metadata.updated_at {
        attributes.insert(column.to_owned(), time);
    }
    attributes
}

pub(super) fn is_trashed(value: Option<&Value>) -> bool {
    value.is_some_and(|value| !matches!(value, Value::Null))
}
