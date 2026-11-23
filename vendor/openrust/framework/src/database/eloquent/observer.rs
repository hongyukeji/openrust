use std::sync::Arc;

use crate::{
    database::{Connection, DatabaseFuture, DatabaseResult, Transaction},
    support::facades::application_scope,
};

use super::{
    Model,
    concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
};

macro_rules! model_observer {
    ($($event:ident => $method:ident),+ $(,)?) => {
        /// Groups lifecycle handlers for one model type.
        #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
        pub trait Observer<M: Model>: Send + Sync + 'static {
            /// Lifecycle methods to register. Rust registers this list at compile time.
            const EVENTS: &'static [ModelEvent];

            $(
                fn $method<'a>(
                    &'a self,
                    _model: &'a mut M,
                    _connection: &'a Connection,
                ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
                    Box::pin(async { Ok(ModelEventResponse::Continue) })
                }
            )+
        }

        impl<M: Model, O: Observer<M>> ModelEventListener<M> for ObserverListener<O> {
            fn handle<'a>(
                &'a self,
                event: ModelEvent,
                model: &'a mut M,
                connection: &'a Connection,
            ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
                match event {
                    $(ModelEvent::$event => self.0.$method(model, connection),)+
                }
            }
        }
    };
}

pub(super) struct ObserverListener<O>(pub(super) O);

model_observer! {
    Retrieved => retrieved,
    Creating => creating,
    Created => created,
    Updating => updating,
    Updated => updated,
    Saving => saving,
    Saved => saved,
    Restoring => restoring,
    Restored => restored,
    Replicating => replicating,
    Trashed => trashed,
    Deleting => deleting,
    Deleted => deleted,
    ForceDeleting => forceDeleting,
    ForceDeleted => force_deleted,
}

pub(super) struct AfterCommitObserver<O>(Arc<ObserverListener<O>>);

impl<O> AfterCommitObserver<O> {
    pub(super) fn new(observer: O) -> Self {
        Self(Arc::new(ObserverListener(observer)))
    }
}

impl<M: Model + Clone, O: Observer<M>> ModelEventListener<M> for AfterCommitObserver<O> {
    fn handle<'a>(
        &'a self,
        event: ModelEvent,
        model: &'a mut M,
        connection: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        self.0.handle(event, model, connection)
    }

    fn handle_in_transaction<'a>(
        &'a self,
        event: ModelEvent,
        model: &'a mut M,
        transaction: &'a mut Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        if matches!(
            event,
            ModelEvent::Creating
                | ModelEvent::Updating
                | ModelEvent::Saving
                | ModelEvent::Deleting
                | ModelEvent::Restoring
                | ModelEvent::ForceDeleting
        ) {
            return self.handle(event, model, transaction.query_connection());
        }
        let mut snapshot = model.clone();
        let observer = Arc::clone(&self.0);
        let connection = transaction.query_connection().clone();
        let application = Connection::model_container(Some(&connection));
        Box::pin(async move {
            transaction.after_commit(move || {
                Box::pin(async move {
                    let handle = observer.handle(event, &mut snapshot, &connection);
                    match application {
                        Some(application) => application_scope(application, handle).await?,
                        None => handle.await?,
                    };
                    Ok(())
                })
            })?;
            Ok(ModelEventResponse::Continue)
        })
    }
}
