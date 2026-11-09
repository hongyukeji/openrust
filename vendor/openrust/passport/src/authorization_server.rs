//! 固定Passport授权码/刷新grant的编排；HTTP身份与同意由唯一认证适配层提供。

use std::sync::Arc;

use base64::{
    Engine as _, alphabet,
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig},
};
use openrust::{
    encryption::ContextualEncrypter,
    hashing::{HashManager, HashOptions},
};

use crate::{
    AuthorizationFailure, AuthorizationRedirect, BridgeClient, CodeChallenge,
    GrantAuthorityBinding, GrantConfig, GrantStorage, OAuthAccessSigner, OAuthError,
    OAuthParameters, ScopeRepository, TokenResponse, ValidatedRedirectUri,
    grant_records::{AccessRecord, ClientSnapshot, CodeRecord, RefreshRecord, TokenRecords},
    identifier_retry::{IdentifierRetryBudget, replace_token_identifier},
    protocol_token::{CodePayload, GrantTokenCodec, RefreshPayload, random_identifier},
    redirect_uri::validate_token_redirect_uri,
};

/// 已通过client/redirect/scope/PKCE验证的请求；不提供Clone或公开构造器。
///
/// 同意适配层须保护请求的一次消费、身份和CSRF；不得从表单重新构造客户端或scope。
pub struct AuthorizationRequest {
    client: ClientSnapshot,
    redirect: ValidatedRedirectUri,
    state: Option<String>,
    scopes: Vec<String>,
    challenge: Option<CodeChallenge>,
}

impl AuthorizationRequest {
    /// 供同意视图展示的客户端协议实体，不包含secret。
    #[must_use]
    pub fn client(&self) -> &BridgeClient {
        &self.client.entity
    }
    /// 供同意视图展示的原请求scope，不把已登录等同于用户同意。
    #[must_use]
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }
}

/// OAuth授权码/PKCE和刷新编排；不自动发布路由或另建UserProvider。
///
/// 构造的仓储必须安装当前认证模式的必选 `GrantAuthority`。`approve` 中的 `user_id`
/// 只能来自已完成必要验证的Guard，不能使用HTTP请求体中自报的用户标识。
pub struct AuthorizationServer {
    repository: Arc<dyn GrantStorage>,
    hashing: Arc<HashManager>,
    signer: Arc<OAuthAccessSigner>,
    codec: GrantTokenCodec,
    scopes: ScopeRepository,
    config: GrantConfig,
}

impl AuthorizationServer {
    /// 复用所选持久仓储、统一哈希及独立OAuth密钥；不解析scope缓存作为权威。
    pub fn new(
        repository: Arc<dyn GrantStorage>,
        hashing: Arc<HashManager>,
        signer: Arc<OAuthAccessSigner>,
        encryption: Arc<ContextualEncrypter>,
        scopes: ScopeRepository,
        config: GrantConfig,
    ) -> Self {
        Self {
            repository,
            hashing,
            signer,
            codec: GrantTokenCodec::new(encryption),
            scopes,
            config,
        }
    }

    /// 验证授权查询；在可信redirect建立前任何错误都只返回本地JSON。
    pub async fn prepare_authorization(
        &self,
        parameters: &OAuthParameters,
    ) -> Result<AuthorizationRequest, AuthorizationFailure> {
        if !parameters.is_set("response_type") {
            return Err(OAuthError::invalid_request("response_type", None).into());
        }
        // canRespondToAuthorizationRequest先作原始字符串严格比较，不能先trim。
        if !parameters.raw_string_matches("response_type", "code")
            || !parameters.is_set("client_id")
        {
            return Err(OAuthError::unsupported_grant_type().into());
        }
        let identifier = parameters
            .parameter("client_id", None)?
            .ok_or_else(|| OAuthError::invalid_request("client_id", None))?;
        let client = self.client(&identifier, "authorization_code", None).await?;
        let registered = client.entity.get_redirect_uri();
        let requested = parameters.parameter("redirect_uri", None)?;
        // 固定上游先验证显式URI或省略时的数量；只有单项默认的键0读取后置。
        let prevalidated = if requested.is_some() || registered.values().count() != 1 {
            Some(
                ValidatedRedirectUri::from_client_array(requested.as_deref(), registered)
                    .map_err(client_redirect_error)?,
            )
        } else {
            None
        };
        let state = parameters.parameter("state", None)?;
        let default = self.config.default_scopes.join(" ");
        let requested_scopes = parameters.parameter("scope", Some(&default))?;
        let redirect = match prevalidated {
            Some(redirect) => redirect,
            None => ValidatedRedirectUri::from_client_array(None, registered)
                .map_err(client_redirect_error)?,
        };
        let scopes = self
            .validate_scopes(requested_scopes.as_deref())
            .map_err(|error| {
                AuthorizationFailure::Redirect(AuthorizationRedirect::invalid_scope(
                    &redirect,
                    state.as_deref(),
                    &error,
                ))
            })?;
        let requested_challenge = parameters.parameter("code_challenge", None)?;
        let challenge = match requested_challenge.as_deref() {
            Some(challenge) => CodeChallenge::from_request(
                Some(challenge),
                parameters
                    .parameter("code_challenge_method", Some("plain"))?
                    .as_deref(),
                !client.entity.is_confidential(),
            ),
            None => CodeChallenge::from_request(None, None, !client.entity.is_confidential()),
        }
        .map_err(OAuthError::from)?;
        Ok(AuthorizationRequest {
            client,
            redirect,
            state,
            scopes,
            challenge,
        })
    }

