use openrust::{
    config::{ConfigError, Environment, Value},
    console::{Application as Artisan, Output},
    contracts::{console::Kernel as _, foundation::Application as _},
    foundation::{Application, console::Kernel},
};
use openrust_passport::{Passport, PassportServiceProvider, PrivateKey, PublicKey};
use rsa::{
    PrivateKey as RsaPrivateKey, PublicKey as RsaPublicKey,
    pkcs8::{DecodePrivateKey, DecodePublicKey},
    traits::PublicKeyParts,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "passport-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("storage")).unwrap();
        fs::write(path.join(".env"), "APP_ENV=testing\nPASSPORT_CONNECTION=(null)\nPASSPORT_PRIVATE_KEY=(null)\nPASSPORT_PUBLIC_KEY=null\n").unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn application(&self) -> Application {
        Application::configure(self.0.clone())
            .withConfig(configuration)
            .withProviders(vec![Box::new(PassportServiceProvider)])
            .create()
            .unwrap()
    }
    fn console(&self) -> Kernel {
        Kernel::new(self.application(), Artisan::new())
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn configuration(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    Ok(BTreeMap::from([
        (
            "cache".into(),
            json!({"default":"array","stores":{"array":{"driver":"array"}}}),
        ),
        (
            "app".into(),
            json!({"env":"testing","host":"127.0.0.1","port":0,"key":"k".repeat(32)}),
        ),
        (
            "passport".into(),
            json!({"guard":"staff","middleware":["custom"],"path":"nested/oauth"}),
        ),
    ]))
}
async fn run(kernel: &mut Kernel, args: &[&str]) -> (i32, Output) {
    let args: Vec<OsString> = args.iter().map(Into::into).collect();
    let mut output = Output::default();
    let status = kernel.handle(&args, &mut output).await;
    (status, output)
}
fn contents(root: &Root) -> (String, String) {
    (
        fs::read_to_string(root.0.join("storage/oauth-public.key")).unwrap(),
        fs::read_to_string(root.0.join("storage/oauth-private.key")).unwrap(),
    )
}

#[tokio::test]
async fn command_keys_are_lazy_overwrite_is_explicit_and_native_pair_is_valid() {
    let root = Root::new();
    let mut console = root.console();
    console.bootstrap().await.unwrap();
    let app = console.get_application();
    assert!(!root.0.join("storage/oauth-private.key").exists());
    assert_eq!(
        app.config().unwrap().get("passport.guard"),
        Some(json!("staff"))
    );
    assert_eq!(
        app.config().unwrap().get("passport.middleware"),
        Some(json!(["custom"]))
    );
    assert_eq!(
        app.config().unwrap().get("passport.connection"),
        Some(Value::Null)
    );
    assert!(app.make::<PrivateKey>().await.is_err());
    let (status, output) = run(&mut console, &["passport:keys"]).await;
    assert_eq!(status, 0, "{}", output.stderr);
    assert!(
        output
            .stdout
            .contains("Encryption keys generated successfully.")
    );
    let (public, private) = contents(&root);
    let key = RsaPrivateKey::from_pkcs8_pem(&private).unwrap();
    let public_key = RsaPublicKey::from_public_key_pem(&public).unwrap();
    assert_eq!(key.n().bit_width_vartime(), 4096);
    assert_eq!(key.n(), public_key.n());
    let app = console.get_application();
    let key = app.make::<PrivateKey>().await.unwrap();
    let pubkey = app.make::<PublicKey>().await.unwrap();
    assert_eq!(key.getKeyContents(), private);
    assert_eq!(pubkey.getKeyContents(), public);
    assert_eq!(format!("{key:?}"), "PrivateKey(CryptKey([REDACTED]))");
    assert_eq!(
        key.getKeyPath().unwrap(),
        root.0.join("storage/oauth-private.key")
    );
    let (status, output) = run(&mut console, &["passport:keys"]).await;
    assert_eq!(status, 1);
    assert!(output.stdout.contains("Encryption keys already exist"));
    assert_eq!(contents(&root), (public.clone(), private.clone()));
    let (status, _) = run(&mut console, &["passport:keys", "--force", "--length=2048"]).await;
    assert_eq!(status, 0);
    let next = contents(&root);
    assert_ne!(next.0, public);
    assert_eq!(
        RsaPrivateKey::from_pkcs8_pem(&next.1)
            .unwrap()
            .n()
            .bit_width_vartime(),
        2048
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(root.0.join("storage/oauth-private.key"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(root.0.join("storage/oauth-public.key"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o660
        );
    }
}

#[tokio::test]
async fn publication_is_explicit_preserves_overrides_and_only_implemented_commands_are_registered()
{
    let mut app = openrust::testing::TestApp::create(|builder| {
        builder
            .withConfig(configuration)
            .withProviders(vec![Box::new(PassportServiceProvider)])
    })
    .unwrap();
    app.artisan(["vendor:publish", "--tag=passport-config"])
        .await
        .assertSuccessful();
    let path = app.root().join("config/passport.rs");
    let source = fs::read_to_string(&path).unwrap();
    assert!(source.contains("PASSPORT_PRIVATE_KEY"));
    fs::write(&path, "// application override").unwrap();
    app.artisan(["vendor:publish", "--tag=passport-config"])
        .await
        .assertSuccessful();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "// application override"
    );
    app.artisan(["vendor:publish", "--tag=passport-config", "--force"])
        .await
        .assertSuccessful();
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    app.artisan(["vendor:publish", "--tag=passport-migrations"])
        .await
        .assertSuccessful();
    let directory = app.root().join("database/migrations");
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 5);
    app.artisan(["vendor:publish", "--tag=passport-migrations"])
        .await
        .assertSuccessful();
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 5);
    let listing = app.artisan(["list"]).await;
    listing.assertSuccessful();
    assert!(listing.stdout.contains("passport:keys"));
    assert!(listing.stdout.contains("passport:client"));
}

#[tokio::test]
async fn configured_key_directories_and_inline_pem_are_application_local() {
    let first = Root::new();
    let second = Root::new();
    let mut a = first.console();
    let mut b = second.console();
    let (ra, rb) = tokio::join!(a.bootstrap(), b.bootstrap());
    ra.unwrap();
    rb.unwrap();
    let left = a.get_application().make::<Passport>().await.unwrap();
    let right = b.get_application().make::<Passport>().await.unwrap();
    let custom = first.0.join("custom");
    fs::create_dir(&custom).unwrap();
    left.loadKeysFrom(&custom);
    assert_eq!(
        left.keyPath("/\\oauth-private.key"),
        custom.join("oauth-private.key")
    );
    assert_eq!(
        right.keyPath("oauth-private.key"),
        second.0.join("storage/oauth-private.key")
    );
    assert_eq!(run(&mut a, &["passport:keys", "--length=1024"]).await.0, 0);
    let private = fs::read_to_string(custom.join("oauth-private.key")).unwrap();
    let public = fs::read_to_string(custom.join("oauth-public.key")).unwrap();
    assert_eq!(
        RsaPrivateKey::from_pkcs8_pem(&private)
            .unwrap()
            .n()
            .bit_width_vartime(),
        1024
    );
    b.get_application()
        .config()
        .unwrap()
        .set("passport.private_key", json!(private.replace('\n', "\\n")))
        .unwrap();
    b.get_application()
        .config()
        .unwrap()
        .set("passport.public_key", json!(public.replace('\n', "\\n")))
        .unwrap();
    assert_eq!(
        b.get_application()
            .make::<PrivateKey>()
            .await
            .unwrap()
            .getKeyContents(),
        private
    );
    assert_eq!(
        b.get_application()
            .make::<PublicKey>()
            .await
            .unwrap()
            .getKeyContents(),
        public
    );
    assert!(!second.0.join("storage/oauth-private.key").exists());
    assert_eq!(
        a.get_application()
            .make::<PrivateKey>()
            .await
            .unwrap()
            .getKeyContents(),
        private
    );
    assert!(right.keyPath("oauth-private.key").starts_with(&second.0));
}

#[tokio::test]
async fn invalid_lengths_and_unwritable_targets_fail_without_disclosing_key_material() {
    let root = Root::new();
    let mut console = root.console();
    assert_eq!(
        run(&mut console, &["passport:keys", "--length=invalid"])
            .await
            .0,
        1
    );
    assert!(!root.0.join("storage/oauth-private.key").exists());
    console
        .get_application()
        .make::<Passport>()
        .await
        .unwrap()
        .loadKeysFrom(root.0.join("missing/keys"));
    let (status, output) = run(&mut console, &["passport:keys", "--length=2048"]).await;
    assert_eq!(status, 1);
    assert!(!output.stderr.contains("BEGIN PRIVATE KEY"));
    assert!(!output.stderr.contains(root.0.to_str().unwrap()));
    let config = console.get_application().config().unwrap();
    config.set("passport.private_key", json!(true)).unwrap();
    assert!(
        console
            .get_application()
            .make::<PrivateKey>()
            .await
            .is_err()
    );
    config
        .set(
            "passport.private_key",
            json!("-----BEGIN PRIVATE KEY-----\ninvalid\n-----END PRIVATE KEY-----"),
        )
        .unwrap();
    assert!(
        console
            .get_application()
            .make::<PrivateKey>()
            .await
            .is_err()
    );
}

fn routes(_: &Application) -> Result<openrust::routing::Router, openrust::routing::RoutingError> {
    let mut router = openrust::routing::Router::new();
    router.get("/probe", probe)?;
    Ok(router)
}
fn probe(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        use openrust::contracts::container::Container as _;
        let container = openrust::container::Container::getInstance();
        let key = container.make::<PrivateKey>()?;
        assert!(!format!("{key:?}").contains("BEGIN"));
        Ok(openrust::http::Response::new(
            openrust::http::StatusCode::OK,
            openrust::http::Body::from("key loaded"),
        ))
    })
}
struct CustomMiddleware;
impl openrust::contracts::http::Middleware for CustomMiddleware {
    fn handle<'a>(
        &'a self,
        request: &'a mut openrust::http::Request,
        next: openrust::http::Next<'a>,
    ) -> openrust::http::HttpFuture<'a, Result<openrust::http::Response, openrust::http::HttpError>>
    {
        Box::pin(async move {
            Ok(next.run(request).await.header(
                axum::http::HeaderName::from_static("x-passport-custom"),
                axum::http::HeaderValue::from_static("ran"),
            ))
        })
    }
}
async fn serve(root: &Root) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(root.0.clone())
        .withConfig(configuration)
        .withProviders(vec![Box::new(PassportServiceProvider)])
        .withRouting(openrust::foundation::configuration::Routing::new(routes))
        .withMiddleware(|middleware| {
            middleware.alias("custom", std::sync::Arc::new(CustomMiddleware));
        })
        .withExceptions(|exceptions| {
            exceptions.shouldRenderJsonWhen(|request, _| request.expectsJson());
        })
        .create()
        .unwrap();
    let mut kernel = openrust::foundation::http::Kernel::new(app);
    let listener = kernel.listen().await.unwrap();
    let address = listener.local_addr().unwrap();
    let kernel = std::sync::Arc::new(kernel);
    let router = axum::Router::new()
        .fallback(move |request: axum::extract::Request| kernel.clone().handleTransport(request));
    (
        address,
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        }),
    )
}
#[tokio::test]
async fn real_http_key_resolution_errors_recovery_and_two_applications_are_isolated() {
    let a = Root::new();
    let b = Root::new();
    let mut cli = a.console();
    assert_eq!(
        run(&mut cli, &["passport:keys", "--length=2048"]).await.0,
        0
    );
    let (first, second) = tokio::join!(serve(&a), serve(&b));
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .get(format!("http://{}/probe", first.0))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let response = client
        .get(format!("http://{}/probe", second.0))
        .header("Accept", "application/json")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 500);
    let body = response.text().await.unwrap();
    assert!(!body.contains("BEGIN"));
    serde_json::from_str::<serde_json::Value>(&body).unwrap();
    let response = client
        .get(format!("http://{}/probe", second.0))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 500);
    assert!(
        response.headers()["content-type"]
            .to_str()
            .unwrap()
            .contains("text/html")
    );
    for address in [first.0, second.0] {
        let response = client
            .post(format!("http://{address}/nested/oauth/token"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400, "{}", response.text().await.unwrap());
        assert_eq!(response.headers()["x-passport-custom"], "ran");
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap()["error"],
            "unsupported_grant_type"
        );
    }
    let mut other = b.console();
    assert_eq!(
        run(&mut other, &["passport:keys", "--length=2048"]).await.0,
        0
    );
    assert_eq!(
        client
            .get(format!("http://{}/probe", second.0))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        client
            .get(format!("http://{}/probe", first.0))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert!(contents(&a) != contents(&b));
    first.1.abort();
    second.1.abort();
    let _ = tokio::join!(first.1, second.1);
}

