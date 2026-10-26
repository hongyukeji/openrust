//! 使用真实 Keycloak authorization-code 流程验证 OIDC relying-party。
#![cfg(feature = "auth-oidc")]

use std::{error::Error as StdError, time::Duration};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use openrust::auth::OidcProvider;
use openrust::http::{DependencyPolicy, HttpClient, HttpClientConfig, HttpTargetPolicy};
use sha2::{Digest, Sha256};

type TestResult = std::result::Result<(), Box<dyn StdError>>;

const CLIENT_ID: &str = "openrust-rp";
const CLIENT_SECRET: &str = "openrust-test-client-secret";
const REDIRECT_URI: &str = "http://127.0.0.1:3000/callback";
const TEST_USERNAME: &str = "openrust-test-user";
const TEST_PASSWORD: &str = "openrust-test-password";

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "真实 IdP 验收必须在单一浏览器授权码流程中检查 PKCE 与 ID Token"
)]
async fn keycloak_authorization_code_pkce_and_id_token_validation() -> TestResult {
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
    let provider = OidcProvider::discover(
        &issuer,
        "keycloak",
        CLIENT_ID,
        Some(CLIENT_SECRET.to_owned()),
        REDIRECT_URI,
        ["profile".to_owned(), "email".to_owned()],
        Duration::from_secs(45),
        dependency,
    )
    .await?;

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
        .timeout(Duration::from_secs(20))
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
        .header(reqwest::header::COOKIE, cookie)
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
    Ok(())
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
