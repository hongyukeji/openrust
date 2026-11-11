use std::{collections::BTreeMap, fs, net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Lang, Route},
    translation::{__, TranslationError, trans_choice},
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

#[path = "../config/mod.rs"]
mod config;

struct Root(PathBuf);

impl Root {
    fn new(default_locale: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "openrust-localization-{}-{default_locale}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        fs::write(
            path.join(".env"),
            format!("APP_LOCALE={default_locale}\nAPP_FALLBACK_LOCALE=en\n"),
        )
        .unwrap();
        for (locale, source) in [
            (
                "en",
                r#"{"welcome":"Hi :Name / :NAME","items":"one item|:count items","fallback":"English fallback","nested":{"values":[":name","second"]}}"#,
            ),
            (
                "zh_Hans",
                r#"{"welcome":"你好，:name","items":"{0} 没有项目|[1,*] :count 个项目"}"#,
            ),
            (
                "zh_Hant",
                r#"{"welcome":"您好，:name","items":"{0} 沒有項目|[1,*] :count 個項目"}"#,
            ),
        ] {
            let directory = path.join("lang").join(locale);
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("messages.json"), source).unwrap();
        }
        fs::write(path.join("lang/zh_Hans.json"), r#"{"Hello":"你好"}"#).unwrap();
        fs::write(
            path.join("lang/en.json"),
            r#"{"English only":"Only in English"}"#,
        )
        .unwrap();
        Self(path)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn translate(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let locale = request.decodedPath().into_owned();
    Box::pin(async move {
        let outer = Lang::getLocale().map_err(http_error)?;
        let body = Lang::withLocale(&locale, async {
            // Yield while the override is installed to exercise task isolation.
            tokio::task::yield_now().await;
            let args = BTreeMap::from([("name".to_owned(), "ada".to_owned())]);
            Ok::<_, TranslationError>(json!({
                "greeting": __("messages.welcome", &args)?,
                "choice": trans_choice("messages.items", 2_i64, &args)?,
                "fallback": __("messages.fallback", &args)?,
                "literal": __("Hello", &args)?,
                "literal_miss": __("English only", &args)?,
                "unknown": __("missing.key", &args)?,
                "nested": __("messages.nested.values.0", &args)?,
            }))
        })
        .await
        .map_err(http_error)?
        .map_err(http_error)?;
        assert_eq!(Lang::getLocale().map_err(http_error)?, outer);
        Ok(Response::new(StatusCode::OK, Body::from(body.to_string())))
    })
}

fn default_locale(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let locale = Lang::getLocale().map_err(http_error)?;
        Ok(Response::new(StatusCode::OK, Body::from(locale)))
    })
}

fn http_error(source: TranslationError) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    let (parts, body) = incoming.into_parts();
    let (parts, body) = kernel
        .handle(&mut Request::from_parts(parts, body))
        .await
        .into_parts();
    axum::response::Response::from_parts(parts, body)
}

async fn start(root: &Root) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let application = Application::configure(root.0.clone())
        .withConfig(|base, environment| {
            let mut config = config::load(base, environment)?;
            config.get_mut("app").unwrap()["key"] = "a".repeat(32).into();
            config.get_mut("session").unwrap()["driver"] = "array".into();
            Ok(config)
        })
        .withRouting(Routing::default().web(|| {
            for route in ["/en", "/zh_Hans", "/zh_Hant"] {
                Route::get(route, translate)?;
            }
            Route::get("/default", default_locale)
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    (
        address,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
    )
}

async fn send(address: SocketAddr, path: &str) -> String {
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    socket
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
    response.split_once("\r\n\r\n").unwrap().1.to_owned()
}

#[tokio::test]
async fn localizes_http_requests_with_isolated_defaults_and_fallback() {
    let english = Root::new("en");
    let traditional = Root::new("zh_Hant");
    let ((first, one), (second, two)) = tokio::join!(start(&english), start(&traditional));
    let (en, hans, hant) = tokio::join!(
        send(first, "/en"),
        send(first, "/zh_Hans"),
        send(second, "/zh_Hant")
    );
    let parse = |value: &str| serde_json::from_str::<Value>(value).unwrap();
    let (en, hans, hant) = (parse(&en), parse(&hans), parse(&hant));
    assert_eq!(en["greeting"], "Hi Ada / ADA");
    assert_eq!(en["choice"], "2 items");
    assert_eq!(hans["greeting"], "你好，ada");
    assert_eq!(hans["choice"], "2 个项目");
    assert_eq!(hans["literal"], "你好");
    assert_eq!(hans["literal_miss"], "English only");
    assert_eq!(hant["greeting"], "您好，ada");
    assert_eq!(hant["choice"], "2 個項目");
    for output in [en, hans, hant] {
        assert_eq!(output["fallback"], "English fallback");
        assert_eq!(output["unknown"], "missing.key");
        assert_eq!(output["nested"], "ada");
    }
    assert_eq!(send(first, "/default").await, "en");
    assert_eq!(send(second, "/default").await, "zh_Hant");
    one.abort();
    two.abort();
}

#[tokio::test]
async fn loads_package_overrides_and_keeps_invalid_catalogs_retryable() {
    use openrust::{
        contracts::translation::Loader as _,
        translation::{FileLoader, Line, Translator},
    };

    let root = Root::new("loader");
    let lang = root.0.join("lang");
    let package = root.0.join("package");
    fs::create_dir_all(package.join("en")).unwrap();
    fs::create_dir_all(lang.join("vendor/shop/en")).unwrap();
    fs::write(
        package.join("en/cart.json"),
        r#"{"summary":{"heading":"Package","notice":"For :name"}}"#,
    )
    .unwrap();
    fs::write(
        lang.join("vendor/shop/en/cart.json"),
        r#"{"summary":{"heading":"Override"}}"#,
    )
    .unwrap();
    let mut loader = FileLoader::new(lang.clone());
    loader.addNamespace("shop", package);
    assert!(matches!(
        loader.load("../en", "cart", Some("shop")).await,
        Err(TranslationError::InvalidLocale)
    ));
    assert!(matches!(
        loader.load("en", "sub/../cart", Some("shop")).await,
        Err(TranslationError::InvalidKey)
    ));
    let translator = Translator::new(loader, "en".into(), "en".into()).unwrap();
    let replacements = BTreeMap::from([("name".to_owned(), "Ada".to_owned())]);
    assert_eq!(
        translator
            .array("shop::cart.summary", &replacements, None)
            .unwrap(),
        BTreeMap::from([
            ("heading".into(), Line::Text("Override".into())),
            ("notice".into(), Line::Text("For Ada".into())),
        ])
    );
    let broken = lang.join("en/broken.json");
    fs::write(&broken, "{invalid").unwrap();
    assert!(matches!(
        translator.string("broken.value", &replacements, None),
        Err(TranslationError::Source(_))
    ));
    fs::write(&broken, r#"{"value":"fixed"}"#).unwrap();
    assert_eq!(
        translator
            .string("broken.value", &replacements, None)
            .unwrap(),
        "fixed"
    );
}
