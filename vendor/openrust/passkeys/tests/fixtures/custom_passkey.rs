#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use openrust::{
    contracts::{
        auth::Authenticatable,
        events::{Dispatcher as _, Event, Listener},
    },
    database::{
        Value,
        eloquent::{
            Model, ModelMetadata, ModelState,
            casts::{Attribute, AttributeCast},
        },
    },
    events::{EventContext, EventFuture, EventResult, ListenerResponse},
};
use openrust_fortify::passkeys::{Passkey, PasskeyModel};
use std::{
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Clone, Default)]
pub struct CustomPasskey(ModelState);
impl Model for CustomPasskey {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("custom_passkeys")
            .primaryKey("pk")
            .fillable(&["name", "credential_id", "credential"])
            .hidden(&["credential"])
            .appends(&["authenticator", "custom_marker"]);
        &META
    }
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        Passkey::casts()
    }
    fn attribute(name: &str) -> Option<Attribute> {
        match name {
            "name" => Some(Attribute::set(|value, _| {
                let Value::Text(text) = value else {
                    return Err(openrust::database::DatabaseError::InvalidQuery(
                        "custom passkey name",
                    ));
                };
                Ok([("name".into(), text.to_uppercase().into())].into())
            })),
            "custom_marker" => Some(Attribute::get(|_, _| Ok("selected-model".into()))),
            _ => Passkey::attribute(name),
        }
    }
    fn from_state(state: ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}
impl PasskeyModel for CustomPasskey {}
static SAVED: AtomicUsize = AtomicUsize::new(0);
static DELETED: AtomicUsize = AtomicUsize::new(0);
static REGISTERED_EVENT: AtomicUsize = AtomicUsize::new(0);
static VERIFIED_EVENT: AtomicUsize = AtomicUsize::new(0);
static DELETED_EVENT: AtomicUsize = AtomicUsize::new(0);
struct Observe;
impl openrust::database::eloquent::Observer<CustomPasskey> for Observe {
    const EVENTS: &'static [openrust::database::eloquent::concerns::ModelEvent] = &[
        openrust::database::eloquent::concerns::ModelEvent::Saved,
        openrust::database::eloquent::concerns::ModelEvent::Deleted,
    ];
    fn saved<'a>(
        &'a self,
        model: &'a mut CustomPasskey,
        _: &'a openrust::database::Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<openrust::database::eloquent::concerns::ModelEventResponse>,
    > {
        Box::pin(async move {
            assert_eq!(model.getKeyName(), "pk");
            assert!(model.getKey().is_some());
            SAVED.fetch_add(1, Ordering::SeqCst);
            Ok(openrust::database::eloquent::concerns::ModelEventResponse::Continue)
        })
    }
    fn deleted<'a>(
        &'a self,
        model: &'a mut CustomPasskey,
        _: &'a openrust::database::Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<openrust::database::eloquent::concerns::ModelEventResponse>,
    > {
        Box::pin(async move {
            assert_eq!(model.getTable(), "custom_passkeys");
            DELETED.fetch_add(1, Ordering::SeqCst);
            Ok(openrust::database::eloquent::concerns::ModelEventResponse::Continue)
        })
    }
}
struct ObserveEvents<U>(PhantomData<fn() -> U>);
impl<U: Authenticatable> Listener for ObserveEvents<U> {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            macro_rules! check {
                ($kind:ty,$counter:ident) => {
                    if let Some(event) = event.downcast_ref::<$kind>() {
                        let user = event.user::<U>(context).expect("actual typed user context");
                        assert_eq!(user.getAuthIdentifier(), event.user);
                        let model = event
                            .passkey::<CustomPasskey>(context)
                            .expect("actual custom Eloquent context");
                        assert_eq!(model.getTable(), "custom_passkeys");
                        let actual = event
                            .passkey
                            .model::<CustomPasskey>()
                            .expect("preserved actual model");
                        assert_eq!(actual.lock().await.getKey(), model.getKey());
                        assert!(event.passkey.sameInstance(&event.passkey.clone()));
                        $counter.fetch_add(1, Ordering::SeqCst);
                    }
                };
            }
            check!(
                openrust_fortify::passkeys::events::PasskeyRegistered,
                REGISTERED_EVENT
            );
            check!(
                openrust_fortify::passkeys::events::PasskeyVerified,
                VERIFIED_EVENT
            );
            check!(
                openrust_fortify::passkeys::events::PasskeyDeleted,
                DELETED_EVENT
            );
            Ok(None)
        })
    }
}
pub async fn observe<U: Authenticatable>(
    app: &openrust::foundation::Application,
) -> openrust::support::ProviderResult {
    CustomPasskey::observe(app, Observe)?;
    app.make::<openrust::events::Dispatcher>().await?.listen(
        "*",
        Arc::new(ObserveEvents::<U>(PhantomData)),
        openrust::events::Delivery::Sync,
    )?;
    Ok(())
}
pub fn report() -> serde_json::Value {
    serde_json::json!({"saved":SAVED.load(Ordering::SeqCst),"deleted":DELETED.load(Ordering::SeqCst),"registered_event":REGISTERED_EVENT.load(Ordering::SeqCst),"verified_event":VERIFIED_EVENT.load(Ordering::SeqCst),"deleted_event":DELETED_EVENT.load(Ordering::SeqCst)})
}
