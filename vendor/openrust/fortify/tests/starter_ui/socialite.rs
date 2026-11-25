use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use openrust::{
    contracts::auth::Authenticatable as _,
    database::{
        eloquent::Model,
        migrations::{MigrationEntry, MigrationRegistry, Migrator},
    },
    http::HttpFuture,
    support::{
        ProviderResult, ServiceProvider,
        facades::{App, Facade as _},
    },
};
#[path = "../../stubs/starter/migrations/social-identities.rs"]
mod identity_migration;
#[path = "../../stubs/starter/migrations/provider-password.rs"]
mod password_migration;

static ENTRIES: [MigrationEntry; 2] = [
    MigrationEntry::new(
        "database/migrations/0001_01_01_000026_create_user_social_identities_table.rs",
        &identity_migration::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/0001_01_01_000027_allow_provider_accounts_without_password.rs",
        &password_migration::Definition,
    ),
];

pub(super) struct FixtureProvider;
impl ServiceProvider for FixtureProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let config = app
                .make::<dyn openrust::contracts::config::Repository>()
                .await?;
            if let Some(endpoint) = config
                .get("user.socialite.endpoint")
                .and_then(|v| v.as_str().map(str::to_owned))
            {
                let mut base = config.get("app.url").unwrap().as_str().unwrap().to_owned();
                if let Some(prefix) = config
                    .get("user.socialite.fixture_prefix")
                    .and_then(|v| v.as_str().map(str::to_owned))
                {
                    config.set("fortify.prefix", prefix.clone().into())?;
                    base = format!("{}/{}", base, prefix.trim_matches('/'));
                }
                for provider in ["google", "github"] {
                    config.set(&format!("services.{provider}"),json!({"client_id":"fixture-client","client_secret":"fixture-secret","redirect":format!("{base}/auth/{provider}/callback")}))?;
                }
                let manager = app.make::<openrust_socialite::SocialiteManager>().await?;
                manager.extend(
                    "google",
                    Arc::new(openrust_socialite::two::GoogleProvider::with_endpoints(
                        openrust_socialite::two::ProviderEndpoints::new(
                            format!("{endpoint}/authorize"),
                            format!("{endpoint}/token"),
                            format!("{endpoint}/user"),
                        ),
                        format!("{endpoint}/jwks"),
                    )),
                );
                manager.extend(
                    "github",
                    Arc::new(openrust_socialite::two::GithubProvider::with_endpoints(
                        format!("{endpoint}/authorize"),
                        format!("{endpoint}/token"),
                        format!("{endpoint}/user"),
                        format!("{endpoint}/emails"),
                    )),
                );
            }
            Ok(())
        })
    }
}
#[derive(Default)]
struct Protocol {
    next: std::sync::atomic::AtomicU64,
    codes: std::sync::Mutex<BTreeMap<String, (String, String)>>,
    tokens: std::sync::Mutex<BTreeMap<String, String>>,
}
struct Mock {
    address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn protocol(
    axum::extract::State(state): axum::extract::State<Arc<Protocol>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    let uri = url::Url::parse(&format!("http://mock{}", request.uri())).unwrap();
    let params = uri
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect::<BTreeMap<_, _>>();
    match uri.path() {
        "/authorize" => {
            let subject = params
                .get("fixture_subject")
                .cloned()
                .unwrap_or_else(|| "new-user".into());
            let id = state.next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let code = format!("fixture-code-{id}");
            state
                .codes
                .lock()
                .unwrap()
                .insert(code.clone(), (params["code_challenge"].clone(), subject));
            let mut callback = url::Url::parse(&params["redirect_uri"]).unwrap();
            callback
                .query_pairs_mut()
                .append_pair("code", &code)
                .append_pair("state", &params["state"]);
            (
                axum::http::StatusCode::FOUND,
                [("location", callback.to_string())],
            )
                .into_response()
        }
        "/token" => {
            let body = axum::body::to_bytes(request.into_body(), 16384)
                .await
                .unwrap();
            let params = url::form_urlencoded::parse(&body)
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect::<BTreeMap<_, _>>();
            let Some((expected, subject)) = params
                .get("code")
                .and_then(|c| state.codes.lock().unwrap().remove(c))
            else {
                return axum::http::StatusCode::BAD_REQUEST.into_response();
            };
            let Some(verifier) = params.get("code_verifier") else {
                return axum::http::StatusCode::BAD_REQUEST.into_response();
            };
            let actual = URL_SAFE_NO_PAD.encode(
                aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, verifier.as_bytes()).as_ref(),
            );
            if actual != expected
                || params["client_id"] != "fixture-client"
                || params["client_secret"] != "fixture-secret"
            {
                return axum::http::StatusCode::BAD_REQUEST.into_response();
            }
            let token = format!(
                "fixture-token-{}",
                state.next.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            );
            state.tokens.lock().unwrap().insert(token.clone(), subject);
            axum::Json(json!({"access_token":token,"token_type":"Bearer","scope":"user:email"}))
                .into_response()
        }
        "/user" | "/emails" => {
            let Some(subject) = request
                .headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| {
                    v.strip_prefix("Bearer ")
                        .or_else(|| v.strip_prefix("token "))
                })
                .and_then(|t| state.tokens.lock().unwrap().get(t).cloned())
            else {
                return axum::http::StatusCode::UNAUTHORIZED.into_response();
            };
            let email = format!("{subject}@example.test");
            let verified = subject != "unverified";
            if uri.path() == "/emails" {
                axum::Json(json!([{"email":email,"primary":true,"verified":verified}]))
                    .into_response()
            } else {
                axum::Json(json!({"id":subject,"sub":subject,"name":"第三方用户","login":subject,"email":email,"email_verified":verified})).into_response()
            }
        }
        _ => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}
