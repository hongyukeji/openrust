use crate::{
    auth::events::Registered,
    contracts::events::{Event, Listener},
    events::{EventContext, EventError, EventFuture, EventResult, ListenerResponse},
};

/// Sends the default verification notification when a verification-capable account registers.
pub struct SendEmailVerificationNotification;

impl Listener for SendEmailVerificationNotification {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            if let Some(event) = event.downcast_ref::<Registered>()
                && let Some(user) = event.user.as_email_verification()
                && !user.hasVerifiedEmail()
            {
                user.sendEmailVerificationNotification()
                    .await
                    .map_err(|source| EventError::Listener {
                        source: Box::new(source),
                    })?;
            }
            Ok(None)
        })
    }
}
