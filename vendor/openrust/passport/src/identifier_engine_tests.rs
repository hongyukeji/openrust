//! 受控原子存储只模拟明确回滚的碰撞；不冒充真实数据库回滚或Model生命周期。

use super::*;
use crate::{
    ClientFactory, ClientModelFactory, ClientRecord, GrantIdentifierKind as K,
    GrantStorageError as E, GrantStorageResult, OAuthAccessVerifier, RefreshRedemption,
    test_keys::{PRIVATE, PUBLIC},
};
use openrust::{encryption::EncryptionConfig, support::BoxFuture};
use serde_json::json;
use std::collections::VecDeque;
use std::sync::Mutex;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const NOW: i64 = 1_700_000_000;

#[derive(Default)]
struct AttemptState {
    errors: VecDeque<E>,
    tokens: Vec<TokenRecords>,
    codes: Vec<CodeRecord>,
    tokens_committed: bool,
}
struct ControlledStore(Mutex<AttemptState>);
impl ControlledStore {
    fn new(errors: Vec<E>) -> Self {
        Self(Mutex::new(AttemptState {
            errors: errors.into(),
            ..Default::default()
        }))
    }
    fn tokens(&self, records: &TokenRecords) -> GrantStorageResult<()> {
        let mut state = self.0.lock().map_err(|_| {
            E::Unavailable(openrust::Error::custom("fixture_lock_failed", "测试锁失败"))
        })?;
        state.tokens.push(records.clone());
        if state.tokens_committed {
            return Err(E::CodeRevoked);
        }
        if let Some(error) = state.errors.pop_front() {
            return Err(error);
        }
        state.tokens_committed = true;
        Ok(())
    }
}
impl GrantStorage for ControlledStore {
    fn current_time(&self) -> GrantStorageResult<i64> {
        Ok(NOW)
    }
    fn client_by_protocol<'a>(
        &'a self,
        _: &'a str,
    ) -> BoxFuture<'a, GrantStorageResult<Option<ClientRecord>>> {
        Box::pin(async { Ok(None) })
    }
    fn persist_code<'a>(
        &'a self,
        code: &'a CodeRecord,
        _: &'a ClientSnapshot,
    ) -> BoxFuture<'a, GrantStorageResult<()>> {
        Box::pin(async move {
            let mut state = self.0.lock().map_err(|_| {
                E::Unavailable(openrust::Error::custom("fixture_lock_failed", "测试锁失败"))
            })?;
            state.codes.push(code.clone());
            state.errors.pop_front().map_or(Ok(()), Err)
        })
    }
    fn exchange_code<'a>(
        &'a self,
        _: &'a CodeRecord,
        _: &'a ClientSnapshot,
        tokens: &'a TokenRecords,
    ) -> BoxFuture<'a, GrantStorageResult<()>> {
        Box::pin(async move { self.tokens(tokens) })
    }
    fn rotate_refresh<'a>(
        &'a self,
        _: &'a RefreshRedemption,
        _: &'a ClientSnapshot,
        tokens: &'a TokenRecords,
        _: bool,
    ) -> BoxFuture<'a, GrantStorageResult<()>> {
        Box::pin(async move { self.tokens(tokens) })
    }
    fn is_code_revoked<'a>(
        &'a self,
        _id: &'a str,
    ) -> openrust::support::BoxFuture<'a, crate::GrantStorageResult<bool>> {
        Box::pin(async {
            Err(E::Unavailable(openrust::Error::custom(
                "fixture_revocation_read_unsupported",
                "夹具不提供已提交撤销读取",
            )))
        })
    }
    fn is_refresh_revoked<'a>(
        &'a self,
        _id: &'a str,
    ) -> openrust::support::BoxFuture<'a, crate::GrantStorageResult<bool>> {
        Box::pin(async {
            Err(E::Unavailable(openrust::Error::custom(
                "fixture_revocation_read_unsupported",
                "夹具不提供已提交撤销读取",
            )))
        })
    }
    fn is_access_revoked<'a>(&'a self, _: &'a str) -> BoxFuture<'a, GrantStorageResult<bool>> {
        Box::pin(async { Ok(true) })
    }
    fn revoke_access<'a>(&'a self, _: &'a str) -> BoxFuture<'a, GrantStorageResult<()>> {
        Box::pin(async { Ok(()) })
    }
}

