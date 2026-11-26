use std::sync::Mutex;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use openrust::{
    auth::{AuthError, AuthFuture, AuthResult},
    contracts::auth::UserProvider,
};

use super::*;

#[derive(Clone)]
struct Account {
    hash: String,
    token: Option<String>,
}

impl Authenticatable for Account {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        "001".into()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        Some(&self.hash)
    }
    fn get_remember_token(&self) -> Option<&str> {
        self.token.as_deref()
    }
    fn set_remember_token(&mut self, token: Option<String>) {
        self.token = token;
    }
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
}

struct Accounts {
    user: Mutex<Account>,
    hasher: Arc<dyn Hasher>,
}

impl Accounts {
    fn snapshot(&self) -> Box<dyn Authenticatable> {
        Box::new(self.user.lock().unwrap().clone())
    }
}

impl UserProvider for Accounts {
    fn retrieve_by_id<'a>(
        &'a self,
        id: &'a AuthIdentifier,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async { Ok((*id == AuthIdentifier::from("001")).then(|| self.snapshot())) })
    }

    fn retrieve_by_token<'a>(
        &'a self,
        id: &'a AuthIdentifier,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async {
            Ok(self
                .retrieve_by_id(id)
                .await?
                .filter(|user| user.get_remember_token() == Some(token)))
        })
    }

    fn update_remember_token<'a>(
        &'a self,
        user: &'a mut dyn Authenticatable,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async {
            self.user.lock().unwrap().token = Some(token.to_owned());
            user.set_remember_token(Some(token.to_owned()));
            Ok(())
        })
    }

    fn retrieve_by_credentials<'a>(
        &'a self,
        credentials: &'a Credentials,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async {
            let matches = !credentials.fields().is_empty()
                && credentials
                    .fields()
                    .iter()
                    .all(|(name, value)| match name.as_str() {
                        "email" => *value == "a@example.test".into(),
                        "username" => *value == "tester".into(),
                        _ => false,
                    });
            Ok(matches.then(|| self.snapshot()))
        })
    }

    fn validate_credentials<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        credentials: &'a Credentials,
    ) -> AuthFuture<'a, AuthResult<bool>> {
        Box::pin(async {
            self.hasher
                .check(
                    credentials.password().as_bytes(),
                    user.get_auth_password().unwrap(),
                    &HashOptions::new(),
                )
                .await
                .map_err(|error| AuthError::Backend(Box::new(error)))
        })
    }

    fn rehash_password_if_required<'a>(
        &'a self,
        _: &'a mut dyn Authenticatable,
        credentials: &'a Credentials,
        force: bool,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async move {
            if force {
                let hash = self
                    .hasher
                    .make(credentials.password().as_bytes(), &HashOptions::new())
                    .await
                    .map_err(|error| AuthError::Backend(Box::new(error)))?;
                self.user.lock().unwrap().hash = hash;
            }
            Ok(())
        })
    }
}

async fn start(
    web: fn() -> Result<(), RoutingError>,
    api: fn() -> Result<(), RoutingError>,
    configure: impl FnOnce(&Application),
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let mut app = Application::configure(std::env::temp_dir())
        .with_config(|_, _| {
            Ok([
                ("app".into(), json!({"key":"a".repeat(32)})),
                (
                    "cache".into(),
                    json!({"default":"array","stores":{"array":{"driver":"array"}}}),
                ),
                (
                    "session".into(),
                    json!({"driver":"array","cookie":"auth_session","lottery":[0,100]}),
                ),
                (
                    "hashing".into(),
                    json!({"driver":"bcrypt","bcrypt":{"rounds":4}}),
                ),
                (
                    "auth".into(),
                    json!({
                        "defaults":{"guard":"web"},
                        "guards":{"web":{"driver":"session","provider":"users"}},
                        "providers":{"users":{"driver":"test-user"}}
                    }),
                ),
            ]
            .into())
        })
        .with_routing(Routing::default().web(web).api(api))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let hasher = app.container().make::<dyn Hasher>().unwrap();
    let hash = hasher
        .make(b"good:secret", &HashOptions::new())
        .await
        .unwrap();
    let provider = Arc::new(Accounts {
        user: Mutex::new(Account { hash, token: None }),
        hasher,
    });
    app.container()
        .make::<AuthManager>()
        .unwrap()
        .provider("test-user", move |_, _| Ok(provider.clone()));
    configure(&app);
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    (address, task)
}

fn authorization(username: &str, password: &str) -> String {
    format!(
        "Authorization: Basic {}\r\n",
        STANDARD.encode(format!("{username}:{password}"))
    )
}

