use crate::{PasskeyRecord, PasskeyResult};
use openrust::{
    contracts::auth::Authenticatable,
    database::{DatabaseResult, Value, eloquent::Model},
};

/// Explicit compiled counterpart of PasskeyUser plus PasskeyAuthenticatable defaults.
pub trait PasskeyUser: Authenticatable + Model {
    fn passkeys(&self) -> crate::PasskeyRelation<Self> {
        crate::PasskeyRelation::new()
    }
    fn has_passkeys_enabled(&self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        async move { self.passkeys().exists(self).await }
    }
    fn get_passkey_user_handle(&self, secret: &str) -> PasskeyResult<String> {
        let key = self.get_key().ok_or(crate::PasskeyError::Format)?;
        let text = scalar(key).ok_or(crate::PasskeyError::Format)?;
        let key = aws_lc_rs::hmac::Key::new(aws_lc_rs::hmac::HMAC_SHA256, secret.as_bytes());
        let digest = aws_lc_rs::hmac::sign(&key, format!("{}|{text}", self.get_table()).as_bytes());
        Ok(digest.as_ref().iter().map(|b| format!("{b:02x}")).collect())
    }
    fn get_passkey_display_name(&self) -> PasskeyResult<String> {
        for name in ["name", "email"] {
            if let Some(value) = self.get_attribute(name).filter(|v| **v != Value::Null) {
                return scalar(value).ok_or(crate::PasskeyError::Format);
            }
        }
        Ok(self.get_auth_identifier().to_string())
    }
    fn get_passkey_username(&self) -> PasskeyResult<String> {
        if let Some(value) = self.get_attribute("email").filter(|v| **v != Value::Null) {
            return scalar(value).ok_or(crate::PasskeyError::Format);
        }
        Ok(self.get_auth_identifier().to_string())
    }
}
pub(crate) fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::Text(v) => Some(v.clone()),
        Value::Integer(v) => Some(v.to_string()),
        Value::Unsigned(v) => Some(v.to_string()),
        _ => None,
    }
}

pub trait Response: Send + Sync {
    fn to_response<'a>(
        &'a self,
        request: &'a mut openrust::http::Request,
        passkey: Option<&'a PasskeyRecord>,
    ) -> openrust::http::HttpFuture<'a, Result<openrust::http::Response, openrust::http::HttpError>>;
}
macro_rules! responses {($($name:ident),+)=>{$(pub trait $name:Response {})+};}
responses!(
    PasskeyLoginResponse,
    PasskeyRegistrationResponse,
    PasskeyConfirmationResponse,
    PasskeyDeletedResponse
);

/*
 * HTTP动作的可替换边界；默认复用原生Action，应用可在容器中装配安全策略。
 * Options完整状态只在服务器流转，不接受客户端传回的状态对象。
 */
pub trait PasskeyActions: Send + Sync {
    fn service(&self) -> std::sync::Arc<crate::Passkeys>;
    fn before_login<'a>(
        &'a self,
        _request: &'a mut openrust::http::Request,
        _user: &'a dyn Authenticatable,
        _record: &'a PasskeyRecord,
    ) -> openrust::http::HttpFuture<
        'a,
        Result<Option<openrust::http::Response>, openrust::http::HttpError>,
    > {
        Box::pin(async { Ok(None) })
    }
    fn confirmed<'a>(
        &'a self,
        _request: &'a mut openrust::http::Request,
        _user: &'a dyn Authenticatable,
    ) -> openrust::http::HttpFuture<'a, Result<(), openrust::http::HttpError>> {
        Box::pin(async { Ok(()) })
    }
    fn registration_options<'a>(
        &'a self,
        _request: &'a mut openrust::http::Request,
        user: &'a dyn Authenticatable,
    ) -> openrust::http::HttpFuture<'a, Result<crate::RegistrationOptions, openrust::http::HttpError>>
    {
        Box::pin(async move {
            crate::actions::GenerateRegistrationOptions(self.service())
                .call(user)
                .await
        })
    }
    fn verification_options<'a>(
        &'a self,
        _request: &'a mut openrust::http::Request,
        user: Option<&'a dyn Authenticatable>,
    ) -> openrust::http::HttpFuture<
        'a,
        Result<crate::AuthenticationOptions, openrust::http::HttpError>,
    > {
        Box::pin(async move {
            crate::actions::GenerateVerificationOptions(self.service())
                .call(user)
                .await
        })
    }
    fn store<'a>(
        &'a self,
        _request: &'a mut openrust::http::Request,
        user: &'a dyn Authenticatable,
        name: &'a str,
        value: serde_json::Value,
        options: &'a crate::RegistrationOptions,
    ) -> openrust::http::HttpFuture<'a, Result<PasskeyRecord, openrust::http::HttpError>> {
        Box::pin(async move {
            crate::actions::StorePasskey(self.service())
                .call(user, name, value, options)
                .await
        })
    }
    fn verify<'a>(
        &'a self,
        _request: &'a mut openrust::http::Request,
        value: serde_json::Value,
        options: &'a crate::AuthenticationOptions,
        user: Option<&'a dyn Authenticatable>,
    ) -> openrust::http::HttpFuture<'a, Result<PasskeyRecord, openrust::http::HttpError>> {
        Box::pin(async move {
            crate::actions::VerifyPasskey(self.service())
                .call(value, options, user)
                .await
        })
    }
    fn delete<'a>(
        &'a self,
        _request: &'a mut openrust::http::Request,
        user: &'a dyn Authenticatable,
        record: &'a PasskeyRecord,
    ) -> openrust::http::HttpFuture<'a, Result<(), openrust::http::HttpError>> {
        Box::pin(async move { crate::actions::DeletePasskey.call(user, record).await })
    }
}
