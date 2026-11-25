use openrust::{
    auth::{
        AuthFuture, AuthIdentifier, AuthResult,
        notifications::{ResetPassword, VerifyEmail},
    },
    contracts::auth::{Authenticatable, CanResetPassword, MustVerifyEmail},
    database::{
        Value,
        eloquent::{Model, ModelMetadata, ModelState, casts::AttributeCast},
    },
};

openrust::model_fields! {
    #[generated(command = "artisan model:fields User")]
    User {
        /// 用户表
        TABLE = "users";
        /// 主键
        ID = "id";
        /// 用户名
        NAME = "name";
        /// 邮箱
        EMAIL = "email";
        /// 邮箱验证时间
        EMAIL_VERIFIED_AT = "email_verified_at";
        /// 密码哈希
        PASSWORD = "password";
        /// 记住登录令牌
        REMEMBER_TOKEN = "remember_token";
        /// 创建时间
        CREATED_AT = "created_at";
        /// 更新时间
        UPDATED_AT = "updated_at";
    }
}

pub(crate) struct User(ModelState);

impl Model for User {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new(User::TABLE)
            .fillable(&[User::NAME, User::EMAIL, User::PASSWORD])
            .hidden(&[User::PASSWORD, User::REMEMBER_TOKEN]);
        &METADATA
    }

    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[
            (Self::EMAIL_VERIFIED_AT, AttributeCast::DateTime),
            (User::PASSWORD, AttributeCast::Hashed),
        ]
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
        self.get_key_name()
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        self.get_key()
            .and_then(|value| value.try_into().ok())
            .unwrap_or_else(|| AuthIdentifier::String(String::new()))
    }
    fn get_auth_password_name(&self) -> &str {
        User::PASSWORD
    }
    fn get_auth_password(&self) -> Option<&str> {
        text(self.get_attribute(User::PASSWORD))
    }
    fn get_remember_token(&self) -> Option<&str> {
        text(self.get_attribute(User::REMEMBER_TOKEN))
    }
    fn set_remember_token(&mut self, token: Option<String>) {
        self.set_attribute(User::REMEMBER_TOKEN, token.map_or(Value::Null, Value::Text));
    }
    fn get_remember_token_name(&self) -> &str {
        User::REMEMBER_TOKEN
    }
    fn get_auth_attribute(&self, name: &str) -> AuthResult<Option<Value>> {
        self.get_attribute_value(name).map_err(Into::into)
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

impl CanResetPassword for User {
    fn get_email_for_password_reset(&self) -> &str {
        text(self.get_attribute(Self::EMAIL)).unwrap_or_default()
    }

    fn send_password_reset_notification<'a>(
        &'a self,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async move { ResetPassword::new(token).send_to(self).await })
    }
}

impl MustVerifyEmail for User {
    fn get_email_verification_identifier(&self) -> AuthResult<AuthIdentifier> {
        self.get_key()
            .ok_or(openrust::auth::AuthError::InvalidIdentity)?
            .try_into()
    }
    fn has_verified_email(&self) -> bool {
        self.get_attribute(Self::EMAIL_VERIFIED_AT)
            .is_some_and(|value| *value != Value::Null)
    }

    fn mark_email_as_verified(&mut self) -> AuthFuture<'_, AuthResult<bool>> {
        Box::pin(self.set_verification(true))
    }

    fn mark_email_as_unverified(&mut self) -> AuthFuture<'_, AuthResult<bool>> {
        Box::pin(self.set_verification(false))
    }

    fn send_email_verification_notification(&self) -> AuthFuture<'_, AuthResult<()>> {
        Box::pin(VerifyEmail::send(self))
    }

    fn get_email_for_verification(&self) -> &str {
        self.get_email_for_password_reset()
    }
}

impl User {
    async fn set_verification(&mut self, verified: bool) -> AuthResult<bool> {
        openrust::auth::email_verification::set_verification(
            self,
            verified,
            Self::EMAIL,
            Self::EMAIL_VERIFIED_AT,
            Self::UPDATED_AT,
        )
        .await
    }
}

fn text(value: Option<&Value>) -> Option<&str> {
    match value {
        Some(Value::Text(value)) => Some(value),
        _ => None,
    }
}
