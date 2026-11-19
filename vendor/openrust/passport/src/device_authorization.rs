//! Fixed League device grant, retaining pending-poll writes and atomic issuance.
use super::*;
use crate::DeviceCode;
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
#[allow(non_snake_case)]
impl AuthorizationServer {
    pub(super) fn device_grant_enabled(&self) -> bool {
        self.passport
            .deviceCodeGrantEnabled
            .load(std::sync::atomic::Ordering::Relaxed)
            && openrust::support::facades::URL::route("passport.device", &[], false).is_ok()
    }
    pub async fn respondToDeviceAuthorizationRequest(
        &self,
        parameters: &OAuthParameters,
        authorization: Option<&[u8]>,
        verification_uri: String,
    ) -> Result<DeviceCodeResponse, OAuthError> {
        if !self.device_grant_enabled() {
            return Err(OAuthError::unsupported_grant_type());
        }
        let (basic_id, _) = basic_credentials(authorization);
        let id = parameters
            .parameter(
                "client_id",
                basic_id.as_ref().and_then(|v| std::str::from_utf8(v).ok()),
            )?
            .ok_or_else(|| OAuthError::invalid_request("client_id", None))?;
        let client = self
            .clients
            .findActive(id.as_str())
            .await
            .map_err(internal)?
            .ok_or_else(|| OAuthError::invalid_client(authorization))?;
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
            .unwrap_or_else(|| self.passport.default_scopes());
        let mut scopes = Vec::new();
        for scope in requested {
            if !self.passport.has_scope(&scope) {
                return Err(OAuthError::invalid_scope_for(&scope));
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
            let mut model =
                DeviceCode::newInstanceOn(&connection, Attributes::new()).map_err(internal)?;
            model
                .forceFillAsync(Attributes::from([
                    ("id".into(), device_code.clone().into()),
                    ("user_code".into(), user_code.clone().into()),
                    (
                        "client_id".into(),
                        client
                            .getKey()
                            .await
                            .ok_or_else(|| OAuthError::invalid_client(authorization))?,
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
                Ok(false) => return Err(OAuthError::server_error("Device code save cancelled")),
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
        Err(OAuthError::server_error("Device code generation exhausted"))
    }
    pub(crate) async fn deviceCodeByUserCode(
        &self,
        user_code: &str,
    ) -> Result<Option<DeviceCode>, OAuthError> {
        let connection = self.passport.token_models.parent().map_err(internal)?;
        let mut query = DeviceCode::queryOn(&connection);
        query
            .r#where("user_code", "=", user_code.replace('-', ""))
            .whereNull("user_id")
            .r#where("revoked", "=", false)
            .r#where("expires_at", ">", sql_date(now()?)?);
        query.first().await.map_err(internal)
    }
    pub async fn completeDeviceAuthorizationRequest(
        &self,
        device_code: &str,
        user: AuthIdentifier,
        approved: bool,
    ) -> Result<(), OAuthError> {
        let connection = self.passport.token_models.parent().map_err(internal)?;
        let mut transaction = connection.beginTransaction().await.map_err(internal)?;
        let mut model = DeviceCode::queryOn(&connection)
            .find(device_code)
            .await
            .map_err(internal)?
            .ok_or_else(|| {
                OAuthError::invalid_request("device_code", Some("Device code does not exist"))
            })?;
        model
            .refreshForUpdate(&mut transaction)
            .await
            .map_err(internal)?;
        if model.getAttributeValue("revoked").map_err(internal)? != Some(Value::Bool(false))
            || !matches!(model.getAttribute("user_id"), None | Some(Value::Null))
        {
            return Err(OAuthError::invalid_request(
                "device_code",
                Some("Device code does not exist"),
            ));
        }
        if user.to_string().is_empty() {
            return Err(OAuthError::invalid_request(
                "user_id",
                Some("User ID is required"),
            ));
        }
        model
            .forceFillAsync(Attributes::from([
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
        save(&mut model, &mut transaction).await?;
        transaction.commit().await.map_err(internal)
    }
    pub(super) async fn exchange_device(
        &self,
        parameters: &OAuthParameters,
        client: ClientRecord,
        connection: openrust::database::Connection,
        mut transaction: openrust::database::Transaction,
    ) -> Result<TokenResponse, OAuthError> {
        let code = parameters
            .parameter("device_code", None)?
            .ok_or_else(|| OAuthError::invalid_request("device_code", None))?;
        let mut model = DeviceCode::queryOn(&connection)
            .find(code.as_str())
            .await
            .map_err(internal)?
            .ok_or_else(|| OAuthError::invalid_grant(""))?;
        model
            .refreshForUpdate(&mut transaction)
            .await
            .map_err(internal)?;
        if model.getAttributeValue("revoked").map_err(internal)? != Some(Value::Bool(false)) {
            return Err(OAuthError::invalid_grant(""));
        }
        let now = now()?;
        if timestamp(&model, "expires_at")?.is_none_or(|expiry| now > expiry) {
            return Err(OAuthError::expired_token());
        }
        if model.getAttributeValue("client_id").map_err(internal)? != client.getKey().await {
            return Err(OAuthError::invalid_request(
                "device_code",
                Some("Device code was not issued to this client"),
            ));
        }
        let user = model
            .getAttribute("user_id")
            .filter(|v| !matches!(v, Value::Null))
            .map(AuthIdentifier::try_from)
            .transpose()
            .map_err(internal)?;
        let Some(user) = user else {
            let soon = timestamp(&model, "last_polled_at")?
                .is_some_and(|time| time.saturating_add(5) > now);
            model
                .forceFillAsync(Attributes::from([(
                    "last_polled_at".into(),
                    sql_date(now)?.into(),
                )]))
                .await
                .map_err(internal)?;
            save(&mut model, &mut transaction).await?;
            transaction.commit().await.map_err(internal)?;
            return Err(if soon {
                OAuthError::slow_down()
            } else {
                OAuthError::authorization_pending()
            });
        };
        if matches!(
            model.getAttribute("user_approved_at"),
            None | Some(Value::Null)
        ) {
            return Err(OAuthError::access_denied(None));
        }
        let scopes = match model.getAttributeValue("scopes").map_err(internal)? {
            Some(Value::Json(value)) => serde_json::from_value(value).map_err(internal)?,
            _ => return Err(OAuthError::server_error("Invalid device scopes")),
        };
        let scopes = self.finalize_scopes(&client, scopes, GRANT).await?;
        let id = AuthIdentifier::try_from(
            &client
                .getKey()
                .await
                .ok_or_else(|| OAuthError::invalid_client(None))?,
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
            .forceFillAsync(Attributes::from([("revoked".into(), true.into())]))
            .await
            .map_err(internal)?;
        save(&mut model, &mut transaction).await?;
        transaction.commit().await.map_err(internal)?;
        Ok(response)
    }
}
async fn save(
    model: &mut DeviceCode,
    transaction: &mut openrust::database::Transaction,
) -> Result<(), OAuthError> {
    if !model
        .saveInTransaction(transaction)
        .await
        .map_err(internal)?
    {
        return Err(OAuthError::server_error("Device code save cancelled"));
    }
    Ok(())
}
fn timestamp(model: &DeviceCode, name: &str) -> Result<Option<i64>, OAuthError> {
    match model.getAttributeValue(name).map_err(internal)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Text(value)) => chrono::DateTime::parse_from_rfc3339(&value)
            .map(|d| Some(d.timestamp()))
            .map_err(internal),
        _ => Err(OAuthError::server_error("Invalid device timestamp")),
    }
}
fn user_code() -> Result<String, OAuthError> {
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