fn current(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let user = Auth::user(request).await?;
        Response::json(
            StatusCode::OK,
            json!({"user":user.map(|user|user.get_auth_identifier())}),
        )
    })
}

fn stateless(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Auth::once_basic(request, "email", &[]).await?;
        current(request).await
    })
}

#[tokio::test]
async fn http_basic_supports_session_and_stateless_authentication() {
    let (address, server) = start(
        || {
            Route::get_with("/basic", current, |route| {
                route.middleware_name("auth.basic");
                Ok(())
            })?;
            Route::get_with("/basic-custom", current, |route| {
                route.middleware_name("auth.basic:web,username");
                Ok(())
            })?;
            Ok(())
        },
        || {
            Route::get("/stateless", stateless)?;
            Route::get("/identity", current)?;
            Ok(())
        },
        |_| {},
    )
    .await;

    for headers in [
        String::new(),
        "Authorization: Basic malformed!\r\n".into(),
        "Authorization: Bearer token\r\n".into(),
        authorization("a@example.test", "wrong"),
    ] {
        let wire = send(address, "GET", "/basic", &headers, "", 401).await;
        assert!(wire.headers.contains("www-authenticate: Basic"));
        assert_eq!(wire.body["message"], "Invalid credentials.");
    }
    let login = send(
        address,
        "GET",
        "/basic",
        &authorization("a@example.test", "good:secret"),
        "",
        200,
    )
    .await;
    let cookie = login.cookie("auth_session");
    assert_eq!(login.body["user"], "001");
    assert_eq!(
        get(address, "/basic", &cookie, 200).await.body["user"],
        "001"
    );
    assert_eq!(
        send(
            address,
            "GET",
            "/basic-custom",
            &authorization("tester", "good:secret"),
            "",
            200
        )
        .await
        .body["user"],
        "001"
    );

    let wire = send(
        address,
        "GET",
        "/api/stateless",
        &authorization("a@example.test", "good:secret"),
        "",
        200,
    )
    .await;
    assert_eq!(wire.body["user"], "001");
    assert!(!wire.headers.contains("set-cookie:"));
    assert_eq!(
        get(address, "/api/identity", "", 200).await.body["user"],
        Json::Null
    );
    assert!(
        get(address, "/api/stateless", "", 401)
            .await
            .headers
            .contains("www-authenticate: Basic")
    );
    server.abort();
}

