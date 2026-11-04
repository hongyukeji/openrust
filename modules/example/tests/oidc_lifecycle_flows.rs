//! 真实 Keycloak 与 RP TCP 的 MFA/退出业务；旧自包含 access JWT 不宣称立即撤销。
#![cfg(all(
    feature = "auth-oidc",
    any(feature = "db-postgres", feature = "db-mysql")
))]

use super::oidc_flows::{
    self, AuthorizationGrant, BASE, BrowserNode, BrowserSession, Idp, LOCAL_PASSWORD,
};
use openrust::http::axum::http::StatusCode;

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
struct Authorization {
    grant: AuthorizationGrant,
    binding: String,
}
async fn authorize(node: &BrowserNode, idp: &Idp) -> TestResult<Authorization> {
    let response = node
        .client
        .get(format!("{}{BASE}/authorize", node.origin))
        .send()
        .await
        .map_err(|_| "RP authorize request failed")?;
    assert!(response.status().is_redirection());
    let location = response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .ok_or("authorization redirect missing")?
        .to_owned();
    let binding = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|value| {
            openrust::http::Cookie::from_header(value).is_ok_and(|cookie| {
                cookie.http_only()
                    && cookie.secure() == Some(true)
                    && cookie.same_site() == Some(openrust::http::SameSite::Lax)
            })
        })
        .and_then(|value| value.split(';').next())
        .ok_or("binding cookie missing")?
        .to_owned();
    let grant = oidc_flows::authorization_grant(idp, &location).await?;
    Ok(Authorization { grant, binding })
}
async fn complete(
    node: &BrowserNode,
    authorization: &Authorization,
) -> TestResult<serde_json::Value> {
    let response = node
        .client
        .get(format!(
            "{}{BASE}{}",
            node.origin,
            oidc_flows::callback_path(&authorization.grant.state, &authorization.grant.code)
        ))
        .header("cookie", &authorization.binding)
        .send()
        .await
        .map_err(|_| "RP callback request failed")?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_private(&response);
    json(response).await
}
fn assert_private(response: &reqwest::Response) {
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    assert_eq!(
        response
            .headers()
            .get("referrer-policy")
            .and_then(|value| value.to_str().ok()),
        Some("no-referrer")
    );
}
async fn json(mut response: reqwest::Response) -> TestResult<serde_json::Value> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "RP body read failed")? {
        if bytes.len() + chunk.len() > 32 * 1024 {
            return Err("RP response exceeded test budget".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "RP JSON response invalid".into())
}
fn text(value: &serde_json::Value, key: &str) -> TestResult<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "expected lifecycle response field missing".into())
}
async fn post(
    node: &BrowserNode,
    path: &str,
    token: Option<&str>,
    body: &serde_json::Value,
) -> TestResult<reqwest::Response> {
    let mut request = node
        .client
        .post(format!("{}{path}", node.origin))
        .header("origin", &node.origin)
        .header("x-example-request", "same-origin")
        .json(body);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    request
        .send()
        .await
        .map_err(|_| "RP lifecycle request failed".into())
}
async fn link_local(node: &BrowserNode, idp: &Idp, email: &str) -> TestResult<String> {
    let token = node.fixture.local_account(email).await?;
    let authorization = authorize(node, idp).await?;
    let response=node.client.post(format!("{}{BASE}/bind",node.origin)).bearer_auth(&token).header("cookie",&authorization.binding)
        .json(&serde_json::json!({"email":email,"password":LOCAL_PASSWORD,"state":authorization.grant.state,"code":authorization.grant.code}))
        .send().await.map_err(|_|"native fixture binding failed")?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(token)
}
async fn logout(node: &BrowserNode, token: &str, handle: &str) -> TestResult<serde_json::Value> {
    let response = post(
        node,
        &format!("{BASE}/logout"),
        Some(token),
        &serde_json::json!({"handle":handle}),
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_private(&response);
    json(response).await
}
async fn assert_refresh_denied(node: &BrowserNode, refresh: &str) -> TestResult {
    let response = post(
        node,
        "/api/example/auth/refresh",
        None,
        &serde_json::json!({"refresh_token":refresh}),
    )
    .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}
async fn session_attach(
    node: &BrowserNode,
    session: &BrowserSession,
    handle: &str,
    password: &str,
) -> TestResult<reqwest::Response> {
    node.client
        .post(format!("{}{BASE}/session/attach", node.origin))
        .header("cookie", &session.cookie)
        .header("x-csrf-token", &session.csrf)
        .header("origin", &node.origin)
        .header("x-example-request", "same-origin")
        .json(&serde_json::json!({"handle":handle,"email":session.email,"password":password}))
        .send()
        .await
        .map_err(|_| "Session association request failed".into())
}
async fn session_status(node: &BrowserNode, session: &BrowserSession) -> TestResult<StatusCode> {
    let response = node
        .client
        .get(format!(
            "{}/api/example/account/session/preference",
            node.origin
        ))
        .header("cookie", &session.cookie)
        .send()
        .await
        .map_err(|_| "Session status request failed")?;
    Ok(response.status())
}

#[cfg(feature = "auth-mfa")]
async fn enable_factor(
    node: &BrowserNode,
    local: &str,
    email: &str,
) -> TestResult<(String, String)> {
    let enrollment = post(
        node,
        "/api/example/account/mfa/enroll",
        Some(local),
        &serde_json::json!({"email":email,"password":LOCAL_PASSWORD}),
    )
    .await?;
    assert_eq!(enrollment.status(), StatusCode::OK);
    let enrollment = json(enrollment).await?;
    let uri = text(&enrollment["data"], "provisioning_uri")?;
    let code = super::account_security_flows::authenticator_code(&uri)?;
    let confirmed = post(
        node,
        "/api/example/account/mfa/confirm",
        Some(local),
        &serde_json::json!({"email":email,"password":LOCAL_PASSWORD,"code":code}),
    )
    .await?;
    assert_eq!(confirmed.status(), StatusCode::OK);
    let confirmed = json(confirmed).await?;
    let recovery = confirmed["data"]["recovery_codes"]
        .as_array()
        .ok_or("recovery codes missing")?;
    Ok((
        recovery[0]
            .as_str()
            .ok_or("recovery code missing")?
            .to_owned(),
        recovery[1]
            .as_str()
            .ok_or("recovery code missing")?
            .to_owned(),
    ))
}

#[cfg(feature = "auth-mfa")]
#[tokio::test]
async fn real_oidc_mfa_gates_credentials_once_and_logout_revokes_associated_refresh() -> TestResult
{
    let Some(idp) = Idp::environment()? else {
        return Ok(());
    };
    let Some(node) = BrowserNode::start(&idp).await? else {
        return Ok(());
    };
    let email = "oidc-mfa-lifecycle@example.test";
    let local = link_local(&node, &idp, email).await?;
    let (first, second) = enable_factor(&node, &local, email).await?;
    let authorization = authorize(&node, &idp).await?;
    let response = complete(&node, &authorization).await?;
    assert_eq!(response["data"]["status"], "mfa_required");
    assert!(response["data"].get("credentials").is_none());
    let handle = text(&response["data"], "external_login_handle")?;
    let challenge = text(&response["data"], "challenge")?;
    let wrong = post(
        &node,
        "/api/example/account/login/mfa",
        None,
        &serde_json::json!({"challenge":challenge,"code":"not-an-authenticator-code"}),
    )
    .await?;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    let completed = post(
        &node,
        "/api/example/account/login/mfa",
        None,
        &serde_json::json!({"challenge":challenge,"code":first}),
    )
    .await?;
    assert_eq!(completed.status(), StatusCode::OK);
    assert_eq!(
        completed
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    let credentials = json(completed).await?;
    let access = text(&credentials["data"], "access_token")?;
    let refresh = text(&credentials["data"], "refresh_token")?;
    let replay = post(
        &node,
        "/api/example/account/login/mfa",
        None,
        &serde_json::json!({"challenge":challenge,"code":first}),
    )
    .await?;
    assert!(!replay.status().is_success());
    // 尚未完成的另一 OIDC challenge 也必须能被本地退出撤销，不能在退出后再签发凭据。
    let pending = authorize(&node, &idp).await?;
    let pending_response = complete(&node, &pending).await?;
    let pending_handle = text(&pending_response["data"], "external_login_handle")?;
    let pending_challenge = text(&pending_response["data"], "challenge")?;
    logout(&node, &access, &pending_handle).await?;
    let revoked = post(
        &node,
        "/api/example/account/login/mfa",
        None,
        &serde_json::json!({"challenge":pending_challenge,"code":second}),
    )
    .await?;
    assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
    logout(&node, &access, &handle).await?;
    assert_refresh_denied(&node, &refresh).await?;
    // 当前 JWT 无逐请求撤销表检查；关联 refresh 的退出不被伪称为旧 access JWT 瞬时撤销。
    let jwt = node
        .client
        .get(format!("{}{BASE}/links", node.origin))
        .bearer_auth(&access)
        .send()
        .await
        .map_err(|_| "old JWT boundary request failed")?;
    assert_eq!(jwt.status(), StatusCode::OK);
    node.close().await
}

#[tokio::test]
async fn rp_logout_revokes_attached_session_but_not_independent_session_and_consumes_state_once()
-> TestResult {
    let Some(idp) = Idp::environment()? else {
        return Ok(());
    };
    let Some(node) = BrowserNode::start(&idp).await? else {
        return Ok(());
    };
    let email = "oidc-session-lifecycle@example.test";
    link_local(&node, &idp, email).await?;
    let attached = node.existing_session(email).await?;
    let independent = node.existing_session(email).await?;
    let authorization = authorize(&node, &idp).await?;
    let response = complete(&node, &authorization).await?;
    assert_eq!(response["data"]["status"], "authenticated");
    let handle = text(&response["data"], "external_login_handle")?;
    let access = text(&response["data"]["credentials"], "access_token")?;
    let refresh = text(&response["data"]["credentials"], "refresh_token")?;
    let wrong = session_attach(&node, &attached, &handle, "wrong password").await?;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    let linked = session_attach(&node, &attached, &handle, LOCAL_PASSWORD).await?;
    assert_eq!(linked.status(), StatusCode::OK);
    assert_eq!(session_status(&node, &attached).await?, StatusCode::OK);
    let exited = logout(&node, &access, &handle).await?;
    assert_refresh_denied(&node, &refresh).await?;
    assert_eq!(
        session_status(&node, &attached).await?,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(session_status(&node, &independent).await?, StatusCode::OK);
    let logout_url = text(&exited["data"], "end_session_url")?;
    let state = drive_real_end_session(&idp, &authorization.grant, &logout_url).await?;
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("state", &state)
        .finish();
    let url = format!("{}{BASE}/callback?{query}", node.origin);
    let completed = node
        .client
        .get(&url)
        .send()
        .await
        .map_err(|_| "logout callback failed")?;
    assert_eq!(completed.status(), StatusCode::OK);
    assert_private(&completed);
    assert!(
        completed.headers().get("location").is_none(),
        "logout callback must not redirect to itself"
    );
    let body = json(completed).await?;
    assert_eq!(body["data"]["status"], "logged_out");
    let replay = node
        .client
        .get(&url)
        .send()
        .await
        .map_err(|_| "logout replay request failed")?;
    assert!(!replay.status().is_success());
    let wrong = node
        .client
        .get(format!(
            "{}{BASE}/callback?state=unregistered-logout-state",
            node.origin
        ))
        .send()
        .await
        .map_err(|_| "wrong logout state request failed")?;
    assert!(!wrong.status().is_success());
    node.close().await
}

async fn drive_real_end_session(
    idp: &Idp,
    grant: &AuthorizationGrant,
    target: &str,
) -> TestResult<String> {
    let target = url::Url::parse(target).map_err(|_| "logout URL invalid")?;
    let issuer = url::Url::parse(&idp.config.issuer).map_err(|_| "issuer URL invalid")?;
    if target.origin() != issuer.origin() {
        return Err("logout endpoint escaped configured issuer origin".into());
    }
    let mut response = grant
        .browser
        .get(target.clone())
        .header("cookie", &grant.cookies)
        .send()
        .await
        .map_err(|_| "real IdP end-session request failed")?;
    if response.status().is_success() {
        let cookies = merge_cookies(&grant.cookies, &response);
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "logout page body failed")?
        {
            if bytes.len() + chunk.len() > 128 * 1024 {
                return Err("logout page exceeded budget".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let html = String::from_utf8(bytes).map_err(|_| "logout page encoding invalid")?;
        let (action, fields) = logout_confirmation(&html, &target)?;
        if action.origin() != issuer.origin() {
            return Err("logout confirmation escaped issuer origin".into());
        }
        response = grant
            .browser
            .post(action)
            .header("cookie", cookies)
            .form(&fields)
            .send()
            .await
            .map_err(|_| "real IdP logout confirmation failed")?;
    }
    if !response.status().is_redirection() {
        return Err("IdP logout did not return the fixed callback redirect".into());
    }
    let location = response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .ok_or("IdP logout redirect missing")?;
    let destination = url::Url::parse(location).map_err(|_| "logout redirect invalid")?;
    let configured = url::Url::parse(&idp.config.redirect_uri)
        .map_err(|_| "configured logout redirect invalid")?;
    if destination.origin() != configured.origin() || destination.path() != configured.path() {
        return Err("IdP logout returned a different callback".into());
    }
    destination
        .query_pairs()
        .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
        .ok_or_else(|| "IdP logout state missing".into())
}
fn merge_cookies(existing: &str, response: &reqwest::Response) -> String {
    let mut cookies = std::collections::BTreeMap::new();
    for pair in existing.split("; ").chain(
        response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok()?.split(';').next()),
    ) {
        if let Some((name, value)) = pair.split_once('=') {
            cookies.insert(name, value);
        }
    }
    cookies
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}
fn html_value(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}
fn logout_confirmation(
    html: &str,
    base: &url::Url,
) -> TestResult<(url::Url, Vec<(String, String)>)> {
    // 仅测试浏览器驱动，沿已有 Keycloak fixture 的 form/input 解析；协议验证仍由框架提供。
    for form in html.split("<form").skip(1) {
        let (tag, body) = form.split_once('>').ok_or("logout form malformed")?;
        let action = oidc_flows::attribute(tag, "action");
        if oidc_flows::attribute(tag, "id").as_deref() != Some("kc-logout-confirm")
            && !action
                .as_deref()
                .is_some_and(|action| action.contains("/logout/logout-confirm"))
        {
            continue;
        }
        let action = base
            .join(&html_value(&action.ok_or("logout form action missing")?))
            .map_err(|_| "logout action invalid")?;
        let body = body
            .split("</form>")
            .next()
            .ok_or("logout form body missing")?;
        let fields = body
            .split("<input")
            .skip(1)
            .filter_map(|input| {
                let tag = input.split('>').next()?;
                Some((
                    oidc_flows::attribute(tag, "name")?,
                    html_value(&oidc_flows::attribute(tag, "value")?),
                ))
            })
            .collect::<Vec<_>>();
        if fields.is_empty() {
            return Err("logout confirmation fields missing".into());
        }
        return Ok((action, fields));
    }
    Err("IdP logout confirmation form missing".into())
}

#[tokio::test]
async fn signed_backchannel_endpoint_rejects_forged_signature_without_revoking_the_login()
-> TestResult {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    let Some(idp) = Idp::environment()? else {
        return Ok(());
    };
    let Some(node) = BrowserNode::start(&idp).await? else {
        return Ok(());
    };
    let email = "oidc-forged-backchannel@example.test";
    let local = link_local(&node, &idp, email).await?;
    let attached = node.existing_session(email).await?;
    let authorization = authorize(&node, &idp).await?;
    let authenticated = complete(&node, &authorization).await?;
    let refresh = text(&authenticated["data"]["credentials"], "refresh_token")?;
    let handle = text(&authenticated["data"], "external_login_handle")?;
    let linked = session_attach(&node, &attached, &handle, LOCAL_PASSWORD).await?;
    assert_eq!(linked.status(), StatusCode::OK);
    assert_eq!(session_status(&node, &attached).await?, StatusCode::OK);
    let links = node
        .client
        .get(format!("{}{BASE}/links", node.origin))
        .bearer_auth(&local)
        .send()
        .await
        .map_err(|_| "linked identity request failed")?;
    assert_eq!(links.status(), StatusCode::OK);
    let links = json(links).await?;
    let subject = links["data"]
        .as_array()
        .and_then(|links| {
            links.iter().find(|link| {
                link["provider"] == "example-oidc" && link["issuer"] == idp.config.issuer
            })
        })
        .and_then(|link| link["subject"].as_str())
        .filter(|subject| !subject.is_empty())
        .ok_or("real linked external subject missing")?;
    // 先证明目标 refresh 家族有效；旋转仍属于同一个已关联的外部登录家族。
    let before = post(
        &node,
        "/api/example/auth/refresh",
        None,
        &serde_json::json!({"refresh_token":refresh}),
    )
    .await?;
    assert_eq!(before.status(), StatusCode::OK);
    let before = json(before).await?;
    let refresh = text(&before["data"], "refresh_token")?;
    let certs = authorization
        .grant
        .browser
        .get(format!(
            "{}/protocol/openid-connect/certs",
            idp.config.issuer
        ))
        .send()
        .await
        .map_err(|_| "controlled issuer public key request failed")?;
    assert_eq!(certs.status(), StatusCode::OK);
    let certs = json(certs).await?;
    let kid = certs["keys"]
        .as_array()
        .and_then(|keys| keys.iter().find(|key| key["alg"] == "RS256"))
        .and_then(|key| key["kid"].as_str())
        .ok_or("controlled issuer RS256 key missing")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(
        &serde_json::json!({"alg":"RS256","typ":"logout+jwt","kid":kid}),
    )?);
    let payload=URL_SAFE_NO_PAD.encode(serde_json::to_vec(&serde_json::json!({
        "iss":idp.config.issuer,"aud":idp.config.client_id,"iat":now,"exp":now+60,"jti":"untrusted-signature-test","sub":subject,
        "events":{"http://schemas.openid.net/event/backchannel-logout":{}}
    }))?);
    // 使用真实公钥 kid 和正确形状，但没有 IdP 私钥；不得把算法或未知 kid 拒绝冒充签名校验。
    let forged = format!("{header}.{payload}.{}", URL_SAFE_NO_PAD.encode([0_u8; 256]));
    let response = node
        .client
        .post(format!("{}{BASE}/backchannel", node.origin))
        .form(&[("logout_token", forged.as_str())])
        .send()
        .await
        .map_err(|_| "forged backchannel request failed")?;
    // Error::custom 的默认状态为 400；精确错误码可排除路由 404 和其他早期失败。
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_private(&response);
    let rejected = json(response).await?;
    assert_eq!(rejected["code"], "auth_oidc_failed");
    assert_eq!(session_status(&node, &attached).await?, StatusCode::OK);
    let refreshed = post(
        &node,
        "/api/example/auth/refresh",
        None,
        &serde_json::json!({"refresh_token":refresh}),
    )
    .await?;
    assert_eq!(
        refreshed.status(),
        StatusCode::OK,
        "unverified logout must not revoke the valid family"
    );
    node.close().await
}

// 固定端口仅供本任务受控 Keycloak 容器访问；测试必须串行运行，不修改 IdP 配置。
struct BackchannelCapture {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    server: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
    received: tokio::sync::mpsc::Receiver<CapturedLogout>,
}
struct CapturedLogout {
    token: String,
    status: StatusCode,
    outcome: String,
    revoked: Option<u64>,
}
impl BackchannelCapture {
    async fn start(origin: &str) -> TestResult<Self> {
        Self::listen(18768, Some(format!("{origin}{BASE}/backchannel"))).await
    }
    async fn listen(port: u16, target: Option<String>) -> TestResult<Self> {
        use openrust::http::axum::{Router, body::to_bytes, extract::Request, routing::post};
        let listener =
            tokio::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port)).await?;
        let (send, received) = tokio::sync::mpsc::channel(8);
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        let router = Router::new().route(
            &format!("{BASE}/backchannel"),
            post(move |request: Request| {
                let send = send.clone();
                let client = client.clone();
                let target = target.clone();
                async move {
                    eprintln!("backchannel_capture request_received");
                    let result = async {
                        let bytes = tokio::time::timeout(
                            std::time::Duration::from_secs(5),
                            to_bytes(request.into_body(), 16 * 1024),
                        )
                        .await
                        .map_err(|_| ())?
                        .map_err(|_| ())?;
                        let fields = url::form_urlencoded::parse(&bytes).collect::<Vec<_>>();
                        if fields.len() != 1 || fields[0].0 != "logout_token" {
                            return Err(());
                        }
                        let token = fields[0].1.to_string();
                        let (status, outcome, revoked) = if let Some(target) = target {
                            let response = client
                                .post(target)
                                .form(&[("logout_token", token.as_str())])
                                .send()
                                .await
                                .map_err(|_| ())?;
                            let status = response.status();
                            let body = json(response).await.map_err(|_| ())?;
                            let outcome = body["data"]["status"]
                                .as_str()
                                .unwrap_or("missing")
                                .to_owned();
                            (status, outcome, body["data"]["revoked"].as_u64())
                        } else {
                            (StatusCode::OK, "captured".to_owned(), None)
                        };
                        // token 只存进当前测试内存；不落盘、不进入错误或 Debug 输出。
                        send.try_send(CapturedLogout {
                            token,
                            status,
                            outcome,
                            revoked,
                        })
                        .map_err(|_| ())?;
                        Ok::<_, ()>(status)
                    }
                    .await;
                    if result.is_err() {
                        eprintln!("backchannel_capture rejected_or_proxy_failed");
                    }
                    result.unwrap_or(StatusCode::BAD_GATEWAY)
                }
            }),
        );
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            openrust::http::axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
        });
        Ok(Self {
            stop: Some(stop),
            server: Some(server),
            received,
        })
    }
    async fn receive(&mut self) -> TestResult<CapturedLogout> {
        tokio::time::timeout(std::time::Duration::from_secs(15), self.received.recv())
            .await
            .map_err(|_| "real IdP backchannel delivery exceeded deadline")?
            .ok_or_else(|| "real IdP backchannel capture closed".into())
    }
    async fn close(&mut self) -> TestResult {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(mut server) = self.server.take() {
            if let Ok(result) =
                tokio::time::timeout(std::time::Duration::from_secs(10), &mut server).await
            {
                result??;
            } else {
                server.abort();
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), server).await;
                return Err("backchannel capture shutdown exceeded deadline".into());
            }
        }
        Ok(())
    }
}
impl Drop for BackchannelCapture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = &self.server {
            server.abort();
        }
    }
}

