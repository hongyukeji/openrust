/// 直接等待有界 future；外层捕获业务 panic，超时取消不会遗留分离任务。
async fn run_bounded_mfa_flow(address: SocketAddr, mail: &CaptureMailer) -> TestResult {
    openrust::tokio::time::timeout(
        std::time::Duration::from_secs(60),
        run_mfa_flow(address, mail),
    )
    .await?
}

/// 仅由 --auth-mfa 生成；所有账户状态转换均经过真实 HTTP 路由。
async fn run_mfa_flow(address: SocketAddr, mail: &CaptureMailer) -> TestResult {
    let email = format!("mfa-{}@example.test", std::process::id());
    let credentials = serde_json::json!({"email": email, "password": "mfa-test-password"});
    let registered = mfa_post(
        address,
        "/api/auth/register",
        credentials.clone(),
        None,
        "203.0.113.81",
    )
    .await?;
    mfa_success(&registered);
    let verified = mfa_post(
        address,
        "/api/auth/verify-email",
        serde_json::json!({"email":email,"code":mail.latest_code()?}),
        None,
        "203.0.113.81",
    )
    .await?;
    mfa_success(&verified);
    let initial = mfa_post(
        address,
        "/api/auth/login",
        credentials.clone(),
        None,
        "203.0.113.81",
    )
    .await?;
    mfa_success(&initial);
    assert_eq!(initial.body["data"]["status"], "authenticated");
    let token = initial.body["data"]["credentials"]["access_token"]
        .as_str()
        .ok_or("missing initial token")?;
    let initial_refresh = initial.body["data"]["credentials"]["refresh_token"]
        .as_str()
        .ok_or("missing initial refresh")?;
    let user_id = initial.body["data"]["credentials"]["user_id"]
        .as_i64()
        .ok_or("missing initial user ID")?;
    let anonymous = mfa_post(
        address,
        "/api/auth/mfa/enrollment",
        credentials.clone(),
        None,
        "203.0.113.82",
    )
    .await?;
    mfa_unauthorized(&anonymous);
    let wrong_password = serde_json::json!({"email":email,"password":"wrong-password"});
    let denied = mfa_post(
        address,
        "/api/auth/mfa/enrollment",
        wrong_password.clone(),
        Some(token),
        "203.0.113.82",
    )
    .await?;
    mfa_unauthorized(&denied);
    let enrollment = mfa_post(
        address,
        "/api/auth/mfa/enrollment",
        credentials.clone(),
        Some(token),
        "203.0.113.83",
    )
    .await?;
    mfa_success(&enrollment);
    let uri = enrollment.body["data"]["provisioning_uri"]
        .as_str()
        .ok_or("missing provisioning URI")?;
    let code = authenticator_code(uri)?;
    let confirmed = mfa_post(
        address,
        "/api/auth/mfa/enrollment/confirm",
        serde_json::json!({"code":code}),
        Some(token),
        "203.0.113.83",
    )
    .await?;
    mfa_success(&confirmed);
    let codes = confirmed.body["data"]["recovery_codes"]
        .as_array()
        .ok_or("missing recovery codes")?;
    assert_eq!(codes.len(), 10);
    let first = codes[0].as_str().ok_or("missing first recovery code")?;
    let second = codes[1].as_str().ok_or("missing second recovery code")?;
    let unused = codes[2].as_str().ok_or("missing unused recovery code")?;
    assert_mfa_refresh_revoked(address, initial_refresh).await?;

    let challenge = mfa_challenge(address, &credentials, "203.0.113.84").await?;
    let completed = mfa_complete(address, &challenge, first, "203.0.113.84").await?;
    mfa_success(&completed);
    let completed_token = completed.body["data"]["access_token"]
        .as_str()
        .ok_or("missing completed token")?;
    let completed_refresh = completed.body["data"]["refresh_token"]
        .as_str()
        .ok_or("missing completed refresh")?;
    assert_eq!(completed.body["data"]["user_id"], user_id);
    mfa_success(&get(address, "/api/me", completed_token).await?);
    // 同一 challenge 不可复用；另取 challenge 后再次提交同一码，独立证明恢复码一次性。
    mfa_unauthorized(&mfa_complete(address, &challenge, first, "203.0.113.84").await?);
    let next = mfa_challenge(address, &credentials, "203.0.113.85").await?;
    mfa_unauthorized(&mfa_complete(address, &next, first, "203.0.113.85").await?);
    let rotated = mfa_post(
        address,
        "/api/auth/mfa/recovery-codes",
        serde_json::json!({"email":email,"password":"mfa-test-password","code":second}),
        Some(completed_token),
        "203.0.113.86",
    )
    .await?;
    mfa_success(&rotated);
    let new_codes = rotated.body["data"]["recovery_codes"]
        .as_array()
        .ok_or("missing rotated recovery codes")?;
    assert_eq!(new_codes.len(), 10);
    let fresh = new_codes[0].as_str().ok_or("missing fresh recovery code")?;
    let disable_code = new_codes[1]
        .as_str()
        .ok_or("missing disable recovery code")?;
    assert_mfa_refresh_revoked(address, completed_refresh).await?;
    let old_code_challenge = mfa_challenge(address, &credentials, "203.0.113.87").await?;
    mfa_unauthorized(&mfa_complete(address, &old_code_challenge, unused, "203.0.113.87").await?);
    // 拒绝旧码后，用新的 challenge 和新码证明账户仍可正常登录。
    let fresh_challenge = mfa_challenge(address, &credentials, "203.0.113.88").await?;
    let fresh_login = mfa_complete(address, &fresh_challenge, fresh, "203.0.113.88").await?;
    mfa_success(&fresh_login);
    assert_eq!(fresh_login.body["data"]["user_id"], user_id);
    let fresh_token = fresh_login.body["data"]["access_token"]
        .as_str()
        .ok_or("missing fresh token")?;
    let fresh_refresh = fresh_login.body["data"]["refresh_token"]
        .as_str()
        .ok_or("missing fresh refresh")?;
    let denied = mfa_post(
        address,
        "/api/auth/mfa/disable",
        serde_json::json!({"email":email,"password":"wrong-password","code":disable_code}),
        Some(fresh_token),
        "203.0.113.89",
    )
    .await?;
    mfa_unauthorized(&denied);
    let disabled = mfa_post(
        address,
        "/api/auth/mfa/disable",
        serde_json::json!({"email":email,"password":"mfa-test-password","code":disable_code}),
        Some(fresh_token),
        "203.0.113.90",
    )
    .await?;
    mfa_success(&disabled);
    assert_eq!(disabled.body["data"]["status"], "disabled");
    assert_mfa_refresh_revoked(address, fresh_refresh).await?;
    let password_login = mfa_post(
        address,
        "/api/auth/login",
        credentials,
        None,
        "203.0.113.91",
    )
    .await?;
    mfa_success(&password_login);
    assert_eq!(password_login.body["data"]["status"], "authenticated");
    assert!(password_login.body["data"].get("challenge").is_none());
    let final_token = password_login.body["data"]["credentials"]["access_token"]
        .as_str()
        .ok_or("missing token after disable")?;
    let current = get(address, "/api/me", final_token).await?;
    mfa_success(&current);
    assert_eq!(current.body["data"]["user_id"], user_id);
    Ok(())
}

