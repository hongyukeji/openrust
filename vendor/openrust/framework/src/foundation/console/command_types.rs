use std::{any::TypeId, sync::Arc};

use crate::{
    console::{Command, ConsoleError, ConsoleFuture},
    foundation::Application,
};

type ResolveCommand =
    for<'a> fn(&'a Application) -> ConsoleFuture<'a, Result<Arc<dyn Command>, ConsoleError>>;

pub(crate) struct CommandType {
    pub(crate) id: TypeId,
    resolve: ResolveCommand,
}

impl CommandType {
    pub(crate) fn of<C: Command + 'static>() -> Self {
        Self {
            id: TypeId::of::<C>(),
            resolve: |application| {
                Box::pin(async move {
                    application
                        .make::<C>()
                        .await
                        .map(|command| command as Arc<dyn Command>)
                        .map_err(ConsoleError::from_source)
                })
            },
        }
    }

    pub(crate) async fn resolve(
        &self,
        application: &Application,
    ) -> Result<ResolvedCommandType, ConsoleError> {
        Ok(ResolvedCommandType {
            id: self.id,
            command: (self.resolve)(application).await?,
        })
    }
}

pub(crate) struct CommandTypeCatalog(pub(crate) Vec<CommandType>);

pub(crate) struct ResolvedCommandType {
    pub(crate) id: TypeId,
    pub(crate) command: Arc<dyn Command>,
}
