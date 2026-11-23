#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use crate::{PasskeyRecord, PasskeyResult};
use openrust::{
    contracts::auth::Authenticatable,
    database::{DatabaseResult, Value, eloquent::Model},
};

/// Explicit compiled counterpart of PasskeyUser plus PasskeyAuthenticatable defaults.
#[allow(non_snake_case, reason = "Matches Passkeys model contract")]
pub trait PasskeyUser: Authenticatable + Model {
    fn passkeys(&self) -> crate::PasskeyRelation<Self> {
        crate::PasskeyRelation::new()
    }
    fn hasPasskeysEnabled(&self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        async move { self.passkeys().exists(self).await }
    }
    fn getPasskeyUserHandle(&self, secret: &str) -> PasskeyResult<String> {
        let key = self.getKey().ok_or(crate::PasskeyError::Format)?;
        let text = scalar(key).ok_or(crate::PasskeyError::Format)?;
        let key = aws_lc_rs::hmac::Key::new(aws_lc_rs::hmac::HMAC_SHA256, secret.as_bytes());
        let digest = aws_lc_rs::hmac::sign(&key, format!("{}|{text}", self.getTable()).as_bytes());
        Ok(digest.as_ref().iter().map(|b| format!("{b:02x}")).collect())
    }
    fn getPasskeyDisplayName(&self) -> PasskeyResult<String> {
        for name in ["name", "email"] {
            if let Some(value) = self.getAttribute(name).filter(|v| **v != Value::Null) {
                return scalar(value).ok_or(crate::PasskeyError::Format);
            }
        }
        Ok(self.get_auth_identifier().to_string())
    }
    fn getPasskeyUsername(&self) -> PasskeyResult<String> {
        if let Some(value) = self.getAttribute("email").filter(|v| **v != Value::Null) {
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

#[allow(non_snake_case, reason = "Passkeys response contracts")]
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