fn mfa_success(response: &HttpResponse) {
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body["success"], true);
}

fn mfa_unauthorized(response: &HttpResponse) {
    // 生成项目使用 AlwaysOk envelope；稳定机器码独立于传输状态。
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body["success"], false);
    assert_eq!(response.body["code"], "unauthorized");
}

async fn mfa_post(
    address: SocketAddr,
    path: &str,
    payload: serde_json::Value,
    token: Option<&str>,
    source: &str,
) -> std::result::Result<HttpResponse, Box<dyn StdError>> {
    request(
        address,
        "POST",
        path,
        Some(payload.to_string()),
        token,
        Some(source),
    )
    .await
}

async fn mfa_challenge(
    address: SocketAddr,
    credentials: &serde_json::Value,
    source: &str,
) -> std::result::Result<String, Box<dyn StdError>> {
    let response = mfa_post(
        address,
        "/api/auth/login",
        credentials.clone(),
        None,
        source,
    )
    .await?;
    mfa_success(&response);
    assert_eq!(response.body["data"]["status"], "mfa_required");
    assert!(response.body["data"].get("credentials").is_none());
    assert!(response.body["data"].get("access_token").is_none());
    assert!(response.body["data"].get("refresh_token").is_none());
    Ok(response.body["data"]["challenge"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("missing MFA challenge")?
        .to_owned())
}

async fn mfa_complete(
    address: SocketAddr,
    challenge: &str,
    code: &str,
    source: &str,
) -> std::result::Result<HttpResponse, Box<dyn StdError>> {
    mfa_post(
        address,
        "/api/auth/mfa/login",
        serde_json::json!({"challenge":challenge,"code":code}),
        None,
        source,
    )
    .await
}

async fn assert_mfa_refresh_revoked(address: SocketAddr, refresh: &str) -> TestResult {
    let response = mfa_post(
        address,
        "/api/auth/refresh",
        serde_json::json!({"refresh_token":refresh}),
        None,
        "203.0.113.92",
    )
    .await?;
    mfa_unauthorized(&response);
    Ok(())
}

fn authenticator_code(uri: &str) -> TestResult<String> {
    // 仅测试客户端模拟外部 RFC 6238 验证器；生产业务不读取 seed 或实现算法。
    use hmac::{Hmac, Mac, digest::KeyInit};
    let uri = url::Url::parse(uri)?;
    let encoded = uri
        .query_pairs()
        .find_map(|(key, value)| (key == "secret").then(|| value.into_owned()))
        .ok_or("missing authenticator secret")?;
    let (mut bits, mut buffer, mut secret) = (0_u8, 0_u32, Vec::new());
    for byte in encoded.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'2'..=b'7' => byte - b'2' + 26,
            _ => return Err("invalid authenticator encoding".into()),
        };
        buffer = ((buffer << 5) | u32::from(value)) & 0xffff;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            secret.push(u8::try_from((buffer >> bits) & 0xff)?);
        }
    }
    let step = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        / 30;
    let mut mac = <Hmac<sha1::Sha1> as KeyInit>::new_from_slice(&secret)?;
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 0x0f);
    let binary = (u32::from(digest[offset] & 0x7f) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    Ok(format!("{:06}", binary % 1_000_000))
}
