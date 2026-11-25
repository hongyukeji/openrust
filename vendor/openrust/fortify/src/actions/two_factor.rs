use crate::{
    Fortify, FortifyError, RecoveryCode, TwoFactorAuthenticatable,
    contracts::TwoFactorAuthenticationProvider,
    events::{
        RecoveryCodeReplaced, RecoveryCodesGenerated, TwoFactorAuthenticationConfirmed,
        TwoFactorAuthenticationDisabled, TwoFactorAuthenticationEnabled,
    },
    resolver::internal,
    two_factor_authenticatable::text,
};
use openrust::{
    database::Value,
    http::HttpError,
    support::{facades::Lang, now, php_integer_cast},
};
use std::sync::Arc;
use zeroize::Zeroizing;

pub struct EnableTwoFactorAuthentication {
    provider: Arc<dyn TwoFactorAuthenticationProvider>,
    fortify: Arc<Fortify>,
}
pub struct ConfirmTwoFactorAuthentication {
    provider: Arc<dyn TwoFactorAuthenticationProvider>,
    fortify: Arc<Fortify>,
}
pub struct DisableTwoFactorAuthentication {
    fortify: Arc<Fortify>,
}
pub struct GenerateNewRecoveryCodes {
    fortify: Arc<Fortify>,
}
fn recovery_codes(fortify: &Fortify) -> String {
    let codes = Zeroizing::new(
        (0..8)
            .map(|_| RecoveryCode::generate(fortify))
            .collect::<Vec<_>>(),
    );
    crate::recovery_code::encode(&codes)
}
impl EnableTwoFactorAuthentication {
    pub fn new(provider: Arc<dyn TwoFactorAuthenticationProvider>, fortify: Arc<Fortify>) -> Self {
        Self { provider, fortify }
    }
    pub async fn call<T: TwoFactorAuthenticatable>(
        &self,
        user: &mut T,
        force: bool,
    ) -> Result<(), HttpError> {
        self.perform(user, force, None).await
    }
    /// 调用方持有用户锁并负责回滚；保留原生生成、加密、模型及事件顺序。
    pub async fn call_in_transaction<T: TwoFactorAuthenticatable>(
        &self,
        user: &mut T,
        transaction: &mut openrust::database::Transaction,
        force: bool,
    ) -> Result<(), HttpError> {
        validate_transaction(user, transaction)?;
        self.perform(user, force, Some(transaction)).await
    }
    async fn perform<T: TwoFactorAuthenticatable>(
        &self,
        user: &mut T,
        force: bool,
        transaction: Option<&mut openrust::database::Transaction>,
    ) -> Result<(), HttpError> {
        let secret = text(user, "two_factor_secret").map_err(internal)?;
        if !force
            && secret
                .as_deref()
                .is_some_and(|value| !value.is_empty() && value != "0")
        {
            return Ok(());
        }
        let length = self
            .fortify
            .config
            .get("fortify-options.two-factor-authentication.secret-length")
            .unwrap_or(serde_json::json!(16));
        let length = usize::try_from(php_integer_cast(&length))
            .map_err(|_| internal(FortifyError::InvalidSecretLength))?;
        let encrypter = self.fortify.current_encrypter().map_err(internal)?;
        let secret = Zeroizing::new(
            self.provider
                .generate_secret_key(length)
                .map_err(internal)?,
        );
        let encrypted_secret = encrypter
            .encrypt(secret.as_bytes(), true)
            .map_err(internal)?;
        let encrypter = self.fortify.current_encrypter().map_err(internal)?;
        let codes = Zeroizing::new(recovery_codes(&self.fortify));
        let encrypted_codes = encrypter
            .encrypt(codes.as_bytes(), true)
            .map_err(internal)?;
        persist(
            user,
            [
                ("two_factor_secret".into(), encrypted_secret.into()),
                ("two_factor_recovery_codes".into(), encrypted_codes.into()),
            ]
            .into(),
            transaction,
        )
        .await?;
        super::emit(
            TwoFactorAuthenticationEnabled {
                user: user.get_auth_identifier(),
            },
            &[user],
        )
        .await
    }
}
impl ConfirmTwoFactorAuthentication {
    pub fn new(provider: Arc<dyn TwoFactorAuthenticationProvider>, fortify: Arc<Fortify>) -> Self {
        Self { provider, fortify }
    }
    pub async fn call<T: TwoFactorAuthenticatable>(
        &self,
        user: &mut T,
        code: &str,
    ) -> Result<(), HttpError> {
        self.perform(user, code, None).await
    }
    /// 在调用方事务内确认；提供方可能消费验证码，数据库失败后须使用新验证码重试。
    pub async fn call_in_transaction<T: TwoFactorAuthenticatable>(
        &self,
        user: &mut T,
        transaction: &mut openrust::database::Transaction,
        code: &str,
    ) -> Result<(), HttpError> {
        validate_transaction(user, transaction)?;
        self.perform(user, code, Some(transaction)).await
    }
    async fn perform<T: TwoFactorAuthenticatable>(
        &self,
        user: &mut T,
        code: &str,
        transaction: Option<&mut openrust::database::Transaction>,
    ) -> Result<(), HttpError> {
        let secret = text(user, "two_factor_secret").map_err(internal)?;
        let valid = if let Some(encrypted) = secret.filter(|v| !v.is_empty() && v != "0")
            && !code.is_empty()
            && code != "0"
        {
            let secret = Zeroizing::new(
                self.fortify
                    .current_encrypter()
                    .map_err(internal)?
                    .decrypt(&encrypted, true)
                    .map_err(internal)?,
            );
            let secret =
                std::str::from_utf8(&secret).map_err(|_| internal(FortifyError::InvalidUtf8))?;
            self.provider.verify(secret, code).await.map_err(internal)?
        } else {
            false
        };
        if !valid {
            let message = Lang::get(
                "The provided two factor authentication code was invalid.",
                &Default::default(),
                None,
            )
            .map_err(internal)?;
            let mut error = super::invalid("code", message);
            error.error_bag("confirmTwoFactorAuthentication");
            return Err(HttpError::Validation(Box::new(error)));
        }
        let date = now(None)
            .map_err(internal)?
            .as_date_time()
            .strftime(user.get_date_format())
            .to_string();
        persist(
            user,
            [("two_factor_confirmed_at".into(), date.into())].into(),
            transaction,
        )
        .await?;
        super::emit(
            TwoFactorAuthenticationConfirmed {
                user: user.get_auth_identifier(),
            },
            &[user],
        )
        .await
    }
}
impl DisableTwoFactorAuthentication {
    pub fn new(fortify: Arc<Fortify>) -> Self {
        Self { fortify }
    }
    pub async fn call<T: TwoFactorAuthenticatable>(&self, user: &mut T) -> Result<(), HttpError> {
        self.perform(user, None).await
    }
    /// 使用同一用户连接的事务；保存被模型取消时返回错误，由调用方回滚。
    pub async fn call_in_transaction<T: TwoFactorAuthenticatable>(
        &self,
        user: &mut T,
        transaction: &mut openrust::database::Transaction,
    ) -> Result<(), HttpError> {
        validate_transaction(user, transaction)?;
        self.perform(user, Some(transaction)).await
    }
    async fn perform<T: TwoFactorAuthenticatable>(
        &self,
        user: &mut T,
        transaction: Option<&mut openrust::database::Transaction>,
    ) -> Result<(), HttpError> {
        let confirmed = user
            .get_attribute_value("two_factor_confirmed_at")
            .map_err(internal)?
            .is_some_and(|v| v != Value::Null);
        if text(user, "two_factor_secret").map_err(internal)?.is_none()
            && text(user, "two_factor_recovery_codes")
                .map_err(internal)?
                .is_none()
            && !confirmed
        {
            return Ok(());
        }
        let mut attributes = [
            ("two_factor_secret".into(), Value::Null),
            ("two_factor_recovery_codes".into(), Value::Null),
        ]
        .into_iter()
        .collect::<openrust::database::eloquent::Attributes>();
        if self
            .fortify
            .confirms_two_factor_authentication()
            .map_err(internal)?
            || confirmed
        {
            attributes.insert("two_factor_confirmed_at".into(), Value::Null);
        }
        persist(user, attributes, transaction).await?;
        super::emit(
            TwoFactorAuthenticationDisabled {
                user: user.get_auth_identifier(),
            },
            &[user],
        )
        .await
    }
}
impl GenerateNewRecoveryCodes {
    pub fn new(fortify: Arc<Fortify>) -> Self {
        Self { fortify }
    }
    pub async fn call<T: TwoFactorAuthenticatable>(&self, user: &mut T) -> Result<(), HttpError> {
        self.perform(user, None).await
    }
    /// 使用同一用户连接的事务；保存被模型取消时返回错误，由调用方回滚。
    pub async fn call_in_transaction<T: TwoFactorAuthenticatable>(
        &self,
        user: &mut T,
        transaction: &mut openrust::database::Transaction,
    ) -> Result<(), HttpError> {
        validate_transaction(user, transaction)?;
        self.perform(user, Some(transaction)).await
    }
    async fn perform<T: TwoFactorAuthenticatable>(
        &self,
        user: &mut T,
        transaction: Option<&mut openrust::database::Transaction>,
    ) -> Result<(), HttpError> {
        let encrypter = self.fortify.current_encrypter().map_err(internal)?;
        let codes = Zeroizing::new(recovery_codes(&self.fortify));
        persist(
            user,
            [(
                "two_factor_recovery_codes".into(),
                encrypter
                    .encrypt(codes.as_bytes(), true)
                    .map_err(internal)?
                    .into(),
            )]
            .into(),
            transaction,
        )
        .await?;
        super::emit(
            RecoveryCodesGenerated {
                user: user.get_auth_identifier(),
            },
            &[user],
        )
        .await
    }
}

