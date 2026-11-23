//! Authorization-code grant on the application's ordinary Eloquent connection.
use crate::{
    AuthorizationFailure, AuthorizationRedirect, ClientRecord, ClientRepository, CodeChallenge,
    OAuthError, OAuthParameters, Passport, ValidatedRedirectUri,
};
use base64::{
    Engine as _, alphabet,
    engine::{
        DecodePaddingMode,
        general_purpose::{GeneralPurpose, GeneralPurposeConfig, URL_SAFE_NO_PAD},
    },
};
use openrust::{
    auth::AuthIdentifier,
    database::{Value, eloquent::Attributes},
    encryption::Encrypter,
    hashing::{Hash, HashOptions},
    support::{Date, facades::Facade},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use zeroize::Zeroizing;

#[path = "device_authorization.rs"]
mod device;
#[path = "optional_grants.rs"]
mod optional;
#[path = "standalone_grants.rs"]
mod standalone;
pub use device::DeviceCodeResponse;

/// Validated browser request. Its fields cannot be replaced by a consent POST.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationRequest {
    client_id: String,
    redirect_uri: String,
    requested_redirect_uri: Option<String>,
    state: Option<String>,
    scopes: Vec<String>,
    challenge: Option<String>,
    challenge_method: Option<String>,
    #[serde(default)]
    implicit: bool,
}
impl std::fmt::Debug for AuthorizationRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AuthorizationRequest([REDACTED])")
    }
}
#[allow(non_snake_case)]
impl AuthorizationRequest {
    pub fn grantTypeId(&self) -> &str {
        if self.implicit {
            "implicit"
        } else {
            "authorization_code"
        }
    }
    pub fn clientIdentifier(&self) -> &str {
        &self.client_id
    }
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }
    pub(crate) fn redirect(&self) -> Result<ValidatedRedirectUri, OAuthError> {
        ValidatedRedirectUri::from_authorization_request(
            Some(&self.redirect_uri),
            std::slice::from_ref(&self.redirect_uri),
        )
        .map_err(redirect_error)
    }
    pub(crate) fn error_redirect(
        &self,
        error: &OAuthError,
    ) -> Result<AuthorizationRedirect, OAuthError> {
        Ok(AuthorizationRedirect::protocol_error(
            &self.redirect()?,
            self.state.as_deref(),
            error,
            self.implicit,
        ))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CodePayload {
    context: String,
    auth_code_id: String,
    client_id: String,
    user_id: AuthIdentifier,
    scopes: Vec<String>,
    redirect_uri: Option<String>,
    expire_time: i64,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RefreshPayload {
    pub context: String,
    pub refresh_token_id: String,
    pub access_token_id: String,
    pub client_id: String,
    pub user_id: AuthIdentifier,
    pub scopes: Vec<String>,
    pub expire_time: i64,
}
/// Protocol response; no application JSON envelope and no credentials in Debug.
#[derive(Serialize)]
pub struct TokenResponse {
    token_type: &'static str,
    expires_in: i64,
    access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
}
impl std::fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TokenResponse([REDACTED])")
    }
}
/// One grant service per request; configuration and keys belong to its application.
pub struct AuthorizationServer {
    passport: Arc<Passport>,
    clients: Arc<ClientRepository>,
    encryption: Arc<Encrypter>,
    domain: String,
}
impl Facade for AuthorizationServer {
    type Service = Self;
    fn get_facade_accessor() -> openrust::container::ServiceKey {
        openrust::container::ServiceKey::typed::<Self>()
    }
}
#[allow(non_snake_case)]
impl AuthorizationServer {
    pub(crate) fn new(
        passport: Arc<Passport>,
        clients: Arc<ClientRepository>,
        encryption: Arc<Encrypter>,
        domain: String,
    ) -> Self {
        Self {
            passport,
            clients,
            encryption,
            domain,
        }
    }
    pub async fn validateAuthorizationRequest(
        &self,
        parameters: &OAuthParameters,
    ) -> Result<AuthorizationRequest, AuthorizationFailure> {
        let implicit = parameters.raw_string_matches("response_type", "token")
            && self
                .passport
                .implicitGrantEnabled
                .load(std::sync::atomic::Ordering::Relaxed);
        if !implicit && !parameters.raw_string_matches("response_type", "code") {
            return Err(OAuthError::unsupported_grant_type().into());
        }
        let id = parameters
            .parameter("client_id", None)?
            .ok_or_else(|| OAuthError::invalid_request("client_id", None))?;
        let client = if implicit {
            let client = self
                .clients
                .findActive(id.as_str())
                .await
                .map_err(internal)?
                .ok_or_else(|| OAuthError::invalid_client(None))?;
            self.check_client_for_grant(&client, None, "implicit")
                .await?;
            client
        } else {
            self.client(&id, None).await?
        };
        let redirect = ValidatedRedirectUri::from_client_array(
            parameters.parameter("redirect_uri", None)?.as_deref(),
            &client.redirectUris().await.map_err(internal)?,
        )
        .map_err(redirect_error)?;
        let state = parameters.parameter("state", None)?;
        let scopes = match parameters.parameter("scope", None)? {
            Some(scopes) => scopes
                .split(' ')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
            None => self.passport.default_scopes(),
        };
        for scope in &scopes {
            if !self.passport.has_scope(scope) {
                return Err(AuthorizationFailure::Redirect(
                    AuthorizationRedirect::protocol_error(
                        &redirect,
                        state.as_deref(),
                        &OAuthError::invalid_scope_for(scope),
                        implicit,
                    ),
                ));
            }
        }
        let challenge = if implicit {
            None
        } else {
            CodeChallenge::from_request(
                parameters.parameter("code_challenge", None)?.as_deref(),
                parameters
                    .parameter("code_challenge_method", None)?
                    .as_deref(),
                !client.confidential().await,
            )
            .map_err(OAuthError::from)?
        };
        let (challenge, method) = match challenge.as_ref().map(CodeChallenge::stored_parts) {
            Some((value, method)) => (Some(value.to_owned()), Some(method.to_owned())),
            None => (None, None),
        };
        Ok(AuthorizationRequest {
            client_id: id,
            redirect_uri: redirect.as_str().into(),
            requested_redirect_uri: redirect.requested_uri().map(str::to_owned),
            state,
            scopes,
            challenge,
            challenge_method: method,
            implicit,
        })
    }
    pub(crate) async fn hasGrantedScopes(
        &self,
        request: &AuthorizationRequest,
        user: &AuthIdentifier,
    ) -> Result<bool, OAuthError> {
        let client = self
            .clients
            .findActive(request.client_id.as_str())
            .await
            .map_err(internal)?
            .ok_or_else(|| OAuthError::invalid_client(None))?;
        self.check_client_for_grant(&client, None, request.grantTypeId())
            .await?;
        let connection = client.connection().await.map_err(internal)?;
        let tokens = self
            .passport
            .token_models
            .active_scopes(
                &connection,
                client
                    .get_key()
                    .await
                    .ok_or_else(|| OAuthError::invalid_client(None))?,
                Value::from(user.clone()),
                sql_date(now()?)?,
            )
            .await
            .map_err(internal)?;
        let known = self.passport.scopes();
        let requested = request
            .scopes
            .iter()
            .filter(|id| known.iter().any(|scope| scope.id == **id))
            .collect::<Vec<_>>();
        if requested.is_empty() {
            return Ok(!tokens.is_empty());
        }
        Ok(requested.iter().all(|scope| {
            tokens
                .iter()
                .any(|token| token.contains_strict_string(scope))
        }))
    }

