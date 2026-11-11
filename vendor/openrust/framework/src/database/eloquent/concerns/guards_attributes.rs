use crate::database::{DatabaseError, DatabaseResult};

use super::super::{Attributes, ModelMetadata};

pub(in crate::database::eloquent) fn fill(
    incoming: Attributes,
    metadata: &ModelMetadata,
) -> DatabaseResult<Attributes> {
    if metadata.fillable.is_empty() && metadata.guarded == ["*"] {
        if let Some(key) = incoming.keys().next() {
            return Err(DatabaseError::MassAssignment(key.clone()));
        }
        return Ok(incoming);
    }
    Ok(incoming
        .into_iter()
        .filter(|(key, _)| {
            if !metadata.fillable.is_empty() {
                return metadata.fillable.contains(&key.as_str());
            }
            let column = key
                .split_once("->")
                .map_or(key.as_str(), |(column, _)| column);
            !metadata.guarded.contains(&key.as_str())
                && !metadata.guarded.contains(&column)
                && !key.contains('.')
                && !key.starts_with('_')
        })
        .collect())
}
