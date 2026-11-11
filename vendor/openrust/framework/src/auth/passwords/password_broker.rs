use std::{sync::Arc, time::Duration};

use tokio::time::{Instant, sleep_until};

use crate::{
    auth::{AuthError, AuthFuture, AuthResult, Credentials},
    contracts::{
        auth::{
            Authenticatable, CanResetPassword, PasswordBroker as Contract, ResetCallback,
            ResetLinkCallback, UserProvider,
        },
        events::Dispatcher as _,
    },
    events::{Dispatcher, EventContext},
};

use super::{PasswordResetStatus as Status, TokenRepository};

/// Coordinates user lookup, reset notifications, and token consumption.
pub struct PasswordBroker {
    tokens: Arc<dyn TokenRepository>,
    users: Arc<dyn UserProvider>,
    events: Arc<Dispatcher>,
    timebox: Duration,
}

#[allow(non_snake_case, reason = "Matches Laravel's PasswordBroker API")]
impl PasswordBroker {
    pub fn new(
        tokens: Arc<dyn TokenRepository>,
        users: Arc<dyn UserProvider>,
        events: Arc<Dispatcher>,
        timebox: Duration,
    ) -> Self {
        Self {
            tokens,
            users,
            events,
            timebox,
        }
    }

    pub async fn getUser(
        &self,
        credentials: &Credentials,
    ) -> AuthResult<Option<Box<dyn Authenticatable>>> {
        let lookup = Credentials::new(
            credentials
                .fields()
                .iter()
                .filter(|(field, _)| field.as_str() != "token")
                .map(|(field, value)| (field.as_str(), value.clone())),
            "",
        );
        let user = self.users.retrieve_by_credentials(&lookup).await?;
        if user
            .as_ref()
            .is_some_and(|user| user.as_password_reset().is_none())
        {
            return Err(AuthError::Configuration(
                "user must implement CanResetPassword",
            ));
        }
        Ok(user)
    }

    pub fn getRepository(&self) -> &dyn TokenRepository {
        self.tokens.as_ref()
    }

    pub async fn createToken(&self, user: &dyn CanResetPassword) -> AuthResult<String> {
        self.tokens.create(user).await
    }

    pub async fn deleteToken(&self, user: &dyn CanResetPassword) -> AuthResult<()> {
        self.tokens.delete(user).await
    }

    pub async fn tokenExists(&self, user: &dyn CanResetPassword, token: &str) -> AuthResult<bool> {
        self.tokens.exists(user, token).await
    }

    pub async fn sendResetLink(
        &self,
        credentials: &Credentials,
        callback: Option<&ResetLinkCallback<'_>>,
    ) -> AuthResult<Status> {
        let deadline = Instant::now()
            .checked_add(self.timebox)
            .ok_or(AuthError::Configuration("auth.timebox_duration"))?;
        let result = self.send_link(credentials, callback).await;
        sleep_until(deadline).await;
        result
    }

    async fn send_link(
        &self,
        credentials: &Credentials,
        callback: Option<&ResetLinkCallback<'_>>,
    ) -> AuthResult<Status> {
        let Some(user) = self.getUser(credentials).await? else {
            return Ok(Status::InvalidUser);
        };
        let user = user.as_password_reset().ok_or(AuthError::Configuration(
            "user must implement CanResetPassword",
        ))?;
        let Some(token) = self.tokens.issue(user).await? else {
            return Ok(Status::ResetThrottled);
        };
        if let Some(callback) = callback {
            return Ok(callback(user, &token)
                .await?
                .unwrap_or(Status::ResetLinkSent));
        }
        user.sendPasswordResetNotification(&token).await?;
        self.events
            .dispatch(
                Arc::new(crate::auth::events::PasswordResetLinkSent {
                    user: user.get_auth_identifier(),
                }),
                &mut EventContext::new(),
            )
            .await
            .map_err(AuthError::backend)?;
        Ok(Status::ResetLinkSent)
    }

    pub async fn reset(
        &self,
        credentials: &Credentials,
        token: &str,
        password: &[u8],
        callback: &ResetCallback<'_>,
    ) -> AuthResult<Status> {
        let deadline = Instant::now()
            .checked_add(self.timebox)
            .ok_or(AuthError::Configuration("auth.timebox_duration"))?;
        let result = self
            .apply_reset(credentials, token, password, callback)
            .await;
        if !matches!(result, Ok(Status::PasswordReset)) {
            sleep_until(deadline).await;
        }
        result
    }

    async fn apply_reset(
        &self,
        credentials: &Credentials,
        token: &str,
        password: &[u8],
        callback: &ResetCallback<'_>,
    ) -> AuthResult<Status> {
        let Some(mut user) = self.getUser(credentials).await? else {
            return Ok(Status::InvalidUser);
        };
        let user = user
            .as_password_reset_mut()
            .ok_or(AuthError::Configuration(
                "user must implement CanResetPassword",
            ))?;
        let Some(claim) = self.tokens.claim(user, token).await? else {
            return Ok(Status::InvalidToken);
        };
        // Finalize using the email at validation time even if the callback changes the user.
        let email = user.getEmailForPasswordReset().to_owned();
        let result = callback(user, password).await;
        let completion = self.tokens.finish(&email, claim, result.is_ok()).await;
        result?;
        completion?;
        Ok(Status::PasswordReset)
    }
}

impl Contract for PasswordBroker {
    fn sendResetLink<'a>(
        &'a self,
        credentials: &'a Credentials,
        callback: Option<&'a ResetLinkCallback<'a>>,
    ) -> AuthFuture<'a, AuthResult<Status>> {
        Box::pin(self.sendResetLink(credentials, callback))
    }

    fn reset<'a>(
        &'a self,
        credentials: &'a Credentials,
        token: &'a str,
        password: &'a [u8],
        callback: &'a ResetCallback<'a>,
    ) -> AuthFuture<'a, AuthResult<Status>> {
        Box::pin(self.reset(credentials, token, password, callback))
    }
}
