//! 使用真实 Keycloak authorization-code 流程验证 OIDC relying-party。
#![cfg(feature = "auth-oidc")]

use std::{
    collections::{BTreeMap, HashMap},
    error::Error as StdError,
    sync::Arc,
    time::Duration,
};

use axum::{
    Form, Json, Router,
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
#[cfg(feature = "auth-mfa")]
use hmac::{Hmac, Mac, digest::KeyInit};
use openrust::auth::{ExternalIdentityProvider, OidcProvider, VerifiedLogoutToken};
use openrust::http::{DependencyPolicy, HttpClient, HttpClientConfig, HttpTargetPolicy};
#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
use openrust::session::SessionStore as _;
#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
use openrust::session::{MemorySessionStore as MemoryStore, SessionRecord as Record};
#[cfg(feature = "auth-mfa")]
use sha1::Sha1;
use sha2::{Digest, Sha256};
use tokio::{
    net::TcpListener,
    sync::{Mutex, mpsc},
    task::JoinHandle,
};

type TestResult = std::result::Result<(), Box<dyn StdError>>;
type CapturedLogout = (String, Option<&'static str>, Option<String>);
type LogoutConfirmation = (url::Url, Vec<(String, String)>);

const CLIENT_ID: &str = "openrust-rp";
const CLIENT_SECRET: &str = "openrust-test-client-secret";
const REDIRECT_URI: &str = "http://127.0.0.1:3000/api/example/oidc/callback";
const TEST_USERNAME: &str = "openrust-test-user";
const TEST_PASSWORD: &str = "openrust-test-password";
const SECOND_CLIENT_ID: &str = "openrust-rp-secondary";
const SECOND_CLIENT_SECRET: &str = "openrust-secondary-test-client-secret";
const SECOND_REDIRECT_URI: &str = "http://127.0.0.1:3001/callback";
const TEST_ACCESS_TTL_SECS: u64 = 15;
const KEYCLOAK_ADMIN_USERNAME: &str = "OPENRUST_KEYCLOAK_ADMIN_USERNAME";
const KEYCLOAK_ADMIN_PASSWORD: &str = "OPENRUST_KEYCLOAK_ADMIN_PASSWORD";

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "真实 IdP 验收必须在单一浏览器授权码流程中检查 PKCE 与 ID Token"
)]
async fn keycloak_two_client_sso_and_backchannel_logout_tokens_are_validated() -> TestResult {
    let Ok(issuer) = std::env::var("OPENRUST_OIDC_ISSUER") else {
        if std::env::var("OPENRUST_REQUIRE_OIDC_IDP").as_deref() == Ok("1") {
            return Err("OPENRUST_REQUIRE_OIDC_IDP=1 but OPENRUST_OIDC_ISSUER is unset".into());
        }
        eprintln!(
            "skipping real Keycloak OIDC integration: OPENRUST_OIDC_ISSUER is unset; configure the local realm fixture first"
        );
        return Ok(());
    };

    let issuer_url = url::Url::parse(&issuer)?;
    if issuer_url.scheme() != "http"
        || issuer_url
            .host_str()
            .and_then(|host| host.parse::<std::net::IpAddr>().ok())
            .is_none_or(|address| !address.is_loopback())
    {
        return Err("the Keycloak acceptance target must be a loopback HTTP issuer".into());
    }
    let host = issuer_url.host_str().ok_or("Keycloak issuer has no host")?;
    let port = issuer_url
        .port_or_known_default()
        .ok_or("issuer has no port")?;
    let network = if host.parse::<std::net::IpAddr>()?.is_ipv4() {
        "127.0.0.0/8".parse()?
    } else {
        "::1/128".parse()?
    };
    let mut policy = HttpTargetPolicy::new();
    policy.allow_internal(host, network, [port])?;
    let client = HttpClient::restricted(
        HttpClientConfig {
            connect_timeout: Duration::from_secs(5),
            timeout: Duration::from_secs(20),
            max_response_bytes: 1024 * 1024,
            proxy: None,
        },
        policy,
    )?;
    let dependency = client.dependency(
        "keycloak-oidc-acceptance",
        DependencyPolicy {
            max_in_flight: 2,
            max_waiting: 2,
            circuit_breaker: None,
        },
    )?;
    let provider = Arc::new(
        OidcProvider::discover(
            &issuer,
            "keycloak",
            CLIENT_ID,
            Some(CLIENT_SECRET.to_owned()),
            REDIRECT_URI,
            ["profile".to_owned(), "email".to_owned()],
            Duration::from_secs(45),
            dependency,
        )
        .await?,
    );
    let second_dependency = client.dependency(
        "keycloak-oidc-secondary-acceptance",
        DependencyPolicy {
            max_in_flight: 2,
            max_waiting: 2,
            circuit_breaker: None,
        },
    )?;
    let second_provider = Arc::new(
        OidcProvider::discover(
            &issuer,
            "keycloak",
            SECOND_CLIENT_ID,
            Some(SECOND_CLIENT_SECRET.to_owned()),
            SECOND_REDIRECT_URI,
            ["profile".to_owned(), "email".to_owned()],
            Duration::from_secs(45),
            second_dependency,
        )
        .await?,
    );
    assert!(provider.end_session_endpoint().is_some());

    let authorization = provider.authorize()?;
    let (authorization_url, expected_state, nonce, verifier) = authorization.into_secrets();
    let query = authorization_url.query_pairs().collect::<Vec<_>>();
    let param = |name: &str| {
        query
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value.as_ref()))
    };
    assert_eq!(param("response_type"), Some("code"));
    assert_eq!(param("client_id"), Some(CLIENT_ID));
    assert_eq!(param("redirect_uri"), Some(REDIRECT_URI));
    assert_eq!(param("state"), Some(expected_state.as_str()));
    assert_eq!(param("code_challenge_method"), Some("S256"));
    assert_eq!(
        param("scope").map(|scope| scope.split_whitespace().collect::<Vec<_>>()),
        Some(vec!["profile", "email", "openid"])
    );
    let expected_challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    assert_eq!(param("code_challenge"), Some(expected_challenge.as_str()));
    assert_ne!(param("code_challenge").unwrap_or_default(), "");

    // 模拟真实浏览器：禁用自动重定向、保留 Keycloak cookie，并提交登录页中的表单。
    let browser = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(60))
        .build()?;
    let login_page = browser.get(authorization_url.clone()).send().await?;
    if !login_page.status().is_success() {
        return Err(format!(
            "Keycloak authorization page returned {}",
            login_page.status()
        )
        .into());
    }
    let cookie = login_page
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .collect::<Vec<_>>()
        .join("; ");
    if cookie.is_empty() {
        return Err("Keycloak authorization response did not set a browser session cookie".into());
    }
    let html = login_page.text().await?;
    let form_action = keycloak_login_action(&html, &authorization_url)?;
    if form_action.origin() != authorization_url.origin() {
        return Err("Keycloak login form action changed the issuer origin".into());
    }
    let form_response = browser
        .post(form_action)
        .header(reqwest::header::COOKIE, cookie.clone())
        .form(&[
            ("username", TEST_USERNAME),
            ("password", TEST_PASSWORD),
            ("credentialId", ""),
        ])
        .send()
        .await?;
    if !form_response.status().is_redirection() {
        return Err(format!(
            "Keycloak login form did not return a redirect: {}",
            form_response.status()
        )
        .into());
    }
    let location = form_response
        .headers()
        .get(reqwest::header::LOCATION)
        .ok_or("Keycloak login redirect has no Location header")?
        .to_str()?;
    let callback = url::Url::parse(location)?;
    let redirect = url::Url::parse(REDIRECT_URI)?;
    if callback.origin() != redirect.origin() || callback.path() != redirect.path() {
        return Err("Keycloak redirected to an unexpected callback URL".into());
    }
    let callback_state = callback
        .query_pairs()
        .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
        .ok_or("Keycloak callback has no state")?;
    let code = callback
        .query_pairs()
        .find_map(|(key, value)| (key == "code").then(|| value.into_owned()))
        .ok_or("Keycloak callback has no authorization code")?;
    assert_eq!(callback_state, expected_state);

    // 错误 state 必须在网络交换前失败；随后同一 code 仍应可成功交换。
    assert!(
        provider
            .authenticate_code("wrong-state", &expected_state, &code, &nonce, &verifier,)
            .await
            .is_err()
    );
    let identity = provider
        .authenticate_code(&callback_state, &expected_state, &code, &nonce, &verifier)
        .await?;
    assert_eq!(identity.issuer(), issuer);
    assert_ne!(identity.subject(), "");
    assert_eq!(identity.email(), Some("openrust-test-user@example.test"));
    assert!(identity.email_verified());

    // 同一浏览器 cookie 使用第二个独立 client 请求授权，不应再次呈现登录表单。
    let authenticated_cookie = merge_cookie_headers(
        &cookie,
        form_response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .map(str::to_owned)
            .collect::<Vec<_>>()
            .as_slice(),
    );
    let second_authorization = second_provider.authorize()?;
    let (second_url, second_expected_state, second_nonce, second_verifier) =
        second_authorization.into_secrets();
    let second_response = browser
        .get(second_url)
        .header(reqwest::header::COOKIE, authenticated_cookie.clone())
        .send()
        .await?;
    if !second_response.status().is_redirection() {
        return Err(format!(
            "second Keycloak client required an interactive login: {}",
            second_response.status()
        )
        .into());
    }
    let second_callback = url::Url::parse(
        second_response
            .headers()
            .get(reqwest::header::LOCATION)
            .ok_or("second client redirect has no Location header")?
            .to_str()?,
    )?;
    let second_redirect = url::Url::parse(SECOND_REDIRECT_URI)?;
    if second_callback.origin() != second_redirect.origin()
        || second_callback.path() != second_redirect.path()
    {
        return Err("Keycloak redirected the second client to an unexpected callback".into());
    }
    let second_callback_state = second_callback
        .query_pairs()
        .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
        .ok_or("second client callback has no state")?;
    let second_code = second_callback
        .query_pairs()
        .find_map(|(key, value)| (key == "code").then(|| value.into_owned()))
        .ok_or("second client callback has no code")?;
    assert_eq!(second_callback_state, second_expected_state);
    let second_identity = second_provider
        .authenticate_code(
            &second_callback_state,
            &second_expected_state,
            &second_code,
            &second_nonce,
            &second_verifier,
        )
        .await?;
    assert_eq!(second_identity.issuer(), issuer);
    assert_eq!(second_identity.subject(), identity.subject());
    assert_eq!(second_identity.email(), identity.email());

    // 让真实 Keycloak 向两个 client 配置的网络端点发送 Back-Channel Logout Token。
    let admin_username = std::env::var(KEYCLOAK_ADMIN_USERNAME).ok();
    let admin_password = std::env::var(KEYCLOAK_ADMIN_PASSWORD).ok();
    if let (Some(admin_username), Some(admin_password)) = (admin_username, admin_password) {
        #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
        let mut app_acceptance = if let Ok(database_url) =
            std::env::var("OPENRUST_TEST_DATABASE_URL")
        {
            let primary_app =
                create_logout_app(&database_url, "keycloak_primary_", provider.clone()).await?;
            let secondary_app = create_logout_app(
                &database_url,
                "keycloak_secondary_",
                second_provider.clone(),
            )
            .await?;
            Some((primary_app, secondary_app))
        } else {
            if std::env::var("OPENRUST_REQUIRE_OIDC_IDP").as_deref() == Ok("1") {
                return Err(
                    "real Keycloak handler acceptance requires OPENRUST_TEST_DATABASE_URL".into(),
                );
            }
            None
        };
        #[cfg(not(any(feature = "db-postgres", feature = "db-mysql")))]
        let app_acceptance: Option<()> = None;

        #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
        if let Some((primary, secondary)) = app_acceptance.as_mut() {
            complete_logout_app_login(primary, &browser, &authenticated_cookie).await?;
            complete_logout_app_login(secondary, &browser, &authenticated_cookie).await?;
        }

        let (backchannel_url, mut tokens, server) =
            if let Some((primary, secondary)) = app_acceptance.as_mut() {
                let (base, receiver, server) = start_logout_apps(primary, secondary).await?;
                (base, receiver, server)
            } else {
                let (base, receiver, server) = start_logout_receiver().await?;
                (base, receiver, server)
            };
        let admin = admin_client(&browser, &issuer_url, &admin_username, &admin_password).await?;
        let primary_url = format!("{backchannel_url}/primary/backchannel");
        let secondary_url = format!("{backchannel_url}/secondary/backchannel");
        configure_backchannel_logout(&admin, &issuer_url, CLIENT_ID, &primary_url).await?;
        configure_backchannel_logout(&admin, &issuer_url, SECOND_CLIENT_ID, &secondary_url).await?;
        #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
        if app_acceptance.is_some() {
            let mut forged_url = url::Url::parse(&backchannel_url)?;
            forged_url.set_host(Some("127.0.0.1"))?;
            forged_url.set_path("/primary/backchannel");
            let forged = browser
                .post(forged_url)
                .form(&[("logout_token", "forged.logout.token")])
                .send()
                .await?;
            assert_eq!(
                forged.status(),
                StatusCode::UNAUTHORIZED,
                "the application HTTP handler must reject a forged Logout Token"
            );
        }
        #[cfg(all(
            feature = "auth-mfa",
            any(feature = "db-postgres", feature = "db-mysql")
        ))]
        let mfa_completion = if let Some((primary, _)) = app_acceptance.as_ref() {
            if primary.api_nodes.is_some() {
                let pending_cookie =
                    start_pending_mfa_callback(primary, &browser, &authenticated_cookie).await?;
                hold_mfa_gates(primary, &browser).await?;
                let secret = primary
                    .mfa_secret
                    .as_deref()
                    .ok_or("primary application has no MFA seed")?;
                let code = keycloak_totp_code(secret, 1)?;
                let mfa_url = url::Url::parse(&primary.redirect_uri)?.join("/mfa")?;
                let mfa_browser = browser.clone();
                let request = tokio::spawn(async move {
                    mfa_browser
                        .post(mfa_url)
                        .header(reqwest::header::COOKIE, pending_cookie)
                        .form(&[("code", code)])
                        .send()
                        .await
                });
                wait_for_mfa_gate(primary, &browser).await?;
                Some(request)
            } else {
                None
            }
        } else {
            None
        };
        #[cfg(all(
            feature = "auth-mfa",
            any(feature = "db-postgres", feature = "db-mysql")
        ))]
        let had_pending_mfa = mfa_completion.is_some();
        #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
        let callback_completion = if let Some((primary, _)) = app_acceptance.as_ref() {
            if primary.api_nodes.is_some() {
                let (callback, cookie) =
                    prepare_pending_callback(primary, &browser, &authenticated_cookie).await?;
                hold_callback_gates(primary, &browser).await?;
                let callback_browser = browser.clone();
                let request = tokio::spawn(async move {
                    callback_browser
                        .get(callback)
                        .header(reqwest::header::COOKIE, cookie)
                        .send()
                        .await
                });
                wait_for_callback_gate(primary, &browser).await?;
                Some(request)
            } else {
                None
            }
        } else {
            None
        };
        #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
        if let Some((primary, _)) = app_acceptance.as_ref() {
            complete_rp_logout(primary, &browser, &authenticated_cookie).await?;
        } else {
            logout_keycloak_user(&admin, &issuer_url, TEST_USERNAME).await?;
        }
        #[cfg(not(any(feature = "db-postgres", feature = "db-mysql")))]
        logout_keycloak_user(&admin, &issuer_url, TEST_USERNAME).await?;

        let mut received = Vec::with_capacity(2);
        for _ in 0..2 {
            received.push(
                tokio::time::timeout(Duration::from_secs(10), tokens.recv())
                    .await?
                    .ok_or("Keycloak logout receiver stopped before receiving two tokens")?,
            );
        }
        #[cfg(feature = "auth-mfa")]
        if let Some((primary, _)) = app_acceptance.as_ref() {
            if let Some(mfa_request) = mfa_completion {
                // logout handler 已处理真实 IdP 通知，现在释放退出前的 MFA completion。
                release_mfa_gates(primary, &browser).await?;
                assert_eq!(mfa_request.await??.status(), StatusCode::UNAUTHORIZED);
            }
            if let Some(callback_request) = callback_completion {
                release_callback_gates(primary, &browser).await?;
                assert_eq!(
                    callback_request.await??.status(),
                    StatusCode::UNAUTHORIZED,
                    "an authorization callback completed after logout must be rejected"
                );
            }
        }
        #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
        if app_acceptance.is_some() {
            for (token, _, _) in &received {
                let wrong_path = if provider.verify_logout_token(token).await.is_ok() {
                    "/secondary/backchannel"
                } else {
                    "/primary/backchannel"
                };
                let mut wrong_url = url::Url::parse(&backchannel_url)?;
                wrong_url.set_host(Some("127.0.0.1"))?;
                wrong_url.set_path(wrong_path);
                let response = browser
                    .post(wrong_url)
                    .form(&[("logout_token", token)])
                    .send()
                    .await?;
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            }
        }
        let mut verified_primary = None;
        let mut verified_secondary = None;
        for (token, handler_outcome, handler_node) in &received {
            if let Ok(verified) = provider.verify_logout_token(token).await {
                if app_acceptance.is_some() {
                    assert_eq!(
                        *handler_outcome,
                        Some(if had_pending_mfa {
                            "processed-one"
                        } else {
                            "processed-other"
                        }),
                        "RP logout has already revoked the primary app association"
                    );
                }
                verified_primary = Some(verified);
                continue;
            }
            if let Ok(verified) = second_provider.verify_logout_token(token).await {
                if app_acceptance.is_some() {
                    if handler_outcome.is_none() {
                        assert_eq!(handler_node.as_deref(), Some("deferred"));
                    } else {
                        assert_eq!(*handler_outcome, Some("processed-one"));
                    }
                }
                verified_secondary = Some(verified);
            }
        }
        let verified_primary =
            verified_primary.ok_or("primary client Logout Token was not received")?;
        let verified_secondary =
            verified_secondary.ok_or("secondary client Logout Token was not received")?;
        assert_logout_token_targets(&verified_primary, CLIENT_ID, identity.subject());
        assert_logout_token_targets(&verified_secondary, SECOND_CLIENT_ID, identity.subject());
        assert_eq!(
            verified_primary.session_id(),
            verified_secondary.session_id()
        );
        assert_ne!(verified_primary.token_id(), verified_secondary.token_id());
        #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
        if let Some((mut primary, mut secondary)) = app_acceptance.take() {
            if primary.defer_secondary_logout.is_some() {
                let token = received
                    .iter()
                    .find_map(|(token, _, node)| {
                        (node.as_deref() == Some("deferred")).then_some(token)
                    })
                    .ok_or("secondary Keycloak logout token was not deferred for race test")?;
                if let Ok(container) = std::env::var("OPENRUST_F7_REDIS_CONTAINER") {
                    set_test_redis_container_state(&container, "stop")?;
                    let mut outage_url = url::Url::parse(&backchannel_url)?;
                    outage_url.set_host(Some("127.0.0.1"))?;
                    outage_url.set_path("/secondary/backchannel");
                    let failed_delivery = tokio::time::timeout(
                        Duration::from_secs(10),
                        browser
                            .post(outage_url.clone())
                            .form(&[("logout_token", token)])
                            .send(),
                    )
                    .await;
                    set_test_redis_container_state(&container, "start")?;
                    wait_for_test_redis_container(&container).await?;
                    assert_eq!(
                        failed_delivery??.status(),
                        StatusCode::SERVICE_UNAVAILABLE,
                        "a verified logout token must report shared Session storage failure for retry"
                    );
                    let nodes = secondary
                        .api_nodes
                        .as_mut()
                        .ok_or("secondary API nodes are missing")?;
                    nodes.restart(0).await?;
                    nodes.restart(1).await?;
                    let nodes = primary
                        .api_nodes
                        .as_mut()
                        .ok_or("primary API nodes are missing")?;
                    nodes.restart(0).await?;
                    nodes.restart(1).await?;
                    let mut recovered = None;
                    for _ in 0..20 {
                        let response = browser
                            .post(outage_url.clone())
                            .form(&[("logout_token", token)])
                            .send()
                            .await?;
                        if response.status() == StatusCode::NO_CONTENT {
                            recovered = Some(response);
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                    assert!(
                        recovered.is_some(),
                        "the same Logout Token must succeed after shared Session storage recovers"
                    );
                    let (replayed, outcome, node) =
                        tokio::time::timeout(Duration::from_secs(5), tokens.recv())
                            .await?
                            .ok_or("logout handler did not report Redis recovery replay")?;
                    assert_eq!(replayed, *token);
                    assert_eq!(outcome, Some("already-processed"));
                    assert!(node.is_some());
                }
                let route = primary
                    .defer_secondary_logout
                    .as_ref()
                    .ok_or("secondary logout race gate is missing")?;
                if std::env::var("OPENRUST_F7_REDIS_CONTAINER").is_err() {
                    assert!(!route.load(std::sync::atomic::Ordering::Acquire));
                    let mut race_url = url::Url::parse(&backchannel_url)?;
                    race_url.set_host(Some("127.0.0.1"))?;
                    race_url.set_path("/secondary/backchannel");
                    let refresh_token = secondary
                        .refresh_token
                        .as_deref()
                        .ok_or("secondary refresh token is missing")?
                        .to_owned();
                    let rotate = secondary.refresh.clone();
                    let refresh_race = tokio::time::timeout(Duration::from_secs(10), async {
                        tokio::join!(
                            browser
                                .post(race_url)
                                .form(&[("logout_token", token)])
                                .send(),
                            rotate.rotate(&refresh_token),
                        )
                    })
                    .await?;
                    let (response, rotation) = refresh_race;
                    assert_eq!(response?.status(), StatusCode::NO_CONTENT);
                    let (processed, outcome, node) =
                        tokio::time::timeout(Duration::from_secs(2), tokens.recv())
                            .await?
                            .ok_or("logout handler stopped before reporting refresh race")?;
                    assert_eq!(processed, *token);
                    assert_eq!(outcome, Some("processed-one"));
                    assert!(node.is_some());
                    let secondary_user_id =
                        secondary.user_id.ok_or("secondary user ID is missing")?;
                    assert_eq!(
                        secondary.active_external_refresh_tokens().await?,
                        Some(0),
                        "back-channel logout must revoke every token in the associated refresh family"
                    );
                    if let Ok(rotation) = rotation {
                        assert_eq!(rotation.user_id(), secondary_user_id);
                        assert!(
                            secondary
                                .refresh
                                .rotate(rotation.token().expose())
                                .await
                                .is_err(),
                            "a token rotated during logout must be revoked with its family"
                        );
                    }
                }
            }
            for (token, _, _) in &received {
                let path = if provider.verify_logout_token(token).await.is_ok() {
                    "/primary/backchannel"
                } else {
                    "/secondary/backchannel"
                };
                let mut replay_url = url::Url::parse(&backchannel_url)?;
                replay_url.set_host(Some("127.0.0.1"))?;
                replay_url.set_path(path);
                let response = browser
                    .post(replay_url)
                    .form(&[("logout_token", token)])
                    .send()
                    .await?;
                assert_eq!(response.status(), StatusCode::NO_CONTENT);
                let (replayed, outcome, node) =
                    tokio::time::timeout(Duration::from_secs(2), tokens.recv())
                        .await?
                        .ok_or("logout handler stopped before reporting replay")?;
                assert_eq!(replayed, *token);
                assert_eq!(outcome, Some("already-processed"));
                assert!(node.is_some(), "logout token must reach an API process");
            }
            if let Some(nodes) = primary.api_nodes.as_mut() {
                nodes.restart(0).await?;
                let mut primary_token = None;
                for (token, _, _) in &received {
                    if provider.verify_logout_token(token).await.is_ok() {
                        primary_token = Some(token);
                        break;
                    }
                }
                let primary_token = primary_token.ok_or("primary logout token is missing")?;
                let mut restart_url = url::Url::parse(&backchannel_url)?;
                restart_url.set_host(Some("127.0.0.1"))?;
                restart_url.set_path("/primary/backchannel");
                let response = browser
                    .post(restart_url)
                    .form(&[("logout_token", primary_token)])
                    .send()
                    .await?;
                assert_eq!(response.status(), StatusCode::NO_CONTENT);
                let (replayed, outcome, node) =
                    tokio::time::timeout(Duration::from_secs(2), tokens.recv())
                        .await?
                        .ok_or("logout handler stopped before reporting post-restart replay")?;
                assert_eq!(replayed, *primary_token);
                assert_eq!(outcome, Some("already-processed"));
                assert_eq!(node.as_deref(), Some("api-a"));
            }
            if std::env::var("OPENRUST_F7_REDIS_CONTAINER").is_err() {
                assert!(
                    primary
                        .session_store
                        .load(
                            primary
                                .session_id
                                .as_ref()
                                .ok_or("primary app Session was not created")?,
                            openrust::session::DEFAULT_MAX_BYTES,
                            time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
                        )
                        .await?
                        .is_none()
                );
                assert!(
                    secondary
                        .session_store
                        .load(
                            secondary
                                .session_id
                                .as_ref()
                                .ok_or("secondary app Session was not created")?,
                            openrust::session::DEFAULT_MAX_BYTES,
                            time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
                        )
                        .await?
                        .is_none()
                );
            }
            #[cfg(feature = "auth-mfa")]
            let expected_primary_revocations = if had_pending_mfa { 2 } else { 1 };
            #[cfg(not(feature = "auth-mfa"))]
            let expected_primary_revocations = 1;
            assert_eq!(
                primary.revoked_associations().await?,
                expected_primary_revocations
            );
            assert_eq!(secondary.revoked_associations().await?, 1);
            assert!(
                primary
                    .refresh
                    .rotate(
                        primary
                            .refresh_token
                            .as_deref()
                            .ok_or("primary refresh token is missing")?
                    )
                    .await
                    .is_err()
            );
            assert!(
                secondary
                    .refresh
                    .rotate(
                        secondary
                            .refresh_token
                            .as_deref()
                            .ok_or("secondary refresh token is missing")?
                    )
                    .await
                    .is_err()
            );
            verify_logout_access_token_window(&primary, &secondary).await?;
            assert_child_nodes_reject_revoked_credentials(&primary, &browser).await?;
            assert_child_nodes_reject_revoked_credentials(&secondary, &browser).await?;
            drop(primary.api_nodes.take());
            drop(secondary.api_nodes.take());
            primary.database.close().await?;
            secondary.database.close().await?;
            for server in primary.callback_servers {
                server.abort();
                let _ = server.await;
            }
            for server in secondary.callback_servers {
                server.abort();
                let _ = server.await;
            }
        }
        server.abort();
    } else if std::env::var("OPENRUST_REQUIRE_OIDC_IDP").as_deref() == Ok("1") {
        return Err(format!(
            "real Keycloak back-channel acceptance requires {KEYCLOAK_ADMIN_USERNAME} and {KEYCLOAK_ADMIN_PASSWORD}"
        )
        .into());
    } else {
        eprintln!(
            "skipping real Keycloak Back-Channel Logout: configure local admin credentials to enable it"
        );
    }
    Ok(())
}