#[tokio::test]
async fn real_idp_signed_backchannel_revokes_only_associated_credentials_and_replay_is_idempotent()
-> TestResult {
    let Some(idp) = Idp::environment()? else {
        return Ok(());
    };
    let trigger = match std::env::var("OPENRUST_TEST_OIDC_ADMIN_TRIGGER_URL") {
        Ok(value) => value,
        Err(_) if std::env::var("OPENRUST_REQUIRE_OIDC_IDP").as_deref() == Ok("1") => {
            return Err("controlled IdP logout trigger missing".into());
        }
        Err(_) => return Ok(()),
    };
    // 此受控入口不携带管理员凭据；测试不能用任意 URL 触发外部管理动作。
    if trigger != "http://127.0.0.1:18769/logout" {
        return Err("controlled IdP logout trigger is not the local fixture".into());
    }
    let Some(node) = BrowserNode::start(&idp).await? else {
        return Ok(());
    };
    let mut capture = BackchannelCapture::start(&node.origin).await?;
    let result = async {
        let email = "oidc-real-backchannel@example.test";
        link_local(&node, &idp, email).await?;
        let attached = node.existing_session(email).await?;
        let independent = node.existing_session(email).await?;
        let independent_login = post(
            &node,
            "/api/example/auth/login",
            None,
            &serde_json::json!({"email":email,"password":LOCAL_PASSWORD}),
        )
        .await?;
        assert_eq!(independent_login.status(), StatusCode::OK);
        let independent_login = json(independent_login).await?;
        let independent_refresh = text(&independent_login["data"], "refresh_token")?;
        let authorization = authorize(&node, &idp).await?;
        let authenticated = complete(&node, &authorization).await?;
        let handle = text(&authenticated["data"], "external_login_handle")?;
        let refresh = text(&authenticated["data"]["credentials"], "refresh_token")?;
        let linked = session_attach(&node, &attached, &handle, LOCAL_PASSWORD).await?;
        assert_eq!(linked.status(), StatusCode::OK);
        assert_eq!(session_status(&node, &attached).await?, StatusCode::OK);
        assert_eq!(session_status(&node, &independent).await?, StatusCode::OK);
        let triggered = node
            .client
            .post(&trigger)
            .send()
            .await
            .map_err(|_| "controlled IdP logout trigger failed")?;
        assert_eq!(triggered.status(), StatusCode::NO_CONTENT);
        // bind 和后续登录会产生独立的 IdP sid；管理退出可发送多个通知，不能把
        // 首个（可能只对应 bind）处理成功误当成目标登录已撤销。
        let delivered = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            loop {
                let delivered = capture.receive().await?;
                assert_eq!(delivered.status, StatusCode::OK);
                assert_eq!(delivered.outcome, "processed");
                if delivered.revoked == Some(1)
                    && session_status(&node, &attached).await? == StatusCode::UNAUTHORIZED
                {
                    break Ok::<CapturedLogout, Box<dyn std::error::Error>>(delivered);
                }
            }
        })
        .await
        .map_err(|_| "target external association was not revoked before deadline")??;
        // 只选择实际撤销目标关联的通知；并发送达的其他 sid 可返回零。
        // 此测试仅创建一个有 handle 的外部登录关联；独立本地登录不应计入。
        assert_eq!(delivered.revoked, Some(1));
        assert_refresh_denied(&node, &refresh).await?;
        assert_eq!(
            session_status(&node, &attached).await?,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(session_status(&node, &independent).await?, StatusCode::OK);
        let unrelated = post(
            &node,
            "/api/example/auth/refresh",
            None,
            &serde_json::json!({"refresh_token":independent_refresh}),
        )
        .await?;
        assert_eq!(unrelated.status(), StatusCode::OK);
        let replay = node
            .client
            .post(format!("{}{BASE}/backchannel", node.origin))
            .form(&[("logout_token", delivered.token.as_str())])
            .send()
            .await
            .map_err(|_| "real signed logout replay request failed")?;
        assert_eq!(replay.status(), StatusCode::OK);
        assert_private(&replay);
        let replay = json(replay).await?;
        assert_eq!(replay["data"]["status"], "already_processed");
        assert_eq!(replay["data"]["revoked"], 0);
        assert_eq!(session_status(&node, &independent).await?, StatusCode::OK);
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    let capture_closed = capture.close().await;
    let node_closed = node.close().await;
    result?;
    capture_closed?;
    node_closed
}