async fn mock() -> Mock {
    use axum::handler::Handler as _;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = protocol
        .with_state(Arc::new(Protocol::default()))
        .into_make_service();
    Mock {
        address,
        task: tokio::spawn(async move {
            axum::serve(listener, service).await.unwrap();
        }),
    }
}
async fn server(protocol: &Mock, profile: &'static str, tfa: bool) -> Server {
    let server=start_security_catalog_mail(profile,false,tfa,"api/admin",false,Some(json!({
        "default":"array","mailers":{"array":{"transport":"array"}},"from":{"address":"admin@example.test"},
        "__user_socialite":{"providers":["google","github"],"api_redirects":["http://127.0.0.1:39091/return"],"endpoint":format!("http://{}",protocol.address)}
    }))).await;
    migrate(&server).await;
    server
}
async fn migrate(server: &Server) {
    let app = server._kernel.get_application();
    let root = &server._root.0;
    for (f, contents) in [
        (
            "0001_01_01_000026_create_user_social_identities_table.rs",
            include_str!("../../stubs/starter/migrations/social-identities.rs"),
        ),
        (
            "0001_01_01_000027_allow_provider_accounts_without_password.rs",
            include_str!("../../stubs/starter/migrations/provider-password.rs"),
        ),
    ] {
        std::fs::create_dir_all(root.join("database/migrations")).unwrap();
        std::fs::write(root.join("database/migrations").join(f), contents).unwrap();
    }
    App::set_facade_application(Some(app), async {
        let migrator = Migrator::new(
            app.make::<openrust::database::DatabaseManager>()
                .await
                .unwrap(),
            Arc::new(MigrationRegistry(&ENTRIES)),
            "socialite_migrations".into(),
            root.clone(),
        );
        let paths = [root.join("database/migrations")];
        assert_eq!(
            migrator
                .run(None, &paths, Default::default())
                .await
                .unwrap_or_else(|error| panic!("迁移失败：{}", migration_error(&error)))
                .len(),
            2
        );
        assert!(
            migrator
                .run(None, &paths, Default::default())
                .await
                .unwrap_or_else(|error| panic!("迁移失败：{}", migration_error(&error)))
                .is_empty()
        );
        assert_eq!(
            migrator
                .rollback(None, &paths, Default::default())
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            migrator
                .run(None, &paths, Default::default())
                .await
                .unwrap_or_else(|error| panic!("迁移失败：{}", migration_error(&error)))
                .len(),
            2
        );
    })
    .await;
}
fn browser(server: &Server) -> Client {
    Client {
        address: server.address,
        cookies: BTreeMap::new(),
        csrf: String::new(),
    }
}
fn location(headers: &str) -> String {
    headers
        .lines()
        .find_map(|line| line.strip_prefix("location: "))
        .unwrap_or_else(|| panic!("缺少重定向：{headers}"))
        .into()
}
fn local(uri: &str) -> String {
    let uri = url::Url::parse(uri).unwrap();
    format!(
        "{}{}",
        uri.path(),
        uri.query().map(|q| format!("?{q}")).unwrap_or_default()
    )
}
async fn authorize(mock: &Mock, url: &str, subject: &str) -> String {
    let mut uri = url::Url::parse(url).unwrap();
    assert!(
        uri.query_pairs()
            .any(|(k, v)| k == "code_challenge_method" && v == "S256")
    );
    uri.query_pairs_mut()
        .append_pair("fixture_subject", subject);
    let mut client = Client {
        address: mock.address,
        cookies: BTreeMap::new(),
        csrf: String::new(),
    };
    let result = client
        .exchange("GET", &local(uri.as_str()), "", false)
        .await;
    assert_eq!(result.0, 302, "{}", result.2);
    location(&result.1)
}
async fn web_login(
    server: &Server,
    mock: &Mock,
    client: &mut Client,
    provider: &str,
    subject: &str,
) -> (u16, String, String) {
    let response = client
        .exchange("GET", &format!("/auth/{provider}/redirect"), "", false)
        .await;
    assert_eq!(response.0, 302, "{}", response.2);
    let callback = authorize(mock, &location(&response.1), subject).await;
    let result = client.exchange("GET", &local(&callback), "", false).await;
    assert!(callback.starts_with(&format!("http://{}", server.address)));
    result
}
#[tokio::test]
async fn socialite_web_creates_verified_account_enforces_state_and_never_links_matching_email() {
    let protocol = mock().await;
    let server = server(&protocol, "full", false).await;
    let mut client = browser(&server);
    let result = web_login(&server, &protocol, &mut client, "google", "new-user").await;
    assert_eq!(result.0, 302, "{}", result.2);
    let account = client.send("GET", "/user/profile", "", true).await;
    assert_eq!(account.0, 200, "{}", account.1);
    assert!(account.1.contains("new-user@example.test"));
    let mut second = browser(&server);
    let result = web_login(&server, &protocol, &mut second, "github", "new-user").await;
    assert_eq!(result.0, 302, "{}", result.2);
    assert!(location(&result.1).ends_with("/login"));
    let mut attacker = browser(&server);
    let response = attacker
        .exchange("GET", "/auth/google/redirect", "", false)
        .await;
    let callback = authorize(&protocol, &location(&response.1), "attacker").await;
    let mut uri = url::Url::parse(&callback).unwrap();
    let code = uri
        .query_pairs()
        .find(|(k, _)| k == "code")
        .unwrap()
        .1
        .into_owned();
    uri.set_query(None);
    uri.query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", "wrong-state");
    let result = attacker
        .exchange("GET", &local(uri.as_str()), "", false)
        .await;
    assert_eq!(result.0, 302);
    assert!(location(&result.1).ends_with("/login"));
    let result = attacker.exchange("GET", &local(&callback), "", false).await;
    assert_eq!(result.0, 302);
    assert!(location(&result.1).ends_with("/login"));
    let mut unverified = browser(&server);
    let result = web_login(&server, &protocol, &mut unverified, "google", "unverified").await;
    assert_eq!(result.0, 302, "{}", result.2);
    assert!(location(&result.1).ends_with("/login"));
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let db = server
            ._kernel
            .get_application()
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        assert_eq!(db.table("users").count("*").await.unwrap(), 1);
        let rows = db.table("user_social_identities").get().await.unwrap();
        assert_eq!(rows.len(), 1);
        let data = match rows[0].get("payload").unwrap() {
            openrust::database::Value::Text(text) => text,
            _ => panic!("身份载荷不是文本"),
        };
        assert!(!data.contains("fixture-token"));
        assert!(!data.contains("fixture-secret"));
        let user = app::models::User::query_on(&db)
            .first()
            .await
            .unwrap()
            .unwrap();
        assert!(user.get_auth_password().is_none());
        let root = &server._root.0;
        let migrator = Migrator::new(
            server
                ._kernel
                .get_application()
                .make::<openrust::database::DatabaseManager>()
                .await
                .unwrap(),
            Arc::new(MigrationRegistry(&ENTRIES)),
            "socialite_migrations".into(),
            root.clone(),
        );
        assert!(
            migrator
                .rollback(
                    None,
                    &[root.join("database/migrations")],
                    Default::default()
                )
                .await
                .is_err()
        );
        assert_eq!(
            db.table("user_social_identities").count("*").await.unwrap(),
            1
        );
        assert_eq!(
            db.table("socialite_migrations").count("*").await.unwrap(),
            2
        );
        let password = db
            .get_schema_builder()
            .get_columns("users")
            .await
            .unwrap()
            .into_iter()
            .find(|c| c.name == "password")
            .unwrap();
        assert!(password.nullable);
    })
    .await;
}
#[tokio::test]
async fn socialite_token_handoff_requires_verifier_has_no_callback_token_and_is_single_use() {
    let protocol = mock().await;
    let server = server(&protocol, "api", false).await;
    let verifier = "client-only-verifier-abcdefghijklmnopqrstuvwxyz0123456789";
    let challenge = URL_SAFE_NO_PAD.encode(
        aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, verifier.as_bytes()).as_ref(),
    );
    let start=token_client_request(server.address,"/api/admin/auth/providers/google",json!({"code_challenge":challenge,"redirect_uri":"http://127.0.0.1:39091/return","device_name":"phone","abilities":["account:read"]})).await;
    assert_eq!(start.0, 200, "{}", start.2);
    let mut handoff_browser = browser(&server);
    let handoff = handoff_browser
        .exchange(
            "GET",
            &local(start.2["data"]["url"].as_str().unwrap()),
            "",
            false,
        )
        .await;
    assert_eq!(handoff.0, 302, "{}", handoff.2);
    let callback = authorize(&protocol, &location(&handoff.1), "token-user").await;
    let result = handoff_browser
        .exchange("GET", &local(&callback), "", false)
        .await;
    assert_eq!(result.0, 302, "{}", result.2);
    let target = url::Url::parse(&location(&result.1)).unwrap();
    assert_eq!(target.path(), "/return");
    let fragment = target.fragment().unwrap();
    assert!(!fragment.contains("token="));
    let code = fragment.strip_prefix("code=").unwrap();
    let wrong=token_client_request(server.address,"/api/admin/auth/providers/exchange",json!({"code":code,"code_verifier":"wrong-client-verifier-abcdefghijklmnopqrstuvwxyz0123456789"})).await;
    assert_eq!(wrong.0, 401, "{}", wrong.2);
    let exchange = token_client_request(
        server.address,
        "/api/admin/auth/providers/exchange",
        json!({"code":code,"code_verifier":verifier}),
    )
    .await;
    assert_eq!(exchange.0, 201, "{}", exchange.2);
    let token = exchange.2["data"]["token"].as_str().unwrap();
    assert!(!token.is_empty());
    let replay = token_client_request(
        server.address,
        "/api/admin/auth/providers/exchange",
        json!({"code":code,"code_verifier":verifier}),
    )
    .await;
    assert_eq!(replay.0, 401, "{}", replay.2);
    let current = account_request(
        server.address,
        token,
        "GET",
        "/api/admin/user/connected-accounts",
        json!({}),
    )
    .await;
    assert_eq!(current.0, 200, "{}", current.2);
    let mut stranger = browser(&server);
    assert_eq!(
        stranger
            .send("GET", "/api/admin/user/connected-accounts", "", true)
            .await
            .0,
        401
    );
}