/*
 * 普通入口保持固定Fortify的保存行为；事务入口拒绝跨连接与模型取消。
 * 事件仍在保存后、调用方提交前派发，外部副作用由监听器使用提交后机制。
 */
fn validate_transaction<T: TwoFactorAuthenticatable>(
    user: &T,
    transaction: &openrust::database::Transaction,
) -> Result<(), HttpError> {
    if transaction.transaction_level() == 0 {
        return Err(internal(
            openrust::database::DatabaseError::TransactionClosed,
        ));
    }
    if !transaction.belongs_to(&user.get_connection().map_err(internal)?) {
        return Err(internal(openrust::database::DatabaseError::InvalidQuery(
            "双因素操作必须使用用户连接的同一事务",
        )));
    }
    Ok(())
}
async fn persist<T: TwoFactorAuthenticatable>(
    user: &mut T,
    attributes: openrust::database::eloquent::Attributes,
    transaction: Option<&mut openrust::database::Transaction>,
) -> Result<(), HttpError> {
    user.force_fill(attributes).map_err(internal)?;
    if let Some(transaction) = transaction {
        if !user
            .save_in_transaction(transaction)
            .await
            .map_err(internal)?
        {
            return Err(internal(openrust::database::DatabaseError::InvalidQuery(
                "用户模型取消双因素变更，须回滚调用方事务",
            )));
        }
    } else {
        user.save().await.map_err(internal)?;
    }
    Ok(())
}