    /// 消费已验证请求并在主库当前身份/客户端末检后发码。
    ///
    /// 适配层必须先通过官方Web authToken/CSRF或显式JWT同意事务的一次消费。
    /// 该方法不把first-party、已登录或已持有JWT等同于同意。
    pub async fn approve(
        &self,
        request: AuthorizationRequest,
        user_id: &str,
        authority: GrantAuthorityBinding,
    ) -> Result<AuthorizationRedirect, OAuthError> {
        if user_id.is_empty() {
            return Err(OAuthError::access_denied(None));
        }
        let now = self
            .repository
            .current_time()
            .map_err(|_| OAuthError::server_error("OAuth storage clock unavailable"))?;
        // 固定AuthCodeGrant先保存用户同意的原scope；仅兑换时finalize。
        let scopes = request.scopes.clone();
        let mut record = CodeRecord {
            id: random_identifier().map_err(internal_error)?,
            client_id: request.client.entity.get_identifier().to_owned(),
            user_id: user_id.to_owned(),
            scopes,
            expires_at: self
                .config
                .authorization_code_lifetime
                .expires_at(now)
                .map_err(internal_error)?,
            authority,
        };
        let mut budget = IdentifierRetryBudget::code();
        loop {
            let payload = CodePayload::new(
                &record,
                request.redirect.requested_uri().map(str::to_owned),
                request.challenge.as_ref(),
            );
            // 每次提交前编码最终ID；失败不消费同意或留下不可交付的码。
            let encoded = self.codec.encode_code(&payload).map_err(internal_error)?;
            match self.repository.persist_code(&record, &request.client).await {
                Ok(()) => {
                    return Ok(AuthorizationRedirect::approved(
                        &request.redirect,
                        &encoded,
                        request.state.as_deref(),
                    ));
                }
                Err(error) => {
                    if budget.retry(&error).is_none() {
                        return Err(identifier_terminal_error(
                            &budget,
                            error,
                            crate::GrantOperation::Authorize,
                            None,
                        ));
                    }
                    record.id = random_identifier().map_err(internal_error)?;
                }
            }
        }
    }

    /// 拒绝时只使用原已验证目标和state，不再次接受表单中的redirect。
    #[must_use]
    pub fn deny(&self, request: AuthorizationRequest) -> AuthorizationRedirect {
        let AuthorizationRequest {
            redirect, state, ..
        } = request;
        AuthorizationRedirect::denied(&redirect, state.as_deref())
    }

    /// 处理Token端点的授权码或refresh；成功响应仅在全部原子存储提交后返回。
    /// `authorization` 接收首个HTTP授权头的原始bytes；文本调用方使用 `header.as_bytes()`。
    pub async fn token(
        &self,
        parameters: &OAuthParameters,
        authorization: Option<&[u8]>,
    ) -> Result<TokenResponse, OAuthError> {
        if parameters.raw_string_matches("grant_type", "authorization_code") {
            self.exchange_code(parameters, authorization).await
        } else if parameters.raw_string_matches("grant_type", "refresh_token") {
            self.refresh(parameters, authorization).await
        } else {
            Err(OAuthError::unsupported_grant_type())
        }
    }