async fn register_local(server: &Server) {
    let result=token_client_request(server.address,"/api/admin/auth/register",json!({"name":"本地用户","email":"device@example.test","password":"long-password-123","password_confirmation":"long-password-123"})).await;
    assert_eq!(result.0, 201, "{}", token_failure(&result.2));
}
async fn password_browser(server: &Server) -> Client {
    let mut browser = browser(server);
    assert_eq!(browser.send("GET", "/login", "", false).await.0, 200);
    let result = browser
        .send(
            "POST",
            "/login",
            r#"{"email":"device@example.test","password":"long-password-123"}"#,
            true,
        )
        .await;
    assert_eq!(result.0, 200, "{}", result.1);
    assert_eq!(
        browser
            .send("GET", "/user/confirm-password", "", false)
            .await
            .0,
        200
    );
    let result = browser
        .send(
            "POST",
            "/user/confirm-password",
            r#"{"password":"long-password-123"}"#,
            true,
        )
        .await;
    assert_eq!(result.0, 201, "{}", result.1);
    browser
}
async fn link_web(server: &Server, protocol: &Mock, browser: &mut Client, confirm: bool) {
    let start = browser
        .exchange("POST", "/user/connected-accounts/google", "{}", false)
        .await;
    assert_eq!(start.0, 302, "{}", start.2);
    let callback = authorize(protocol, &location(&start.1), "device").await;
    let preview = browser.exchange("GET", &local(&callback), "", false).await;
    assert_eq!(preview.0, 200, "{}", preview.2);
    assert!(preview.2.contains("device@example.test"));
    let result = browser
        .exchange(
            "POST",
            "/user/connected-accounts/confirm",
            &json!({"confirm":confirm}).to_string(),
            false,
        )
        .await;
    assert_eq!(result.0, 302, "{}", result.2);
    let page = browser
        .send("GET", "/user/connected-accounts", "", false)
        .await;
    assert_eq!(page.0, 200, "{}", page.1);
    assert!(page.1.contains(if confirm {
        "解除关联"
    } else {
        "关联账户"
    }));
    let _ = server;
}
#[tokio::test]
async fn socialite_binding_requires_explicit_confirmation_and_unlink_invalidates_pending_factor_login()
 {
    let protocol = mock().await;
    let server = server(&protocol, "full", true).await;
    register_local(&server).await;
    let mut owner = password_browser(&server).await;
    link_web(&server, &protocol, &mut owner, false).await;
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let db = server
            ._kernel
            .get_application()
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        assert_eq!(
            db.table("user_social_identities").count("*").await.unwrap(),
            0
        );
    })
    .await;
    link_web(&server, &protocol, &mut owner, true).await;
    // 已验收的TOTP动作独立覆盖；本单元设置真实加密因素并从公开登录消费恢复码。
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let app = server._kernel.get_application();
        let fortify = app.make::<openrust_fortify::Fortify>().await.unwrap();
        let encrypter = fortify.current_encrypter().unwrap();
        let secret = encrypter.encrypt(b"JBSWY3DPEHPK3PXP", true).unwrap();
        let recovery = encrypter
            .encrypt(
                br#"["socialite-one","socialite-two","socialite-three"]"#,
                true,
            )
            .unwrap();
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        db.table("users")
            .r#where("email", "=", "device@example.test")
            .update(
                [
                    ("two_factor_secret".into(), secret.into()),
                    ("two_factor_recovery_codes".into(), recovery.into()),
                    (
                        "two_factor_confirmed_at".into(),
                        "2026-01-01 00:00:00".into(),
                    ),
                ]
                .into(),
            )
            .await
            .unwrap();
    })
    .await;
    // 旧密码确认绑定已变化，先通过密码加恢复码完成当前会话，再重新确认。
    let mut next = browser(&server);
    let login = web_login(&server, &protocol, &mut next, "google", "device").await;
    assert_eq!(login.0, 302, "{}", login.2);
    assert!(location(&login.1).ends_with("/two-factor-challenge"));
    assert_eq!(
        next.send("GET", "/two-factor-challenge", "", false).await.0,
        200
    );
    let done = next
        .send(
            "POST",
            "/two-factor-challenge",
            r#"{"recovery_code":"socialite-one"}"#,
            true,
        )
        .await;
    assert_eq!(done.0, 200, "{}", done.1);
    assert_eq!(next.send("GET", "/home", "", false).await.0, 200);
    let mut stale = browser(&server);
    let challenge = web_login(&server, &protocol, &mut stale, "google", "device").await;
    assert_eq!(challenge.0, 302);
    assert_eq!(
        stale
            .send("GET", "/two-factor-challenge", "", false)
            .await
            .0,
        200
    );
    assert_eq!(
        next.send(
            "POST",
            "/user/confirm-password",
            r#"{"password":"long-password-123"}"#,
            true
        )
        .await
        .0,
        201
    );
    let unlink = next
        .send("DELETE", "/user/connected-accounts/google", "{}", true)
        .await;
    assert_eq!(unlink.0, 302, "{}", unlink.1);
    let denied = stale
        .send(
            "POST",
            "/two-factor-challenge",
            r#"{"recovery_code":"socialite-two"}"#,
            true,
        )
        .await;
    assert_eq!(denied.0, 401, "{}", denied.1);
}

