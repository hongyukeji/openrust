use crate::{
    AuthenticationOptions, PasskeyRecord, Passkeys, RegistrationOptions, internal, invalid,
};
use openrust::{contracts::auth::Authenticatable, database::Value, http::HttpError};
use std::sync::Arc;

pub struct GenerateRegistrationOptions(pub Arc<Passkeys>);
pub struct GenerateVerificationOptions(pub Arc<Passkeys>);
pub struct StorePasskey(pub Arc<Passkeys>);
pub struct VerifyPasskey(pub Arc<Passkeys>);
pub struct DeletePasskey;
impl GenerateRegistrationOptions {
    pub async fn call(&self, user: &dyn Authenticatable) -> Result<RegistrationOptions, HttpError> {
        let account = self.0.account(user)?;
        let records = self
            .0
            .passkeyStore()
            .records("user_id", account.key)
            .await
            .map_err(internal)?;
        let mut excluded = Vec::with_capacity(records.len());
        for record in records {
            excluded.push(record.credential().await.map_err(internal)?);
        }
        self.0
            .webauthn()
            .map_err(internal)?
            .registration_options(
                &account.handle,
                &account.username,
                &account.display_name,
                &excluded,
            )
            .map_err(internal)
    }
}
impl GenerateVerificationOptions {
    pub async fn call(
        &self,
        user: Option<&dyn Authenticatable>,
    ) -> Result<AuthenticationOptions, HttpError> {
        let records = if let Some(user) = user {
            self.0
                .passkeyStore()
                .records("user_id", self.0.account(user)?.key)
                .await
                .map_err(internal)?
        } else {
            vec![]
        };
        let mut allowed = Vec::with_capacity(records.len());
        for record in records {
            allowed.push(record.credential().await.map_err(internal)?);
        }
        self.0
            .webauthn()
            .map_err(internal)?
            .verification_options(&allowed)
            .map_err(internal)
    }
}
impl StorePasskey {
    pub async fn call(
        &self,
        user: &dyn Authenticatable,
        name: &str,
        value: serde_json::Value,
        options: &RegistrationOptions,
    ) -> Result<PasskeyRecord, HttpError> {
        let account = self.0.account(user)?;
        let credential = self
            .0
            .webauthn()
            .map_err(internal)?
            .register(value, options)
            .map_err(|_| invalid("Unable to register passkey. Please try again."))?;
        if credential.user_handle() != account.handle {
            return Err(invalid("Unable to register passkey. Please try again."));
        }
        let id = credential.credential_id();
        let store = self.0.passkeyStore();
        if store
            .exists("credential_id", id.clone().into())
            .await
            .map_err(internal)?
        {
            return Err(invalid("Unable to register this passkey."));
        }
        let result = store
            .create(
                [
                    ("name".into(), name.into()),
                    ("credential_id".into(), id.into()),
                    (
                        "credential".into(),
                        Value::Json(serde_json::to_value(credential).map_err(internal)?),
                    ),
                ]
                .into(),
                account.key,
            )
            .await;
        let passkey = match result {
            Ok(passkey) => passkey,
            Err(error) => {
                if matches!(&error,openrust::database::DatabaseError::Driver{source,..} if source.as_database_error().is_some_and(|error|error.is_unique_violation()))
                {
                    return Err(invalid("Unable to register this passkey."));
                }
                return Err(internal(error));
            }
        };
        crate::events::registered(user, &passkey).await?;
        Ok(passkey)
    }
}
impl VerifyPasskey {
    pub async fn call(
        &self,
        value: serde_json::Value,
        options: &AuthenticationOptions,
        user: Option<&dyn Authenticatable>,
    ) -> Result<PasskeyRecord, HttpError> {
        let id = value
            .get("rawId")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| invalid("Invalid credential format."))?;
        let passkey = self
            .0
            .passkeyStore()
            .records("credential_id", id.into())
            .await
            .map_err(internal)?
            .into_iter()
            .next()
            .ok_or_else(|| {
                invalid("Passkey not recognized. It may have been removed from your account.")
            })?;
        let connection = passkey.connection().await.map_err(internal)?;
        let mut transaction = connection.beginTransaction().await.map_err(internal)?;
        let result = async {
            let expected = user
                .map(|user| self.0.account(user).map(|account| account.key))
                .transpose()?;
            let key = passkey
                .verify(
                    value,
                    options,
                    &self.0.webauthn().map_err(internal)?,
                    expected.as_ref(),
                    &mut transaction,
                )
                .await?;
            let owner = self.0.model()?.find(key).await?.ok_or_else(|| {
                invalid("Passkey not recognized. It may have been removed from your account.")
            })?;
            crate::events::verified(owner.as_ref(), &passkey).await
        }
        .await;
        match result {
            Ok(()) => transaction.commit().await.map_err(internal)?,
            Err(error) => {
                transaction.rollBack(None).await.map_err(internal)?;
                return Err(error);
            }
        }
        Ok(passkey)
    }
}
impl DeletePasskey {
    pub async fn call(
        &self,
        user: &dyn Authenticatable,
        passkey: &PasskeyRecord,
    ) -> Result<(), HttpError> {
        passkey.delete().await.map_err(internal)?;
        crate::events::deleted(user, passkey).await
    }
}
