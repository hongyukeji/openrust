use crate::database::{Connection, DatabaseFuture, DatabaseResult};

use super::{
    Model,
    concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
};

macro_rules! model_observer {
    ($($event:ident => $method:ident),+ $(,)?) => {
        /// Groups lifecycle handlers for one model type.
        #[allow(non_snake_case, reason = "Matches Laravel's observer method names")]
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
    ForceDeleted => forceDeleted,
}