async fn token(server: &Server) -> String {
    let result =
        token_client_request(server.address, "/api/admin/auth/token", device_login()).await;
    assert_eq!(result.0, 201, "{}", token_failure(&result.2));
    result.2["data"]["token"].as_str().unwrap().into()
}
async fn proof(server: &Server, token: &str) -> String {
    let result = account_request(
        server.address,
        token,
        "POST",
        "/api/admin/auth/confirm",
        json!({"purpose":"factors","password":"long-password-123"}),
    )
    .await;
    assert_eq!(result.0, 201, "{}", token_failure(&result.2));
    result.2["data"]["proof"].as_str().unwrap().into()
}
async fn api_handoff(server: &Server, protocol: &Mock, url: &str, subject: &str) -> String {
    let mut browser = browser(server);
    let handoff = browser.exchange("GET", &local(url), "", false).await;
    assert_eq!(handoff.0, 302, "{}", handoff.2);
    let callback = authorize(protocol, &location(&handoff.1), subject).await;
    let result = browser.exchange("GET", &local(&callback), "", false).await;
    assert_eq!(result.0, 302, "{}", result.2);
    url::Url::parse(&location(&result.1))
        .unwrap()
        .fragment()
        .unwrap()
        .strip_prefix("code=")
        .unwrap()
        .to_owned()
}
#[tokio::test]
async fn socialite_api_links_only_after_preview_and_explicit_confirm_for_the_same_token() {
    let protocol = mock().await;
    let server = server(&protocol, "api", false).await;
    register_local(&server).await;
    let token = token(&server).await;
    let other = self::token(&server).await;
    let verifier = "client-verifier-for-account-link-abcdefghijklmnopqrstuvwxyz0123456789";
    let hash = URL_SAFE_NO_PAD.encode(
        aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, verifier.as_bytes()).as_ref(),
    );
    let proof = proof(&server, &token).await;
    let begin = account_request(
        server.address,
        &token,
        "POST",
        "/api/admin/user/connected-accounts/github",
        json!({"proof":proof,"code_challenge":hash,"redirect_uri":"http://127.0.0.1:39091/return"}),
    )
    .await;
    assert_eq!(begin.0, 200, "{}", token_failure(&begin.2));
    let code = api_handoff(
        &server,
        &protocol,
        begin.2["data"]["url"].as_str().unwrap(),
        "device",
    )
    .await;
    let preview = account_request(
        server.address,
        &token,
        "POST",
        "/api/admin/user/connected-accounts/exchange",
        json!({"code":code,"code_verifier":verifier}),
    )
    .await;
    assert_eq!(preview.0, 200, "{}", token_failure(&preview.2));
    assert_eq!(preview.2["data"]["email"], "device@example.test");
    let challenge = preview.2["data"]["challenge"].clone();
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let db = server
            ._kernel
            .get_application()
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        assert_eq!(
            db.table("user_social_identities").count("*").await.unwrap(),
            0
        );
    })
    .await;
    let wrong = account_request(
        server.address,
        &other,
        "POST",
        "/api/admin/user/connected-accounts/confirm",
        json!({"challenge":challenge,"confirm":true}),
    )
    .await;
    assert_eq!(wrong.0, 401, "{}", token_failure(&wrong.2));
    let confirm = account_request(
        server.address,
        &token,
        "POST",
        "/api/admin/user/connected-accounts/confirm",
        json!({"challenge":challenge,"confirm":true}),
    )
    .await;
    assert_eq!(confirm.0, 200, "{}", token_failure(&confirm.2));
    let list = account_request(
        server.address,
        &token,
        "GET",
        "/api/admin/user/connected-accounts",
        json!({}),
    )
    .await;
    assert_eq!(list.0, 200, "{}", list.2);
    assert_eq!(list.2["data"][1]["linked"], true);
    let proof = self::proof(&server, &token).await;
    let remove = account_request(
        server.address,
        &token,
        "DELETE",
        "/api/admin/user/connected-accounts/github",
        json!({"proof":proof}),
    )
    .await;
    assert_eq!(remove.0, 204, "{}", remove.2);
    let url_secret = token_client_request(
        server.address,
        "/api/admin/auth/providers/exchange?code_verifier=forbidden",
        json!({}),
    )
    .await;
    assert_eq!(url_secret.0, 400, "{}", url_secret.2);
}
#[tokio::test]
async fn socialite_refuses_last_primary_and_password_reset_establishes_local_credentials() {
    let protocol = mock().await;
    let server = server(&protocol, "full", false).await;
    let mut browser = browser(&server);
    let result = web_login(&server, &protocol, &mut browser, "google", "reset-user").await;
    assert_eq!(result.0, 302, "{}", result.2);
    let page = browser
        .send("GET", "/user/confirm-password", "", false)
        .await;
    assert_eq!(page.0, 200);
    assert!(!page.1.contains("id=\"password\""));
    assert!(page.1.contains("/user/password/recovery"));
    assert_eq!(
        browser
            .send("DELETE", "/user/connected-accounts/google", "{}", true)
            .await
            .0,
        423
    );
    let recovery = browser
        .exchange("POST", "/user/password/recovery", "{}", false)
        .await;
    assert_eq!(recovery.0, 302, "{}", recovery.2);
    assert!(location(&recovery.1).ends_with("/forgot-password"));
    let form = browser.send("GET", "/forgot-password", "", false).await;
    assert_eq!(form.0, 200);
    assert!(form.1.contains("reset-user@example.test"));
    let result = browser
        .send(
            "POST",
            "/forgot-password",
            r#"{"email":"reset-user@example.test"}"#,
            true,
        )
        .await;
    assert_eq!(result.0, 200, "{}", result.1);
    let url = server.mail_link("/reset-password/");
    let uri = url::Url::parse(&format!("http://fixture{url}")).unwrap();
    let reset_token = uri.path().strip_prefix("/reset-password/").unwrap();
    let reset=browser.send("POST","/reset-password",&json!({"email":"reset-user@example.test","token":reset_token,"password":"new-local-password-123","password_confirmation":"new-local-password-123"}).to_string(),true).await;
    assert_eq!(reset.0, 200, "{}", reset.1);
    let old = browser.send("GET", "/home", "", true).await;
    assert_eq!(old.0, 401, "{}", old.1);
    assert_eq!(browser.send("GET", "/login", "", false).await.0, 200);
    let login = browser
        .send(
            "POST",
            "/login",
            r#"{"email":"reset-user@example.test","password":"new-local-password-123"}"#,
            true,
        )
        .await;
    assert_eq!(login.0, 200, "{}", login.1);
    browser
        .send("GET", "/user/confirm-password", "", false)
        .await;
    assert_eq!(
        browser
            .send(
                "POST",
                "/user/confirm-password",
                r#"{"password":"new-local-password-123"}"#,
                true
            )
            .await
            .0,
        201
    );
    let remove = browser
        .send("DELETE", "/user/connected-accounts/google", "{}", true)
        .await;
    assert_eq!(remove.0, 302, "{}", remove.1);
}