// 默认模型能力委托此实现；保留加密器、生成器、保存及事件的原执行顺序。
pub(crate) async fn replace_recovery_code<T: TwoFactorAuthenticatable>(
    user: &mut T,
    code: &str,
    fortify: &Fortify,
) -> Result<(), HttpError> {
    let ciphertext = text(user, "two_factor_recovery_codes")
        .map_err(internal)?
        .ok_or_else(|| internal(FortifyError::InvalidAttribute("two_factor_recovery_codes")))?;
    let encrypter = fortify.current_encrypter().map_err(internal)?;
    let replacement = Zeroizing::new(RecoveryCode::generate(fortify));
    let cleartext = Zeroizing::new(
        fortify
            .current_encrypter()
            .map_err(internal)?
            .decrypt(&ciphertext, true)
            .map_err(internal)?,
    );
    let cleartext =
        std::str::from_utf8(&cleartext).map_err(|_| internal(FortifyError::InvalidUtf8))?;
    let updated = Zeroizing::new(cleartext.replace(code, &replacement));
    let encrypted = encrypter
        .encrypt(updated.as_bytes(), true)
        .map_err(internal)?;
    user.force_fill([("two_factor_recovery_codes".into(), encrypted.into())].into())
        .map_err(internal)?
        .save()
        .await
        .map_err(internal)?;
    super::emit(
        RecoveryCodeReplaced {
            user: user.get_auth_identifier(),
            code: Zeroizing::new(code.into()),
        },
        &[user],
    )
    .await
}