#[cfg(unix)]
#[tokio::test]
async fn key_permission_validation_can_be_disabled_per_application() {
    use std::os::unix::fs::PermissionsExt;
    let root = Root::new();
    let mut cli = root.console();
    assert_eq!(
        run(&mut cli, &["passport:keys", "--length=2048"]).await.0,
        0
    );
    let private = root.0.join("storage/oauth-private.key");
    fs::set_permissions(&private, fs::Permissions::from_mode(0o644)).unwrap();
    let mut a = root.application();
    let mut b = root.application();
    a.boot().await.unwrap();
    b.boot().await.unwrap();
    assert!(a.make::<PrivateKey>().await.is_err());
    b.make::<Passport>()
        .await
        .unwrap()
        .validateKeyPermissions
        .store(false, Ordering::Relaxed);
    assert!(b.make::<PrivateKey>().await.is_ok());
    assert!(a.make::<PrivateKey>().await.is_err());
    fs::set_permissions(&private, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(a.make::<PrivateKey>().await.is_ok());
}

#[tokio::test]
async fn competing_non_force_generators_leave_one_complete_matching_pair() {
    let root = Root::new();
    let mut a = root.console();
    let mut b = root.console();
    let (a, b) = tokio::join!(
        run(&mut a, &["passport:keys", "--length=2048"]),
        run(&mut b, &["passport:keys", "--length=2048"])
    );
    assert!(matches!((a.0, b.0), (0, 1) | (1, 0)));
    let (public, private) = contents(&root);
    let private = RsaPrivateKey::from_pkcs8_pem(&private).unwrap();
    let public = RsaPublicKey::from_public_key_pem(&public).unwrap();
    assert_eq!(private.n(), public.n());
}

#[tokio::test]
async fn reserved_environment_values_use_the_same_defaults_as_published_configuration() {
    let root = Root::new();
    fs::write(root.0.join(".env"),"APP_ENV=testing\nPASSPORT_PRIVATE_KEY=false\nPASSPORT_PUBLIC_KEY=(false)\nPASSPORT_CONNECTION=null\n").unwrap();
    let mut cli = root.console();
    assert_eq!(
        run(&mut cli, &["passport:keys", "--length=2048"]).await.0,
        0
    );
    assert_eq!(
        cli.get_application()
            .config()
            .unwrap()
            .get("passport.private_key"),
        Some(json!(false))
    );
    assert!(cli.get_application().make::<PrivateKey>().await.is_ok());
    assert!(cli.get_application().make::<PublicKey>().await.is_ok());
}

mod documented {
    include!("fixtures/configure_keys.rs");
}
#[tokio::test]
async fn documented_provider_key_configuration_example_compiles_and_generates() {
    let root = Root::new();
    let mut cli = root.console();
    cli.bootstrap().await.unwrap();
    fs::create_dir(root.0.join("storage/keys")).unwrap();
    documented::configure_keys(cli.get_application())
        .await
        .unwrap();
    assert_eq!(
        run(&mut cli, &["passport:keys", "--length=2048"]).await.0,
        0
    );
    assert!(root.0.join("storage/keys/oauth-private.key").is_file());
    assert!(!root.0.join("storage/oauth-private.key").exists());
}

#[tokio::test]
async fn file_uri_and_localhost_are_loaded_without_binding_relative_paths_to_app_root() {
    let root = Root::new();
    let mut cli = root.console();
    assert_eq!(
        run(&mut cli, &["passport:keys", "--length=2048"]).await.0,
        0
    );
    let private = root.0.join("storage/oauth-private.key");
    let public = root.0.join("storage/oauth-public.key");
    let mut other = root.application();
    other.boot().await.unwrap();
    other
        .config()
        .unwrap()
        .set(
            "passport.private_key",
            json!(format!("file://{}", private.display())),
        )
        .unwrap();
    other
        .config()
        .unwrap()
        .set("passport.public_key", json!(public.to_str().unwrap()))
        .unwrap();
    assert!(other.make::<PrivateKey>().await.is_ok());
    assert!(other.make::<PublicKey>().await.is_ok());
    #[cfg(unix)]
    {
        let mut local = root.application();
        local.boot().await.unwrap();
        local
            .config()
            .unwrap()
            .set(
                "passport.private_key",
                json!(format!("file://localhost{}", private.display())),
            )
            .unwrap();
        assert!(local.make::<PrivateKey>().await.is_ok());
        let mut relative = root.application();
        relative.boot().await.unwrap();
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let mut existing_relative = PathBuf::new();
        for component in cwd.components() {
            if matches!(component, std::path::Component::Normal(_)) {
                existing_relative.push("..");
            }
        }
        existing_relative.push(private.strip_prefix("/").unwrap());
        assert!(existing_relative.is_file());
        relative
            .config()
            .unwrap()
            .set(
                "passport.private_key",
                json!(existing_relative.to_str().unwrap()),
            )
            .unwrap();
        assert!(relative.make::<PrivateKey>().await.is_err());
    }
}
