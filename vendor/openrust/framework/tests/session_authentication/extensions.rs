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
        .withConfig(|_, _| {
            Ok([
                ("app".into(), json!({"key":"a".repeat(32)})),
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
        .withRouting(Routing::default().web(web).api(api))
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
        Auth::onceBasic(request, "email", &[]).await?;
        current(request).await
    })
}

#[tokio::test]
async fn http_basic_supports_session_and_stateless_authentication() {
    let (address, server) = start(
        || {
            Route::getWith("/basic", current, |route| {
                route.middleware_name("auth.basic");
                Ok(())
            })?;
            Route::getWith("/basic-custom", current, |route| {
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