#[tokio::test]
async fn session_guard_emits_authentication_lifecycle_events() {
    use openrust::{
        auth::events::{
            Attempting, Authenticated, CurrentDeviceLogout, Failed, Login, Logout, Validated,
        },
        contracts::events::{Dispatcher as _, Event, Listener},
        events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
    };

    type Recorded = Arc<Mutex<Vec<(String, bool)>>>;
    struct ListenerProbe(Recorded);

    impl Listener for ListenerProbe {
        fn handle<'a>(
            &'a self,
            event: &'a dyn Event,
            context: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                let mut extra = false;
                if let Some(event) = event.downcast_ref::<Attempting>() {
                    assert_eq!(event.guard, "web");
                    assert!(
                        event
                            .credentials(context)
                            .unwrap()
                            .fields()
                            .contains_key("email")
                    );
                    extra = event.remember;
                }
                if let Some(event) = event.downcast_ref::<Failed>() {
                    assert!(event.credentials(context).is_some());
                    extra = event.user.is_some();
                }
                if let Some(event) = event.downcast_ref::<Validated>() {
                    assert_eq!(
                        event
                            .user::<Account>(context)
                            .unwrap()
                            .get_auth_identifier(),
                        event.user
                    );
                }
                if let Some(event) = event.downcast_ref::<Login>() {
                    assert_eq!(
                        event.user.get_auth_identifier(),
                        AuthIdentifier::from("001")
                    );
                    extra = event.remember;
                }
                if let Some(event) = event.downcast_ref::<Authenticated>() {
                    assert_eq!(
                        event.user.get_auth_identifier(),
                        AuthIdentifier::from("001")
                    );
                }
                if let Some(event) = event.downcast_ref::<Logout>() {
                    extra = event.user.is_some();
                }
                if let Some(event) = event.downcast_ref::<CurrentDeviceLogout>() {
                    extra = event.user.is_some();
                }
                self.0.lock().unwrap().push((event.name().into(), extra));
                Ok(None)
            })
        }
    }

    fn remember(req: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let credentials = Credentials::new([("email", "a@example.test".into())], "good:secret");
            assert!(
                Auth::session_guard(req, None)?
                    .attempt(&credentials, true)
                    .await?
            );
            current(req).await
        })
    }

    fn end_device(req: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            Auth::session_guard(req, None)?
                .logout_current_device()
                .await?;
            current(req).await
        })
    }

    let recorded: Recorded = Arc::default();
    let captured = recorded.clone();
    let (address, server) = start(
        || {
            Route::get_with("/basic", current, |route| {
                route.middleware_name("auth.basic");
                Ok(())
            })?;
            Route::get("/who", current)?;
            Route::post("/remember", remember)?;
            Route::post("/end-device", end_device)?;
            Route::post("/logout", logout)?;
            Ok(())
        },
        || {
            Route::get("/stateless", stateless)?;
            Ok(())
        },
        move |app| {
            let dispatcher = app.container().make::<Dispatcher>().unwrap();
            let listener = Arc::new(ListenerProbe(captured));
            for name in [
                "Attempting",
                "Validated",
                "Authenticated",
                "Failed",
                "Login",
                "Logout",
                "CurrentDeviceLogout",
            ] {
                dispatcher
                    .listen(name, listener.clone(), Delivery::Sync)
                    .unwrap();
            }
        },
    )
    .await;
    let take = || std::mem::take(&mut *recorded.lock().unwrap());
    let expected = |sequence: &[(&str, bool)]| {
        sequence
            .iter()
            .map(|(name, value)| ((*name).to_owned(), *value))
            .collect::<Vec<_>>()
    };

    send(
        address,
        "GET",
        "/basic",
        &authorization("a@example.test", "wrong"),
        "",
        401,
    )
    .await;
    assert_eq!(take(), expected(&[("Attempting", false), ("Failed", true)]));
    send(
        address,
        "GET",
        "/basic",
        &authorization("other@example.test", "wrong"),
        "",
        401,
    )
    .await;
    assert_eq!(
        take(),
        expected(&[("Attempting", false), ("Failed", false)])
    );
    send(
        address,
        "GET",
        "/api/stateless",
        &authorization("a@example.test", "good:secret"),
        "",
        200,
    )
    .await;
    assert_eq!(
        take(),
        expected(&[
            ("Attempting", false),
            ("Validated", false),
            ("Authenticated", false)
        ])
    );

    let login = send(address, "POST", "/remember", "", "", 200).await;
    let main_cookie = login.cookie("auth_session");
    let remember_cookie = login.cookie("remember_web");
    assert_eq!(
        take(),
        expected(&[
            ("Attempting", true),
            ("Validated", false),
            ("Login", true),
            ("Authenticated", false)
        ])
    );
    get(address, "/who", &main_cookie, 200).await;
    assert_eq!(take(), expected(&[("Authenticated", false)]));
    let recalled = get(address, "/who", &remember_cookie, 200).await;
    assert_eq!(take(), expected(&[("Login", true)]));

    let recalled_cookie = recalled.cookie("auth_session");
    send(
        address,
        "POST",
        "/end-device",
        &format!("Cookie: {recalled_cookie}\r\n"),
        "",
        200,
    )
    .await;
    assert_eq!(
        take(),
        expected(&[("Authenticated", false), ("CurrentDeviceLogout", true)])
    );
    get(address, "/who", &remember_cookie, 200).await;
    assert_eq!(take(), expected(&[("Login", true)]));
    send(
        address,
        "POST",
        "/logout",
        &format!("Cookie: {main_cookie}\r\n"),
        "",
        200,
    )
    .await;
    assert_eq!(
        take(),
        expected(&[("Authenticated", false), ("Logout", true)])
    );
    assert_eq!(
        get(address, "/who", &remember_cookie, 200).await.body["user"],
        Json::Null
    );
    assert!(take().is_empty());
    server.abort();
}