async fn verifier(idp: &Idp) -> TestResult<openrust::auth::OidcProvider> {
    use openrust::http::{DependencyPolicy, HttpClient, HttpClientConfig, HttpTargetPolicy};
    let issuer = url::Url::parse(&idp.config.issuer).map_err(|_| "fixture issuer invalid")?;
    let host = issuer.host_str().ok_or("fixture issuer host missing")?;
    let address: std::net::IpAddr = host
        .parse()
        .map_err(|_| "fixture issuer must be loopback")?;
    if !address.is_loopback() {
        return Err("fixture issuer must be loopback".into());
    }
    let port = issuer
        .port_or_known_default()
        .ok_or("fixture issuer port missing")?;
    let network = if address.is_ipv4() {
        "127.0.0.0/8".parse()?
    } else {
        "::1/128".parse()?
    };
    let mut policy = HttpTargetPolicy::new();
    policy.allow_internal(host, network, [port])?;
    let http = HttpClient::restricted(
        HttpClientConfig {
            connect_timeout: std::time::Duration::from_secs(5),
            timeout: std::time::Duration::from_secs(15),
            max_response_bytes: 1024 * 1024,
            proxy: None,
        },
        policy,
    )?;
    let dependency = http.dependency(
        "example-logout-audience-test",
        DependencyPolicy {
            max_in_flight: 2,
            max_waiting: 2,
            circuit_breaker: None,
        },
    )?;
    // discovery/验签无需 token endpoint 的 client secret，不能把 primary secret 发给 secondary。
    Ok(openrust::auth::OidcProvider::discover(
        &idp.config.issuer,
        "example-oidc",
        idp.config.client_id.clone(),
        None,
        &idp.config.redirect_uri,
        ["profile".to_owned(), "email".to_owned()],
        std::time::Duration::from_secs(30),
        dependency,
    )
    .await?)
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // 完整保留次级签名、错误受众与主凭据不可撤销的关联证据。
async fn real_secondary_signature_with_wrong_audience_cannot_revoke_primary_credentials()
-> TestResult {
    use openrust::auth::ExternalIdentityProvider;
    let Some(idp) = Idp::environment()? else {
        return Ok(());
    };
    let required = std::env::var("OPENRUST_REQUIRE_OIDC_IDP").as_deref() == Ok("1");
    let settings = [
        "OPENRUST_TEST_OIDC_SECONDARY_CLIENT_ID",
        "OPENRUST_TEST_OIDC_SECONDARY_REDIRECT_URI",
        "OPENRUST_TEST_OIDC_ADMIN_TRIGGER_URL",
    ]
    .into_iter()
    .map(std::env::var)
    .collect::<std::result::Result<Vec<_>, _>>();
    let Ok(settings) = settings else {
        return if required {
            Err("controlled secondary OIDC fixture missing".into())
        } else {
            Ok(())
        };
    };
    if settings[0] != "openrust-rp-secondary"
        || settings[1] != "http://127.0.0.1:3001/callback"
        || settings[2] != "http://127.0.0.1:18769/logout"
    {
        return Err("secondary OIDC fixture does not match task-owned endpoints".into());
    }
    let secondary = idp.for_client(&settings[0], &settings[1]);
    let secondary_provider = verifier(&secondary).await?;
    let primary_provider = verifier(&idp).await?;
    let mut capture = BackchannelCapture::listen(18770, None).await?;
    // 先让 secondary 创建真实 IdP 会话并退出，再创建 primary，避免管理退出先撤销目标。
    let captured = async {
        let authorization = secondary_provider.authorize()?;
        let _grant =
            oidc_flows::authorization_grant(&secondary, authorization.url().as_str()).await?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        let trigger = client
            .post(&settings[2])
            .send()
            .await
            .map_err(|_| "secondary controlled logout trigger failed")?;
        assert_eq!(trigger.status(), StatusCode::NO_CONTENT);
        let delivered = capture.receive().await?;
        assert_eq!(delivered.status, StatusCode::OK);
        let verified = secondary_provider
            .verify_logout_token(&delivered.token)
            .await
            .map_err(|_| "real secondary logout signature was not accepted")?;
        assert_eq!(verified.client_id(), settings[0]);
        let subject = verified
            .subject()
            .filter(|subject| !subject.is_empty())
            .ok_or("secondary signed logout subject missing")?
            .to_owned();
        let rejected = primary_provider.verify_logout_token(&delivered.token).await;
        match rejected {
            Ok(_) => return Err("secondary logout token unexpectedly accepted by primary".into()),
            Err(error) => assert_eq!(error.code(), "auth_oidc_failed"),
        }
        Ok::<_, Box<dyn std::error::Error>>((delivered.token, subject))
    }
    .await;
    let capture_closed = capture.close().await;
    let (token, secondary_subject) = captured?;
    capture_closed?;
    let Some(node) = BrowserNode::start(&idp).await? else {
        return Ok(());
    };
    let result = async {
        let email = "oidc-wrong-audience@example.test";
        let local = link_local(&node, &idp, email).await?;
        let links = node
            .client
            .get(format!("{}{BASE}/links", node.origin))
            .bearer_auth(&local)
            .send()
            .await
            .map_err(|_| "primary linked identity request failed")?;
        assert_eq!(links.status(), StatusCode::OK);
        let links = json(links).await?;
        let same_subject = links["data"].as_array().is_some_and(|links| {
            links.iter().any(|link| {
                link["provider"] == "example-oidc"
                    && link["issuer"] == idp.config.issuer
                    && link["subject"].as_str() == Some(secondary_subject.as_str())
            })
        });
        assert!(
            same_subject,
            "secondary token must target the actual linked subject"
        );
        let attached = node.existing_session(email).await?;
        let authorization = authorize(&node, &idp).await?;
        let authenticated = complete(&node, &authorization).await?;
        let handle = text(&authenticated["data"], "external_login_handle")?;
        let refresh = text(&authenticated["data"]["credentials"], "refresh_token")?;
        let attached_response = session_attach(&node, &attached, &handle, LOCAL_PASSWORD).await?;
        assert_eq!(attached_response.status(), StatusCode::OK);
        assert_eq!(session_status(&node, &attached).await?, StatusCode::OK);
        let before = post(
            &node,
            "/api/example/auth/refresh",
            None,
            &serde_json::json!({"refresh_token":refresh}),
        )
        .await?;
        assert_eq!(before.status(), StatusCode::OK);
        let before = json(before).await?;
        let refresh = text(&before["data"], "refresh_token")?;
        // 临近生产入口再次核验有效签名与期限，避免长时间建立本地账户后只因过期而拒绝。
        secondary_provider
            .verify_logout_token(&token)
            .await
            .map_err(|_| "secondary token expired before primary audience check")?;
        match primary_provider.verify_logout_token(&token).await {
            Ok(_) => return Err("primary accepted the secondary audience".into()),
            Err(error) => assert_eq!(error.code(), "auth_oidc_failed"),
        }
        let rejected = node
            .client
            .post(format!("{}{BASE}/backchannel", node.origin))
            .form(&[("logout_token", token.as_str())])
            .send()
            .await
            .map_err(|_| "wrong-audience backchannel request failed")?;
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
        assert_private(&rejected);
        let rejected = json(rejected).await?;
        assert_eq!(rejected["code"], "auth_oidc_failed");
        secondary_provider
            .verify_logout_token(&token)
            .await
            .map_err(|_| "secondary token expired during primary audience check")?;
        assert_eq!(session_status(&node, &attached).await?, StatusCode::OK);
        let after = post(
            &node,
            "/api/example/auth/refresh",
            None,
            &serde_json::json!({"refresh_token":refresh}),
        )
        .await?;
        assert_eq!(after.status(), StatusCode::OK);
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    let closed = node.close().await;
    result?;
    closed
}
