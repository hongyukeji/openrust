#![allow(non_snake_case, reason = "认证与队列公开契约沿用 Laravel 原名")]

use crate::{PasskeyRecord, internal};
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
        pub struct $name {pub user:AuthIdentifier,pub passkey:PasskeyRecord}
        impl EventContract for $name {fn name(&self)->&str {concat!("OpenRust\\Passkeys\\Events\\",stringify!($name))}}
        impl $name { pub fn user<'a,T:Authenticatable>(&self,context:&EventContext<'a>)->Option<&'a T> {context.arguments_for(self)?.first()?.downcast_ref()} pub fn passkey<'a,T:crate::PasskeyModel>(&self,context:&EventContext<'a>)->Option<&'a T> {context.arguments_for(self)?.get(1)?.downcast_ref()} }
        pub(crate) async fn $emit(user:&dyn Authenticatable,passkey:&PasskeyRecord)->Result<(),HttpError> {
            let event=Arc::new($name{user:user.getAuthIdentifier(),passkey:passkey.clone()});
            let snapshot=passkey.snapshot().await;
            let arguments=[user as &(dyn std::any::Any+Send+Sync),snapshot.as_ref()];
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