fn migration_error(error: &dyn std::error::Error) -> String {
    let mut messages = vec![error.to_string()];
    let mut current = error.source();
    while let Some(error) = current {
        messages.push(error.to_string());
        current = error.source();
    }
    messages.join(": ")
}

#[tokio::test]
async fn socialite_handoff_respects_authentication_prefix_and_headless_disables_web_login() {
    let protocol = mock().await;
    let server=start_security_catalog_mail("api",false,false,"api/admin",false,Some(json!({
        "default":"array","mailers":{"array":{"transport":"array"}},"from":{"address":"admin@example.test"},
        "__user_socialite":{"providers":["google"],"api_redirects":["http://127.0.0.1:39091/return"],"endpoint":format!("http://{}",protocol.address),"fixture_prefix":"members"}
    }))).await;
    migrate(&server).await;
    let verifier = "client-verifier-for-auth-prefix-abcdefghijklmnopqrstuvwxyz0123456789";
    let hash = URL_SAFE_NO_PAD.encode(
        aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, verifier.as_bytes()).as_ref(),
    );
    let begin=token_client_request(server.address,"/api/admin/auth/providers/google",json!({"code_challenge":hash,"redirect_uri":"http://127.0.0.1:39091/return","device_name":"phone"})).await;
    assert_eq!(begin.0, 200, "{}", token_failure(&begin.2));
    let url = begin.2["data"]["url"].as_str().unwrap();
    assert!(url.contains("/members/auth/google/handoff"));
    let code = api_handoff(&server, &protocol, url, "prefixed").await;
    let result = token_client_request(
        server.address,
        "/api/admin/auth/providers/exchange",
        json!({"code":code,"code_verifier":verifier}),
    )
    .await;
    assert_eq!(result.0, 201, "{}", token_failure(&result.2));
    let mut browser = browser(&server);
    assert_eq!(
        browser
            .send("GET", "/members/auth/google/redirect", "", true)
            .await
            .0,
        404
    );
}
