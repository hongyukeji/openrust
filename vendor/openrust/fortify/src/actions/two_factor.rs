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
        user.force_fill(
            [
                ("two_factor_secret".into(), encrypted_secret.into()),
                ("two_factor_recovery_codes".into(), encrypted_codes.into()),
            ]
            .into(),
        )
        .map_err(internal)?
        .save()
        .await
        .map_err(internal)?;
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
        user.force_fill([("two_factor_confirmed_at".into(), date.into())].into())
            .map_err(internal)?
            .save()
            .await
            .map_err(internal)?;
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
        user.force_fill(attributes)
            .map_err(internal)?
            .save()
            .await
            .map_err(internal)?;
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
        let encrypter = self.fortify.current_encrypter().map_err(internal)?;
        let codes = Zeroizing::new(recovery_codes(&self.fortify));
        user.force_fill(
            [(
                "two_factor_recovery_codes".into(),
                encrypter
                    .encrypt(codes.as_bytes(), true)
                    .map_err(internal)?
                    .into(),
            )]
            .into(),
        )
        .map_err(internal)?
        .save()
        .await
        .map_err(internal)?;
        super::emit(
            RecoveryCodesGenerated {
                user: user.get_auth_identifier(),
            },
            &[user],
        )
        .await
    }
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
