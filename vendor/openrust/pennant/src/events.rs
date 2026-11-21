use crate::{Error, Result, Scope};
use openrust::{
    contracts::events::{Dispatcher as _, Event},
    events::{Dispatcher, EventContext},
};
use serde_json::Value;
use std::sync::Arc;

macro_rules! event {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug)]
        pub struct $name { $(pub $field: $ty),* }
        impl $name { pub const NAME: &'static str = stringify!($name); }
        impl Event for $name { fn name(&self) -> &str { Self::NAME } }
    };
}
event!(FeatureResolved {
    feature: String,
    scope: Scope,
    value: Value
});
event!(FeatureRetrieved {
    feature: String,
    scope: Scope,
    value: Value
});
event!(UnknownFeatureResolved {
    feature: String,
    scope: Scope
});
event!(UnexpectedNullScopeEncountered { feature: String });
event!(FeatureUpdated {
    feature: String,
    scope: Scope,
    value: Value
});
event!(FeatureUpdatedForAllScopes {
    feature: String,
    value: Value
});
event!(FeatureDeleted {
    feature: String,
    scope: Scope
});
event!(FeaturesPurged { features: Vec<String> });
event!(AllFeaturesPurged {});

pub(crate) async fn dispatch(
    dispatcher: Option<&Arc<Dispatcher>>,
    event: impl Event,
) -> Result<()> {
    if let Some(dispatcher) = dispatcher {
        dispatcher
            .dispatch(Arc::new(event), &mut EventContext::new())
            .await
            .map_err(Error::Event)?;
    }
    Ok(())
}