    async fn exchange_code(
        &self,
        parameters: &OAuthParameters,
        authorization: Option<&[u8]>,
    ) -> Result<TokenResponse, OAuthError> {
        let client = self
            .authenticate_client(parameters, authorization, "authorization_code")
            .await?;
        let encoded = parameters
            .parameter("code", None)?
            .ok_or_else(|| OAuthError::invalid_request("code", None))?;
        let payload = self.codec.decode_code(&encoded).map_err(|_| {
            OAuthError::invalid_request("code", Some("Issue decrypting the authorization code"))
        })?;
        let now = self
            .repository
            .current_time()
            .map_err(|_| OAuthError::server_error("OAuth storage clock unavailable"))?;
        if now > payload.expire_time {
            return Err(OAuthError::invalid_grant("Authorization code has expired"));
        }
        if self
            .repository
            .is_code_revoked(&payload.auth_code_id)
            .await
            .map_err(|_| OAuthError::server_error("OAuth revocation lookup unavailable"))?
        {
            return Err(OAuthError::invalid_grant(
                "Authorization code has been revoked",
            ));
        }
        if payload.client_id != client.entity.get_identifier() {
            return Err(OAuthError::invalid_request(
                "code",
                Some("Authorization code was not issued to this client"),
            ));
        }
        let redirect = parameters.parameter("redirect_uri", None)?;
        validate_token_redirect_uri(payload.redirect_uri.as_deref(), redirect.as_deref()).map_err(
            |error| match error {
                crate::RedirectUriError::MissingTokenRedirectUri => {
                    OAuthError::invalid_request("redirect_uri", None)
                }
                _ => OAuthError::invalid_request("redirect_uri", Some("Invalid redirect URI")),
            },
        )?;
        self.ensure_scopes(&payload.scopes)?;
        let scopes = self
            .finalize_client_scopes(
                &payload.scopes,
                "authorization_code",
                &client,
                authorization,
            )
            .await?;
        let challenge = payload.challenge().map_err(internal_error)?;
        let verifier = parameters.parameter("code_verifier", None)?;
        crate::verify_optional_challenge(challenge.as_ref(), verifier.as_deref())
            .map_err(OAuthError::from)?;
        let (records, response) = self.prepare_tokens(
            &client,
            &payload.user_id,
            scopes,
            payload.authority().map_err(internal_error)?,
            now,
        )?;
        let authority = payload.authority().map_err(internal_error)?;
        let code = CodeRecord {
            id: payload.auth_code_id,
            client_id: payload.client_id,
            user_id: payload.user_id,
            scopes: payload.scopes,
            expires_at: payload.expire_time,
            authority,
        };
        self.persist_tokens(
            TokenRedemption::Code(&code),
            &client,
            records,
            response,
            authorization,
        )
        .await
    }

    async fn refresh(
        &self,
        parameters: &OAuthParameters,
        authorization: Option<&[u8]>,
    ) -> Result<TokenResponse, OAuthError> {
        let client = self
            .authenticate_client(parameters, authorization, "refresh_token")
            .await?;
        let encoded = parameters
            .parameter("refresh_token", None)?
            .ok_or_else(|| OAuthError::invalid_request("refresh_token", None))?;
        let payload = self.codec.decode_refresh(&encoded).map_err(|_| {
            OAuthError::invalid_refresh_token(Some("Cannot decrypt the refresh token"))
        })?;
        if payload.client_id != client.entity.get_identifier() {
            return Err(OAuthError::invalid_refresh_token(Some(
                "Token is not linked to client",
            )));
        }
        let now = self
            .repository
            .current_time()
            .map_err(|_| OAuthError::server_error("OAuth storage clock unavailable"))?;
        if payload.expire_time < now {
            return Err(OAuthError::invalid_refresh_token(Some("Token has expired")));
        }
        if self
            .repository
            .is_refresh_revoked(&payload.refresh_token_id)
            .await
            .map_err(|_| OAuthError::server_error("OAuth revocation lookup unavailable"))?
        {
            return Err(OAuthError::invalid_refresh_token(Some(
                "Token has been revoked",
            )));
        }
        let default = payload.scopes.join(" ");
        let scopes =
            self.validate_scopes(parameters.parameter("scope", Some(&default))?.as_deref())?;
        if let Some(scope) = scopes.iter().find(|scope| !payload.scopes.contains(scope)) {
            return Err(OAuthError::invalid_scope_for(scope));
        }
        let scopes = self
            .finalize_client_scopes(&scopes, "refresh_token", &client, authorization)
            .await?;
        let (records, response) = self.prepare_tokens(
            &client,
            &payload.user_id,
            scopes,
            payload.authority().map_err(internal_error)?,
            now,
        )?;
        // 旧access的签发/到期信息不在refresh密文中；存储读取当前关联行，不能伪造比较依据。
        let redemption = crate::RefreshRedemption {
            id: payload.refresh_token_id.clone(),
            client_id: payload.client_id.clone(),
            access_token_id: payload.access_token_id.clone(),
            user_id: payload.user_id.clone(),
            scopes: payload.scopes.clone(),
            expires_at: payload.expire_time,
            authority: payload.authority().map_err(internal_error)?,
        };
        self.persist_tokens(
            TokenRedemption::Refresh(&redemption),
            &client,
            records,
            response,
            authorization,
        )
        .await
    }