    pub async fn completeAuthorizationRequest(
        &self,
        request: AuthorizationRequest,
        user: AuthIdentifier,
        approved: bool,
    ) -> Result<AuthorizationRedirect, OAuthError> {
        if request.implicit {
            return self.complete_implicit(request, user, approved).await;
        }
        let redirect = request.redirect()?;
        if !approved {
            return Ok(AuthorizationRedirect::denied(
                &redirect,
                request.state.as_deref(),
            ));
        }
        let client = self.client(&request.client_id, None).await?;
        let connection = client.connection().await.map_err(internal)?;
        let mut transaction = connection.begin_transaction().await.map_err(internal)?;
        client
            .refresh_for_update(&mut transaction)
            .await
            .map_err(internal)?;
        self.check_client(&client, None).await?;
        let current_redirect = ValidatedRedirectUri::from_client_array(
            request.requested_redirect_uri.as_deref(),
            &client.redirectUris().await.map_err(internal)?,
        )
        .map_err(redirect_error)?;
        if current_redirect.as_str() != request.redirect_uri {
            return Err(OAuthError::invalid_client(None));
        }
        CodeChallenge::from_request(
            request.challenge.as_deref(),
            request.challenge_method.as_deref(),
            !client.confidential().await,
        )
        .map_err(OAuthError::from)?;
        let now = now()?;
        let payload = CodePayload {
            context: format!("{}:code", self.domain),
            auth_code_id: identifier()?,
            client_id: request.client_id,
            user_id: user,
            scopes: request.scopes,
            redirect_uri: request.requested_redirect_uri,
            expire_time: now + 600,
            code_challenge: request.challenge,
            code_challenge_method: request.challenge_method,
        };
        let encoded = self.encrypt(&payload)?;
        let mut model = self
            .passport
            .grant_models
            .auth()
            .new_record(&connection)
            .map_err(internal)?;
        model
            .fill(Attributes::from([
                ("id".into(), payload.auth_code_id.into()),
                (
                    "client_id".into(),
                    client
                        .get_key()
                        .await
                        .ok_or_else(|| OAuthError::invalid_client(None))?,
                ),
                ("user_id".into(), payload.user_id.into()),
                (
                    "scopes".into(),
                    serde_json::to_string(&payload.scopes)
                        .map_err(internal)?
                        .into(),
                ),
                ("revoked".into(), false.into()),
                ("expires_at".into(), sql_date(payload.expire_time)?.into()),
            ]))
            .map_err(internal)?;
        if !model.save_in(&mut transaction).await.map_err(internal)? {
            return Err(OAuthError::server_error(
                "Authorization code save cancelled",
            ));
        }
        transaction.commit().await.map_err(internal)?;
        Ok(AuthorizationRedirect::approved(
            &redirect,
            &encoded,
            request.state.as_deref(),
        ))
    }
    pub async fn respondToAccessTokenRequest(
        &self,
        parameters: &OAuthParameters,
        authorization: Option<&[u8]>,
    ) -> Result<TokenResponse, OAuthError> {
        let grant = if parameters.raw_string_matches("grant_type", "authorization_code") {
            "authorization_code"
        } else if parameters.raw_string_matches("grant_type", "password")
            && self
                .passport
                .passwordGrantEnabled
                .load(std::sync::atomic::Ordering::Relaxed)
        {
            "password"
        } else if parameters.raw_string_matches("grant_type", "client_credentials") {
            "client_credentials"
        } else if parameters.raw_string_matches("grant_type", "refresh_token") {
            "refresh_token"
        } else if parameters.raw_string_matches("grant_type", device::GRANT)
            && self.device_grant_enabled()
        {
            device::GRANT
        } else {
            return Err(OAuthError::unsupported_grant_type());
        };
        let (basic_id, basic_secret) = basic_credentials(authorization);
        let id = parameters
            .parameter(
                "client_id",
                basic_id
                    .as_ref()
                    .and_then(|id| std::str::from_utf8(id).ok()),
            )?
            .ok_or_else(|| OAuthError::invalid_request("client_id", None))?;
        let client = self
            .clients
            .findActive(id.as_str())
            .await
            .map_err(internal)?
            .ok_or_else(|| OAuthError::invalid_client(authorization))?;
        let connection = client.connection().await.map_err(internal)?;
        let mut transaction = connection.begin_transaction().await.map_err(internal)?;
        client
            .refresh_for_update(&mut transaction)
            .await
            .map_err(internal)?;
        self.check_client_for_grant(&client, authorization, grant)
            .await?;
        if client.confidential().await {
            let secret = parameters
                .secret_parameter("client_secret", basic_secret.as_deref().map(Vec::as_slice))?
                .ok_or_else(|| OAuthError::invalid_request("client_secret", None))?;
            let stored = match client
                .get_attribute_value("secret")
                .await
                .map_err(internal)?
            {
                Some(Value::Text(value)) => Zeroizing::new(value),
                _ => return Err(OAuthError::invalid_client(authorization)),
            };
            if !Hash::check(secret.as_slice(), &stored, &HashOptions::default())
                .await
                .map_err(internal)?
            {
                return Err(OAuthError::invalid_client(authorization));
            }
        }
        if grant == "password" {
            return self
                .exchange_password(parameters, client, connection, transaction)
                .await;
        }
        if grant == "client_credentials" {
            if !client.confidential().await {
                return Err(OAuthError::invalid_client(authorization));
            }
            return self
                .exchange_client_credentials(parameters, client, connection, transaction)
                .await;
        }
        if grant == device::GRANT {
            return self
                .exchange_device(parameters, client, connection, transaction)
                .await;
        }
        if grant == "refresh_token" {
            return self
                .exchange_refresh(parameters, client, connection, transaction)
                .await;
        }
        let encoded = parameters
            .parameter("code", None)?
            .ok_or_else(|| OAuthError::invalid_request("code", None))?;
        let plaintext = Zeroizing::new(self.encryption.decrypt_string(&encoded).map_err(|_| {
            OAuthError::invalid_grant("Cannot validate the provided authorization code")
        })?);
        let payload: CodePayload = serde_json::from_slice(&plaintext).map_err(|_| {
            OAuthError::invalid_grant("Cannot validate the provided authorization code")
        })?;
        let now = now()?;
        if payload.context != format!("{}:code", self.domain) || payload.client_id != id {
            return Err(OAuthError::invalid_grant(
                "Authorization code was issued to another client",
            ));
        }
        if now > payload.expire_time {
            return Err(OAuthError::invalid_grant("Authorization code has expired"));
        }
        crate::validate_token_redirect_uri(
            payload.redirect_uri.as_deref(),
            parameters.parameter("redirect_uri", None)?.as_deref(),
        )
        .map_err(|error| match error {
            crate::RedirectUriError::MissingTokenRedirectUri => {
                OAuthError::invalid_request("redirect_uri", None)
            }
            _ => OAuthError::invalid_request("redirect_uri", Some("Invalid redirect URI")),
        })?;
        let challenge = CodeChallenge::from_request(
            payload.code_challenge.as_deref(),
            payload.code_challenge_method.as_deref(),
            false,
        )
        .map_err(OAuthError::from)?;
        crate::verify_optional_challenge(
            challenge.as_ref(),
            parameters.parameter("code_verifier", None)?.as_deref(),
        )
        .map_err(OAuthError::from)?;
        let mut code = self
            .passport
            .grant_models
            .auth()
            .find(&connection, &payload.auth_code_id)
            .await
            .map_err(internal)?
            .ok_or_else(|| OAuthError::invalid_grant("Authorization code has been revoked"))?;
        code.lock(&mut transaction).await.map_err(internal)?;
        if code.value("revoked").map_err(internal)? != Some(Value::Bool(false))
            || code.value("client_id").map_err(internal)? != client.get_key().await
            || code.value("scopes").map_err(internal)?
                != Some(
                    serde_json::to_string(&payload.scopes)
                        .map_err(internal)?
                        .into(),
                )
            || code
                .value("user_id")
                .map_err(internal)?
                .as_ref()
                .and_then(|v| AuthIdentifier::try_from(v).ok())
                .map(|v| v.to_string())
                != Some(payload.user_id.to_string())
        {
            return Err(OAuthError::invalid_grant(
                "Authorization code has been revoked",
            ));
        }
        let scopes = self
            .finalize_scopes(&client, payload.scopes, "authorization_code")
            .await?;
        let response = self
            .issue_tokens(
                &connection,
                &mut transaction,
                &client,
                TokenIssue {
                    id: &id,
                    user: payload.user_id.clone(),
                    scopes,
                    now,
                },
            )
            .await?;
        code.fill(Attributes::from([("revoked".into(), true.into())]))
            .map_err(internal)?;
        if !code.save_in(&mut transaction).await.map_err(internal)? {
            return Err(OAuthError::server_error(
                "Authorization code consumption cancelled",
            ));
        }
        transaction.commit().await.map_err(internal)?;
        Ok(response)
    }
    async fn exchange_refresh(
        &self,
        parameters: &OAuthParameters,
        client: ClientRecord,
        connection: openrust::database::Connection,
        mut transaction: openrust::database::Transaction,
    ) -> Result<TokenResponse, OAuthError> {
        let encoded = parameters
            .parameter("refresh_token", None)?
            .ok_or_else(|| OAuthError::invalid_request("refresh_token", None))?;
        let plaintext = Zeroizing::new(self.encryption.decrypt_string(&encoded).map_err(|_| {
            OAuthError::invalid_refresh_token(Some("Cannot decrypt the refresh token"))
        })?);
        let payload: RefreshPayload = serde_json::from_slice(&plaintext).map_err(|_| {
            OAuthError::invalid_refresh_token(Some("Cannot decrypt the refresh token"))
        })?;
        let id = AuthIdentifier::try_from(
            &client
                .get_key()
                .await
                .ok_or_else(|| OAuthError::invalid_client(None))?,
        )
        .map_err(internal)?
        .to_string();
        let now = now()?;
        if payload.context != format!("{}:refresh", self.domain) || payload.client_id != id {
            return Err(OAuthError::invalid_refresh_token(Some(
                "Token is not linked to client",
            )));
        }
        if now > payload.expire_time {
            return Err(OAuthError::invalid_refresh_token(Some("Token has expired")));
        }
        let scopes = match parameters.parameter("scope", None)? {
            Some(scope) => scope
                .split(' ')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>(),
            None => payload.scopes.clone(),
        };
        for scope in &scopes {
            if !payload.scopes.contains(scope) || !self.passport.has_scope(scope) {
                return Err(OAuthError::invalid_scope_for(scope));
            }
        }
        if !self
            .passport
            .token_models
            .consume_refresh(
                &connection,
                &mut transaction,
                &payload.refresh_token_id,
                &payload.access_token_id,
            )
            .await
            .map_err(internal)?
        {
            return Err(OAuthError::invalid_refresh_token(Some(
                "Token has been revoked",
            )));
        }
        let scopes = self
            .finalize_scopes(&client, scopes, "refresh_token")
            .await?;
        let response = self
            .issue_tokens(
                &connection,
                &mut transaction,
                &client,
                TokenIssue {
                    id: &id,
                    user: payload.user_id,
                    scopes,
                    now,
                },
            )
            .await?;
        transaction.commit().await.map_err(internal)?;
        Ok(response)
    }
    async fn finalize_scopes(
        &self,
        client: &ClientRecord,
        requested: Vec<String>,
        grant: &str,
    ) -> Result<Vec<String>, OAuthError> {
        let mut scopes = Vec::new();
        for scope in requested {
            if !self.passport.has_scope(&scope) {
                return Err(OAuthError::invalid_scope_for(&scope));
            }
            if (scope != "*"
                || matches!(grant, "password" | "personal_access" | "client_credentials"))
                && client.hasScope(&scope).await.map_err(internal)?
                && !scopes.contains(&scope)
            {
                scopes.push(scope);
            }
        }
        Ok(scopes)
    }
    async fn issue_tokens(
        &self,
        connection: &openrust::database::Connection,
        transaction: &mut openrust::database::Transaction,
        client: &ClientRecord,
        issue: TokenIssue<'_>,
    ) -> Result<TokenResponse, OAuthError> {
        let TokenIssue {
            id,
            user,
            scopes,
            now,
        } = issue;
        let expires = self.passport.tokensExpireIn(None).for_application(now)?;
        let access_id = identifier()?;
        let access_token = self.sign(id, &user.to_string(), &access_id, &scopes, now, expires)?;
        let refresh = if client
            .hasGrantType("refresh_token")
            .await
            .map_err(internal)?
        {
            Some(RefreshPayload {
                context: format!("{}:refresh", self.domain),
                refresh_token_id: identifier()?,
                access_token_id: access_id.clone(),
                client_id: id.to_owned(),
                user_id: user.clone(),
                scopes: scopes.clone(),
                expire_time: self
                    .passport
                    .refreshTokensExpireIn(None)
                    .for_application(now)?,
            })
        } else {
            None
        };
        let refresh_token = refresh
            .as_ref()
            .map(|payload| self.encrypt(payload))
            .transpose()?;
        let refresh_attributes = refresh.map(|r| {
            Attributes::from([
                ("id".into(), r.refresh_token_id.into()),
                ("access_token_id".into(), r.access_token_id.into()),
                ("revoked".into(), false.into()),
                (
                    "expires_at".into(),
                    sql_date(r.expire_time).expect("checked expiry").into(),
                ),
            ])
        });
        self.passport
            .token_models
            .persist(
                connection,
                transaction,
                Attributes::from([
                    ("id".into(), access_id.into()),
                    ("user_id".into(), user.into()),
                    (
                        "client_id".into(),
                        client
                            .get_key()
                            .await
                            .ok_or_else(|| OAuthError::invalid_client(None))?,
                    ),
                    ("scopes".into(), Value::Json(serde_json::json!(scopes))),
                    ("revoked".into(), false.into()),
                    ("expires_at".into(), sql_date(expires)?.into()),
                ]),
                refresh_attributes,
            )
            .await
            .map_err(internal)?;
        Ok(TokenResponse {
            token_type: "Bearer",
            expires_in: expires - now,
            access_token,
            refresh_token,
        })
    }

