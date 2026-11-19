//! Opt-in grants, kept behind application-local default-off switches.
use super::*;
impl AuthorizationServer {
    pub(super) async fn exchange_password(
        &self,
        parameters: &OAuthParameters,
        client: ClientRecord,
        connection: openrust::database::Connection,
        mut transaction: openrust::database::Transaction,
    ) -> Result<TokenResponse, OAuthError> {
        let requested: Vec<String> = parameters
            .parameter("scope", None)?
            .map(|value| {
                value
                    .split(' ')
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_else(|| self.passport.default_scopes());
        for scope in &requested {
            if !self.passport.has_scope(scope) {
                return Err(OAuthError::invalid_scope_for(scope));
            }
        }
        let username = parameters
            .parameter("username", None)?
            .ok_or_else(|| OAuthError::invalid_request("username", None))?;
        let password = Zeroizing::new(
            parameters
                .parameter("password", None)?
                .ok_or_else(|| OAuthError::invalid_request("password", None))?,
        );
        let user = crate::bridge::UserRepository::getUserEntityByUserCredentials(
            &username, &password, "password", &client,
        )
        .await?
        .ok_or_else(OAuthError::invalidCredentials)?;
        let scopes = self.finalize_scopes(&client, requested, "password").await?;
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
                    now: now()?,
                },
            )
            .await?;
        transaction.commit().await.map_err(internal)?;
        Ok(response)
    }
    pub(super) async fn complete_implicit(
        &self,
        request: AuthorizationRequest,
        user: AuthIdentifier,
        approved: bool,
    ) -> Result<AuthorizationRedirect, OAuthError> {
        if !self
            .passport
            .implicitGrantEnabled
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return Err(OAuthError::unsupported_grant_type());
        }
        let redirect = request.redirect()?;
        if !approved {
            return Ok(AuthorizationRedirect::denied_fragment(
                &redirect,
                request.state.as_deref(),
            ));
        }
        let client = self
            .clients
            .findActive(request.client_id.as_str())
            .await
            .map_err(internal)?
            .ok_or_else(|| OAuthError::invalid_client(None))?;
        let connection = client.connection().await.map_err(internal)?;
        let mut transaction = connection.beginTransaction().await.map_err(internal)?;
        client
            .refresh_for_update(&mut transaction)
            .await
            .map_err(internal)?;
        self.check_client_for_grant(&client, None, "implicit")
            .await?;
        let current = ValidatedRedirectUri::from_client_array(
            request.requested_redirect_uri.as_deref(),
            &client.redirectUris().await.map_err(internal)?,
        )
        .map_err(redirect_error)?;
        if current.as_str() != request.redirect_uri {
            return Err(OAuthError::invalid_client(None));
        }
        let scopes = self
            .finalize_scopes(&client, request.scopes, "implicit")
            .await?;
        let (token, _) = self
            .issue_standalone(
                &connection,
                &mut transaction,
                &client,
                Some(user),
                scopes,
                None,
                self.passport.tokensExpireIn(None),
            )
            .await?;
        transaction.commit().await.map_err(internal)?;
        Ok(AuthorizationRedirect::implicit(
            &redirect,
            &token.access_token,
            token.expires_in,
            request.state.as_deref(),
        ))
    }
}