async fn start_logout_receiver() -> std::result::Result<
    (
        String,
        mpsc::UnboundedReceiver<CapturedLogout>,
        JoinHandle<()>,
    ),
    Box<dyn StdError>,
> {
    let listener = TcpListener::bind(("0.0.0.0", 0)).await?;
    let port = listener.local_addr()?.port();
    let (sender, receiver) = mpsc::unbounded_channel();
    let app = Router::new()
        .route("/primary/backchannel", post(receive_logout_token))
        .route("/secondary/backchannel", post(receive_logout_token))
        .with_state(sender);
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((
        format!("http://host.docker.internal:{port}"),
        receiver,
        server,
    ))
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[derive(Clone)]
struct LogoutAppState {
    flows: openrust::auth::ExternalIdentityFlows,
    sessions: openrust::auth::SessionStoreHandle,
    completed: mpsc::UnboundedSender<CapturedLogout>,
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[derive(Clone)]
struct LoginCallbackState {
    flows: openrust::auth::ExternalIdentityFlows,
    refresh: openrust::auth::RefreshTokens,
    #[cfg(feature = "auth-mfa")]
    auth: openrust::auth::AuthFlows,
    #[cfg(feature = "auth-mfa")]
    mfa_gate: MfaTestGate,
    callback_gate: MfaTestGate,
    session_store: TestSessionStore,
    sessions: openrust::auth::SessionStoreHandle,
    outcome: std::sync::Arc<Mutex<Option<LoginCallbackOutcome>>>,
    binding_requests: std::sync::Arc<Mutex<HashMap<String, BindingCredentials>>>,
    logout_callback: std::sync::Arc<Mutex<Option<url::Url>>>,
    node_id: String,
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[derive(Clone, Default)]
struct MfaTestGate {
    held: Arc<std::sync::atomic::AtomicBool>,
    waiting: Arc<std::sync::atomic::AtomicUsize>,
    released: Arc<tokio::sync::Notify>,
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
impl MfaTestGate {
    async fn wait(&self) {
        if !self.held.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        self.waiting
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        loop {
            let notified = self.released.notified();
            if !self.held.load(std::sync::atomic::Ordering::Acquire) {
                break;
            }
            notified.await;
        }
        self.waiting
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }

    fn hold(&self) {
        self.held.store(true, std::sync::atomic::Ordering::Release);
    }

    fn release(&self) {
        self.held.store(false, std::sync::atomic::Ordering::Release);
        self.released.notify_waiters();
    }
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[derive(Clone)]
struct TestLoadBalancer {
    upstreams: [std::net::SocketAddr; 2],
    client: reqwest::Client,
    next_node: Arc<std::sync::atomic::AtomicUsize>,
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[derive(Clone)]
struct TestBackchannelBalancer {
    primary: [std::net::SocketAddr; 2],
    secondary: [std::net::SocketAddr; 2],
    client: reqwest::Client,
    primary_next: Arc<std::sync::atomic::AtomicUsize>,
    secondary_next: Arc<std::sync::atomic::AtomicUsize>,
    defer_secondary: Arc<std::sync::atomic::AtomicBool>,
    completed: mpsc::UnboundedSender<CapturedLogout>,
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
struct TestApiNodes {
    children: Vec<std::process::Child>,
    addresses: [std::net::SocketAddr; 2],
    configurations: [Vec<(String, String)>; 2],
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn spawn_test_api_nodes(
    app: &LogoutApp,
) -> std::result::Result<TestApiNodes, Box<dyn StdError>> {
    let issuer = std::env::var("OPENRUST_OIDC_ISSUER")?;
    let prefix = if app.redirect_uri == REDIRECT_URI {
        "keycloak_primary_"
    } else {
        "keycloak_secondary_"
    };
    let executable = std::env::current_exe()?;
    let mut children = Vec::with_capacity(2);
    let mut addresses = Vec::with_capacity(2);
    let mut configurations = Vec::with_capacity(2);
    for node in ["api-a", "api-b"] {
        let reservation = TcpListener::bind("127.0.0.1:0").await?;
        let address = reservation.local_addr()?;
        drop(reservation);
        let configuration = vec![
            (
                "OPENRUST_F7_NODE_DATABASE_URL".to_owned(),
                app.database.url().to_owned(),
            ),
            ("OPENRUST_F7_NODE_PREFIX".to_owned(), prefix.to_owned()),
            (
                "OPENRUST_F7_NODE_PORT".to_owned(),
                address.port().to_string(),
            ),
            ("OPENRUST_F7_NODE_ID".to_owned(), node.to_owned()),
            (
                "OPENRUST_TEST_SESSION_NAMESPACE".to_owned(),
                app.session_namespace.clone(),
            ),
            ("OPENRUST_REQUIRE_SHARED_SESSION".to_owned(), "1".to_owned()),
            ("OPENRUST_REQUIRE_OIDC_IDP".to_owned(), "1".to_owned()),
            ("OPENRUST_OIDC_ISSUER".to_owned(), issuer.clone()),
        ];
        let child = std::process::Command::new(&executable)
            .args([
                "--exact",
                "keycloak_api_node_process",
                "--ignored",
                "--nocapture",
            ])
            .envs(configuration.iter().map(|(key, value)| (key, value)))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .spawn()?;
        children.push(child);
        addresses.push(address);
        configurations.push(configuration);
    }
    let addresses: [std::net::SocketAddr; 2] = addresses
        .try_into()
        .map_err(|_| "failed to start two Keycloak test API nodes")?;
    let configurations: [Vec<(String, String)>; 2] = configurations
        .try_into()
        .map_err(|_| "failed to configure two Keycloak test API nodes")?;
    for (index, address) in addresses.iter().enumerate() {
        let mut ready = false;
        for _ in 0..100 {
            if children[index].try_wait()?.is_some() {
                return Err(format!(
                    "Keycloak API child {} exited before becoming ready",
                    index + 1
                )
                .into());
            }
            if tokio::net::TcpStream::connect(address).await.is_ok() {
                ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if !ready {
            return Err(format!("Keycloak API child {} did not become ready", index + 1).into());
        }
    }
    Ok(TestApiNodes {
        children,
        addresses,
        configurations,
    })
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn probe_test_api_nodes(
    app: &LogoutApp,
    browser: &reqwest::Client,
) -> std::result::Result<LoginOutcomeProbe, Box<dyn StdError>> {
    let nodes = app
        .api_nodes
        .as_ref()
        .ok_or("API child nodes are not running")?;
    for address in nodes.addresses {
        let response = browser
            .get(format!("http://{address}/probe"))
            .send()
            .await?;
        if response.status().is_success() {
            return Ok(response.json().await?);
        }
    }
    Err("no API child retained the completed login outcome".into())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn assert_child_nodes_reject_revoked_credentials(
    app: &LogoutApp,
    browser: &reqwest::Client,
) -> std::result::Result<(), Box<dyn StdError>> {
    let nodes = app
        .api_nodes
        .as_ref()
        .ok_or("API child nodes are not running")?;
    let session_id = app
        .session_id
        .as_ref()
        .ok_or("application Session was not created")?;
    let refresh_token = app
        .refresh_token
        .as_deref()
        .ok_or("application refresh token is missing")?;
    for address in nodes.addresses {
        let session = browser
            .get(format!(
                "http://{address}/session-probe?session_id={session_id}"
            ))
            .send()
            .await?;
        assert_eq!(
            session.status(),
            StatusCode::NOT_FOUND,
            "each API node must observe the revoked shared Session"
        );
        let refresh = browser
            .post(format!("http://{address}/refresh-probe"))
            .form(&[("refresh_token", refresh_token)])
            .send()
            .await?;
        assert_eq!(
            refresh.status(),
            StatusCode::UNAUTHORIZED,
            "each API node must reject the revoked refresh token"
        );
    }
    Ok(())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
fn set_test_redis_container_state(
    name: &str,
    action: &str,
) -> std::result::Result<(), Box<dyn StdError>> {
    if !name.starts_with("openrust-f7-session-redis-") || !matches!(action, "stop" | "start") {
        return Err("F7 Redis outage test requires its dedicated tagged container".into());
    }
    let inspected = std::process::Command::new("docker")
        .args([
            "inspect",
            "--format",
            "{{index .Config.Labels \"com.openrust.task\"}}",
            name,
        ])
        .output()?;
    if !inspected.status.success()
        || String::from_utf8_lossy(&inspected.stdout).trim() != "f7-session-outage"
    {
        return Err(
            "refusing to change a Redis container not created for F7 outage testing".into(),
        );
    }
    let changed = std::process::Command::new("docker")
        .args([action, name])
        .output()?;
    if !changed.status.success() {
        return Err(format!("docker {action} failed for the dedicated F7 Redis container").into());
    }
    Ok(())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn wait_for_test_redis_container(name: &str) -> std::result::Result<(), Box<dyn StdError>> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let ping = std::process::Command::new("docker")
                .args(["exec", name, "redis-cli", "ping"])
                .output()?;
            if ping.status.success() && String::from_utf8_lossy(&ping.stdout).trim() == "PONG" {
                return Ok::<(), Box<dyn StdError>>(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await??;
    Ok(())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
impl Drop for TestApiNodes {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn forward_logout_to_api_node(
    State(state): State<TestBackchannelBalancer>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    Form(fields): Form<HashMap<String, String>>,
) -> Response {
    let Some(token) = fields.get("logout_token") else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let (nodes, next) = if uri.path().starts_with("/primary/") {
        (&state.primary, &state.primary_next)
    } else if uri.path().starts_with("/secondary/") {
        (&state.secondary, &state.secondary_next)
    } else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if uri.path().starts_with("/secondary/")
        && state
            .defer_secondary
            .swap(false, std::sync::atomic::Ordering::AcqRel)
    {
        if state
            .completed
            .send((token.clone(), None, Some("deferred".to_owned())))
            .is_err()
        {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        return StatusCode::NO_CONTENT.into_response();
    }
    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % nodes.len();
    let Ok(response) = state
        .client
        .post(format!("http://{}/backchannel", nodes[index]))
        .form(&[("logout_token", token)])
        .send()
        .await
    else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    let status = response.status();
    if status.is_success() {
        let outcome = response
            .headers()
            .get("x-openrust-outcome")
            .and_then(|value| value.to_str().ok())
            .and_then(parse_outcome_label);
        let node = response
            .headers()
            .get("x-openrust-node")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        if state
            .completed
            .send((token.clone(), outcome, node))
            .is_err()
        {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }
    status.into_response()
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
fn parse_outcome_label(value: &str) -> Option<&'static str> {
    match value {
        "processed-one" => Some("processed-one"),
        "processed-other" => Some("processed-other"),
        "already-processed" => Some("already-processed"),
        _ => None,
    }
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
impl TestApiNodes {
    async fn restart(&mut self, index: usize) -> std::result::Result<(), Box<dyn StdError>> {
        let child = self
            .children
            .get_mut(index)
            .ok_or("Keycloak API node index is out of range")?;
        let _ = child.kill();
        let _ = child.wait();
        let executable = std::env::current_exe()?;
        let configuration = &self.configurations[index];
        let mut replacement = std::process::Command::new(executable)
            .args([
                "--exact",
                "keycloak_api_node_process",
                "--ignored",
                "--nocapture",
            ])
            .envs(configuration.iter().map(|(key, value)| (key, value)))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .spawn()?;
        let address = self.addresses[index];
        for _ in 0..100 {
            if replacement.try_wait()?.is_some() {
                return Err(format!("Keycloak API node {index} exited during restart").into());
            }
            if tokio::net::TcpStream::connect(address).await.is_ok() {
                self.children[index] = replacement;
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let _ = replacement.kill();
        let _ = replacement.wait();
        Err(format!("Keycloak API node {index} did not recover after restart").into())
    }
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[derive(Clone)]
enum TestSessionStore {
    Memory(MemoryStore),
    #[cfg(feature = "session-redis")]
    Redis(openrust::auth::RedisSessionStore),
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
impl std::fmt::Debug for TestSessionStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Memory(_) => formatter.write_str("TestSessionStore::Memory"),
            #[cfg(feature = "session-redis")]
            Self::Redis(store) => store.fmt(formatter),
        }
    }
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
impl openrust::session::SessionStore for TestSessionStore {
    fn write<'a>(
        &'a self,
        record: &'a Record,
        mode: openrust::session::SessionWriteMode,
        max_bytes: usize,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<openrust::session::SessionWriteOutcome>>
    {
        match self {
            Self::Memory(store) => store.write(record, mode, max_bytes),
            #[cfg(feature = "session-redis")]
            Self::Redis(store) => store.write(record, mode, max_bytes),
        }
    }
    fn load<'a>(
        &'a self,
        id: &'a openrust::session::SessionId,
        max_bytes: usize,
        empty_expiry: time::OffsetDateTime,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<Option<Record>>> {
        match self {
            Self::Memory(store) => store.load(id, max_bytes, empty_expiry),
            #[cfg(feature = "session-redis")]
            Self::Redis(store) => store.load(id, max_bytes, empty_expiry),
        }
    }
    fn delete<'a>(
        &'a self,
        id: &'a openrust::session::SessionId,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        match self {
            Self::Memory(store) => store.delete(id),
            #[cfg(feature = "session-redis")]
            Self::Redis(store) => store.delete(id),
        }
    }
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn test_session_store(
    namespace: &str,
) -> std::result::Result<TestSessionStore, Box<dyn StdError>> {
    #[cfg(feature = "session-redis")]
    if let Ok(redis_url) = std::env::var("OPENRUST_TEST_SESSION_REDIS_URL") {
        return Ok(TestSessionStore::Redis(
            openrust::auth::RedisSessionStore::connect(redis_url, namespace).await?,
        ));
    }
    if std::env::var("OPENRUST_REQUIRE_SHARED_SESSION").as_deref() == Ok("1") {
        return Err(
            "shared-session acceptance requires OPENRUST_TEST_SESSION_REDIS_URL and session-redis"
                .into(),
        );
    }
    #[cfg(not(feature = "session-redis"))]
    let _ = namespace;
    Ok(TestSessionStore::Memory(MemoryStore::default()))
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
fn test_session_namespace(prefix: &str) -> String {
    format!("{prefix}-{}", std::process::id())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
fn callback_node(state: LoginCallbackState, callback_path: &str) -> Router {
    let routes = Router::new()
        .route(callback_path, get(node_login_callback))
        .route("/probe", get(probe_login_outcome))
        .route("/session-probe", get(probe_shared_session))
        .route("/refresh-probe", post(probe_revoked_refresh))
        .route("/backchannel", post(process_node_backchannel));
    #[cfg(feature = "auth-mfa")]
    let routes = routes
        .route("/mfa", post(node_mfa_callback))
        .route("/test/mfa-gate/hold", post(hold_mfa_gate))
        .route("/test/mfa-gate/release", post(release_mfa_gate))
        .route("/test/mfa-gate/status", get(mfa_gate_status));
    let routes = routes
        .route("/test/callback-gate/hold", post(hold_callback_gate))
        .route("/test/callback-gate/release", post(release_callback_gate))
        .route("/test/callback-gate/status", get(callback_gate_status));
    routes.with_state(state)
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[derive(serde::Deserialize, serde::Serialize)]
struct LoginOutcomeProbe {
    user_id: i64,
    session_id: String,
    access_token: String,
    refresh_token: String,
    external_handle: String,
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn probe_login_outcome(
    State(state): State<LoginCallbackState>,
) -> std::result::Result<Json<LoginOutcomeProbe>, StatusCode> {
    let outcome = state.outcome.lock().await;
    let Some(outcome) = outcome.as_ref() else {
        return Err(StatusCode::NOT_FOUND);
    };
    Ok(Json(LoginOutcomeProbe {
        user_id: outcome.user_id,
        session_id: outcome.session_id.to_string(),
        access_token: outcome.access_token.clone(),
        refresh_token: outcome.refresh_token.clone(),
        external_handle: outcome.handle.expose().to_owned(),
    }))
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[derive(serde::Deserialize)]
struct SharedSessionProbeQuery {
    session_id: String,
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn probe_shared_session(
    State(state): State<LoginCallbackState>,
    Query(query): Query<SharedSessionProbeQuery>,
) -> StatusCode {
    let Ok(session_id) = query.session_id.parse() else {
        return StatusCode::BAD_REQUEST;
    };
    match state
        .session_store
        .load(
            &session_id,
            openrust::session::DEFAULT_MAX_BYTES,
            time::OffsetDateTime::now_utc() + time::Duration::minutes(120),
        )
        .await
    {
        Ok(Some(_)) => StatusCode::NO_CONTENT,
        Ok(None) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[derive(serde::Deserialize)]
struct RefreshProbeForm {
    refresh_token: String,
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn probe_revoked_refresh(
    State(state): State<LoginCallbackState>,
    Form(input): Form<RefreshProbeForm>,
) -> StatusCode {
    match state.refresh.rotate(&input.refresh_token).await {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::UNAUTHORIZED,
    }
}

#[cfg(all(
    any(feature = "db-postgres", feature = "db-mysql"),
    feature = "auth-mfa"
))]
async fn hold_mfa_gate(State(state): State<LoginCallbackState>) -> StatusCode {
    state.mfa_gate.hold();
    StatusCode::NO_CONTENT
}

#[cfg(all(
    any(feature = "db-postgres", feature = "db-mysql"),
    feature = "auth-mfa"
))]
async fn release_mfa_gate(State(state): State<LoginCallbackState>) -> StatusCode {
    state.mfa_gate.release();
    StatusCode::NO_CONTENT
}

#[cfg(all(
    any(feature = "db-postgres", feature = "db-mysql"),
    feature = "auth-mfa"
))]
async fn mfa_gate_status(State(state): State<LoginCallbackState>) -> Json<usize> {
    Json(
        state
            .mfa_gate
            .waiting
            .load(std::sync::atomic::Ordering::Acquire),
    )
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn hold_callback_gate(State(state): State<LoginCallbackState>) -> StatusCode {
    state.callback_gate.hold();
    StatusCode::NO_CONTENT
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn release_callback_gate(State(state): State<LoginCallbackState>) -> StatusCode {
    state.callback_gate.release();
    StatusCode::NO_CONTENT
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn callback_gate_status(State(state): State<LoginCallbackState>) -> Json<usize> {
    Json(
        state
            .callback_gate
            .waiting
            .load(std::sync::atomic::Ordering::Acquire),
    )
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[derive(serde::Deserialize)]
struct BackchannelForm {
    logout_token: String,
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn process_node_backchannel(
    State(state): State<LoginCallbackState>,
    Form(input): Form<BackchannelForm>,
) -> Response {
    let result = state
        .flows
        .process_backchannel_logout("keycloak", &input.logout_token, &state.sessions)
        .await;
    let mut response = match result {
        Ok(outcome) => {
            let mut response = StatusCode::NO_CONTENT.into_response();
            response.headers_mut().insert(
                "x-openrust-outcome",
                HeaderValue::from_static(outcome_label(outcome)),
            );
            response
        }
        Err(error) if error.code() == "session_store_unavailable" => {
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
        Err(_) => StatusCode::UNAUTHORIZED.into_response(),
    };
    if let Ok(value) = HeaderValue::from_str(&state.node_id) {
        response.headers_mut().insert("x-openrust-node", value);
    }
    response
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[tokio::test]
#[ignore = "由真实 Keycloak 集成测试作为独立 API 子进程启动"]
async fn keycloak_api_node_process() -> TestResult {
    let Ok(port) = std::env::var("OPENRUST_F7_NODE_PORT") else {
        return Ok(());
    };
    let database_url = std::env::var("OPENRUST_F7_NODE_DATABASE_URL")?;
    let prefix = std::env::var("OPENRUST_F7_NODE_PREFIX")?;
    let namespace = std::env::var("OPENRUST_TEST_SESSION_NAMESPACE")?;
    let node_id = std::env::var("OPENRUST_F7_NODE_ID")?;
    let issuer = std::env::var("OPENRUST_OIDC_ISSUER")?;
    serve_keycloak_api_node(&database_url, &prefix, &namespace, &node_id, &issuer, &port).await
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "子进程测试节点须显式接收应用数据库、共享 Session、OIDC client 与监听地址"
)]
async fn serve_keycloak_api_node(
    database_url: &str,
    prefix: &str,
    namespace: &str,
    node_id: &str,
    issuer: &str,
    port: &str,
) -> TestResult {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    #[cfg(feature = "auth-mfa")]
    use openrust::auth::MfaServiceProvider;
    use openrust::{
        auth::{
            AuthFlows, AuthFlowsServiceProvider, DatabaseVerificationCodeStore, JwtConfig,
            JwtTokens, PasswordConfig, Passwords, RefreshTokens, SessionStoreHandle,
            VerificationCodes,
        },
        config::EncryptionConfig,
        config::FrameworkConfig,
        core::RateLimiter,
        database::{DatabaseRateLimitStore, DatabaseTableNames, Db},
        security::Encryptor,
    };

    let names = DatabaseTableNames::new(prefix, BTreeMap::new())?;
    let raw = openrust::database::sea_orm::Database::connect(database_url).await?;
    let connection = Db::with_table_names(names);
    connection.initialize(raw)?;
    let passwords = Passwords::new(PasswordConfig {
        memory_kib: 19_456,
        iterations: 1,
        parallelism: 1,
    })?;
    let jwt = JwtTokens::new(
        JwtConfig::new(
            "keycloak-test-issuer",
            "keycloak-test-audience",
            "keycloak-test-key",
            BTreeMap::from([(
                "keycloak-test-key".to_owned(),
                STANDARD.encode([0x65_u8; 32]),
            )]),
        )
        .with_ttl(TEST_ACCESS_TTL_SECS, TEST_ACCESS_TTL_SECS),
    )?;
    let refresh = RefreshTokens::new(connection.clone());
    let codes = VerificationCodes::new(
        DatabaseVerificationCodeStore::new(connection.clone()),
        passwords.clone(),
    );
    let encryption_key = STANDARD.encode([0xa8_u8; 32]);
    let encryptor = Encryptor::new(EncryptionConfig::new(
        "keycloak-test-key",
        BTreeMap::from([("keycloak-test-key".to_owned(), encryption_key)]),
    ))?;
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(TestInstance(std::sync::Arc::new(connection.clone())))
        .provider(TestInstance(std::sync::Arc::new(passwords)))
        .provider(TestInstance(std::sync::Arc::new(jwt.clone())))
        .provider(TestInstance(std::sync::Arc::new(refresh)))
        .provider(TestInstance(std::sync::Arc::new(codes)))
        .provider(TestInstance(std::sync::Arc::new(RateLimiter::new(
            DatabaseRateLimitStore::new(connection.clone()),
        ))))
        .provider(TestInstance(std::sync::Arc::new(encryptor.clone())));
    #[cfg(feature = "auth-mfa")]
    builder.provider(MfaServiceProvider);
    builder.provider(AuthFlowsServiceProvider::new());
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let auth = (*app.state().resolve::<AuthFlows>().await?).clone();

    let issuer_url = url::Url::parse(issuer)?;
    let host = issuer_url.host_str().ok_or("Keycloak issuer has no host")?;
    let port_number = issuer_url
        .port_or_known_default()
        .ok_or("Keycloak issuer has no port")?;
    let network = if host.parse::<std::net::IpAddr>()?.is_ipv4() {
        "127.0.0.0/8".parse()?
    } else {
        "::1/128".parse()?
    };
    let mut policy = HttpTargetPolicy::new();
    policy.allow_internal(host, network, [port_number])?;
    let client = HttpClient::restricted(
        HttpClientConfig {
            connect_timeout: Duration::from_secs(5),
            timeout: Duration::from_secs(20),
            max_response_bytes: 1024 * 1024,
            proxy: None,
        },
        policy,
    )?;
    let dependency = client.dependency(
        format!("keycloak-f7-{node_id}"),
        DependencyPolicy {
            max_in_flight: 2,
            max_waiting: 2,
            circuit_breaker: None,
        },
    )?;
    let (client_id, client_secret, redirect_uri) = if prefix == "keycloak_primary_" {
        (CLIENT_ID, CLIENT_SECRET, REDIRECT_URI)
    } else {
        (SECOND_CLIENT_ID, SECOND_CLIENT_SECRET, SECOND_REDIRECT_URI)
    };
    let provider = Arc::new(
        OidcProvider::discover(
            issuer,
            "keycloak",
            client_id,
            Some(client_secret.to_owned()),
            redirect_uri,
            ["profile".to_owned(), "email".to_owned()],
            Duration::from_secs(45),
            dependency,
        )
        .await?,
    );
    let provider_trait: Arc<dyn ExternalIdentityProvider> = provider;
    let flows = openrust::auth::ExternalIdentityFlows::new(
        connection.clone(),
        auth.clone(),
        encryptor,
        [provider_trait],
    )?;
    let refresh = openrust::auth::RefreshTokens::new(connection);
    let session_store = test_session_store(namespace).await?;
    let state = LoginCallbackState {
        flows,
        refresh,
        #[cfg(feature = "auth-mfa")]
        auth,
        #[cfg(feature = "auth-mfa")]
        mfa_gate: MfaTestGate::default(),
        callback_gate: MfaTestGate::default(),
        sessions: SessionStoreHandle::new(session_store.clone()),
        session_store,
        outcome: Arc::new(Mutex::new(None)),
        binding_requests: Arc::new(Mutex::new(HashMap::new())),
        logout_callback: Arc::new(Mutex::new(None)),
        node_id: node_id.to_owned(),
    };
    let application = callback_node(state.clone(), url::Url::parse(redirect_uri)?.path());
    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    axum::serve(listener, application).await?;
    Ok(())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn node_login_callback(
    State(state): State<LoginCallbackState>,
    Query(query): Query<LoginCallbackQuery>,
    headers: HeaderMap,
) -> Response {
    let node_id = state.node_id.clone();
    if query.code.is_some() {
        state.callback_gate.wait().await;
    }
    let mut response = process_login_callback(State(state), Query(query), headers).await;
    if let Ok(value) = HeaderValue::from_str(&node_id) {
        response.headers_mut().insert("x-openrust-node", value);
    }
    response
}

#[cfg(all(
    any(feature = "db-postgres", feature = "db-mysql"),
    feature = "auth-mfa"
))]
async fn node_mfa_callback(
    State(state): State<LoginCallbackState>,
    headers: HeaderMap,
    Form(input): Form<MfaCallbackForm>,
) -> Response {
    let node_id = state.node_id.clone();
    let mut response = process_mfa_callback(State(state), headers, Form(input)).await;
    if let Ok(value) = HeaderValue::from_str(&node_id) {
        response.headers_mut().insert("x-openrust-node", value);
    }
    response
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn forward_to_api_node(
    State(state): State<TestLoadBalancer>,
    request: axum::extract::Request,
) -> Response {
    let node = state
        .next_node
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        % state.upstreams.len();
    let upstream = state.upstreams[node];
    let (parts, body) = request.into_parts();
    let path_and_query = parts
        .uri
        .path_and_query()
        .map_or_else(|| parts.uri.path().to_owned(), ToString::to_string);
    let mut headers = parts.headers;
    headers.remove(header::HOST);
    headers.remove(header::CONTENT_LENGTH);
    let Ok(body) = axum::body::to_bytes(body, 1_048_576).await else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Ok(response) = state
        .client
        .request(parts.method, format!("http://{upstream}{path_and_query}"))
        .headers(headers)
        .body(body)
        .send()
        .await
    else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    let status = response.status();
    let response_headers = response.headers().clone();
    let Ok(body) = response.bytes().await else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    let mut forwarded = Response::new(Body::from(body));
    *forwarded.status_mut() = status;
    for (name, value) in &response_headers {
        if name != header::CONTENT_LENGTH && name != header::TRANSFER_ENCODING {
            forwarded.headers_mut().append(name.clone(), value.clone());
        }
    }
    forwarded
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
struct LoginCallbackOutcome {
    user_id: i64,
    session_id: openrust::session::SessionId,
    access_token: String,
    refresh_token: String,
    handle: openrust::auth::ExternalLoginHandle,
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
struct BindingCredentials {
    user_id: i64,
    email: String,
    password: String,
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
struct LogoutApp {
    database: openrust::testing::TestDatabase,
    connection: openrust::database::Db,
    state: LogoutAppState,
    session_store: TestSessionStore,
    session_id: Option<openrust::session::SessionId>,
    jwt: openrust::auth::JwtTokens,
    access_token: Option<String>,
    refresh: openrust::auth::RefreshTokens,
    refresh_token: Option<String>,
    redirect_uri: String,
    session_namespace: String,
    callback_outcome: std::sync::Arc<Mutex<Option<LoginCallbackOutcome>>>,
    binding_requests: std::sync::Arc<Mutex<HashMap<String, BindingCredentials>>>,
    logout_callback: std::sync::Arc<Mutex<Option<url::Url>>>,
    callback_servers: Vec<JoinHandle<()>>,
    api_nodes: Option<TestApiNodes>,
    defer_secondary_logout: Option<Arc<std::sync::atomic::AtomicBool>>,
    user_id: Option<i64>,
    handle: Option<openrust::auth::ExternalLoginHandle>,
    #[cfg(feature = "auth-mfa")]
    auth: openrust::auth::AuthFlows,
    #[cfg(feature = "auth-mfa")]
    mfa_secret: Option<Vec<u8>>,
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
struct KeycloakMigrator;

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
impl openrust::database::sea_orm_migration::MigratorTrait for KeycloakMigrator {
    fn migrations() -> Vec<Box<dyn openrust::database::sea_orm_migration::MigrationTrait>> {
        use openrust::database::sea_orm_migration::MigrationTrait;
        use openrust::{
            audit::{DatabaseAuditMigration, DatabaseAuditTenantMigration},
            auth,
            database::DatabaseRateLimitMigration,
        };

        #[allow(unused_mut)]
        let mut migrations: Vec<Box<dyn MigrationTrait>> = vec![
            Box::new(auth::AuthMigration),
            Box::new(auth::AuthRefreshTokenMigration),
            Box::new(auth::AuthVerificationCodeMigration),
            Box::new(DatabaseRateLimitMigration),
            Box::new(DatabaseAuditMigration),
            Box::new(DatabaseAuditTenantMigration),
            Box::new(auth::AuthExternalIdentityMigration),
            Box::new(auth::AuthExternalSessionMigration),
        ];
        #[cfg(feature = "auth-mfa")]
        migrations.push(Box::new(auth::AuthMfaMigration));
        migrations
    }
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[allow(
    clippy::too_many_lines,
    reason = "独立应用退出回调必须装配真实数据库、关联 Session 和 provider"
)]
async fn create_logout_app(
    admin_url: &str,
    prefix: &str,
    provider: std::sync::Arc<OidcProvider>,
) -> std::result::Result<LogoutApp, Box<dyn StdError>> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    #[cfg(feature = "auth-mfa")]
    use openrust::auth::MfaServiceProvider;
    use openrust::{
        auth::{
            AuthFlows, AuthFlowsServiceProvider, DatabaseVerificationCodeStore, JwtConfig,
            JwtTokens, PasswordConfig, Passwords, RefreshTokens, SessionStoreHandle,
            VerificationCodes,
        },
        config::EncryptionConfig,
        config::FrameworkConfig,
        core::RateLimiter,
        database::{DatabaseRateLimitStore, DatabaseTableNames, Db},
        security::Encryptor,
    };

    let names = DatabaseTableNames::new(prefix, BTreeMap::new())?;
    let database = openrust::testing::TestDatabase::create_with_table_names::<KeycloakMigrator>(
        admin_url,
        names.clone(),
    )
    .await?;
    let raw_connection = database.connection()?;
    let connection = Db::with_table_names(names);
    connection.initialize(raw_connection.clone())?;
    let passwords = Passwords::new(PasswordConfig {
        memory_kib: 19_456,
        iterations: 1,
        parallelism: 1,
    })?;
    let jwt = JwtTokens::new(
        JwtConfig::new(
            "keycloak-test-issuer",
            "keycloak-test-audience",
            "keycloak-test-key",
            BTreeMap::from([(
                "keycloak-test-key".to_owned(),
                STANDARD.encode([0x65_u8; 32]),
            )]),
        )
        .with_ttl(TEST_ACCESS_TTL_SECS, TEST_ACCESS_TTL_SECS),
    )?;
    let refresh = RefreshTokens::new(connection.clone());
    let codes = VerificationCodes::new(
        DatabaseVerificationCodeStore::new(connection.clone()),
        passwords.clone(),
    );
    let key = STANDARD.encode([0xa8_u8; 32]);
    let encryptor = Encryptor::new(EncryptionConfig::new(
        "keycloak-test-key",
        BTreeMap::from([("keycloak-test-key".to_owned(), key)]),
    ))?;
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(TestInstance(std::sync::Arc::new(connection.clone())))
        .provider(TestInstance(std::sync::Arc::new(passwords)))
        .provider(TestInstance(std::sync::Arc::new(jwt.clone())))
        .provider(TestInstance(std::sync::Arc::new(refresh.clone())))
        .provider(TestInstance(std::sync::Arc::new(codes)))
        .provider(TestInstance(std::sync::Arc::new(RateLimiter::new(
            DatabaseRateLimitStore::new(connection.clone()),
        ))))
        .provider(TestInstance(std::sync::Arc::new(encryptor.clone())));
    #[cfg(feature = "auth-mfa")]
    builder.provider(MfaServiceProvider);
    builder.provider(AuthFlowsServiceProvider::new());
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let auth = (*app.state().resolve::<AuthFlows>().await?).clone();
    let provider_trait: std::sync::Arc<dyn ExternalIdentityProvider> = provider;
    let flows = openrust::auth::ExternalIdentityFlows::new(
        connection.clone(),
        auth.clone(),
        encryptor.clone(),
        [provider_trait],
    )?;

    let session_namespace = std::env::var("OPENRUST_TEST_SESSION_NAMESPACE")
        .unwrap_or_else(|_| test_session_namespace(prefix));
    let session_store = test_session_store(&session_namespace).await?;
    let sessions = SessionStoreHandle::new(session_store.clone());
    let redirect_uri = if prefix == "keycloak_primary_" {
        REDIRECT_URI
    } else {
        SECOND_REDIRECT_URI
    }
    .to_owned();
    Ok(LogoutApp {
        database,
        connection,
        state: LogoutAppState {
            flows,
            sessions,
            completed: mpsc::unbounded_channel().0,
        },
        session_store,
        session_id: None,
        jwt,
        access_token: None,
        refresh,
        refresh_token: None,
        redirect_uri,
        session_namespace,
        callback_outcome: std::sync::Arc::new(Mutex::new(None)),
        binding_requests: std::sync::Arc::new(Mutex::new(HashMap::new())),
        logout_callback: std::sync::Arc::new(Mutex::new(None)),
        callback_servers: Vec::new(),
        api_nodes: None,
        defer_secondary_logout: None,
        user_id: None,
        handle: None,
        #[cfg(feature = "auth-mfa")]
        auth,
        #[cfg(feature = "auth-mfa")]
        mfa_secret: None,
    })
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[allow(
    clippy::too_many_lines,
    clippy::similar_names,
    reason = "真实 Keycloak 回调须贯通授权、state、refresh 与应用 Session 关联"
)]
async fn complete_logout_app_login(
    app: &mut LogoutApp,
    browser: &reqwest::Client,
    browser_cookie: &str,
) -> std::result::Result<(), Box<dyn StdError>> {
    let authorization = app
        .state
        .flows
        .authorize("keycloak", "127.0.0.1")
        .await
        .map_err(|error| format!("application authorize failed: {error:?}"))?;
    let expected_state = authorization
        .url()
        .query_pairs()
        .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
        .ok_or("OpenRust authorization URL has no state")?;
    #[cfg(feature = "auth-mfa")]
    if app.redirect_uri == REDIRECT_URI && app.mfa_secret.is_none() {
        let (user_id, secret) = prepare_keycloak_mfa_user(app).await?;
        app.mfa_secret = Some(secret);
        bind_keycloak_mfa_identity(app, browser, browser_cookie, user_id).await?;
    }
    let process_mode = std::env::var("OPENRUST_REQUIRE_SHARED_SESSION").as_deref() == Ok("1");
    let upstreams = if process_mode {
        let nodes = spawn_test_api_nodes(app).await?;
        let addresses = nodes.addresses;
        app.api_nodes = Some(nodes);
        addresses
    } else {
        let node_a = TcpListener::bind("127.0.0.1:0").await?;
        let node_a_addr = node_a.local_addr()?;
        let node_b = TcpListener::bind("127.0.0.1:0").await?;
        let node_b_addr = node_b.local_addr()?;
        let node_state = |node_id: &str| LoginCallbackState {
            flows: app.state.flows.clone(),
            refresh: app.refresh.clone(),
            #[cfg(feature = "auth-mfa")]
            auth: app.auth.clone(),
            #[cfg(feature = "auth-mfa")]
            mfa_gate: MfaTestGate::default(),
            callback_gate: MfaTestGate::default(),
            session_store: app.session_store.clone(),
            sessions: openrust::auth::SessionStoreHandle::new(app.session_store.clone()),
            outcome: app.callback_outcome.clone(),
            binding_requests: app.binding_requests.clone(),
            logout_callback: app.logout_callback.clone(),
            node_id: node_id.to_owned(),
        };
        let node_a_app = callback_node(
            node_state("api-a"),
            url::Url::parse(&app.redirect_uri)?.path(),
        );
        let node_b_app = callback_node(
            node_state("api-b"),
            url::Url::parse(&app.redirect_uri)?.path(),
        );
        app.callback_servers.extend([
            tokio::spawn(async move {
                let _ = axum::serve(node_a, node_a_app).await;
            }),
            tokio::spawn(async move {
                let _ = axum::serve(node_b, node_b_app).await;
            }),
        ]);
        [node_a_addr, node_b_addr]
    };
    let balancer = TcpListener::bind(callback_socket(&app.redirect_uri)?).await?;
    let balancer_app = Router::new()
        .fallback(forward_to_api_node)
        .with_state(TestLoadBalancer {
            upstreams,
            client: reqwest::Client::builder().no_proxy().build()?,
            next_node: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        });
    app.callback_servers.push(tokio::spawn(async move {
        let _ = axum::serve(balancer, balancer_app).await;
    }));
    let response = browser
        .get(authorization.url().clone())
        .header(reqwest::header::COOKIE, browser_cookie)
        .send()
        .await?;
    if !response.status().is_redirection() {
        return Err(format!(
            "Keycloak required interactive login during app callback setup: {}",
            response.status()
        )
        .into());
    }
    let callback = url::Url::parse(
        response
            .headers()
            .get(reqwest::header::LOCATION)
            .ok_or("Keycloak app callback redirect has no Location")?
            .to_str()?,
    )?;
    let callback_state = callback
        .query_pairs()
        .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
        .ok_or("Keycloak app callback has no state")?;
    let code = callback
        .query_pairs()
        .find_map(|(name, value)| (name == "code").then(|| value.into_owned()))
        .ok_or("Keycloak app callback has no authorization code")?;
    if callback_state != expected_state {
        return Err("Keycloak app callback state did not match the authorization request".into());
    }
    let mut bad_state_callback = callback.clone();
    bad_state_callback
        .query_pairs_mut()
        .clear()
        .append_pair("state", "invalid-state")
        .append_pair("code", &code);
    let bad_cookie = callback_cookie(&authorization)?;
    let rejected = browser
        .get(bad_state_callback)
        .header(
            reqwest::header::COOKIE,
            format!("{browser_cookie}; {bad_cookie}"),
        )
        .send()
        .await?;
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);

    let callback_response = browser
        .get(callback)
        .header(
            reqwest::header::COOKIE,
            format!("{browser_cookie}; {bad_cookie}"),
        )
        .send()
        .await?;
    #[cfg(feature = "auth-mfa")]
    let login_node = callback_response
        .headers()
        .get("x-openrust-node")
        .and_then(|value| value.to_str().ok())
        .ok_or("load balancer response omitted its API node")?
        .to_owned();
    #[cfg(feature = "auth-mfa")]
    let callback_response = if callback_response.status() == StatusCode::ACCEPTED {
        let secret = app
            .mfa_secret
            .as_deref()
            .ok_or("application returned MFA challenge without a local factor")?;
        let session_cookie = callback_response
            .headers()
            .get(reqwest::header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .ok_or("application returned an MFA challenge without a shared Session cookie")?;
        let mfa_url = url::Url::parse(&app.redirect_uri)?.join("/mfa")?;
        let mfa_response = browser
            .post(mfa_url)
            .header(reqwest::header::COOKIE, session_cookie)
            .form(&[("code", keycloak_totp_code(secret, 1)?)])
            .send()
            .await?;
        let mfa_node = mfa_response
            .headers()
            .get("x-openrust-node")
            .and_then(|value| value.to_str().ok())
            .ok_or("MFA response omitted its API node")?;
        if login_node == mfa_node {
            return Err("load balancer sent MFA completion to the same API node".into());
        }
        mfa_response
    } else {
        callback_response
    };
    #[cfg(not(feature = "auth-mfa"))]
    let callback_response = {
        if callback_response.status() == StatusCode::ACCEPTED {
            return Err("application returned MFA challenge without the auth-mfa feature".into());
        }
        callback_response
    };
    if callback_response.status() != StatusCode::NO_CONTENT {
        return Err(format!(
            "application OIDC callback returned {}",
            callback_response.status()
        )
        .into());
    }
    if app.api_nodes.is_some() {
        let outcome = probe_test_api_nodes(app, browser).await?;
        app.session_id = Some(outcome.session_id.parse()?);
        app.access_token = Some(outcome.access_token);
        app.refresh_token = Some(outcome.refresh_token);
        app.user_id = Some(outcome.user_id);
        app.handle = Some(openrust::auth::ExternalLoginHandle::from_exposed(
            outcome.external_handle,
        )?);
    } else {
        let outcome = app
            .callback_outcome
            .lock()
            .await
            .take()
            .ok_or("application OIDC callback produced no login result")?;
        app.session_id = Some(outcome.session_id);
        app.access_token = Some(outcome.access_token);
        app.refresh_token = Some(outcome.refresh_token);
        app.user_id = Some(outcome.user_id);
        app.handle = Some(outcome.handle);
    }
    Ok(())
}

#[cfg(all(
    any(feature = "db-postgres", feature = "db-mysql"),
    feature = "auth-mfa"
))]
async fn start_pending_mfa_callback(
    app: &LogoutApp,
    browser: &reqwest::Client,
    browser_cookie: &str,
) -> std::result::Result<String, Box<dyn StdError>> {
    let authorization = app.state.flows.authorize("keycloak", "127.0.0.1").await?;
    let response = browser
        .get(authorization.url().clone())
        .header(reqwest::header::COOKIE, browser_cookie)
        .send()
        .await?;
    if !response.status().is_redirection() {
        return Err(format!(
            "pending MFA authorization required interaction: {}",
            response.status()
        )
        .into());
    }
    let callback = url::Url::parse(
        response
            .headers()
            .get(reqwest::header::LOCATION)
            .ok_or("pending MFA authorization has no callback Location")?
            .to_str()?,
    )?;
    let binding_cookie = callback_cookie(&authorization)?;
    let callback_response = browser
        .get(callback)
        .header(
            reqwest::header::COOKIE,
            format!("{browser_cookie}; {binding_cookie}"),
        )
        .send()
        .await?;
    assert_eq!(callback_response.status(), StatusCode::ACCEPTED);
    let session_cookie = callback_response
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .ok_or("pending MFA callback omitted its Session cookie")?;
    Ok(session_cookie.to_owned())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn prepare_pending_callback(
    app: &LogoutApp,
    browser: &reqwest::Client,
    browser_cookie: &str,
) -> std::result::Result<(url::Url, String), Box<dyn StdError>> {
    let authorization = app.state.flows.authorize("keycloak", "127.0.0.1").await?;
    let response = browser
        .get(authorization.url().clone())
        .header(reqwest::header::COOKIE, browser_cookie)
        .send()
        .await?;
    if !response.status().is_redirection() {
        return Err(format!(
            "pending callback authorization required interaction: {}",
            response.status()
        )
        .into());
    }
    let callback = url::Url::parse(
        response
            .headers()
            .get(reqwest::header::LOCATION)
            .ok_or("pending callback authorization has no Location")?
            .to_str()?,
    )?;
    let cookie = callback_cookie(&authorization)?;
    Ok((callback, format!("{browser_cookie}; {cookie}")))
}

#[cfg(all(
    any(feature = "db-postgres", feature = "db-mysql"),
    feature = "auth-mfa"
))]
async fn hold_mfa_gates(
    app: &LogoutApp,
    browser: &reqwest::Client,
) -> std::result::Result<(), Box<dyn StdError>> {
    let nodes = app
        .api_nodes
        .as_ref()
        .ok_or("API nodes are not running for the MFA race")?;
    for address in nodes.addresses {
        let response = browser
            .post(format!("http://{address}/test/mfa-gate/hold"))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
    Ok(())
}

#[cfg(all(
    any(feature = "db-postgres", feature = "db-mysql"),
    feature = "auth-mfa"
))]
async fn wait_for_mfa_gate(
    app: &LogoutApp,
    browser: &reqwest::Client,
) -> std::result::Result<(), Box<dyn StdError>> {
    let nodes = app
        .api_nodes
        .as_ref()
        .ok_or("API nodes are not running for the MFA race")?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            for address in nodes.addresses {
                let response = browser
                    .get(format!("http://{address}/test/mfa-gate/status"))
                    .send()
                    .await?;
                if response.status().is_success() && response.json::<usize>().await? > 0 {
                    return Ok::<(), Box<dyn StdError>>(());
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    Ok(())
}

#[cfg(all(
    any(feature = "db-postgres", feature = "db-mysql"),
    feature = "auth-mfa"
))]
async fn release_mfa_gates(
    app: &LogoutApp,
    browser: &reqwest::Client,
) -> std::result::Result<(), Box<dyn StdError>> {
    let nodes = app
        .api_nodes
        .as_ref()
        .ok_or("API nodes are not running for the MFA race")?;
    for address in nodes.addresses {
        let response = browser
            .post(format!("http://{address}/test/mfa-gate/release"))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
    Ok(())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn hold_callback_gates(
    app: &LogoutApp,
    browser: &reqwest::Client,
) -> std::result::Result<(), Box<dyn StdError>> {
    let nodes = app.api_nodes.as_ref().ok_or("API nodes are not running")?;
    for address in nodes.addresses {
        let response = browser
            .post(format!("http://{address}/test/callback-gate/hold"))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
    Ok(())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn wait_for_callback_gate(
    app: &LogoutApp,
    browser: &reqwest::Client,
) -> std::result::Result<(), Box<dyn StdError>> {
    let nodes = app.api_nodes.as_ref().ok_or("API nodes are not running")?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            for address in nodes.addresses {
                let response = browser
                    .get(format!("http://{address}/test/callback-gate/status"))
                    .send()
                    .await?;
                if response.status().is_success() && response.json::<usize>().await? > 0 {
                    return Ok::<(), Box<dyn StdError>>(());
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    Ok(())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn release_callback_gates(
    app: &LogoutApp,
    browser: &reqwest::Client,
) -> std::result::Result<(), Box<dyn StdError>> {
    let nodes = app.api_nodes.as_ref().ok_or("API nodes are not running")?;
    for address in nodes.addresses {
        let response = browser
            .post(format!("http://{address}/test/callback-gate/release"))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
    Ok(())
}

#[cfg(all(
    any(feature = "db-postgres", feature = "db-mysql"),
    feature = "auth-mfa"
))]
async fn prepare_keycloak_mfa_user(
    app: &LogoutApp,
) -> std::result::Result<(i64, Vec<u8>), Box<dyn StdError>> {
    use sea_orm::ConnectionTrait;

    let email = "openrust-test-user@example.test";
    let password = "openrust-local-mfa-password";
    let issued = app
        .auth
        .register(email, password, "keycloak-mfa-register")
        .await?
        .ok_or("Keycloak MFA test account was not created")?;
    if !app
        .auth
        .verify_email(email, issued.code(), "keycloak-mfa-verify")
        .await?
    {
        return Err("Keycloak MFA test account email was not verified".into());
    }
    let users = app.connection.quoted_table_name("users")?;
    let raw = app.connection.connection()?;
    let backend = raw.get_database_backend();
    let row = raw
        .query_one_raw(openrust::database::sea_orm::Statement::from_sql_and_values(
            backend,
            format!(
                "SELECT id FROM {users} WHERE email = {}",
                if backend == openrust::database::sea_orm::DbBackend::Postgres {
                    "$1"
                } else {
                    "?"
                }
            ),
            [email.into()],
        ))
        .await?
        .ok_or("Keycloak MFA test account row was not stored")?;
    let user_id = row.try_get::<i64>("", "id")?;
    let enrollment = app
        .auth
        .begin_totp_enrollment(
            user_id,
            email,
            password,
            "OpenRust Keycloak test",
            "keycloak-mfa-enroll",
        )
        .await?;
    let uri = url::Url::parse(enrollment.provisioning_uri())?;
    let encoded = uri
        .query_pairs()
        .find_map(|(name, value)| (name == "secret").then(|| value.into_owned()))
        .ok_or("Keycloak MFA enrollment omitted its seed")?;
    let secret = decode_keycloak_base32(&encoded)?;
    let recovery_codes = app
        .auth
        .confirm_totp_enrollment(
            user_id,
            &keycloak_totp_code(&secret, 0)?,
            "keycloak-mfa-confirm",
        )
        .await?;
    assert_eq!(recovery_codes.expose().len(), 10);
    Ok((user_id, secret))
}

#[cfg(all(
    any(feature = "db-postgres", feature = "db-mysql"),
    feature = "auth-mfa"
))]
async fn bind_keycloak_mfa_identity(
    app: &LogoutApp,
    browser: &reqwest::Client,
    browser_cookie: &str,
    user_id: i64,
) -> std::result::Result<(), Box<dyn StdError>> {
    let email = "openrust-test-user@example.test";
    let password = "openrust-local-mfa-password";
    let authorization = app.state.flows.authorize("keycloak", "127.0.0.1").await?;
    let state = authorization
        .url()
        .query_pairs()
        .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
        .ok_or("Keycloak identity bind URL has no state")?;
    let response = browser
        .get(authorization.url().clone())
        .header(reqwest::header::COOKIE, browser_cookie)
        .send()
        .await?;
    if !response.status().is_redirection() {
        return Err(format!(
            "Keycloak identity binding required interactive login: {}",
            response.status()
        )
        .into());
    }
    let callback = url::Url::parse(
        response
            .headers()
            .get(reqwest::header::LOCATION)
            .ok_or("Keycloak identity binding redirect has no Location")?
            .to_str()?,
    )?;
    let callback_state = callback
        .query_pairs()
        .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
        .ok_or("Keycloak identity binding callback has no state")?;
    if callback_state != state {
        return Err("Keycloak identity binding state did not match".into());
    }
    let code = callback
        .query_pairs()
        .find_map(|(name, value)| (name == "code").then(|| value.into_owned()))
        .ok_or("Keycloak identity binding callback has no authorization code")?;
    let browser_binding = authorization.browser_binding_cookie().to_owned();
    assert_binding_request_state(app, &state, &browser_binding).await?;
    app.state
        .flows
        .bind(
            user_id,
            email,
            password,
            "127.0.0.1",
            "keycloak",
            &state,
            &code,
            &browser_binding,
        )
        .await
        .map_err(|error| format!("Keycloak MFA identity binding failed: {error:?}"))?;
    Ok(())
}

#[cfg(all(
    any(feature = "db-postgres", feature = "db-mysql"),
    feature = "auth-mfa"
))]
async fn assert_binding_request_state(
    app: &LogoutApp,
    state: &str,
    browser_binding: &str,
) -> std::result::Result<(), Box<dyn StdError>> {
    use sea_orm::ConnectionTrait;

    let backend = app.connection.connection()?.get_database_backend();
    let table = app
        .connection
        .quoted_table_name("external_identity_requests")?;
    let (sql, state_value) = match backend {
        openrust::database::sea_orm::DbBackend::Postgres => (
            format!(
                "SELECT provider, browser_binding_hash FROM {table} WHERE state_hash = $1 AND expires_at > CURRENT_TIMESTAMP"
            ),
            format!("{:x}", Sha256::digest(state.as_bytes())),
        ),
        openrust::database::sea_orm::DbBackend::MySql => (
            format!(
                "SELECT provider, browser_binding_hash FROM {table} WHERE state_hash = ? AND expires_at > CURRENT_TIMESTAMP(6)"
            ),
            format!("{:x}", Sha256::digest(state.as_bytes())),
        ),
        _ => return Err("unsupported database backend for binding-state probe".into()),
    };
    let row = app
        .connection
        .connection()?
        .query_one_raw(openrust::database::sea_orm::Statement::from_sql_and_values(
            backend,
            sql,
            [state_value.into()],
        ))
        .await?
        .ok_or("Keycloak binding state was not persisted or already expired")?;
    let provider = row.try_get::<String>("", "provider")?;
    assert_eq!(provider, "keycloak", "binding state provider mismatch");
    let stored_binding = row.try_get::<String>("", "browser_binding_hash")?;
    let actual_binding = format!("{:x}", Sha256::digest(browser_binding.as_bytes()));
    assert!(
        stored_binding == actual_binding,
        "Keycloak binding browser cookie does not match persisted state"
    );
    Ok(())
}

#[cfg(all(
    any(feature = "db-postgres", feature = "db-mysql"),
    feature = "auth-mfa"
))]
fn decode_keycloak_base32(encoded: &str) -> std::result::Result<Vec<u8>, Box<dyn StdError>> {
    let mut output = Vec::new();
    let mut buffer = 0_u32;
    let mut bits = 0_u8;
    for byte in encoded.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'2'..=b'7' => byte - b'2' + 26,
            _ => return Err("invalid TOTP seed encoding".into()),
        };
        buffer = ((buffer << 5) | u32::from(value)) & 0xffff;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            output.push(<u8 as std::convert::TryFrom<u32>>::try_from(
                (buffer >> bits) & 0xff,
            )?);
        }
    }
    Ok(output)
}

#[cfg(all(
    any(feature = "db-postgres", feature = "db-mysql"),
    feature = "auth-mfa"
))]
fn keycloak_totp_code(
    secret: &[u8],
    step_offset: u64,
) -> std::result::Result<String, Box<dyn StdError>> {
    let step = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        / 30
        + step_offset;
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret)?;
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 0x0f);
    let value = (u32::from(digest[offset] & 0x7f) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    Ok(format!("{:06}", value % 1_000_000))
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn complete_rp_logout(
    app: &LogoutApp,
    browser: &reqwest::Client,
    browser_cookie: &str,
) -> std::result::Result<(), Box<dyn StdError>> {
    let user_id = app.user_id.ok_or("application callback has no user ID")?;
    let handle = app
        .handle
        .as_ref()
        .ok_or("application callback has no handle")?;
    let redirect = app
        .state
        .flows
        .begin_logout(user_id, handle, &app.redirect_uri, &app.state.sessions)
        .await?
        .ok_or("configured Keycloak did not produce an RP logout URL")?;
    let expected_state = redirect.state().to_owned();
    let mut response = browser
        .get(redirect.url().clone())
        .header(reqwest::header::COOKIE, browser_cookie)
        .send()
        .await?;
    if response.status() == StatusCode::OK {
        let confirmation = response.text().await?;
        let (action, fields) = keycloak_logout_confirmation(&confirmation, redirect.url())?;
        response = browser
            .post(action)
            .header(reqwest::header::COOKIE, browser_cookie)
            .form(&fields)
            .send()
            .await?;
    }
    if !response.status().is_redirection() {
        return Err(format!(
            "Keycloak RP-Initiated Logout returned {}",
            response.status()
        )
        .into());
    }
    let callback = url::Url::parse(
        response
            .headers()
            .get(reqwest::header::LOCATION)
            .ok_or("Keycloak logout redirect has no Location")?
            .to_str()?,
    )?;
    let configured = url::Url::parse(&app.redirect_uri)?;
    if callback.origin() != configured.origin() || callback.path() != configured.path() {
        return Err("Keycloak RP logout returned an unexpected callback URI".into());
    }
    let callback_state = callback
        .query_pairs()
        .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
        .ok_or("Keycloak logout callback has no state")?;
    assert_eq!(callback_state, expected_state);
    let callback_response = browser.get(callback.clone()).send().await?;
    assert_eq!(callback_response.status(), StatusCode::NO_CONTENT);
    if app.api_nodes.is_none() {
        assert_eq!(
            app.logout_callback.lock().await.take().as_ref(),
            Some(&configured)
        );
    }
    assert_eq!(
        browser.get(callback).send().await?.status(),
        StatusCode::UNAUTHORIZED,
        "the app logout callback must consume state only once"
    );
    assert!(
        app.session_store
            .load(
                app.session_id
                    .as_ref()
                    .ok_or("application Session is missing")?,
                openrust::session::DEFAULT_MAX_BYTES,
                time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
            )
            .await?
            .is_none(),
        "RP logout must remove the local Session before redirecting to Keycloak"
    );
    assert!(
        app.refresh
            .rotate(
                app.refresh_token
                    .as_deref()
                    .ok_or("application refresh token is missing")?
            )
            .await
            .is_err(),
        "RP logout must revoke the local refresh family before redirecting"
    );
    Ok(())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[derive(serde::Deserialize)]
struct LoginCallbackQuery {
    state: String,
    code: Option<String>,
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[allow(
    clippy::too_many_lines,
    reason = "同一 HTTP callback 同时验证 RP logout、身份绑定、state cookie 与 MFA challenge 语义"
)]
async fn process_login_callback(
    State(state): State<LoginCallbackState>,
    Query(query): Query<LoginCallbackQuery>,
    headers: HeaderMap,
) -> Response {
    #[cfg(feature = "auth-mfa")]
    use openrust::auth::LoginAttempt;
    use openrust::auth::Session;

    let Some(code) = query.code.as_deref() else {
        let Ok(redirect) = state.flows.complete_logout("keycloak", &query.state).await else {
            return StatusCode::UNAUTHORIZED.into_response();
        };
        *state.logout_callback.lock().await = Some(redirect);
        return StatusCode::NO_CONTENT.into_response();
    };

    let Ok(cookie_name) =
        openrust::auth::ExternalIdentityAuthorization::cookie_name_for_callback(&query.state)
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Some(cookie_header) = headers
        .get(axum::http::header::COOKIE)
        .and_then(|value| value.to_str().ok())
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Some(binding) = cookie_header.split(';').find_map(|cookie| {
        let (name, value) = cookie.trim().split_once('=')?;
        (name == cookie_name).then_some(value)
    }) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if let Some(credentials) = state.binding_requests.lock().await.remove(&query.state) {
        return match state
            .flows
            .bind(
                credentials.user_id,
                &credentials.email,
                &credentials.password,
                "127.0.0.1",
                "keycloak",
                &query.state,
                code,
                binding,
            )
            .await
        {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(_) => StatusCode::UNAUTHORIZED.into_response(),
        };
    }
    let Ok(login) = state
        .flows
        .complete_with_handle("keycloak", &query.state, code, binding, "127.0.0.1")
        .await
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let (attempt, handle) = login.into_parts();
    #[cfg(feature = "auth-mfa")]
    let credentials = match attempt {
        LoginAttempt::Authenticated(credentials) => credentials,
        LoginAttempt::MfaRequired(challenge) => {
            let Ok(session) = Session::new(None, Arc::new(state.session_store.clone()), None)
            else {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            };
            if session.start().await.is_err() {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
            if session
                .set_expiry(Some(openrust::session::Expiry::OnInactivity(
                    time::Duration::minutes(5),
                )))
                .is_err()
            {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
            if session
                .put("openrust.auth.mfa.user_id", challenge.user_id())
                .is_err()
                || session
                    .put("openrust.auth.mfa.challenge", challenge.expose())
                    .is_err()
                || session
                    .put("openrust.auth.mfa.external_handle", handle.expose())
                    .is_err()
                || session.save().await.is_err()
            {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
            let Ok(session_id) = session.id() else {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            };
            return response_with_session_cookie(StatusCode::ACCEPTED, &session_id);
        }
    };
    #[cfg(not(feature = "auth-mfa"))]
    let openrust::auth::LoginAttempt::Authenticated(credentials) = attempt;
    let user_id = credentials.user_id();
    let access_token = credentials.access_token().to_owned();
    let refresh_token = credentials.refresh_token().expose().to_owned();
    let Ok(session) = Session::new(None, Arc::new(state.session_store.clone()), None) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if session.start().await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if session
        .set_expiry(Some(openrust::session::Expiry::OnInactivity(
            time::Duration::minutes(5),
        )))
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if session.put("openrust.auth.user_id", user_id).is_err() || session.save().await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let Ok(session_id) = session.id() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if state
        .flows
        .attach_session(user_id, &handle, &session)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let mut outcome = state.outcome.lock().await;
    if outcome.is_some() {
        return StatusCode::CONFLICT.into_response();
    }
    *outcome = Some(LoginCallbackOutcome {
        user_id,
        session_id: session_id.clone(),
        access_token,
        refresh_token,
        handle,
    });
    response_with_session_cookie(StatusCode::NO_CONTENT, &session_id)
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
fn response_with_session_cookie(
    status: StatusCode,
    session_id: &openrust::session::SessionId,
) -> Response {
    let Ok(cookie) = HeaderValue::from_str(&format!(
        "openrust_session={session_id}; Path=/; HttpOnly; SameSite=Lax"
    )) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut response = status.into_response();
    response.headers_mut().insert(header::SET_COOKIE, cookie);
    response
}

#[cfg(all(
    any(feature = "db-postgres", feature = "db-mysql"),
    feature = "auth-mfa"
))]
#[derive(serde::Deserialize)]
struct MfaCallbackForm {
    code: String,
}

#[cfg(all(
    any(feature = "db-postgres", feature = "db-mysql"),
    feature = "auth-mfa"
))]
#[allow(
    clippy::too_many_lines,
    reason = "测试 handler 贯通共享 Session、MFA challenge 与外部身份凭据关联"
)]
async fn process_mfa_callback(
    State(state): State<LoginCallbackState>,
    headers: HeaderMap,
    Form(input): Form<MfaCallbackForm>,
) -> Response {
    use openrust::auth::{ExternalLoginHandle, Session};

    let Some(cookie_header) = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Some(session_id) = cookie_header.split(';').find_map(|cookie| {
        let (name, value) = cookie.trim().split_once('=')?;
        (name == "openrust_session")
            .then_some(value.parse().ok())
            .flatten()
    }) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Ok(session) = Session::new(
        Some(session_id),
        Arc::new(state.session_store.clone()),
        None,
    ) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if session.start().await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let (Some(pending_user_id), Some(challenge), Some(exposed_handle)) = (
        session
            .get_typed::<i64>("openrust.auth.mfa.user_id")
            .ok()
            .flatten(),
        session
            .get_typed::<String>("openrust.auth.mfa.challenge")
            .ok()
            .flatten(),
        session
            .get_typed::<String>("openrust.auth.mfa.external_handle")
            .ok()
            .flatten(),
    ) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Ok(handle) = ExternalLoginHandle::from_exposed(exposed_handle) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    state.mfa_gate.wait().await;
    let Ok(credentials) = state
        .auth
        .complete_mfa_login(&challenge, &input.code, "127.0.0.1")
        .await
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let user_id = credentials.user_id();
    if user_id != pending_user_id {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let access_token = credentials.access_token().to_owned();
    let refresh_token = credentials.refresh_token().expose().to_owned();
    if session.migrate(true).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if session
        .set_expiry(Some(openrust::session::Expiry::OnInactivity(
            time::Duration::minutes(5),
        )))
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if session.forget(&["openrust.auth.mfa.user_id"]).is_err()
        || session.forget(&["openrust.auth.mfa.challenge"]).is_err()
        || session
            .forget(&["openrust.auth.mfa.external_handle"])
            .is_err()
        || session.put("openrust.auth.user_id", user_id).is_err()
        || session.save().await.is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let Ok(session_id) = session.id() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if state
        .flows
        .attach_session(user_id, &handle, &session)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let mut outcome = state.outcome.lock().await;
    if outcome.is_some() {
        return StatusCode::CONFLICT.into_response();
    }
    *outcome = Some(LoginCallbackOutcome {
        user_id,
        session_id: session_id.clone(),
        access_token,
        refresh_token,
        handle,
    });
    response_with_session_cookie(StatusCode::NO_CONTENT, &session_id)
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
fn callback_socket(uri: &str) -> std::result::Result<std::net::SocketAddr, Box<dyn StdError>> {
    Ok(url::Url::parse(uri)?
        .socket_addrs(|| None)?
        .into_iter()
        .next()
        .ok_or("callback URI has no socket address")?)
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
fn callback_cookie(
    authorization: &openrust::auth::ExternalIdentityAuthorization,
) -> std::result::Result<String, Box<dyn StdError>> {
    Ok(authorization
        .set_cookie_header()
        .split(';')
        .next()
        .ok_or("authorization cookie has no name/value")?
        .to_owned())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn verify_logout_access_token_window(
    primary: &LogoutApp,
    secondary: &LogoutApp,
) -> std::result::Result<(), Box<dyn StdError>> {
    let apps = [primary, secondary];
    let mut expirations = Vec::with_capacity(apps.len());
    for app in apps {
        let user_id = app.user_id.ok_or("logout app user ID is missing")?;
        let token = app
            .access_token
            .as_deref()
            .ok_or("logout app access token is missing")?;
        assert_eq!(
            app.jwt.verify_access_token(token).ok(),
            Some(user_id),
            "logout must not claim to instantly revoke a stateless access token"
        );
        expirations.push(jwt_expiration(token)?);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let max_remaining = expirations
        .into_iter()
        .max()
        .ok_or("logout apps produced no access token expiry")?
        .saturating_sub(now);
    assert!(
        max_remaining > 0,
        "test access token expired before post-logout residual validation"
    );
    assert!(
        max_remaining <= TEST_ACCESS_TTL_SECS,
        "post-logout JWT residual exceeded its configured maximum"
    );
    tokio::time::sleep(Duration::from_secs(max_remaining + 1)).await;
    for app in apps {
        let token = app
            .access_token
            .as_deref()
            .ok_or("logout app access token is missing")?;
        assert!(
            app.jwt.verify_access_token(token).is_err(),
            "access token must fail verification after its configured expiry"
        );
    }
    Ok(())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
fn jwt_expiration(token: &str) -> std::result::Result<u64, Box<dyn StdError>> {
    let payload = token
        .split('.')
        .nth(1)
        .ok_or("access token has no claims segment")?;
    let claims: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload)?)?;
    claims["exp"]
        .as_u64()
        .ok_or_else(|| "access token expiry claim is missing".into())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn start_logout_apps(
    primary: &mut LogoutApp,
    secondary: &mut LogoutApp,
) -> std::result::Result<
    (
        String,
        mpsc::UnboundedReceiver<CapturedLogout>,
        JoinHandle<()>,
    ),
    Box<dyn StdError>,
> {
    let listener = TcpListener::bind(("0.0.0.0", 0)).await?;
    let port = listener.local_addr()?.port();
    let (sender, receiver) = mpsc::unbounded_channel();
    primary.state.completed = sender.clone();
    secondary.state.completed = sender;
    let defer_secondary = Arc::new(std::sync::atomic::AtomicBool::new(
        primary.api_nodes.is_some() && secondary.api_nodes.is_some(),
    ));
    primary.defer_secondary_logout = Some(defer_secondary.clone());
    if let (Some(primary_nodes), Some(secondary_nodes)) =
        (primary.api_nodes.as_ref(), secondary.api_nodes.as_ref())
    {
        let app = Router::new()
            .route("/primary/backchannel", post(forward_logout_to_api_node))
            .route("/secondary/backchannel", post(forward_logout_to_api_node))
            .with_state(TestBackchannelBalancer {
                primary: primary_nodes.addresses,
                secondary: secondary_nodes.addresses,
                client: reqwest::Client::builder()
                    .no_proxy()
                    .timeout(Duration::from_secs(10))
                    .build()?,
                primary_next: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                secondary_next: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                defer_secondary,
                completed: primary.state.completed.clone(),
            });
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        return Ok((
            format!("http://host.docker.internal:{port}"),
            receiver,
            server,
        ));
    }
    let app = Router::new()
        .route("/primary/backchannel", post(process_logout_token))
        .route("/secondary/backchannel", post(process_logout_token))
        .with_state((primary.state.clone(), secondary.state.clone()));
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((
        format!("http://host.docker.internal:{port}"),
        receiver,
        server,
    ))
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn process_logout_token(
    State((primary, secondary)): State<(LogoutAppState, LogoutAppState)>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    Form(fields): Form<HashMap<String, String>>,
) -> StatusCode {
    let Some(token) = fields.get("logout_token") else {
        return StatusCode::BAD_REQUEST;
    };
    let state = if uri.path().starts_with("/primary/") {
        primary
    } else {
        secondary
    };
    match state
        .flows
        .process_backchannel_logout("keycloak", token, &state.sessions)
        .await
    {
        Ok(outcome) => {
            let _ = state
                .completed
                .send((token.clone(), Some(outcome_label(outcome)), None));
            StatusCode::NO_CONTENT
        }
        Err(_) => StatusCode::UNAUTHORIZED,
    }
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
fn outcome_label(outcome: openrust::auth::BackchannelLogoutOutcome) -> &'static str {
    match outcome {
        openrust::auth::BackchannelLogoutOutcome::Processed(1) => "processed-one",
        openrust::auth::BackchannelLogoutOutcome::Processed(_) => "processed-other",
        openrust::auth::BackchannelLogoutOutcome::AlreadyProcessed => "already-processed",
    }
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
impl LogoutApp {
    async fn active_external_refresh_tokens(
        &self,
    ) -> std::result::Result<Option<u64>, Box<dyn StdError>> {
        use sea_orm::ConnectionTrait;

        let user_id = self.user_id.ok_or("application user ID is missing")?;
        let backend = self.connection.connection()?.get_database_backend();
        let associations = self
            .connection
            .quoted_table_name("auth_external_sessions")?;
        let refresh = self.connection.quoted_table_name("auth_refresh_tokens")?;
        let (lookup, value) = match backend {
            openrust::database::sea_orm::DbBackend::Postgres => (
                format!("SELECT refresh_family_id FROM {associations} WHERE user_id = $1"),
                user_id.into(),
            ),
            openrust::database::sea_orm::DbBackend::MySql => (
                format!("SELECT refresh_family_id FROM {associations} WHERE user_id = ?"),
                user_id.into(),
            ),
            _ => return Err("unsupported database backend for refresh family probe".into()),
        };
        let Some(row) = self
            .connection
            .connection()?
            .query_one_raw(openrust::database::sea_orm::Statement::from_sql_and_values(
                backend,
                lookup,
                [value],
            ))
            .await?
        else {
            return Ok(None);
        };
        let Some(family) = row.try_get::<Option<String>>("", "refresh_family_id")? else {
            return Ok(None);
        };
        let (query, value) = match backend {
            openrust::database::sea_orm::DbBackend::Postgres => (
                format!(
                    "SELECT COUNT(*) AS active_count FROM {refresh} WHERE family_id = $1 AND revoked_at IS NULL"
                ),
                family.into(),
            ),
            openrust::database::sea_orm::DbBackend::MySql => (
                format!(
                    "SELECT COUNT(*) AS active_count FROM {refresh} WHERE family_id = ? AND revoked_at IS NULL"
                ),
                family.into(),
            ),
            _ => return Err("unsupported database backend for refresh count probe".into()),
        };
        let count = self
            .connection
            .connection()?
            .query_one_raw(openrust::database::sea_orm::Statement::from_sql_and_values(
                backend,
                query,
                [value],
            ))
            .await?
            .ok_or("active refresh token count query returned no row")?
            .try_get::<i64>("", "active_count")?;
        Ok(Some(u64::try_from(count)?))
    }

    async fn revoked_associations(&self) -> std::result::Result<u64, Box<dyn StdError>> {
        use sea_orm::ConnectionTrait;
        let table = self
            .connection
            .quoted_table_name("auth_external_sessions")?;
        let count = self
            .connection
            .connection()?
            .query_one_raw(openrust::database::sea_orm::Statement::from_string(
                self.connection.connection()?.get_database_backend(),
                format!("SELECT COUNT(*) AS count FROM {table} WHERE revoked_at IS NOT NULL"),
            ))
            .await?
            .ok_or("association count query returned no row")?
            .try_get::<i64>("", "count")?;
        Ok(u64::try_from(count)?)
    }
}

async fn receive_logout_token(
    State(sender): State<mpsc::UnboundedSender<CapturedLogout>>,
    Form(fields): Form<HashMap<String, String>>,
) -> StatusCode {
    let Some(token) = fields.get("logout_token") else {
        return StatusCode::BAD_REQUEST;
    };
    if sender.send((token.clone(), None, None)).is_err() {
        return StatusCode::SERVICE_UNAVAILABLE;
    }
    StatusCode::NO_CONTENT
}

async fn admin_client(
    browser: &reqwest::Client,
    issuer: &url::Url,
    username: &str,
    password: &str,
) -> std::result::Result<reqwest::Client, Box<dyn StdError>> {
    let token_url = issuer.join("/realms/master/protocol/openid-connect/token")?;
    let token_response: serde_json::Value = browser
        .post(token_url)
        .form(&[
            ("grant_type", "password"),
            ("client_id", "admin-cli"),
            ("username", username),
            ("password", password),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let token = token_response["access_token"]
        .as_str()
        .ok_or("Keycloak admin token response has no access token")?;
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(20))
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))?,
            );
            headers
        })
        .build()?)
}

async fn configure_backchannel_logout(
    admin: &reqwest::Client,
    issuer: &url::Url,
    client_id: &str,
    endpoint: &str,
) -> std::result::Result<(), Box<dyn StdError>> {
    let clients_url = issuer.join(&format!(
        "/admin/realms/openrust/clients?clientId={client_id}"
    ))?;
    let mut clients: Vec<serde_json::Value> = admin
        .get(clients_url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if clients.len() != 1 {
        return Err(format!("expected one Keycloak client for {client_id}").into());
    }
    let mut client = clients.pop().ok_or("Keycloak client list was empty")?;
    let attributes = client["attributes"]
        .as_object_mut()
        .ok_or("Keycloak client attributes were not an object")?;
    attributes.insert(
        "backchannel.logout.url".to_owned(),
        serde_json::Value::String(endpoint.to_owned()),
    );
    attributes.insert(
        "backchannel.logout.session.required".to_owned(),
        serde_json::Value::String("true".to_owned()),
    );
    client["frontchannelLogout"] = serde_json::Value::Bool(false);
    let client_uuid = client["id"].as_str().ok_or("Keycloak client has no UUID")?;
    let response = admin
        .put(issuer.join(&format!("/admin/realms/openrust/clients/{client_uuid}"))?)
        .json(&client)
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await?;
        let body = body
            .replace(CLIENT_SECRET, "[redacted]")
            .replace(SECOND_CLIENT_SECRET, "[redacted]");
        return Err(format!("Keycloak client update failed with {status}: {body}").into());
    }
    Ok(())
}

async fn logout_keycloak_user(
    admin: &reqwest::Client,
    issuer: &url::Url,
    username: &str,
) -> std::result::Result<(), Box<dyn StdError>> {
    let users_url = issuer.join(&format!("/admin/realms/openrust/users?username={username}"))?;
    let users: Vec<serde_json::Value> = admin
        .get(users_url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if users.len() != 1 {
        return Err("expected one Keycloak test user".into());
    }
    let user_id = users[0]["id"].as_str().ok_or("Keycloak user has no ID")?;
    admin
        .post(issuer.join(&format!("/admin/realms/openrust/users/{user_id}/logout"))?)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

fn assert_logout_token_targets(token: &VerifiedLogoutToken, client_id: &str, subject: &str) {
    assert_eq!(token.client_id(), client_id);
    assert_ne!(token.token_id(), "");
    assert!(token.session_id().is_some());
    assert_eq!(token.subject(), Some(subject));
}

fn merge_cookie_headers(existing: &str, updates: &[String]) -> String {
    let mut cookies = BTreeMap::new();
    for item in existing
        .split(';')
        .chain(updates.iter().map(String::as_str))
    {
        if let Some((name, value)) = item.trim().split_once('=') {
            cookies.insert(name.to_owned(), value.to_owned());
        }
    }
    cookies
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn keycloak_login_action(
    html: &str,
    base: &url::Url,
) -> std::result::Result<url::Url, Box<dyn StdError>> {
    let mut remainder = html;
    while let Some(start) = remainder.find("<form") {
        remainder = &remainder[start..];
        let end = remainder.find('>').ok_or("unterminated login form tag")?;
        let tag = &remainder[..=end];
        let id = html_attribute(tag, "id");
        if id.as_deref() == Some("kc-form-login") {
            let action = html_attribute(tag, "action").ok_or("login form has no action")?;
            return Ok(base.join(&decode_html_attribute(&action))?);
        }
        remainder = &remainder[end + 1..];
    }
    Err("Keycloak login page does not contain the kc-form-login form".into())
}

fn keycloak_logout_confirmation(
    html: &str,
    base: &url::Url,
) -> std::result::Result<LogoutConfirmation, Box<dyn StdError>> {
    let mut remainder = html;
    while let Some(start) = remainder.find("<form") {
        remainder = &remainder[start..];
        let tag_end = remainder.find('>').ok_or("unterminated logout form tag")?;
        let tag = &remainder[..=tag_end];
        let action = html_attribute(tag, "action");
        let is_confirmation = html_attribute(tag, "id").as_deref() == Some("kc-logout-confirm")
            || action
                .as_deref()
                .is_some_and(|value| value.contains("/logout/logout-confirm"));
        if is_confirmation {
            let action = action.ok_or("logout form has no action")?;
            let body_end = remainder[tag_end + 1..]
                .find("</form>")
                .ok_or("unterminated logout form")?
                + tag_end
                + 1;
            let body = &remainder[tag_end + 1..body_end];
            let mut fields = Vec::new();
            let mut input_remainder = body;
            while let Some(input_start) = input_remainder.find("<input") {
                input_remainder = &input_remainder[input_start..];
                let input_end = input_remainder
                    .find('>')
                    .ok_or("unterminated logout input")?;
                let input = &input_remainder[..=input_end];
                if let (Some(name), Some(value)) = (
                    html_attribute(input, "name"),
                    html_attribute(input, "value"),
                ) {
                    fields.push((name, decode_html_attribute(&value)));
                }
                input_remainder = &input_remainder[input_end + 1..];
            }
            if fields.is_empty() {
                return Err("Keycloak logout confirmation has no form fields".into());
            }
            return Ok((base.join(&decode_html_attribute(&action))?, fields));
        }
        remainder = &remainder[tag_end + 1..];
    }
    Err("Keycloak logout response has no confirmation form".into())
}

fn html_attribute(tag: &str, name: &str) -> Option<String> {
    let mut rest = tag;
    while let Some(index) = rest.find(name) {
        let candidate = &rest[index + name.len()..];
        let begins_attribute = index == 0
            || rest[..index]
                .chars()
                .last()
                .is_some_and(char::is_whitespace);
        rest = candidate;
        if !begins_attribute {
            continue;
        }
        let candidate = candidate.trim_start();
        let candidate = candidate.strip_prefix('=')?.trim_start();
        let quote = candidate.chars().next()?;
        if !matches!(quote, '\'' | '"') {
            return None;
        }
        let value = &candidate[quote.len_utf8()..];
        let end = value.find(quote)?;
        return Some(value[..end].to_owned());
    }
    None
}

fn decode_html_attribute(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&#x2F;", "/")
        .replace("&#x2f;", "/")
        .replace("&#47;", "/")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

struct TestInstance<T: Send + Sync + 'static>(std::sync::Arc<T>);
impl<T: Send + Sync + 'static> openrust::core::ServiceProvider for TestInstance<T> {
    fn register(&self, app: &mut openrust::core::RegisterContext<'_>) -> openrust::Result<()> {
        app.instance(std::sync::Arc::clone(&self.0));
        Ok(())
    }
}
