use std::future::Future;

use crate::database::{DatabaseError, DatabaseResult};

use super::super::{Attributes, ModelMetadata};

tokio::task_local! {
    static UNGUARDED: ();
}

pub(in crate::database) async fn unguarded<F: Future>(work: F) -> F::Output {
    UNGUARDED.scope((), work).await
}

pub(in crate::database::eloquent) fn unguarded_sync<T>(work: impl FnOnce() -> T) -> T {
    UNGUARDED.sync_scope((), work)
}

pub(in crate::database::eloquent) fn fill(
    incoming: Attributes,
    metadata: &ModelMetadata,
) -> DatabaseResult<Attributes> {
    if UNGUARDED.try_with(|()| ()).is_ok() {
        return Ok(incoming);
    }
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