    async fn client(&self, id: &str, header: Option<&[u8]>) -> Result<ClientRecord, OAuthError> {
        let client = self
            .clients
            .findActive(id)
            .await
            .map_err(internal)?
            .ok_or_else(|| OAuthError::invalid_client(header))?;
        self.check_client(&client, header).await?;
        Ok(client)
    }
    async fn check_client(
        &self,
        client: &ClientRecord,
        header: Option<&[u8]>,
    ) -> Result<(), OAuthError> {
        self.check_client_for_grant(client, header, "authorization_code")
            .await
    }
    async fn check_client_for_grant(
        &self,
        client: &ClientRecord,
        header: Option<&[u8]>,
        grant: &str,
    ) -> Result<(), OAuthError> {
        if client
            .get_attribute_value("revoked")
            .await
            .map_err(internal)?
            == Some(Value::Bool(true))
        {
            return Err(OAuthError::invalid_client(header));
        }
        if !client.hasGrantType(grant).await.map_err(internal)? {
            return Err(OAuthError::unauthorized_client(None));
        }
        Ok(())
    }
    fn encrypt(&self, payload: &impl Serialize) -> Result<String, OAuthError> {
        let plaintext = Zeroizing::new(serde_json::to_vec(payload).map_err(internal)?);
        self.encryption.encrypt_string(&plaintext).map_err(internal)
    }
    fn sign(
        &self,
        client: &str,
        user: &str,
        id: &str,
        scopes: &[String],
        issued: i64,
        expires: i64,
    ) -> Result<String, OAuthError> {
        use openrust::contracts::container::Container as _;
        use rsa::{
            pkcs1::DecodeRsaPrivateKey,
            pkcs8::{DecodePrivateKey, EncodePrivateKey},
        };
        let key = openrust::container::Container::get_instance()
            .make::<crate::PrivateKey>()
            .map_err(internal)?;
        let key = rsa::PrivateKey::from_pkcs8_pem(key.getKeyContents())
            .or_else(|_| rsa::PrivateKey::from_pkcs1_pem(key.getKeyContents()))
            .map_err(internal)?;
        let der = key.to_pkcs8_der().map_err(internal)?;
        let key = aws_lc_rs::signature::RsaKeyPair::from_pkcs8(der.as_bytes())
            .map_err(|_| OAuthError::server_error("Invalid OAuth RSA private key"))?;
        let claims = serde_json::json!({"aud":client,"jti":id,"sub":user,"scopes":scopes,"iat":issued,"nbf":issued,"exp":expires});
        let signing = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"RS256"}"#),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).map_err(internal)?)
        );
        let mut signature = vec![0; key.public_modulus_len()];
        key.sign(
            &aws_lc_rs::signature::RSA_PKCS1_SHA256,
            &aws_lc_rs::rand::SystemRandom::new(),
            signing.as_bytes(),
            &mut signature,
        )
        .map_err(|_| OAuthError::server_error("OAuth signing failed"))?;
        Ok(format!("{signing}.{}", URL_SAFE_NO_PAD.encode(signature)))
    }
}
fn redirect_error(error: crate::RedirectUriError) -> OAuthError {
    match error {
        crate::RedirectUriError::InvalidClient => OAuthError::invalid_client(None),
        _ => OAuthError::server_error("Invalid client redirect registration"),
    }
}
fn now() -> Result<i64, OAuthError> {
    Ok(Date::now(None)
        .map_err(internal)?
        .as_date_time()
        .timestamp()
        .as_nanosecond()
        .div_euclid(1_000_000_000) as i64)
}
fn sql_date(value: i64) -> Result<String, OAuthError> {
    let current = Date::now(None).map_err(internal)?;
    Ok(jiff::Timestamp::from_second(value)
        .map_err(|_| OAuthError::server_error("Invalid OAuth expiry"))?
        .to_zoned(current.as_date_time().time_zone().clone())
        .strftime("%Y-%m-%d %H:%M:%S")
        .to_string())
}

