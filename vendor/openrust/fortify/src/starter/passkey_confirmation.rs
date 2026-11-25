//! 近期确认绑定实际仍启用的凭据，不绑定不断变化的签名计数器。
use super::client;
use crate::passkeys::{PasskeyRecord, Passkeys};
use crate::{Features, Fortify};
use openrust::{
    auth::{AuthError, AuthIdentifier, AuthResult},
    contracts::auth::Authenticatable,
    database::Transaction,
    support::facades::App,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct CredentialProof {
    model: String,
    key: AuthIdentifier,
    fingerprint: String,
}
impl CredentialProof {
    pub async fn from_record(service: &Passkeys, record: &PasskeyRecord) -> AuthResult<Self> {
        Ok(Self {
            model: service.passkey_model().into(),
            key: AuthIdentifier::try_from(
                &record.get_key().await.ok_or(AuthError::InvalidIdentity)?,
            )?,
            fingerprint: record
                .credential()
                .await
                .map_err(|source| AuthError::Backend(Box::new(source)))?
                .identity_fingerprint()
                .map_err(|source| AuthError::Backend(Box::new(source)))?,
        })
    }
    pub async fn current(
        &self,
        user: &dyn Authenticatable,
        fortify: &Fortify,
        transaction: Option<&mut Transaction>,
    ) -> AuthResult<bool> {
        if !Features::new(fortify.config.clone()).enabled(Features::passkeys())? {
            return Ok(false);
        }
        let service = App::make::<Passkeys>()
            .await
            .map_err(|source| AuthError::Backend(Box::new(source)))?;
        if service.passkey_model() != self.model {
            return Ok(false);
        }
        let record = if let Some(transaction) = transaction {
            service
                .find_owned_in_transaction(user, transaction, self.key.clone().into())
                .await?
        } else {
            let mut found = None;
            for record in service.for_user(user).await? {
                let key = record.get_key().await.ok_or(AuthError::InvalidIdentity)?;
                let key = AuthIdentifier::try_from(&key)?;
                if serde_json::to_value(key)? == serde_json::to_value(&self.key)? {
                    found = Some(record);
                    break;
                }
            }
            found
        };
        let Some(record) = record else {
            return Ok(false);
        };
        let fingerprint = record
            .credential()
            .await
            .map_err(|source| AuthError::Backend(Box::new(source)))?
            .identity_fingerprint()
            .map_err(|source| AuthError::Backend(Box::new(source)))?;
        Ok(client::same_binding(&fingerprint, &self.fingerprint))
    }
}
#[derive(Debug, thiserror::Error)]
#[error("请重新确认身份")]
pub(super) struct ConfirmationRequired;
