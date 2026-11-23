#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use crate::{
    Fortify, FortifyError, RecoveryCode, TwoFactorAuthenticatable,
    contracts::TwoFactorAuthenticationProvider,
    events::{
        RecoveryCodesGenerated, TwoFactorAuthenticationConfirmed, TwoFactorAuthenticationDisabled,
        TwoFactorAuthenticationEnabled,
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
        let encrypter = self.fortify.currentEncrypter().map_err(internal)?;
        let secret = Zeroizing::new(self.provider.generateSecretKey(length).map_err(internal)?);
        let encrypted_secret = encrypter
            .encrypt(secret.as_bytes(), true)
            .map_err(internal)?;
        let encrypter = self.fortify.currentEncrypter().map_err(internal)?;
        let codes = Zeroizing::new(recovery_codes(&self.fortify));
        let encrypted_codes = encrypter
            .encrypt(codes.as_bytes(), true)
            .map_err(internal)?;
        user.forceFill(
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
                    .currentEncrypter()
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
            .asDateTime()
            .strftime(user.getDateFormat())
            .to_string();
        user.forceFill([("two_factor_confirmed_at".into(), date.into())].into())
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
            .getAttributeValue("two_factor_confirmed_at")
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
            .confirmsTwoFactorAuthentication()
            .map_err(internal)?
            || confirmed
        {
            attributes.insert("two_factor_confirmed_at".into(), Value::Null);
        }
        user.forceFill(attributes)
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
        let encrypter = self.fortify.currentEncrypter().map_err(internal)?;
        let codes = Zeroizing::new(recovery_codes(&self.fortify));
        user.forceFill(
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
