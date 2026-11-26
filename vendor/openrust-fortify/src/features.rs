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
    pub fn option_enabled(&self, feature: &str, option: &str) -> Result<bool, ConfigError> {
        Ok(self.enabled(feature)?
            && self
                .config
                .get(&format!("fortify-options.{feature}.{option}"))
                == Some(Value::Bool(true)))
    }
    pub fn has_profile_features(&self) -> Result<bool, ConfigError> {
        for feature in [
            Self::update_profile_information(),
            Self::update_passwords(),
            Self::two_factor_authentication(),
            Self::passkeys(),
        ] {
            if self.enabled(feature)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn can_update_profile_information(&self) -> Result<bool, ConfigError> {
        self.enabled(Self::update_profile_information())
    }
    pub fn has_security_features(&self) -> Result<bool, ConfigError> {
        Ok(self.can_update_passwords()?
            || self.can_manage_two_factor_authentication()?
            || self.can_manage_passkeys()?)
    }
    pub fn can_update_passwords(&self) -> Result<bool, ConfigError> {
        self.enabled(Self::update_passwords())
    }
    pub fn can_manage_two_factor_authentication(&self) -> Result<bool, ConfigError> {
        self.enabled(Self::two_factor_authentication())
    }
    pub fn can_manage_passkeys(&self) -> Result<bool, ConfigError> {
        self.enabled(Self::passkeys())
    }
    pub fn registration() -> &'static str {
        "registration"
    }
    pub fn reset_passwords() -> &'static str {
        "reset-passwords"
    }
    pub fn email_verification() -> &'static str {
        "email-verification"
    }
    pub fn update_profile_information() -> &'static str {
        "update-profile-information"
    }
    pub fn update_passwords() -> &'static str {
        "update-passwords"
    }
    pub fn two_factor_authentication() -> &'static str {
        "two-factor-authentication"
    }
    pub fn passkeys() -> &'static str {
        "passkeys"
    }
    /// Rust expresses PHP's optional argument with a separate named method.
    pub fn two_factor_authentication_with_options(
        &self,
        options: Map<String, Value>,
    ) -> Result<&'static str, ConfigError> {
        self.set_options(Self::two_factor_authentication(), options)
    }
    pub fn passkeys_with_options(
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