fn fixture(
    errors: Vec<E>,
) -> TestResult<(AuthorizationServer, Arc<ControlledStore>, ClientSnapshot)> {
    let factory = ClientFactory::default();
    let value = json!({"id":"client", "name":"fixture", "secret":null,
        "redirect_uris":["https://rp.test/cb"], "grant_types":["authorization_code","refresh_token"], "revoked":false});
    let attributes = value.as_object().ok_or("模型非对象")?.clone();
    let model = factory.new_from_builder(attributes)?;
    let client = ClientSnapshot::capture(model.as_ref())?;
    let store = Arc::new(ControlledStore::new(errors));
    let encryption = Arc::new(ContextualEncrypter::new(EncryptionConfig::new(
        "fixture",
        [(
            "fixture".to_owned(),
            base64::engine::general_purpose::STANDARD.encode([41; 32]),
        )]
        .into(),
    ))?);
    let server = AuthorizationServer::new(
        store.clone(),
        Arc::new(HashManager::new(
            openrust::hashing::HashingConfig::default(),
        )?),
        Arc::new(OAuthAccessSigner::from_pem(PRIVATE)?),
        encryption,
        ScopeRepository::default(),
        GrantConfig::default(),
    );
    Ok((server, store, client))
}

fn binding() -> openrust::Result<GrantAuthorityBinding> {
    GrantAuthorityBinding::new("verified-synthetic-fixture".to_owned())
}
fn old_code() -> openrust::Result<CodeRecord> {
    Ok(CodeRecord {
        id: "old-code".to_owned(),
        client_id: "client".to_owned(),
        user_id: "00042".to_owned(),
        scopes: vec!["read".to_owned()],
        expires_at: NOW + 600,
        authority: binding()?,
    })
}

fn assert_stable(previous: &TokenRecords, current: &TokenRecords, kind: K) -> TestResult {
    let a = &previous.access;
    let b = &current.access;
    assert_eq!(
        (
            &a.client_id,
            &a.user_id,
            &a.scopes,
            a.issued_at,
            a.expires_at,
            a.authority.as_str()
        ),
        (
            &b.client_id,
            &b.user_id,
            &b.scopes,
            b.issued_at,
            b.expires_at,
            b.authority.as_str()
        )
    );
    let old = previous.refresh.as_ref().ok_or("旧refresh缺失")?;
    let new = current.refresh.as_ref().ok_or("新refresh缺失")?;
    assert_eq!(old.expires_at, new.expires_at);
    assert_eq!(new.access.id, b.id);
    assert_eq!(
        (
            &new.access.client_id,
            &new.access.user_id,
            &new.access.scopes,
            new.access.issued_at,
            new.access.expires_at,
            new.access.authority.as_str()
        ),
        (
            &b.client_id,
            &b.user_id,
            &b.scopes,
            b.issued_at,
            b.expires_at,
            b.authority.as_str()
        )
    );
    if kind == K::Access {
        assert_eq!(old.id, new.id);
    }
    if kind == K::Refresh {
        assert_eq!(a.id, b.id);
    }
    Ok(())
}

#[tokio::test]
async fn mixed_collision_response_matches_last_successful_ids() -> TestResult {
    let errors = (1..10)
        .flat_map(|_| {
            [
                E::IdentifierCollision(K::Access),
                E::IdentifierCollision(K::Refresh),
            ]
        })
        .collect();
    let (server, store, client) = fixture(errors)?;
    let (records, response) =
        server.prepare_tokens(&client, "00042", vec!["read".to_owned()], binding()?, NOW)?;
    let code = old_code()?;
    let response = Box::pin(server.persist_tokens(
        TokenRedemption::Code(&code),
        &client,
        records,
        response,
        None,
    ))
    .await?;
    let state = store.0.lock().map_err(|_| "测试锁失败")?;
    assert_eq!(state.tokens.len(), 19);
    for (index, pair) in state.tokens.windows(2).enumerate() {
        assert_stable(
            &pair[0],
            &pair[1],
            if index % 2 == 0 {
                K::Access
            } else {
                K::Refresh
            },
        )?;
    }
    let final_records = state.tokens.last().ok_or("缺少成功记录")?;
    let claims = OAuthAccessVerifier::from_pem(PUBLIC)?.verify_at(
        response.access_token(),
        "client",
        u64::try_from(NOW)?,
    )?;
    assert_eq!(claims.jti, final_records.access.id);
    assert_eq!(claims.sub, "00042");
    assert_eq!(claims.scopes, ["read"]);
    let payload = server
        .codec
        .decode_refresh(response.refresh_token().ok_or("缺少refresh响应")?)?;
    assert_eq!(payload.access_token_id, final_records.access.id);
    assert_eq!(
        payload.refresh_token_id,
        final_records.refresh.as_ref().ok_or("缺少refresh记录")?.id
    );
    Ok(())
}

