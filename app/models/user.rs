use openrust::{
    auth::{
        AuthFuture, AuthIdentifier, AuthResult,
        notifications::{ResetPassword, VerifyEmail},
    },
    contracts::auth::{Authenticatable, CanResetPassword, MustVerifyEmail},
    database::{
        DB, Value,
        eloquent::{Model, ModelMetadata, ModelState, casts::AttributeCast},
    },
};

/*
|--------------------------------------------------------------------------
| 模型表与字段
|--------------------------------------------------------------------------
|
| 根据数据库结构自动生成，勿手动修改。
| 请在项目根目录执行 `./artisan model:fields User` 同步。
|
*/
openrust::model_fields! {
    User {
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

#[allow(non_snake_case, reason = "认证契约保留公开方法名")]
impl Authenticatable for User {
    fn getAuthIdentifierName(&self) -> &str {
        self.getKeyName()
    }
    fn getAuthIdentifier(&self) -> AuthIdentifier {
        self.getKey()
            .and_then(|value| value.try_into().ok())
            .unwrap_or_else(|| AuthIdentifier::String(String::new()))
    }
    fn getAuthPasswordName(&self) -> &str {
        User::PASSWORD
    }
    fn getAuthPassword(&self) -> Option<&str> {
        text(self.getAttribute(User::PASSWORD))
    }
    fn getRememberToken(&self) -> Option<&str> {
        text(self.getAttribute(User::REMEMBER_TOKEN))
    }
    fn setRememberToken(&mut self, token: Option<String>) {
        self.setAttribute(User::REMEMBER_TOKEN, token.map_or(Value::Null, Value::Text));
    }
    fn getRememberTokenName(&self) -> &str {
        User::REMEMBER_TOKEN
    }
    fn getAuthAttribute(&self, name: &str) -> AuthResult<Option<Value>> {
        self.getAttributeValue(name).map_err(Into::into)
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

#[allow(non_snake_case, reason = "密码重置契约保留公开方法名")]
impl CanResetPassword for User {
    fn getEmailForPasswordReset(&self) -> &str {
        text(self.getAttribute(Self::EMAIL)).unwrap_or_default()
    }

    fn sendPasswordResetNotification<'a>(
        &'a self,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async move { ResetPassword::new(token).sendTo(self).await })
    }
}

#[allow(non_snake_case, reason = "邮箱验证契约保留公开方法名")]
impl MustVerifyEmail for User {
    fn hasVerifiedEmail(&self) -> bool {
        self.getAttribute(Self::EMAIL_VERIFIED_AT)
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
        let now = jiff::Timestamp::now()
            .strftime("%Y-%m-%d %H:%M:%S")
            .to_string();
        let value = if verified {
            Value::Text(now.clone())
        } else {
            Value::Null
        };
        let query = DB::connection(self.getConnectionName())?
            .table(self.getTable())
            .r#where(
                self.getKeyName(),
                "=",
                Value::from(self.getAuthIdentifier()),
            )
            .whereExactText(Self::EMAIL, self.getEmailForVerification());
        let query = if verified {
            query.whereNull(Self::EMAIL_VERIFIED_AT)
        } else {
            query.whereNotNull(Self::EMAIL_VERIFIED_AT)
        };
        let changed = query
            .update(
                [
                    (Self::EMAIL_VERIFIED_AT.into(), value.clone()),
                    (Self::UPDATED_AT.into(), now.clone().into()),
                ]
                .into(),
            )
            .await?
            != 0;
        if changed {
            self.setAttribute(Self::EMAIL_VERIFIED_AT, value);
            self.setAttribute(Self::UPDATED_AT, now);
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
