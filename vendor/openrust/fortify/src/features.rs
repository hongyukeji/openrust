use openrust::{
    config::ConfigError, contracts::config::Repository, support::php_loose_string_value_equal,
};
use serde_json::{Map, Value};
use std::sync::Arc;

/// Feature queries use the application's repository, including live option changes.
#[derive(Clone)]
pub struct Features {
    config: Arc<dyn Repository>,
}
#[allow(non_snake_case, reason = "Matches Fortify's feature API")]
impl Features {
    pub fn new(config: Arc<dyn Repository>) -> Self {
        Self { config }
    }
    pub fn enabled(&self, feature: &str) -> Result<bool, ConfigError> {
        match self.config.get("fortify.features") {
            None => Ok(false),
            Some(Value::Array(values)) => Ok(values
                .iter()
                .any(|value| php_loose_string_value_equal(feature, value))),
            Some(Value::Object(values)) => Ok(values
                .values()
                .any(|value| php_loose_string_value_equal(feature, value))),
            Some(_) => Err(ConfigError::InvalidType {
                key: "fortify.features".into(),
                expected: "an array",
            }),
        }
    }
    pub fn optionEnabled(&self, feature: &str, option: &str) -> Result<bool, ConfigError> {
        Ok(self.enabled(feature)?
            && self
                .config
                .get(&format!("fortify-options.{feature}.{option}"))
                == Some(Value::Bool(true)))
    }
    pub fn hasProfileFeatures(&self) -> Result<bool, ConfigError> {
        for feature in [
            Self::updateProfileInformation(),
            Self::updatePasswords(),
            Self::twoFactorAuthentication(),
            Self::passkeys(),
        ] {
            if self.enabled(feature)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn canUpdateProfileInformation(&self) -> Result<bool, ConfigError> {
        self.enabled(Self::updateProfileInformation())
    }
    pub fn hasSecurityFeatures(&self) -> Result<bool, ConfigError> {
        Ok(self.canUpdatePasswords()?
            || self.canManageTwoFactorAuthentication()?
            || self.canManagePasskeys()?)
    }
    pub fn canUpdatePasswords(&self) -> Result<bool, ConfigError> {
        self.enabled(Self::updatePasswords())
    }
    pub fn canManageTwoFactorAuthentication(&self) -> Result<bool, ConfigError> {
        self.enabled(Self::twoFactorAuthentication())
    }
    pub fn canManagePasskeys(&self) -> Result<bool, ConfigError> {
        self.enabled(Self::passkeys())
    }
    pub fn registration() -> &'static str {
        "registration"
    }
    pub fn resetPasswords() -> &'static str {
        "reset-passwords"
    }
    pub fn emailVerification() -> &'static str {
        "email-verification"
    }
    pub fn updateProfileInformation() -> &'static str {
        "update-profile-information"
    }
    pub fn updatePasswords() -> &'static str {
        "update-passwords"
    }
    pub fn twoFactorAuthentication() -> &'static str {
        "two-factor-authentication"
    }
    pub fn passkeys() -> &'static str {
        "passkeys"
    }
    /// Rust expresses PHP's optional argument with a separate named method.
    pub fn twoFactorAuthenticationWithOptions(
        &self,
        options: Map<String, Value>,
    ) -> Result<&'static str, ConfigError> {
        self.set_options(Self::twoFactorAuthentication(), options)
    }
    pub fn passkeysWithOptions(
        &self,
        options: Map<String, Value>,
    ) -> Result<&'static str, ConfigError> {
        self.set_options(Self::passkeys(), options)
    }
    fn set_options(
        &self,
        feature: &'static str,
        options: Map<String, Value>,
    ) -> Result<&'static str, ConfigError> {
        if !options.is_empty() {
            self.config.set(
                &format!("fortify-options.{feature}"),
                Value::Object(options),
            )?;
        }
        Ok(feature)
    }
}