fn identifier() -> Result<String, OAuthError> {
    use rand::RngCore as _;
    let mut bytes = [0; 40];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(internal)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
fn internal(_: impl std::fmt::Display) -> OAuthError {
    OAuthError::server_error("OAuth operation failed")
}

type BasicBytes = Option<zeroize::Zeroizing<Vec<u8>>>;

fn basic_credentials(authorization: Option<&[u8]>) -> (BasicBytes, BasicBytes) {
    let Some(header) = authorization else {
        return (None, None);
    };
    let Some(prefix) = header.get(..6) else {
        return (None, None);
    };
    if !prefix.eq_ignore_ascii_case(b"Basic ") {
        return (None, None);
    }
    let Ok(decoded) = decode_basic_payload(&header[6..]) else {
        return (None, None);
    };
    let decoded = zeroize::Zeroizing::new(decoded);
    let Some(colon) = decoded.iter().position(|byte| *byte == b':') else {
        return (None, None);
    };
    if colon == 0 {
        return (None, None);
    }
    (
        Some(zeroize::Zeroizing::new(decoded[..colon].to_vec())),
        Some(zeroize::Zeroizing::new(decoded[colon + 1..].to_vec())),
    )
}

// PHP8.5.12实际oracle允许非零低尾位与完整省略padding，但拒绝partial padding。
// 分别选成熟库的canonical/no-padding模式，避免Indifferent扩大可接受输入。
fn decode_basic_payload(payload: &[u8]) -> Result<Vec<u8>, base64::DecodeError> {
    const PADDED: GeneralPurpose = GeneralPurpose::new(
        &alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
    );
    const UNPADDED: GeneralPurpose = GeneralPurpose::new(
        &alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_allow_trailing_bits(true)
            .with_decode_padding_mode(DecodePaddingMode::RequireNone),
    );
    // PHP strict decoder只忽略这四种字节；不能复用trim或Unicode空白集合。
    let filtered: Vec<u8> = payload
        .iter()
        .copied()
        .filter(|byte| !matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
        .collect();
    if filtered.contains(&b'=') {
        PADDED.decode(filtered)
    } else {
        UNPADDED.decode(filtered)
    }
}

struct TokenIssue<'a> {
    id: &'a str,
    user: AuthIdentifier,
    scopes: Vec<String>,
    now: i64,
}
