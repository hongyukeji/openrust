//! Opt-in grants, kept behind application-local default-off switches.
use super::*;
impl AuthorizationServer {
    pub(super) async fn exchange_password(
        &self,
        parameters: &OauthParameters,
        client: ClientRecord,
        connection: openrust::database::Connection,
        mut transaction: openrust::database::Transaction,
    ) -> Result<TokenResponse, OauthError> {
        let requested: Vec<String> = parameters
            .parameter("scope", None)?
            .map(|value| {
                value
                    .split(' ')
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_else(|| self.passport.configured_default_scopes());
        for scope in &requested {
            if !self.passport.has_scope(scope) {
                return Err(OauthError::invalid_scope_for(scope));
            }
        }
        let username = parameters
            .parameter("username", None)?
            .ok_or_else(|| OauthError::invalid_request("username", None))?;
        let password = Zeroizing::new(
            parameters
                .parameter("password", None)?
                .ok_or_else(|| OauthError::invalid_request("password", None))?,
        );
        let user = crate::bridge::UserRepository::get_user_entity_by_user_credentials(
            &username, &password, "password", &client,
        )
        .await?
        .ok_or_else(OauthError::invalid_credentials)?;
        let scopes = self.finalize_scopes(&client, requested, "password").await?;
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
    ) -> Result<AuthorizationRedirect, OauthError> {
        if !self
            .passport
            .implicit_grant_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return Err(OauthError::unsupported_grant_type());
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
            .find_active(request.client_id.as_str())
            .await
            .map_err(internal)?
            .ok_or_else(|| OauthError::invalid_client(None))?;
        let connection = client.connection().await.map_err(internal)?;
        let mut transaction = connection.begin_transaction().await.map_err(internal)?;
        client
            .refresh_for_update(&mut transaction)
            .await
            .map_err(internal)?;
        self.check_client_for_grant(&client, None, "implicit")
            .await?;
        let current = ValidatedRedirectUri::from_client_array(
            request.requested_redirect_uri.as_deref(),
            &client.redirect_uris().await.map_err(internal)?,
        )
        .map_err(redirect_error)?;
        if current.as_str() != request.redirect_uri {
            return Err(OauthError::invalid_client(None));
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
                self.passport.tokens_expire_in(None),
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
