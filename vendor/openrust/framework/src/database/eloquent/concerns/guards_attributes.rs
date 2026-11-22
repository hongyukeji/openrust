use std::future::Future;

use super::super::ModelMetadata;

tokio::task_local! {
    static UNGUARDED: ();
}

pub(in crate::database::eloquent) fn is_unguarded() -> bool {
    UNGUARDED.try_with(|()| ()).is_ok()
}

pub(in crate::database) async fn unguarded<F: Future>(work: F) -> F::Output {
    UNGUARDED.scope((), work).await
}

pub(in crate::database::eloquent) fn unguarded_sync<T>(work: impl FnOnce() -> T) -> T {
    UNGUARDED.sync_scope((), work)
}

pub(in crate::database::eloquent) fn totally_guarded(metadata: &ModelMetadata) -> bool {
    metadata.fillable.is_empty() && metadata.guarded == ["*"]
}

pub(in crate::database::eloquent) fn is_fillable(key: &str, metadata: &ModelMetadata) -> bool {
    if is_unguarded() || metadata.fillable.contains(&key) {
        return true;
    }
    let column = key.split_once("->").map_or(key, |(column, _)| column);
    metadata.fillable.is_empty()
        && !totally_guarded(metadata)
        && !metadata.guarded.contains(&key)
        && !metadata.guarded.contains(&column)
        && !key.contains('.')
        && !key.starts_with('_')
}
