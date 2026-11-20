use crate::{
    AuthenticationOptions, Passkey, Passkeys, RegistrationOptions, StoredCredential, internal,
    invalid,
};
use openrust::{
    contracts::auth::Authenticatable,
    database::{Value, eloquent::Model},
    http::HttpError,
    support::now,
};
use std::sync::Arc;

pub struct GenerateRegistrationOptions(pub Arc<Passkeys>);
pub struct GenerateVerificationOptions(pub Arc<Passkeys>);
pub struct StorePasskey(pub Arc<Passkeys>);
pub struct VerifyPasskey(pub Arc<Passkeys>);
pub struct DeletePasskey;
async fn records(user_id: Value) -> Result<Vec<Passkey>, HttpError> {
    Ok(query("user_id", user_id)?
        .get()
        .await
        .map_err(internal)?
        .into_iter()
        .collect())
}
impl GenerateRegistrationOptions {
    pub async fn call(&self, user: &dyn Authenticatable) -> Result<RegistrationOptions, HttpError> {
        let account = self.0.account(user)?;
        let excluded = records(account.key)
            .await?
            .iter()
            .map(Passkey::credential)
            .collect::<Result<Vec<_>, _>>()
            .map_err(internal)?;
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
            records(self.0.account(user)?.key).await?
        } else {
            vec![]
        };
        let allowed = records
            .iter()
            .map(Passkey::credential)
            .collect::<Result<Vec<_>, _>>()
            .map_err(internal)?;
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
    ) -> Result<Passkey, HttpError> {
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
        // The unique index also rejects concurrent registrations of one ID.
        if query("credential_id", id.clone().into())?
            .first()
            .await
            .map_err(internal)?
            .is_some()
        {
            return Err(invalid("Unable to register this passkey."));
        }
        let credential = serde_json::to_value(credential).map_err(internal)?;
        let result = async {
            let mut passkey = Passkey::newInstance(
                [
                    ("name".into(), name.into()),
                    ("credential_id".into(), id.into()),
                    ("credential".into(), Value::Json(credential)),
                ]
                .into(),
            )?;
            // Like HasMany::create, assign the server-owned foreign key after fill.
            passkey.forceFill([("user_id".into(), account.key)].into())?;
            passkey.save().await?;
            Ok::<_, openrust::database::DatabaseError>(passkey)
        }
        .await;
        let passkey = match result {
            Ok(p) => p,
            Err(error) => {
                if matches!(&error,openrust::database::DatabaseError::Driver {source,..} if source.as_database_error().is_some_and(|e|e.is_unique_violation()))
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
    ) -> Result<Passkey, HttpError> {
        let id = value
            .get("rawId")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| invalid("Invalid credential format."))?;
        let mut passkey = query("credential_id", id.into())?
            .first()
            .await
            .map_err(internal)?
            .ok_or_else(|| {
                invalid("Passkey not recognized. It may have been removed from your account.")
            })?;
        let connection = passkey.getConnection().map_err(internal)?;
        let mut transaction = connection.beginTransaction().await.map_err(internal)?;
        let result = async {
            passkey
                .refreshForUpdate(&mut transaction)
                .await
                .map_err(internal)?;
            if let Some(user) = user {
                let expected = self.0.account(user)?.key;
                if passkey
                    .getAttribute("user_id")
                    .and_then(crate::contracts::scalar)
                    != crate::contracts::scalar(&expected)
                {
                    return Err(invalid(
                        "Passkey not recognized. It may have been removed from your account.",
                    ));
                }
            }
            let mut credential: StoredCredential = passkey.credential().map_err(internal)?;
            self.0
                .webauthn()
                .map_err(internal)?
                .verify(value, options, &mut credential, user.is_none())
                .map_err(|_| invalid("Unable to verify passkey. Please try again."))?;
            let now = now(None)
                .map_err(internal)?
                .asDateTime()
                .format("%Y-%m-%d %H:%M:%S");
            passkey
                .forceFill(
                    [
                        (
                            "credential".into(),
                            Value::Json(serde_json::to_value(credential).map_err(internal)?),
                        ),
                        ("last_used_at".into(), now.to_string().into()),
                    ]
                    .into(),
                )
                .map_err(internal)?;
            passkey
                .saveInTransaction(&mut transaction)
                .await
                .map_err(internal)?;
            let key = passkey.getAttribute("user_id").cloned().ok_or_else(|| {
                invalid("Passkey not recognized. It may have been removed from your account.")
            })?;
            let owner = self.0.model()?.find(key).await?.ok_or_else(|| {
                invalid("Passkey not recognized. It may have been removed from your account.")
            })?;
            crate::events::verified(owner.as_ref(), &passkey).await?;
            Ok::<_, HttpError>(())
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
        passkey: &mut Passkey,
    ) -> Result<(), HttpError> {
        passkey.delete().await.map_err(internal)?;
        crate::events::deleted(user, passkey).await
    }
}

fn query(
    column: &str,
    value: Value,
) -> Result<openrust::database::eloquent::Builder<'static, Passkey>, HttpError> {
    let mut query = Passkey::query().map_err(internal)?;
    query.r#where(column, "=", value);
    Ok(query)
}