#[tokio::test]
async fn other_device_logout_keeps_current_session_and_rejects_old_sessions_and_recalls() {
    use openrust::{
        auth::events::OtherDeviceLogout,
        contracts::events::{Dispatcher as _, Event},
        events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
    };

    fn remember(req: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let credentials = Credentials::new([("email", "a@example.test".into())], "good:secret");
            assert!(
                Auth::session_guard(req, None)?
                    .attempt(&credentials, true)
                    .await?
            );
            current(req).await
        })
    }

    fn revoke(req: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let password = req.input(Some("password")).await?.unwrap();
            match Auth::logout_other_devices(req, password.as_str().unwrap()).await {
                Err(AuthError::InvalidPassword) => Err(openrust::http::abort(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "Invalid password.",
                    Default::default(),
                )),
                result => {
                    result?;
                    current(req).await
                }
            }
        })
    }

    type Records = Arc<Mutex<Vec<AuthIdentifier>>>;
    struct Revocations(Records);
    impl openrust::contracts::events::Listener for Revocations {
        fn handle<'a>(
            &'a self,
            event: &'a dyn Event,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                let event = event.downcast_ref::<OtherDeviceLogout>().unwrap();
                assert_eq!(event.guard, "web");
                self.0
                    .lock()
                    .unwrap()
                    .push(event.user.get_auth_identifier());
                Ok(None)
            })
        }
    }

    let records: Records = Arc::default();
    let capture = records.clone();
    let (address, server) = start(
        || {
            Route::post("/remember", remember)?;
            Route::get_with("/secure", current, |route| {
                route
                    .middleware_name("auth")
                    .middleware_name("auth.session");
                Ok(())
            })?;
            Route::post("/revoke", revoke)?;
            Ok(())
        },
        || Ok(()),
        move |app| {
            app.container()
                .make::<Dispatcher>()
                .unwrap()
                .listen(
                    "OtherDeviceLogout",
                    Arc::new(Revocations(capture)),
                    Delivery::Sync,
                )
                .unwrap();
        },
    )
    .await;
    let a = send(address, "POST", "/remember", "", "", 200).await;
    let b = send(address, "POST", "/remember", "", "", 200).await;
    let a_session = a.cookie("auth_session");
    let a_remember = a.cookie("remember_web");
    let b_session = b.cookie("auth_session");
    let b_remember = b.cookie("remember_web");
    assert_eq!(
        get(address, "/secure", &a_session, 200).await.body["user"],
        "001"
    );
    assert_eq!(
        get(address, "/secure", &b_session, 200).await.body["user"],
        "001"
    );
    let headers = format!("Cookie: {a_session}; {a_remember}\r\n");
    send(
        address,
        "POST",
        "/revoke",
        &headers,
        r#"{"password":"wrong"}"#,
        422,
    )
    .await;
    assert!(records.lock().unwrap().is_empty());
    get(address, "/secure", &b_session, 200).await;
    let revoked = send(
        address,
        "POST",
        "/revoke",
        &headers,
        r#"{"password":"good:secret"}"#,
        200,
    )
    .await;
    assert_eq!(revoked.body["user"], "001");
    let refreshed = revoked.cookie("remember_web");
    assert_ne!(refreshed, a_remember);
    assert_eq!(*records.lock().unwrap(), [AuthIdentifier::from("001")]);
    get(address, "/secure", &a_session, 200).await;
    get(address, "/secure", &refreshed, 200).await;
    get(address, "/secure", &b_session, 401).await;
    get(address, "/secure", &b_remember, 401).await;
    assert_eq!(
        send(address, "POST", "/remember", "", "", 200).await.body["user"],
        "001"
    );
    server.abort();
}

#[path = "extensions/confirmation.rs"]
mod confirmation;

#[tokio::test]
async fn throttled_login_reports_lockout_with_request_metadata() {
    use std::sync::atomic::{AtomicU64, Ordering};

    use openrust::{
        auth::events::Lockout,
        contracts::events::{Dispatcher as _, Event, Listener},
        events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
    };

    struct Record(Arc<AtomicU64>);
    impl Listener for Record {
        fn handle<'a>(
            &'a self,
            event: &'a dyn Event,
            context: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                let event = event.downcast_ref::<Lockout>().unwrap();
                let request = event.request(context).unwrap();
                assert_eq!(request.method, Method::POST);
                assert_eq!(request.uri.path(), "/throttled");
                assert_eq!(event.guard, "web");
                self.0.store(event.retry_after, Ordering::Relaxed);
                Ok(None)
            })
        }
    }

    fn attempt(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let credentials = Credentials::new([("email", "a@example.test".into())], "wrong");
            let authenticated = Auth::session_guard(request, None)?
                .attempt_throttled(&credentials, false, "fixture", 1, 60)
                .await?;
            assert!(!authenticated);
            Err(HttpError::Unauthenticated)
        })
    }

    let retry = Arc::new(AtomicU64::new(0));
    let recorded = retry.clone();
    let (address, server) = start(
        || {
            Route::post("/throttled", attempt)?;
            Ok(())
        },
        || Ok(()),
        move |app| {
            app.container()
                .make::<Dispatcher>()
                .unwrap()
                .listen("Lockout", Arc::new(Record(recorded)), Delivery::Sync)
                .unwrap();
        },
    )
    .await;
    send(address, "POST", "/throttled", "", "", 401).await;
    assert_eq!(retry.load(Ordering::Relaxed), 0);
    let blocked = send(address, "POST", "/throttled", "", "", 429).await;
    let retry = retry.load(Ordering::Relaxed);
    assert!((1..=60).contains(&retry));
    assert!(blocked.headers.contains(&format!("retry-after: {retry}")));
    server.abort();
}

#[path = "extensions/session_validation.rs"]
mod session_validation;