#[tokio::test]
async fn token_tenth_attempt_success_exhaustion_and_terminal_errors() -> TestResult {
    for kind in [K::Access, K::Refresh] {
        for (collisions, expected_success, attempts) in [(9, true, 10), (10, false, 10)] {
            let errors = (0..collisions)
                .map(|_| E::IdentifierCollision(kind))
                .collect();
            let (server, store, client) = fixture(errors)?;
            let (records, response) =
                server.prepare_tokens(&client, "00042", vec![], binding()?, NOW)?;
            let old = RefreshRedemption {
                id: "old-refresh".to_owned(),
                client_id: "client".to_owned(),
                access_token_id: "old-access".to_owned(),
                user_id: "00042".to_owned(),
                scopes: vec![],
                expires_at: NOW + 900,
                authority: binding()?,
            };
            let result = Box::pin(server.persist_tokens(
                TokenRedemption::Refresh(&old),
                &client,
                records,
                response,
                None,
            ))
            .await;
            assert_eq!(result.is_ok(), expected_success);
            if let Err(error) = &result {
                assert_eq!(
                    serde_json::to_value(error)?["error"],
                    "access_token_duplicate"
                );
            }
            assert_eq!(
                store.0.lock().map_err(|_| "测试锁失败")?.tokens.len(),
                attempts
            );
        }
    }
    for error in [
        E::IdentifierCollision(K::Code),
        E::Expired,
        E::AuthorityDenied,
        E::CodeRevoked,
        E::RefreshRevoked,
        E::ClientChanged,
        E::BindingMismatch,
        E::Unavailable(openrust::Error::custom("commit_unknown", "未知提交结果")),
    ] {
        let (server, store, client) = fixture(vec![error])?;
        let (records, response) =
            server.prepare_tokens(&client, "00042", vec![], binding()?, NOW)?;
        assert!(
            Box::pin(server.persist_tokens(
                TokenRedemption::Code(&old_code()?),
                &client,
                records,
                response,
                None
            ))
            .await
            .is_err()
        );
        assert_eq!(store.0.lock().map_err(|_| "测试锁失败")?.tokens.len(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn code_tenth_attempt_returns_last_encoded_record_and_exhaustion_fails() -> TestResult {
    for (collisions, success) in [(9, true), (10, false)] {
        let (server, store, client) = fixture(
            (0..collisions)
                .map(|_| E::IdentifierCollision(K::Code))
                .collect(),
        )?;
        let request = AuthorizationRequest {
            client,
            redirect: ValidatedRedirectUri::from_authorization_request(
                Some("https://rp.test/cb"),
                &["https://rp.test/cb".to_owned()],
            )?,
            state: None,
            scopes: vec!["read".to_owned()],
            challenge: None,
        };
        let result = Box::pin(server.approve(request, "00042", binding()?)).await;
        assert_eq!(result.is_ok(), success);
        if let Err(error) = &result {
            assert_eq!(
                serde_json::to_value(error)?["error"],
                "access_token_duplicate"
            );
        }
        let state = store.0.lock().map_err(|_| "测试锁失败")?;
        assert_eq!(state.codes.len(), 10);
        for pair in state.codes.windows(2) {
            assert_eq!(
                (
                    &pair[0].user_id,
                    &pair[0].client_id,
                    &pair[0].scopes,
                    pair[0].expires_at,
                    pair[0].authority.as_str()
                ),
                (
                    &pair[1].user_id,
                    &pair[1].client_id,
                    &pair[1].scopes,
                    pair[1].expires_at,
                    pair[1].authority.as_str()
                )
            );
        }
        if let Ok(redirect) = result {
            let url = url::Url::parse(redirect.location())?;
            let encoded = url
                .query_pairs()
                .find(|(k, _)| k == "code")
                .ok_or("缺少code")?
                .1
                .into_owned();
            let payload = server.codec.decode_code(&encoded)?;
            assert_eq!(
                payload.auth_code_id,
                state.codes.last().ok_or("缺少码记录")?.id
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn retry_encoding_failure_never_enters_next_store_attempt() -> TestResult {
    let (server, store, client) = fixture(vec![E::IdentifierCollision(K::Access)])?;
    let (mut records, response) =
        server.prepare_tokens(&client, "00042", vec![], binding()?, NOW)?;
    // 精确触发成熟signer前的时间转换错误，证明重试payload准备失败不会提交第二批。
    records.access.issued_at = -1;
    let code = old_code()?;
    let result = Box::pin(server.persist_tokens(
        TokenRedemption::Code(&code),
        &client,
        records,
        response,
        None,
    ))
    .await;
    assert!(result.is_err());
    assert_eq!(store.0.lock().map_err(|_| "测试锁失败")?.tokens.len(), 1);
    Ok(())
}

#[tokio::test]
async fn expiry_after_collision_stops_without_recomputing_lifetimes() -> TestResult {
    let (server, store, client) = fixture(vec![E::IdentifierCollision(K::Refresh), E::Expired])?;
    let (records, response) = server.prepare_tokens(&client, "00042", vec![], binding()?, NOW)?;
    let code = old_code()?;
    assert!(
        Box::pin(server.persist_tokens(
            TokenRedemption::Code(&code),
            &client,
            records,
            response,
            None
        ))
        .await
        .is_err()
    );
    let state = store.0.lock().map_err(|_| "测试锁失败")?;
    assert_eq!(state.tokens.len(), 2);
    assert_stable(&state.tokens[0], &state.tokens[1], K::Refresh)?;
    Ok(())
}

#[tokio::test]
async fn authorization_rejects_non_code_collisions_and_unavailable_without_retry() -> TestResult {
    for error in [
        E::IdentifierCollision(K::Access),
        E::IdentifierCollision(K::Refresh),
        E::Unavailable(openrust::Error::custom(
            "unique_constraint",
            "普通数据库错误",
        )),
    ] {
        let (server, store, client) = fixture(vec![error])?;
        let request = AuthorizationRequest {
            client,
            redirect: ValidatedRedirectUri::from_authorization_request(
                Some("https://rp.test/cb"),
                &["https://rp.test/cb".to_owned()],
            )?,
            state: None,
            scopes: vec![],
            challenge: None,
        };
        assert!(
            Box::pin(server.approve(request, "00042", binding()?))
                .await
                .is_err()
        );
        assert_eq!(store.0.lock().map_err(|_| "测试锁失败")?.codes.len(), 1);
    }
    Ok(())
}

#[test]
fn default_adapter_never_guesses_collision_from_unique_error_text() {
    let error = E::from_framework(openrust::Error::custom(
        "unique_constraint",
        "duplicate oauth_access_tokens identifier",
    ));
    assert!(matches!(error, E::Unavailable(_)));
    for kind in [K::Code, K::Access, K::Refresh] {
        let error = E::IdentifierCollision(kind);
        assert_eq!(error.to_string(), "OAuth新标识符碰撞");
        assert_eq!(format!("{error:?}"), "OAuth新标识符碰撞");
    }
}

#[tokio::test]
async fn controlled_atomic_collision_does_not_consume_and_two_redeemers_have_one_winner()
-> TestResult {
    let (server, store, client) = fixture(vec![E::IdentifierCollision(K::Access)])?;
    let (left_records, left_response) =
        server.prepare_tokens(&client, "00042", vec![], binding()?, NOW)?;
    let (right_records, right_response) =
        server.prepare_tokens(&client, "00042", vec![], binding()?, NOW)?;
    let code = old_code()?;
    let (left, right) = tokio::join!(
        Box::pin(server.persist_tokens(
            TokenRedemption::Code(&code),
            &client,
            left_records,
            left_response,
            None
        )),
        Box::pin(server.persist_tokens(
            TokenRedemption::Code(&code),
            &client,
            right_records,
            right_response,
            None
        )),
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    let state = store.0.lock().map_err(|_| "测试锁失败")?;
    assert!(state.tokens_committed);
    assert_eq!(state.tokens.len(), 3);
    Ok(())
}

#[tokio::test]
async fn exhausted_collision_has_exact_safe_upstream_json_and_wrong_stage_is_server_error()
-> TestResult {
    use openrust::http::axum::{body::to_bytes, http::StatusCode, response::IntoResponse};
    let response = OAuthError::identifier_collision().into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await?)?;
    assert_eq!(
        body,
        json!({
            "error": "access_token_duplicate",
            "error_description": "Could not create unique access token identifier"
        })
    );
    assert!(body.get("hint").is_none());
    assert!(body.get("code").is_none());
    for kind in [K::Access, K::Refresh] {
        let error = identifier_terminal_error(
            &IdentifierRetryBudget::code(),
            E::IdentifierCollision(kind),
            crate::GrantOperation::Authorize,
            None,
        );
        let body = serde_json::to_value(error)?;
        assert_eq!(body["error"], "server_error");
        assert!(body.get("hint").is_none());
    }
    Ok(())
}

#[tokio::test]
async fn initial_access_time_error_precedes_refresh_lifetime_error() -> TestResult {
    let (mut server, store, client) = fixture(vec![])?;
    server.config.refresh_token_lifetime = crate::GrantLifetime::Seconds(u64::MAX);
    let result = server.prepare_tokens(&client, "00042", vec![], binding()?, -1);
    let Err(error) = result else {
        return Err("无效签发时间必须失败".into());
    };
    let body = serde_json::to_value(error)?;
    assert_eq!(body["error"], "server_error");
    assert_eq!(
        body["error_description"],
        "The authorization server encountered an unexpected condition which prevented it from fulfilling the request: Invalid server time"
    );
    let state = store.0.lock().map_err(|_| "测试锁失败")?;
    assert!(state.tokens.is_empty());
    assert!(state.codes.is_empty());
    Ok(())
}
