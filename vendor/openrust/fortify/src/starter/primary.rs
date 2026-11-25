//! 额外主因素的持久化收据；权限签发和第二因素完成前重新核对实际凭据。
use super::{
    challenges::{ChallengeStore, Pending},
    passkey_confirmation::CredentialProof,
    socialite::Proof as ProviderProof,
};
use crate::{Fortify, TwoFactorAuthenticatable};
use openrust::{
    auth::AuthResult,
    database::{Transaction, eloquent::Model},
    http::HttpError,
};
use serde_json::Value;

#[derive(Clone)]
pub(super) enum Evidence {
    Passkey(CredentialProof),
    Provider(ProviderProof),
}
impl From<CredentialProof> for Evidence {
    fn from(value: CredentialProof) -> Self {
        Self::Passkey(value)
    }
}
impl From<ProviderProof> for Evidence {
    fn from(value: ProviderProof) -> Self {
        Self::Provider(value)
    }
}
impl Evidence {
    pub fn purpose(&self) -> &'static str {
        match self {
            Self::Passkey(_) => "login",
            // 旧消费者只认识login，必须拒绝它不能核对的供应商证明，不能静默降级。
            Self::Provider(_) => "login:socialite",
        }
    }
    pub fn write(&self, payload: &mut Value) -> AuthResult<()> {
        match self {
            Self::Passkey(proof) => payload["passkey"] = serde_json::to_value(proof)?,
            Self::Provider(proof) => payload["provider"] = serde_json::to_value(proof)?,
        }
        Ok(())
    }
    pub fn read(pending: &Pending) -> AuthResult<Option<Self>> {
        let passkey = pending
            .payload
            .get("passkey")
            .filter(|value| !value.is_null());
        let provider = pending
            .payload
            .get("provider")
            .filter(|value| !value.is_null());
        match (pending.purpose.as_str(), passkey, provider) {
            ("login", Some(value), None) => {
                Ok(Some(Self::Passkey(serde_json::from_value(value.clone())?)))
            }
            ("login", None, None) => Ok(None),
            ("login:socialite", None, Some(value)) => {
                Ok(Some(Self::Provider(serde_json::from_value(value.clone())?)))
            }
            _ => Err(HttpError::Unauthenticated.into()),
        }
    }
    pub async fn current<M: Model + TwoFactorAuthenticatable>(
        &self,
        user: &M,
        fortify: &Fortify,
        transaction: &mut Transaction,
    ) -> AuthResult<bool> {
        match self {
            Self::Passkey(proof) => proof.current(user, fortify, Some(transaction)).await,
            Self::Provider(proof) => proof.current(user, fortify, transaction).await,
        }
    }
}

pub(super) async fn resolve(store: &ChallengeStore, raw: &str) -> AuthResult<Option<Pending>> {
    if let Some(pending) = store.resolve(raw, "login").await? {
        return Ok(Some(pending));
    }
    store.resolve(raw, "login:socialite").await
}
