//! Default machine grant and dedicated internal personal-access issuance.
use super::*;
impl AuthorizationServer {
    pub(super) async fn exchange_client_credentials(
        &self,
        parameters: &OAuthParameters,
        client: ClientRecord,
        connection: openrust::database::Connection,
        mut transaction: openrust::database::Transaction,
    ) -> Result<TokenResponse, OAuthError> {
        let scopes = self
            .standalone_scopes(parameters, &client, "client_credentials")
            .await?;
        let response = self
            .issue_standalone(
                &connection,
                &mut transaction,
                &client,
                None,
                scopes,
                None,
                self.passport.clientCredentialsTokensExpireIn(None),
            )
            .await?;
        transaction.commit().await.map_err(internal)?;
        Ok(response.0)
    }
    pub(super) async fn standalone_scopes(
        &self,
        parameters: &OAuthParameters,
        client: &ClientRecord,
        grant: &str,
    ) -> Result<Vec<String>, OAuthError> {
        let requested = parameters
            .parameter("scope", None)?
            .map(|v| {
                v.split(' ')
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_else(|| self.passport.default_scopes());
        self.finalize_scopes(client, requested, grant).await
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn issue_standalone(
        &self,
        connection: &openrust::database::Connection,
        transaction: &mut openrust::database::Transaction,
        client: &ClientRecord,
        user: Option<AuthIdentifier>,
        scopes: Vec<String>,
        name: Option<String>,
        lifetime: crate::GrantLifetime,
    ) -> Result<(TokenResponse, String), OAuthError> {
        let client_id = client
            .get_key()
            .await
            .ok_or_else(|| OAuthError::invalid_client(None))?;
        let client_id = AuthIdentifier::try_from(&client_id)
            .map_err(internal)?
            .to_string();
        let now = now()?;
        let expires = lifetime.for_application(now)?;
        let access_id = identifier()?;
        let subject = user
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| client_id.clone());
        let access_token = self.sign(&client_id, &subject, &access_id, &scopes, now, expires)?;
        self.passport
            .token_models
            .persist(
                connection,
                transaction,
                Attributes::from([
                    ("id".into(), access_id.clone().into()),
                    (
                        "user_id".into(),
                        user.map(Value::from).unwrap_or(Value::Null),
                    ),
                    (
                        "client_id".into(),
                        client
                            .get_key()
                            .await
                            .ok_or_else(|| OAuthError::invalid_client(None))?,
                    ),
                    ("name".into(), name.map(Value::from).unwrap_or(Value::Null)),
                    ("scopes".into(), Value::Json(serde_json::json!(scopes))),
                    ("revoked".into(), false.into()),
                    ("expires_at".into(), sql_date(expires)?.into()),
                ]),
                None,
            )
            .await
            .map_err(internal)?;
        Ok((
            TokenResponse {
                token_type: "Bearer",
                expires_in: expires - now,
                access_token,
                refresh_token: None,
            },
            access_id,
        ))
    }
    pub(crate) async fn issue_personal(
        &self,
        user: AuthIdentifier,
        name: &str,
        scopes: &[String],
        provider: &str,
    ) -> Result<crate::PersonalAccessTokenResult, OAuthError> {
        if user.to_string().is_empty() || user.to_string() == "0" {
            return Err(OAuthError::invalid_request("user_id", None));
        }
        if provider.is_empty() || provider == "0" {
            return Err(OAuthError::invalid_request("provider", None));
        }
        let client = self
            .clients
            .personalAccessClient(provider)
            .await
            .map_err(internal)?;
        let connection = client.connection().await.map_err(internal)?;
        let mut transaction = connection.begin_transaction().await.map_err(internal)?;
        client
            .refresh_for_update(&mut transaction)
            .await
            .map_err(internal)?;
        self.check_client_for_grant(&client, None, "personal_access")
            .await?;
        let parameters = OAuthParameters::new(serde_json::Map::from_iter([(
            "scope".into(),
            serde_json::json!(scopes.join(" ")),
        )]));
        let scopes = self
            .standalone_scopes(&parameters, &client, "personal_access")
            .await?;
        let (response, id) = self
            .issue_standalone(
                &connection,
                &mut transaction,
                &client,
                Some(user),
                scopes,
                Some(name.into()),
                self.passport.personalAccessTokensExpireIn(None),
            )
            .await?;
        transaction.commit().await.map_err(internal)?;
        Ok(crate::PersonalAccessTokenResult::new(
            id,
            response.access_token,
            response.expires_in,
        ))
    }
}
