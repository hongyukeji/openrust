use aws_lc_rs::hmac;

use crate::contracts::config::Repository;

pub(crate) struct SessionFingerprint {
    key: hmac::Key,
    version_attribute: Option<serde_json::Value>,
}

impl SessionFingerprint {
    pub(crate) fn session_key(guard: &str) -> String {
        format!("password_hash_{guard}")
    }

    pub(crate) fn from_config(config: &dyn Repository) -> Self {
        let key = config.get("app.key");
        let key = key
            .as_ref()
            .and_then(serde_json::Value::as_str)
            .unwrap_or("base-key-for-password-hash-mac");
        Self {
            key: hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes()),
            version_attribute: config
                .get("auth.session_credential_version_attribute")
                .filter(|value| !value.is_null()),
        }
    }

    pub(crate) fn for_guard(config: &dyn Repository, guard: &str) -> crate::auth::AuthResult<Self> {
        let mut value = Self::from_config(config);
        if !super::credential_scope::guard_applies(config, guard)? {
            value.version_attribute = None;
        }
        Ok(value)
    }

    pub(crate) fn credential_version(
        &self,
        user: &dyn crate::contracts::auth::Authenticatable,
    ) -> crate::auth::AuthResult<Option<u64>> {
        let Some(attribute) = &self.version_attribute else {
            return Ok(None);
        };
        let attribute = attribute.as_str().filter(|name| !name.is_empty()).ok_or(
            crate::auth::AuthError::Configuration("auth.session_credential_version_attribute"),
        )?;
        Ok(Some(match user.get_auth_attribute(attribute)? {
            Some(crate::database::Value::Integer(value)) if value >= 0 => value as u64,
            Some(crate::database::Value::Unsigned(value)) => value,
            _ => {
                return Err(crate::auth::AuthError::Configuration(
                    "用户持久化凭据版本必须是非负整数",
                ));
            }
        }))
    }

    /// 应用可选择持久化版本列，使无本地密码账户也具有跨节点撤销点。
    pub(crate) fn sign_user(
        &self,
        user: &dyn crate::contracts::auth::Authenticatable,
    ) -> crate::auth::AuthResult<Vec<u8>> {
        let Some(version) = self.credential_version(user)? else {
            return Ok(self.sign(user.get_auth_password()));
        };
        let password = user.get_auth_password().unwrap_or_default();
        let identity = serde_json::to_string(&user.get_auth_identifier())?;
        let name = user.get_auth_identifier_name();
        let value = format!(
            "session-credentials:v1:{}:{}:{}:{}:{}:{}:{}",
            name.len(),
            name,
            identity.len(),
            identity,
            password.len(),
            password,
            version
        );
        Ok(hmac::sign(&self.key, value.as_bytes()).as_ref().to_vec())
    }

    pub(crate) fn sign_for_guard(
        &self,
        user: &dyn crate::contracts::auth::Authenticatable,
        guard: &str,
    ) -> crate::auth::AuthResult<Vec<u8>> {
        let value = self.sign_user(user)?;
        if self.version_attribute.is_none() {
            return Ok(value);
        }
        let mut input = format!("{}:{}:", guard.len(), guard).into_bytes();
        input.extend_from_slice(&value);
        Ok(hmac::sign(&self.key, &input).as_ref().to_vec())
    }

    pub(crate) fn sign(&self, password: Option<&str>) -> Vec<u8> {
        hmac::sign(&self.key, password.unwrap_or_default().as_bytes())
            .as_ref()
            .to_vec()
    }
}