    async fn finalize_client_scopes(
        &self,
        scopes: &[String],
        grant: &str,
        client: &ClientSnapshot,
        authorization: Option<&[u8]>,
    ) -> Result<Vec<String>, OAuthError> {
        // 对应固定ScopeRepository.findActive的fresh时点；仍复用同一Store/Factory。
        let fresh = self
            .repository
            .client_by_protocol(client.entity.get_identifier())
            .await
            .map_err(|_| OAuthError::server_error("Client scope lookup unavailable"))?;
        let active = fresh.ok_or_else(|| OAuthError::invalid_client(authorization))?;
        if active.revoked().map_err(internal_error)? {
            return Err(OAuthError::invalid_client(authorization));
        }
        // 安全增强：fresh缺失/撤销即终止，防止恢复为原snapshot的ABA跳过模型策略。
        self.scopes
            .finalize_model_scopes(scopes, grant, Some(active.as_ref()))
            .map_err(internal_error)
    }

    async fn client(
        &self,
        identifier: &str,
        grant: &str,
        authorization: Option<&[u8]>,
    ) -> Result<ClientSnapshot, OAuthError> {
        let model = self
            .repository
            .client_by_protocol(identifier)
            .await
            .map_err(|error| storage_error(error, crate::GrantOperation::Redeem, authorization))?
            .ok_or_else(|| OAuthError::invalid_client(authorization))?;
        if model.revoked().map_err(internal_error)? {
            return Err(OAuthError::invalid_client(authorization));
        }
        let snapshot = ClientSnapshot::capture(model.as_ref()).map_err(internal_error)?;
        if !snapshot.entity.supports_grant_type(grant) {
            return Err(OAuthError::unauthorized_client(None));
        }
        Ok(snapshot)
    }

    async fn authenticate_client(
        &self,
        parameters: &OAuthParameters,
        authorization: Option<&[u8]>,
        grant: &str,
    ) -> Result<ClientSnapshot, OAuthError> {
        let (basic_user, basic_secret) = basic_credentials(authorization);
        let identifier = if let Some(identifier) = parameters.parameter("client_id", None)? {
            identifier
        } else {
            let bytes = crate::parameters::normalized_bytes(
                basic_user.as_ref().map(|user| user.as_slice()),
            )
            .ok_or_else(|| OAuthError::invalid_request("client_id", None))?;
            // 当前Factory协议键是&str：默认UUID域拒绝binary，不伪造missing或lossy。
            std::str::from_utf8(bytes)
                .map_err(|_| OAuthError::invalid_client(authorization))?
                .to_owned()
        };
        let secret = parameters.secret_parameter(
            "client_secret",
            basic_secret.as_ref().map(|secret| secret.as_slice()),
        )?;
        drop(basic_user);
        drop(basic_secret);
        let client = self.client(&identifier, grant, authorization).await?;
        if client.entity.is_confidential() {
            let secret = secret
                .filter(|secret| !secret.is_empty())
                .ok_or_else(|| OAuthError::invalid_request("client_secret", None))?;
            if secret.as_slice() == b"0"
                || !self
                    .hashing
                    .check(
                        secret.as_slice(),
                        client.encoded_secret.as_deref(),
                        &HashOptions::default(),
                    )
                    .await
                    .map_err(internal_error)?
            {
                return Err(OAuthError::invalid_client(authorization));
            }
        }
        Ok(client)
    }

