use openrust::{
    auth::{
        AuthError, AuthFuture, AuthIdentifier, AuthResult,
        notifications::{ResetPassword, VerifyEmail},
    },
    contracts::auth::{Authenticatable, CanResetPassword, MustVerifyEmail},
    database::{
        DB, Value,
        eloquent::{Model, ModelMetadata, ModelState, casts::AttributeCast},
    },
};

pub(crate) struct User(ModelState);

impl Model for User {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("users")
            .fillable(&["name", "email", "password"])
            .hidden(&["password", "remember_token"]);
        &METADATA
    }

    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[("password", AttributeCast::Hashed)]
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

impl Authenticatable for User {
    fn get_auth_identifier_name(&self) -> &str {
        self.getKeyName()
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        self.getKey()
            .and_then(|value| value.try_into().ok())
            .unwrap_or_else(|| AuthIdentifier::String(String::new()))
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        text(self.getAttribute("password"))
    }
    fn get_remember_token(&self) -> Option<&str> {
        text(self.getAttribute("remember_token"))
    }
    fn set_remember_token(&mut self, token: Option<String>) {
        self.setAttribute("remember_token", token.map_or(Value::Null, Value::Text));
    }
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
    fn as_password_reset(&self) -> Option<&dyn CanResetPassword> {
        Some(self)
    }
    fn as_password_reset_mut(&mut self) -> Option<&mut dyn CanResetPassword> {
        Some(self)
    }
    fn as_email_verification(&self) -> Option<&dyn MustVerifyEmail> {
        Some(self)
    }
}

#[allow(non_snake_case, reason = "Implements Laravel's user reset capability")]
impl CanResetPassword for User {
    fn getEmailForPasswordReset(&self) -> &str {
        text(self.getAttribute("email")).unwrap_or_default()
    }

    fn sendPasswordResetNotification<'a>(
        &'a self,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async move {
            ResetPassword::new(token)
                .send(self.getEmailForPasswordReset())
                .await
        })
    }
}

#[allow(
    non_snake_case,
    reason = "Implements Laravel's user verification capability"
)]
impl MustVerifyEmail for User {
    fn hasVerifiedEmail(&self) -> bool {
        self.getAttribute("email_verified_at")
            .is_some_and(|value| *value != Value::Null)
    }

    fn markEmailAsVerified(&mut self) -> AuthFuture<'_, AuthResult<bool>> {
        Box::pin(self.set_verification(true))
    }

    fn markEmailAsUnverified(&mut self) -> AuthFuture<'_, AuthResult<bool>> {
        Box::pin(self.set_verification(false))
    }

    fn sendEmailVerificationNotification(&self) -> AuthFuture<'_, AuthResult<()>> {
        Box::pin(VerifyEmail::send(self))
    }

    fn getEmailForVerification(&self) -> &str {
        self.getEmailForPasswordReset()
    }
}

impl User {
    async fn set_verification(&mut self, verified: bool) -> AuthResult<bool> {
        let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let value = if verified {
            Value::Text(now.clone())
        } else {
            Value::Null
        };
        let query = DB::connection(self.getConnectionName())
            .map_err(|source| AuthError::Backend(Box::new(source)))?
            .table(self.getTable())
            .r#where(
                self.getKeyName(),
                "=",
                Value::from(self.get_auth_identifier()),
            )
            .r#where("email", "=", self.getEmailForVerification());
        let query = if verified {
            query.whereNull("email_verified_at")
        } else {
            query.whereNotNull("email_verified_at")
        };
        let changed = query
            .update(
                [
                    ("email_verified_at".into(), value.clone()),
                    ("updated_at".into(), now.clone().into()),
                ]
                .into(),
            )
            .await
            .map_err(|source| AuthError::Backend(Box::new(source)))?
            != 0;
        if changed {
            self.setAttribute("email_verified_at", value);
            self.setAttribute("updated_at", now);
        }
        Ok(changed)
    }
}

fn text(value: Option<&Value>) -> Option<&str> {
    match value {
        Some(Value::Text(value)) => Some(value),
        _ => None,
    }
}
