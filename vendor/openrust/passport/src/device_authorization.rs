//! Fixed League device grant, retaining pending-poll writes and atomic issuance.
use super::*;
use crate::grant_store::GrantRecord;
pub(super) const GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

#[derive(Serialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: i64,
    pub interval: u64,
}
impl std::fmt::Debug for DeviceCodeResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeviceCodeResponse([REDACTED])")
    }
}
impl AuthorizationServer {
    pub(super) fn device_grant_enabled(&self) -> bool {
        self.passport
            .device_code_grant_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
            && openrust::support::facades::Url::route("passport.device", &[], false).is_ok()
    }
    pub async fn respond_to_device_authorization_request(
        &self,
        parameters: &OauthParameters,
        authorization: Option<&[u8]>,
        verification_uri: String,
    ) -> Result<DeviceCodeResponse, OauthError> {
        if !self.device_grant_enabled() {
            return Err(OauthError::unsupported_grant_type());
        }
        let (basic_id, _) = basic_credentials(authorization);
        let id = parameters
            .parameter(
                "client_id",
                basic_id.as_ref().and_then(|v| std::str::from_utf8(v).ok()),
            )?
            .ok_or_else(|| OauthError::invalid_request("client_id", None))?;
        let client = self
            .clients
            .find_active(id.as_str())
            .await
            .map_err(internal)?
            .ok_or_else(|| OauthError::invalid_client(authorization))?;
        self.check_client_for_grant(&client, authorization, GRANT)
            .await?;
        let requested = parameters
            .parameter("scope", None)?
            .map(|s| {
                s.split(' ')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_else(|| self.passport.configured_default_scopes());
        let mut scopes = Vec::new();
        for scope in requested {
            if !self.passport.has_scope(&scope) {
                return Err(OauthError::invalid_scope_for(&scope));
            }
            if !scopes.contains(&scope) {
                scopes.push(scope);
            }
        }
        let connection = client.connection().await.map_err(internal)?;
        let now = now()?;
        let base_uri = url::Url::parse(&verification_uri).map_err(internal)?;
        for attempt in 0..10 {
            let device_code = identifier()?;
            let user_code = user_code()?;
            let mut model = self
                .passport
                .grant_models
                .device()
                .new_record(&connection)
                .map_err(internal)?;
            model
                .fill_async(Attributes::from([
                    ("id".into(), device_code.clone().into()),
                    ("user_code".into(), user_code.clone().into()),
                    (
                        "client_id".into(),
                        client
                            .get_key()
                            .await
                            .ok_or_else(|| OauthError::invalid_client(authorization))?,
                    ),
                    ("user_id".into(), Value::Null),
                    ("user_approved_at".into(), Value::Null),
                    ("last_polled_at".into(), Value::Null),
                    (
                        "scopes".into(),
                        Value::Json(serde_json::to_value(&scopes).map_err(internal)?),
                    ),
                    ("revoked".into(), false.into()),
                    ("expires_at".into(), sql_date(now + 600)?.into()),
                ]))
                .await
                .map_err(internal)?;
            match model.save().await {
                Ok(true) => {}
                Ok(false) => return Err(OauthError::server_error("Device code save cancelled")),
                Err(error)
                    if attempt < 9
                        && matches!(&error,openrust::database::DatabaseError::Driver {source,..} if source.as_database_error().is_some_and(|e|e.is_unique_violation())) =>
                {
                    continue;
                }
                Err(error) => return Err(internal(error)),
            }
            let mut complete = base_uri.clone();
            complete
                .query_pairs_mut()
                .append_pair("user_code", &user_code);
            return Ok(DeviceCodeResponse {
                device_code,
                user_code,
                verification_uri: verification_uri.clone(),
                verification_uri_complete: complete.to_string(),
                expires_in: 600,
                interval: 5,
            });
        }
        Err(OauthError::server_error("Device code generation exhausted"))
    }
    pub(crate) async fn device_code_by_user_code(
        &self,
        user_code: &str,
    ) -> Result<Option<Box<dyn GrantRecord>>, OauthError> {
        let connection = self.passport.token_models.parent().map_err(internal)?;
        self.passport
            .grant_models
            .device()
            .pending(&connection, user_code.replace('-', ""), sql_date(now()?)?)
            .await
            .map_err(internal)
    }
    pub async fn complete_device_authorization_request(
        &self,
        device_code: &str,
        user: AuthIdentifier,
        approved: bool,
    ) -> Result<(), OauthError> {
        let connection = self.passport.token_models.parent().map_err(internal)?;
        let mut transaction = connection.begin_transaction().await.map_err(internal)?;
        let mut model = self
            .passport
            .grant_models
            .device()
            .find(&connection, device_code)
            .await
            .map_err(internal)?
            .ok_or_else(|| {
                OauthError::invalid_request("device_code", Some("Device code does not exist"))
            })?;
        model.lock(&mut transaction).await.map_err(internal)?;
        if model.value("revoked").map_err(internal)? != Some(Value::Bool(false))
            || !matches!(model.attribute("user_id"), None | Some(Value::Null))
        {
            return Err(OauthError::invalid_request(
                "device_code",
                Some("Device code does not exist"),
            ));
        }
        if user.to_string().is_empty() {
            return Err(OauthError::invalid_request(
                "user_id",
                Some("User ID is required"),
            ));
        }
        model
            .fill_async(Attributes::from([
                ("user_id".into(), user.into()),
                (
                    "user_approved_at".into(),
                    if approved {
                        sql_date(now()?)?.into()
                    } else {
                        Value::Null
                    },
                ),
            ]))
            .await
            .map_err(internal)?;
        save(model.as_mut(), &mut transaction).await?;
        transaction.commit().await.map_err(internal)
    }
    pub(super) async fn exchange_device(
        &self,
        parameters: &OauthParameters,
        client: ClientRecord,
        connection: openrust::database::Connection,
        mut transaction: openrust::database::Transaction,
    ) -> Result<TokenResponse, OauthError> {
        let code = parameters
            .parameter("device_code", None)?
            .ok_or_else(|| OauthError::invalid_request("device_code", None))?;
        let mut model = self
            .passport
            .grant_models
            .device()
            .find(&connection, code.as_str())
            .await
            .map_err(internal)?
            .ok_or_else(|| OauthError::invalid_grant(""))?;
        model.lock(&mut transaction).await.map_err(internal)?;
        if model.value("revoked").map_err(internal)? != Some(Value::Bool(false)) {
            return Err(OauthError::invalid_grant(""));
        }
        let now = now()?;
        if timestamp(model.as_ref(), "expires_at")?.is_none_or(|expiry| now > expiry) {
            return Err(OauthError::expired_token());
        }
        if model.value("client_id").map_err(internal)? != client.get_key().await {
            return Err(OauthError::invalid_request(
                "device_code",
                Some("Device code was not issued to this client"),
            ));
        }
        let user = model
            .attribute("user_id")
            .filter(|v| !matches!(v, Value::Null))
            .map(AuthIdentifier::try_from)
            .transpose()
            .map_err(internal)?;
        let Some(user) = user else {
            let soon = timestamp(model.as_ref(), "last_polled_at")?
                .is_some_and(|time| time.saturating_add(5) > now);
            model
                .fill_async(Attributes::from([(
                    "last_polled_at".into(),
                    sql_date(now)?.into(),
                )]))
                .await
                .map_err(internal)?;
            save(model.as_mut(), &mut transaction).await?;
            transaction.commit().await.map_err(internal)?;
            return Err(if soon {
                OauthError::slow_down()
            } else {
                OauthError::authorization_pending()
            });
        };
        if matches!(
            model.attribute("user_approved_at"),
            None | Some(Value::Null)
        ) {
            return Err(OauthError::access_denied(None));
        }
        let scopes = match model.value("scopes").map_err(internal)? {
            Some(Value::Json(value)) => serde_json::from_value(value).map_err(internal)?,
            _ => return Err(OauthError::server_error("Invalid device scopes")),
        };
        let scopes = self.finalize_scopes(&client, scopes, GRANT).await?;
        let id = AuthIdentifier::try_from(
            &client
                .get_key()
                .await
                .ok_or_else(|| OauthError::invalid_client(None))?,
        )
        .map_err(internal)?
        .to_string();
        let response = self
            .issue_tokens(
                &connection,
                &mut transaction,
                &client,
                TokenIssue {
                    id: &id,
                    user,
                    scopes,
                    now,
                },
            )
            .await?;
        model
            .fill_async(Attributes::from([("revoked".into(), true.into())]))
            .await
            .map_err(internal)?;
        save(model.as_mut(), &mut transaction).await?;
        transaction.commit().await.map_err(internal)?;
        Ok(response)
    }
}
async fn save(
    model: &mut dyn GrantRecord,
    transaction: &mut openrust::database::Transaction,
) -> Result<(), OauthError> {
    if !model.save_in(transaction).await.map_err(internal)? {
        return Err(OauthError::server_error("Device code save cancelled"));
    }
    Ok(())
}
fn timestamp(model: &dyn GrantRecord, name: &str) -> Result<Option<i64>, OauthError> {
    match model.value(name).map_err(internal)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Text(value)) => value
            .parse::<jiff::Timestamp>()
            .map(|d| Some(d.as_second()))
            .map_err(internal),
        _ => Err(OauthError::server_error("Invalid device timestamp")),
    }
}
fn user_code() -> Result<String, OauthError> {
    use rand::RngCore as _;
    const CHARS: &[u8] = b"BCDFGHJKLMNPQRSTVWXZ";
    let mut rng = rand::rngs::OsRng;
    let mut code = String::with_capacity(8);
    while code.len() < 8 {
        let mut sample = [0];
        rng.try_fill_bytes(&mut sample).map_err(internal)?;
        if sample[0] < 240 {
            code.push(char::from(CHARS[usize::from(sample[0]) % 20]));
        }
    }
    Ok(code)
}
