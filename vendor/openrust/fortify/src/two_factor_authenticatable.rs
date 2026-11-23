#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use crate::{
    Fortify, FortifyError, FortifyResult, RecoveryCode, actions::emit,
    events::RecoveryCodeReplaced, resolver::internal,
};
use openrust::{
    contracts::auth::Authenticatable,
    database::{Value, eloquent::Model},
    http::HttpError,
};
use zeroize::Zeroizing;

/// Explicit model opt-in corresponding to Fortify's Eloquent trait.
pub trait TwoFactorAuthenticatable: Authenticatable + Model {
    fn has_enabled_two_factor_authentication(&self, fortify: &Fortify) -> FortifyResult<bool> {
        Ok(text(self, "two_factor_secret")?.is_some()
            && (!fortify.confirms_two_factor_authentication()?
                || !matches!(
                    self.get_attribute_value("two_factor_confirmed_at")?,
                    None | Some(Value::Null)
                )))
    }
    fn recovery_codes(&self, fortify: &Fortify) -> FortifyResult<Vec<String>> {
        let ciphertext = text(self, "two_factor_recovery_codes")?
            .ok_or(FortifyError::InvalidAttribute("two_factor_recovery_codes"))?;
        let cleartext = Zeroizing::new(fortify.current_encrypter()?.decrypt(&ciphertext, true)?);
        Ok(serde_json::from_slice(&cleartext)?)
    }
    fn replace_recovery_code(
        &mut self,
        code: &str,
        fortify: &Fortify,
    ) -> impl Future<Output = Result<(), HttpError>> + Send {
        async move {
            let ciphertext = text(self, "two_factor_recovery_codes")
                .map_err(internal)?
                .ok_or_else(|| {
                    internal(FortifyError::InvalidAttribute("two_factor_recovery_codes"))
                })?;
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
            self.force_fill([("two_factor_recovery_codes".into(), encrypted.into())].into())
                .map_err(internal)?
                .save()
                .await
                .map_err(internal)?;
            emit(
                RecoveryCodeReplaced {
                    user: self.get_auth_identifier(),
                    code: Zeroizing::new(code.into()),
                },
                &[self],
            )
            .await
        }
    }
    fn two_factor_qr_code_svg(
        &self,
        fortify: &Fortify,
        provider: &dyn crate::contracts::TwoFactorAuthenticationProvider,
    ) -> FortifyResult<String> {
        crate::qr::two_factor_qr_code_svg(&self.two_factor_qr_code_url(fortify, provider)?)
    }
    fn two_factor_qr_code_url(
        &self,
        fortify: &Fortify,
        provider: &dyn crate::contracts::TwoFactorAuthenticationProvider,
    ) -> FortifyResult<String> {
        let encrypted = text(self, "two_factor_secret")?
            .ok_or(FortifyError::InvalidAttribute("two_factor_secret"))?;
        let cleartext = Zeroizing::new(fortify.current_encrypter()?.decrypt(&encrypted, true)?);
        let secret = std::str::from_utf8(&cleartext).map_err(|_| FortifyError::InvalidUtf8)?;
        let username = self.get_attribute_value(&fortify.username())?;
        let username = match username {
            Some(Value::Text(value)) => value,
            _ => return Err(FortifyError::InvalidAttribute("username")),
        };
        Ok(provider.qr_code_url(&fortify.string("app.name", ""), &username, secret))
    }
}
pub(crate) fn text<T: Model>(user: &T, attribute: &'static str) -> FortifyResult<Option<String>> {
    match user.get_attribute_value(attribute)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Text(value)) => Ok(Some(value)),
        _ => Err(FortifyError::InvalidAttribute(attribute)),
    }
}