    fn validate_scopes(&self, raw: Option<&str>) -> Result<Vec<String>, OAuthError> {
        let values = raw
            .unwrap_or("")
            .split(' ')
            .filter(|scope| !scope.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        self.ensure_scopes(&values)?;
        Ok(values)
    }

    fn ensure_scopes(&self, scopes: &[String]) -> Result<(), OAuthError> {
        if let Some(scope) = scopes.iter().find(|scope| !self.scopes.has_scope(scope)) {
            return Err(OAuthError::invalid_scope_for(scope));
        }
        Ok(())
    }

    fn prepare_tokens(
        &self,
        client: &ClientSnapshot,
        user_id: &str,
        scopes: Vec<String>,
        authority: GrantAuthorityBinding,
        now: i64,
    ) -> Result<(TokenRecords, TokenResponse), OAuthError> {
        let access = AccessRecord {
            id: random_identifier().map_err(internal_error)?,
            client_id: client.entity.get_identifier().to_owned(),
            user_id: user_id.to_owned(),
            scopes,
            issued_at: now,
            expires_at: self
                .config
                .access_token_lifetime
                .expires_at(now)
                .map_err(internal_error)?,
            authority,
        };
        let access_token = self.sign_access(&access)?;
        let refresh = if client.entity.supports_grant_type("refresh_token") {
            Some(RefreshRecord {
                id: random_identifier().map_err(internal_error)?,
                access: access.clone(),
                expires_at: self
                    .config
                    .refresh_token_lifetime
                    .expires_at(now)
                    .map_err(internal_error)?,
            })
        } else {
            None
        };
        let records = TokenRecords { access, refresh };
        let response = self.encode_tokens_with_access(&records, access_token)?;
        Ok((records, response))
    }

    fn sign_access(&self, access: &AccessRecord) -> Result<String, OAuthError> {
        self.signer
            .sign_at(
                &access.client_id,
                &access.id,
                Some(&access.user_id),
                &access.scopes,
                timestamp(access.issued_at)?,
                timestamp(access.expires_at)?,
            )
            .map_err(|_| OAuthError::server_error("Access token signing failed"))
    }

    fn encode_tokens(&self, records: &TokenRecords) -> Result<TokenResponse, OAuthError> {
        let access_token = self.sign_access(&records.access)?;
        self.encode_tokens_with_access(records, access_token)
    }

    fn encode_tokens_with_access(
        &self,
        records: &TokenRecords,
        access_token: String,
    ) -> Result<TokenResponse, OAuthError> {
        let access = &records.access;
        let refresh_token = records
            .refresh
            .as_ref()
            .map(|refresh| {
                self.codec.encode_refresh(&RefreshPayload {
                    client_id: refresh.access.client_id.clone(),
                    refresh_token_id: refresh.id.clone(),
                    access_token_id: refresh.access.id.clone(),
                    scopes: refresh.access.scopes.clone(),
                    user_id: refresh.access.user_id.clone(),
                    expire_time: refresh.expires_at,
                    authority_binding: refresh.access.authority.as_str().to_owned(),
                })
            })
            .transpose()
            .map_err(internal_error)?;
        let expires_in = access
            .expires_at
            .checked_sub(access.issued_at)
            .and_then(|seconds| u64::try_from(seconds).ok())
            .ok_or_else(|| OAuthError::server_error("Invalid token lifetime"))?;
        Ok(TokenResponse::new(access_token, expires_in, refresh_token))
    }

    async fn persist_tokens(
        &self,
        redemption: TokenRedemption<'_>,
        client: &ClientSnapshot,
        mut records: TokenRecords,
        mut response: TokenResponse,
        authorization: Option<&[u8]>,
    ) -> Result<TokenResponse, OAuthError> {
        let mut budget = IdentifierRetryBudget::tokens(records.refresh.is_some());
        loop {
            let result = match redemption {
                TokenRedemption::Code(code) => {
                    self.repository.exchange_code(code, client, &records).await
                }
                TokenRedemption::Refresh(refresh) => {
                    self.repository
                        .rotate_refresh(
                            refresh,
                            client,
                            &records,
                            self.config.revoke_refresh_tokens,
                        )
                        .await
                }
            };
            match result {
                Ok(()) => return self.finish_response(&records, response),
                Err(error) => {
                    let Some(kind) = budget.retry(&error) else {
                        return Err(identifier_terminal_error(
                            &budget,
                            error,
                            redemption.operation(),
                            authorization,
                        ));
                    };
                    replace_token_identifier(&mut records, kind, random_identifier)
                        .map_err(internal_error)?;
                    // 对应新ID的JWT与密文必须在重试提交前准备；不在成功提交后签发。
                    response = self.encode_tokens(&records)?;
                }
            }
        }
    }

    fn finish_response(
        &self,
        records: &TokenRecords,
        response: TokenResponse,
    ) -> Result<TokenResponse, OAuthError> {
        // 提交后的时钟失败或过期只可拒绝交付，不可宣称已提交记录已回滚。
        let now = self
            .repository
            .current_time()
            .map_err(|_| OAuthError::server_error("OAuth storage clock unavailable"))?;
        let remaining = records
            .access
            .expires_at
            .checked_sub(now)
            .and_then(|value| u64::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| OAuthError::server_error("Access token expired before delivery"))?;
        Ok(response.with_expires_in(remaining))
    }
}

#[derive(Clone, Copy)]
enum TokenRedemption<'a> {
    Code(&'a CodeRecord),
    Refresh(&'a crate::RefreshRedemption),
}

impl TokenRedemption<'_> {
    fn operation(self) -> crate::GrantOperation {
        match self {
            Self::Code(_) => crate::GrantOperation::Redeem,
            Self::Refresh(_) => crate::GrantOperation::Refresh,
        }
    }
}

fn timestamp(value: i64) -> Result<u64, OAuthError> {
    u64::try_from(value).map_err(|_| OAuthError::server_error("Invalid server time"))
}

fn internal_error(_error: openrust::Error) -> OAuthError {
    OAuthError::server_error("OAuth persistence or configuration unavailable")
}

fn identifier_terminal_error(
    budget: &IdentifierRetryBudget,
    error: crate::GrantStorageError,
    operation: crate::GrantOperation,
    authorization: Option<&[u8]>,
) -> OAuthError {
    if budget.exhausted(&error) {
        OAuthError::identifier_collision()
    } else {
        storage_error(error, operation, authorization)
    }
}

fn storage_error(
    error: crate::GrantStorageError,
    operation: crate::GrantOperation,
    authorization: Option<&[u8]>,
) -> OAuthError {
    use crate::{GrantOperation, GrantStorageError as E};
    let response = match &error {
        E::ClientChanged => OAuthError::invalid_client(authorization),
        E::CodeRevoked => OAuthError::invalid_grant("Authorization code has been revoked"),
        E::RefreshRevoked => OAuthError::invalid_refresh_token(Some("Token has been revoked")),
        E::Expired if operation == GrantOperation::Refresh => {
            OAuthError::invalid_refresh_token(Some("Token has expired"))
        }
        E::Expired => OAuthError::invalid_grant("Authorization code has expired"),
        E::BindingMismatch => OAuthError::invalid_grant("Grant binding is invalid"),
        E::AuthorityDenied if operation == GrantOperation::Authorize => {
            OAuthError::access_denied(None)
        }
        E::AuthorityDenied => OAuthError::invalid_grant("Authorization delegation is invalid"),
        E::IdentifierCollision(_) | E::Unavailable(_) => {
            OAuthError::server_error("OAuth persistence or configuration unavailable")
        }
    };
    // 错误在响应形成后、返回前析构，保持自定义 source 的副作用顺序。
    drop(error);
    response
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

#[cfg(test)]
#[path = "basic_credentials_tests.rs"]
mod basic_credentials_tests;

// 注册失败只在本地返回，不把无效的请求或默认地址升级为可信错误重定向。
fn client_redirect_error(error: crate::RedirectUriError) -> OAuthError {
    match error {
        crate::RedirectUriError::InvalidClient => OAuthError::invalid_client(None),
        _ => OAuthError::server_error("Invalid client redirect registration"),
    }
}

#[cfg(test)]
#[path = "storage_error_tests.rs"]
mod storage_error_tests;

#[cfg(test)]
#[path = "identifier_engine_tests.rs"]
mod identifier_engine_tests;