/// 认证流程中的单次恢复码消费；公开的普通替换方法保留原生契约。
pub(crate) async fn consume_recovery_code<T: TwoFactorAuthenticatable>(
    user: &mut T,
    code: &str,
    fortify: &Fortify,
) -> Result<(), HttpError> {
    use subtle::ConstantTimeEq as _;
    let original = user.state().clone();
    let password = user.get_auth_password().map(str::to_owned);
    let version_column = fortify
        .config
        .get("auth.session_credential_version_attribute")
        .and_then(|value| value.as_str().map(str::to_owned));
    let version = version_column
        .as_deref()
        .map(|name| user.get_auth_attribute(name))
        .transpose()?;
    let result = async {
        if !user.state().exists() || user.get_raw_original(user.get_key_name()) != user.get_key() {
            return Err(HttpError::Unauthenticated);
        }
        let key = user.get_key().cloned().ok_or(HttpError::Unauthenticated)?;
        let connection = user.get_connection().map_err(internal)?;
        let mut transaction = connection.begin_transaction().await.map_err(internal)?;
        if connection.get_driver_name() == "sqlite" {
            // SQLite没有SELECT FOR UPDATE；只对已知非主键列取得写锁，避免写入IDENTITY主键。
            transaction
                .table(user.get_table())
                .r#where(user.get_key_name(), "=", key)
                .update_with(
                    [(
                        "two_factor_recovery_codes".into(),
                        openrust::database::query::Expression::new("two_factor_recovery_codes")
                            .into(),
                    )]
                    .into(),
                )
                .await
                .map_err(internal)?;
        }
        if let Err(error) = user.refresh_for_update(&mut transaction).await {
            return Err(match error {
                openrust::database::DatabaseError::ModelNotFound(_) => HttpError::Unauthenticated,
                error => internal(error),
            });
        }
        if user.get_auth_password() != password.as_deref()
            || version_column
                .as_deref()
                .map(|name| user.get_auth_attribute(name))
                .transpose()?
                != version
        {
            return Err(HttpError::Unauthenticated);
        }
        let codes = Zeroizing::new(user.recovery_codes(fortify).map_err(internal)?);
        if !codes
            .iter()
            .any(|stored| bool::from(stored.as_bytes().ct_eq(code.as_bytes())))
        {
            let message = Lang::get(
                "The provided two factor recovery code was invalid.",
                &Default::default(),
                None,
            )?;
            return Err(HttpError::Validation(Box::new(
                crate::http::responses::invalid("recovery_code", message),
            )));
        }
        let ciphertext = text(user, "two_factor_recovery_codes")
            .map_err(internal)?
            .ok_or_else(|| internal(FortifyError::InvalidAttribute("two_factor_recovery_codes")))?;
        let clear = Zeroizing::new(
            fortify
                .current_encrypter()
                .map_err(internal)?
                .decrypt(&ciphertext, true)
                .map_err(internal)?,
        );
        let clear = std::str::from_utf8(&clear).map_err(|_| internal(FortifyError::InvalidUtf8))?;
        let replacement = Zeroizing::new(RecoveryCode::generate(fortify));
        let updated = Zeroizing::new(clear.replace(code, &replacement));
        let encrypted = fortify
            .current_encrypter()
            .map_err(internal)?
            .encrypt(updated.as_bytes(), true)
            .map_err(internal)?;
        user.force_fill([("two_factor_recovery_codes".into(), encrypted.into())].into())
            .map_err(internal)?;
        if !user
            .save_in_transaction(&mut transaction)
            .await
            .map_err(internal)?
        {
            return Err(internal(std::io::Error::other("用户模型取消恢复码消费")));
        }
        transaction.commit().await.map_err(internal)?;
        Ok(())
    }
    .await;
    if let Err(error) = result {
        *user.state_mut() = original;
        return Err(error);
    }
    super::emit(
        RecoveryCodeReplaced {
            user: user.get_auth_identifier(),
            code: Zeroizing::new(code.into()),
        },
        &[user],
    )
    .await
}
