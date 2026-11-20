use crate::{Passkey, internal};
use openrust::{
    auth::AuthIdentifier,
    contracts::{
        auth::Authenticatable,
        events::{Dispatcher as _, Event as EventContract},
    },
    events::EventContext,
    http::HttpError,
    support::facades::Event,
};
use std::sync::Arc;
macro_rules! events {
    ($(($name:ident,$emit:ident)),+)=>{$(
        pub struct $name {pub user:AuthIdentifier,pub passkey:Passkey}
        impl EventContract for $name {fn name(&self)->&str {concat!("OpenRust\\Passkeys\\Events\\",stringify!($name))}}
        impl $name { pub fn user<'a,T:Authenticatable>(&self,context:&EventContext<'a>)->Option<&'a T> {context.arguments_for(self)?.first()?.downcast_ref()} }
        pub(crate) async fn $emit(user:&dyn Authenticatable,passkey:&Passkey)->Result<(),HttpError> {
            let event=Arc::new($name{user:user.get_auth_identifier(),passkey:passkey.clone()});
            let arguments=[user as &(dyn std::any::Any+Send+Sync),passkey as &(dyn std::any::Any+Send+Sync)];
            let mut context=EventContext::with_arguments(event.as_ref(),&arguments);
            Event::dispatcher().map_err(internal)?.dispatch(event.clone(),&mut context).await.map_err(internal)?;Ok(())
        }
    )+};
}
events!(
    (PasskeyRegistered, registered),
    (PasskeyVerified, verified),
    (PasskeyDeleted, deleted)
);
