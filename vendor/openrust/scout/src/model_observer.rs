use crate::{EngineManager, Searchable};
use openrust::database::{
    Connection, DatabaseFuture, DatabaseResult,
    eloquent::{
        Observer,
        concerns::{ModelEvent, ModelEventResponse},
    },
};
use std::{any::TypeId, sync::Arc};
tokio::task_local! {static SUPPRESSED:Vec<(usize,TypeId)>;}
pub(crate) fn suppressed<M: Searchable>(manager: &EngineManager) -> bool {
    SUPPRESSED
        .try_with(|keys| keys.contains(&(manager as *const _ as usize, TypeId::of::<M>())))
        .unwrap_or(false)
}
pub(crate) async fn withoutSyncing<M: Searchable, F: std::future::Future>(
    manager: &EngineManager,
    future: F,
) -> F::Output {
    let mut keys = SUPPRESSED.try_with(Clone::clone).unwrap_or_default();
    keys.push((manager as *const _ as usize, TypeId::of::<M>()));
    SUPPRESSED.scope(keys, future).await
}
pub struct ModelObserver {
    manager: Arc<EngineManager>,
}
impl ModelObserver {
    pub fn new(manager: Arc<EngineManager>) -> Self {
        Self { manager }
    }
    async fn saved<M: Searchable>(
        &self,
        model: &M,
        force: bool,
    ) -> DatabaseResult<ModelEventResponse> {
        if self.manager.syncingDisabledFor::<M>() || (!force && !model.searchIndexShouldBeUpdated())
        {
            return Ok(ModelEventResponse::Continue);
        }
        if model.shouldBeSearchable() {
            self.manager
                .sync(std::slice::from_ref(model), false, false)
                .await
        } else if model.wasSearchableBeforeUpdate() {
            self.manager
                .sync(std::slice::from_ref(model), true, false)
                .await
        } else {
            Ok(())
        }
        .map_err(crate::databaseError)?;
        Ok(ModelEventResponse::Continue)
    }
}
impl openrust::contracts::events::ShouldHandleEventsAfterCommit for ModelObserver {}
impl<M: Searchable> Observer<M> for ModelObserver {
    const EVENTS: &'static [ModelEvent] = &[
        ModelEvent::Saved,
        ModelEvent::Deleted,
        ModelEvent::ForceDeleted,
        ModelEvent::Restored,
    ];
    fn saved<'a>(
        &'a self,
        model: &'a mut M,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(self.saved(model, false))
    }
    fn deleted<'a>(
        &'a self,
        model: &'a mut M,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            if self.manager.syncingDisabledFor::<M>()
                || !model.wasSearchableBeforeDelete()
                || (model.get_deleted_at_column().is_some() && !model.state().exists())
            {
                return Ok(ModelEventResponse::Continue);
            }
            if self.manager.boolean("soft_delete") && model.get_deleted_at_column().is_some() {
                self.saved(model, true).await
            } else {
                self.manager
                    .sync(std::slice::from_ref(model), true, false)
                    .await
                    .map_err(crate::databaseError)?;
                Ok(ModelEventResponse::Continue)
            }
        })
    }
    fn forceDeleted<'a>(
        &'a self,
        model: &'a mut M,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            if !self.manager.syncingDisabledFor::<M>() {
                self.manager
                    .sync(std::slice::from_ref(model), true, false)
                    .await
                    .map_err(crate::databaseError)?;
            }
            Ok(ModelEventResponse::Continue)
        })
    }
    fn restored<'a>(
        &'a self,
        model: &'a mut M,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(self.saved(model, true))
    }
}
