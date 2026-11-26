use std::{collections::BTreeMap, fs, net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    localization::{__, Line, Replacement, TranslationError, trans_choice},
    support::facades::{Lang, Route},
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

#[path = "support/config_app.rs"]
mod app;

#[path = "../config/mod.rs"]
mod config;

struct Root(PathBuf);

impl Root {
    fn new(default_locale: &str) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT_ROOT: AtomicUsize = AtomicUsize::new(0);

        let path = std::env::temp_dir().join(format!(
            "openrust-localization-{}-{default_locale}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed),
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
                r#"{"welcome":"Hi :Name / :NAME","items":"one item|:count items","fallback":"English fallback","nested":{"values":[":name","second"]},"identity":"messages.identity","empty":[]}"#,
            ),
            (
                "zh-Hans",
                r#"{"welcome":"你好，:name","items":"{0} 没有项目|[1,*] :count 个项目"}"#,
            ),
            (
                "zh-Hant",
                r#"{"welcome":"您好，:name","items":"{0} 沒有項目|[1,*] :count 個項目"}"#,
            ),
        ] {
            let directory = path.join("lang").join(locale);
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("messages.json"), source).unwrap();
        }
        fs::write(
            path.join("lang/zh-Hans.json"),
            r#"{"Hello":"你好","Literal Zero":"0"}"#,
        )
        .unwrap();
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
    let locale = request.decoded_path().into_owned();
    Box::pin(async move {
        let outer = Lang::get_locale()?;
        let body = Lang::with_locale(&locale, async {
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
        .await??;
        assert_eq!(Lang::get_locale()?, outer);
        Ok(Response::new(StatusCode::OK, Body::from(body.to_string())))
    })
}

fn default_locale(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let locale = Lang::get_locale()?;
        Ok(Response::new(StatusCode::OK, Body::from(locale)))
    })
}

fn install_lookup_hooks(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Lang::determine_locales_using(|locales| {
            ["zh-Hant".into(), "zh-Hant".into()]
                .into_iter()
                .chain(locales.iter().cloned())
                .collect()
        })?;
        Lang::handle_missing_keys_using(|missing| {
            if missing.key == "missing.plural" {
                assert!(missing.replacements.is_empty());
                return Some("{1} one|[2,*] :count selected".into());
            }
            let nested = Lang::get("nested.missing", &BTreeMap::new(), None).unwrap();
            Some(format!("{}|{}|{nested}|:Name", missing.key, missing.locale))
        })?;
        Ok(Response::new(StatusCode::OK, Body::from("installed")))
    })
}

fn lookup_hooks(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let replacements = BTreeMap::from([("name".into(), "ada".into())]);
        let lines = (|| -> Result<Value, TranslationError> {
            Ok(json!({
                "chosen": Lang::get("messages.welcome", &replacements, None)?,
                "literal": Lang::get("Hello", &replacements, Some("zh-Hans"))?,
                "missing": Lang::get("missing.key", &replacements, None)?,
                "missing_exists": Lang::has("missing.key", None, true)?,
                "literal_zero_exists": Lang::has_for_locale("Literal Zero", Some("zh-Hans"))?,
                "exact": Lang::has_for_locale("messages.welcome", None)?,
                "fallback": Lang::has("messages.welcome", None, true)?,
                "plural": Lang::choice("missing.plural", 2_i64, &replacements, None)?,
            }))
        })()?;
        Response::json(StatusCode::OK, lines)
    })
}

struct NoticeLabel(&'static str);

impl std::fmt::Display for NoticeLabel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

fn install_translation_replacements(
    _: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Lang::stringable(|label: &NoticeLabel| format!("custom {}", label.0))?;
        Lang::handle_missing_keys_using(|missing| match missing.key {
            "replacements.missing" => {
                assert_eq!(missing.replacements.get("name").unwrap(), "Ada");
                assert_eq!(
                    missing.mixed_replacements[1]
                        .1
                        .as_object::<NoticeLabel>()
                        .unwrap()
                        .0,
                    "status"
                );
                assert!(missing.mixed_replacements[2].1.as_tag().is_some());
                Some("<mark>:name</mark> :LABEL".into())
            }
            "replacements.items" => {
                assert!(missing.mixed_replacements.is_empty());
                Some("{1} <unused>one</unused>|[2,*] <mark>:count :label</mark>".into())
            }
            _ => None,
        })?;
        Ok(Response::new(StatusCode::OK, Body::from("installed")))
    })
}

fn mixed_translation(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let label = NoticeLabel("status");
        let args = [
            ("name", "Ada".into()),
            ("label", Replacement::display(&label)),
            ("mark", Replacement::tag(|content| format!("[{content}]"))),
            (
                "unused",
                Replacement::tag(|_| panic!("unchosen branch was formatted")),
            ),
        ];
        let result = (|| -> Result<Value, TranslationError> {
            Ok(json!({
                "missing": Lang::get_with("replacements.missing", &args, None)?,
                "choice": Lang::choice_with("replacements.items", 2_i64, &args, None)?,
                "label": Lang::get_with(":label", &args, None)?,
            }))
        })()?;
        Response::json(StatusCode::OK, result)
    })
}

fn register_runtime_lines(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use openrust::localization::Line;
        Lang::load(None, "messages", "zh-Hans")?;
        Lang::add_lines(
            [("messages.welcome".into(), Line::Text("运行期 :name".into()))],
            "zh-Hans",
            None,
        )?;
        Ok(Response::new(StatusCode::OK, Body::from("installed")))
    })
}

fn load_external_lines(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use openrust::localization::TranslationResource;
        Lang::load_from(
            &ExternalLoader,
            &[TranslationResource {
                locale: "zh-Hant",
                group: "messages",
                namespace: None,
            }],
        )
        .await?;
        Ok(Response::new(StatusCode::OK, Body::from("loaded")))
    })
}

struct ExternalLoader;

impl openrust::contracts::localization::Loader for ExternalLoader {
    fn load<'a>(
        &'a self,
        locale: &'a str,
        group: &'a str,
        namespace: Option<&'a str>,
    ) -> openrust::localization::TranslationFuture<
        'a,
        openrust::localization::TranslationResult<openrust::localization::Lines>,
    > {
        Box::pin(async move {
            tokio::task::yield_now().await;
            assert_eq!((locale, group, namespace), ("zh-Hant", "messages", None));
            Ok(BTreeMap::from([(
                "welcome".into(),
                openrust::localization::Line::Text("外部 :name".into()),
            )]))
        })
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
        .with_config(|base, environment| {
            let mut config = config::load(base, environment)?;
            config.get_mut("app").unwrap()["key"] = "a".repeat(32).into();
            config.get_mut("session").unwrap()["driver"] = "array".into();
            Ok(config)
        })
        .with_middleware(|middleware| {
            // 本组验证配置默认语言及译器钩子，不启用浏览器识别。
            middleware.set_locale(&[]).unwrap();
        })
        .with_routing(Routing::default().web(|| {
            for route in ["/en", "/zh-Hans", "/ZH-hans", "/zh-Hant", "/ZH-hant"] {
                Route::get(route, translate)?;
            }
            Route::get("/install-hooks", install_lookup_hooks)?;
            Route::get("/hooks", lookup_hooks)?;
            Route::get("/install-replacements", install_translation_replacements)?;
            Route::get("/mixed-translation", mixed_translation)?;
            Route::get("/register-runtime-lines", register_runtime_lines)?;
            Route::get("/load-external-lines", load_external_lines)?;
            Route::get("/default", default_locale)?;
            Ok(())
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
    let traditional = Root::new("zh-Hant");
    let ((first, one), (second, two)) = tokio::join!(start(&english), start(&traditional));
    let (en, hans, hant) = tokio::join!(
        send(first, "/en"),
        send(first, "/zh-Hans"),
        send(second, "/zh-Hant")
    );
    let parse = |value: &str| serde_json::from_str::<Value>(value).unwrap();
    let (en, hans, hant) = (parse(&en), parse(&hans), parse(&hant));
    assert_eq!(en["greeting"], "Hi Ada / ADA");
    assert_eq!(en["choice"], "2 items");
    assert_eq!(hans["greeting"], "你好，ada");
    assert_eq!(hans["choice"], "2 个项目");
    assert_eq!(hans["literal"], "你好");
    assert_eq!(hans["literal_miss"], "English only");
    for alias in ["/zh-Hans", "/ZH-hans"] {
        assert_eq!(parse(&send(first, alias).await), hans);
    }
    for alias in ["/zh-Hant", "/ZH-hant"] {
        assert_eq!(parse(&send(second, alias).await), hant);
    }
    assert_eq!(hant["greeting"], "您好，ada");
    assert_eq!(hant["choice"], "2 個項目");
    for output in [en, hans, hant] {
        assert_eq!(output["fallback"], "English fallback");
        assert_eq!(output["unknown"], "missing.key");
        assert_eq!(output["nested"], "ada");
    }
    assert_eq!(send(first, "/default").await, "en");
    assert_eq!(send(second, "/default").await, "zh-Hant");
    one.abort();
    two.abort();
}

#[test]
fn normalizes_bcp47_without_merging_region_resources() {
    use openrust::localization::{FileLoader, Localizer};

    let root = Root::new("locale-aliases");
    let lang = root.0.join("lang");
    let localizer =
        Localizer::new(FileLoader::new(lang.clone()), "zh-Hans".into(), "en".into()).unwrap();
    let args = BTreeMap::new();
    assert_eq!(localizer.locale(), "zh-Hans");
    assert_eq!(localizer.string("Hello", &args, None).unwrap(), "你好");
    assert_eq!(
        localizer.string("messages.fallback", &args, None).unwrap(),
        "English fallback"
    );
    for alias in ["zh-Hans", "ZH-hans"] {
        assert_eq!(
            localizer.string("Hello", &args, Some(alias)).unwrap(),
            "你好"
        );
    }
    localizer.set_locale("zh-CN").unwrap();
    assert_eq!(localizer.get_locale(), "zh-CN");
    assert_eq!(localizer.string("Hello", &args, None).unwrap(), "Hello");
    assert!(localizer.set_locale("zh_CN").is_err());
    localizer.set_locale("zh-SG").unwrap();
    assert_eq!(localizer.get_locale(), "zh-SG");
    assert!(localizer.set_locale("../zh-CN").is_err());
    assert_eq!(localizer.get_locale(), "zh-SG");

    for (input, canonical) in [
        ("EN-gb", "en-GB"),
        ("en-us", "en-US"),
        ("pt-PT", "pt-PT"),
        ("de-de", "de-DE"),
        ("ko-KR", "ko-KR"),
        ("FR-ca", "fr-CA"),
        ("pt-br", "pt-BR"),
        ("es-419", "es-419"),
        ("ja-JP", "ja-JP"),
        ("sr-latn-rs", "sr-Latn-RS"),
        ("zh-HK", "zh-HK"),
        ("zh-Hant-HK", "zh-Hant-HK"),
        ("zh-hant", "zh-Hant"),
    ] {
        let directory = lang.join(canonical);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("identity.json"),
            format!(r#"{{"locale":"{canonical}"}}"#),
        )
        .unwrap();
        localizer.set_locale(input).unwrap();
        assert_eq!(localizer.get_locale(), canonical);
        assert_eq!(
            localizer
                .string("identity.locale", &args, Some(input))
                .unwrap(),
            canonical
        );
    }
    assert!(localizer.string("Hello", &args, Some("en--GB")).is_err());

    let localizer = Localizer::new(FileLoader::new(lang), "en".into(), "zh-Hans".into()).unwrap();
    assert_eq!(
        localizer
            .string("messages.welcome", &args, Some("fr"))
            .unwrap(),
        "你好，:name"
    );
    assert_eq!(
        localizer.string("Hello", &args, Some("fr")).unwrap(),
        "Hello"
    );
}

#[tokio::test]
async fn loads_package_overrides_and_keeps_invalid_catalogs_retryable() {
    use openrust::{
        contracts::localization::Loader as _,
        localization::{FileLoader, Line, Localizer},
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
    loader.add_namespace("shop", package);
    assert!(matches!(
        loader.load("../en", "cart", Some("shop")).await,
        Err(TranslationError::InvalidLocale)
    ));
    assert!(matches!(
        loader.load("en", "sub/../cart", Some("shop")).await,
        Err(TranslationError::InvalidKey)
    ));
    let localizer = Localizer::new(loader, "en".into(), "en".into()).unwrap();
    let replacements = BTreeMap::from([("name".to_owned(), "Ada".to_owned())]);
    assert_eq!(
        localizer
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
        localizer.string("broken.value", &replacements, None),
        Err(TranslationError::Source(_))
    ));
    fs::write(&broken, r#"{"value":"fixed"}"#).unwrap();
    assert_eq!(
        localizer
            .string("broken.value", &replacements, None)
            .unwrap(),
        "fixed"
    );
}

#[tokio::test]
async fn translation_lookup_hooks_are_local_to_each_http_application() {
    let configured = Root::new("en-GB");
    let untouched = Root::new("en-US");
    let ((first, one), (second, two)) = tokio::join!(start(&configured), start(&untouched));
    assert_eq!(send(first, "/install-hooks").await, "installed");
    let (changed, default) = tokio::join!(send(first, "/hooks"), send(second, "/hooks"));
    let changed: Value = serde_json::from_str(&changed).unwrap();
    let default: Value = serde_json::from_str(&default).unwrap();
    assert_eq!(changed["chosen"], "您好，ada");
    assert_eq!(changed["literal"], "你好");
    assert_eq!(changed["missing"], "missing.key|en-GB|nested.missing|Ada");
    assert_eq!(changed["missing_exists"], false);
    assert_eq!(changed["literal_zero_exists"], true);
    assert_eq!(changed["exact"], false);
    assert_eq!(changed["fallback"], true);
    assert_eq!(changed["plural"], "2 selected");
    assert_eq!(default["chosen"], "Hi Ada / ADA");
    assert_eq!(default["missing"], "missing.key");
    assert_eq!(default["plural"], "missing.plural");
    one.abort();
    two.abort();
}

#[test]
fn translation_existence_and_hooks_preserve_exact_lookup_and_recovery() {
    use openrust::localization::{FileLoader, Localizer};
    use std::{
        panic::AssertUnwindSafe,
        sync::atomic::{AtomicUsize, Ordering},
    };

    let root = Root::new("lookup-hooks");
    let localizer = Localizer::new(
        FileLoader::new(root.0.join("lang")),
        "en".into(),
        "en".into(),
    )
    .unwrap();
    let args = BTreeMap::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    localizer.handle_missing_keys_using(move |missing| {
        assert_eq!(missing.locale, "en");
        assert!(missing.fallback);
        count.fetch_add(1, Ordering::Relaxed);
        None
    });
    for key in ["messages.identity", "messages.empty", "missing.key"] {
        assert!(!localizer.has_for_locale(key, None).unwrap());
        assert!(!localizer.has(key, None, true).unwrap());
    }
    assert!(
        localizer
            .has("Literal Zero", Some("zh-Hans"), true)
            .unwrap()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        localizer.string("missing.key", &args, None).unwrap(),
        "missing.key"
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    localizer.determine_locales_using(|_| vec!["../unsafe".into()]);
    assert!(localizer.has_for_locale("messages.welcome", None).unwrap());
    assert_eq!(
        localizer.string("Hello", &args, Some("zh-Hans")).unwrap(),
        "你好"
    );
    assert!(matches!(
        localizer.string("messages.welcome", &args, None),
        Err(TranslationError::InvalidLocale)
    ));
    localizer.clear_locale_resolver();
    localizer.handle_missing_keys_using(|_| panic!("test-only hook panic"));
    assert!(
        std::panic::catch_unwind(AssertUnwindSafe(|| localizer.string(
            "missing.key",
            &args,
            None
        )))
        .is_err()
    );
    localizer.handle_missing_keys_using(|_| Some("restored".into()));
    assert_eq!(
        localizer.string("missing.key", &args, None).unwrap(),
        "restored"
    );
    localizer.clear_missing_key_handler();
    assert_eq!(
        localizer.string("missing.key", &args, None).unwrap(),
        "missing.key"
    );
}

#[tokio::test]
async fn runtime_translation_lines_extend_a_loaded_group_in_only_one_http_application() {
    let changed = Root::new("fr-CA");
    let original = Root::new("en-CA");
    let ((first, one), (second, two)) = tokio::join!(start(&changed), start(&original));
    assert_eq!(send(first, "/register-runtime-lines").await, "installed");
    let (changed, original) = tokio::join!(send(first, "/zh-Hans"), send(second, "/zh-Hans"));
    let changed: Value = serde_json::from_str(&changed).unwrap();
    let original: Value = serde_json::from_str(&original).unwrap();
    assert_eq!(changed["greeting"], "运行期 ada");
    assert_eq!(changed["choice"], "2 个项目");
    assert_eq!(original["greeting"], "你好，ada");
    one.abort();
    two.abort();
}

#[test]
fn translation_catalogs_obey_path_order_and_runtime_line_registration() {
    use openrust::localization::{FileLoader, Line, Localizer};

    let root = Root::new("paths");
    let lang = root.0.join("lang");
    let extra = root.0.join("extra");
    let json = root.0.join("json-only");
    let package = root.0.join("package");
    for path in [
        extra.join("en"),
        extra.join("vendor/shop/en"),
        package.join("en"),
        json.clone(),
    ] {
        fs::create_dir_all(path).unwrap();
    }
    fs::write(
        lang.join("en/layered.json"),
        r#"{"labels":["first","second"],"view":{"title":"base","note":"keep"}}"#,
    )
    .unwrap();
    fs::write(extra.join("en/messages.json"), r#"{"welcome":"new file"}"#).unwrap();
    fs::write(
        extra.join("en/layered.json"),
        r#"{"labels":["overlay"],"view":{"title":"overlay"}}"#,
    )
    .unwrap();
    fs::write(lang.join("en/cold.json"), r#"{"existing":"from disk"}"#).unwrap();
    fs::write(
        package.join("en/cart.json"),
        r#"{"title":"package","detail":"package detail"}"#,
    )
    .unwrap();
    fs::write(
        extra.join("vendor/shop/en/cart.json"),
        r#"{"title":"local"}"#,
    )
    .unwrap();
    fs::write(
        json.join("en.json"),
        r#"{"Shared":{"first":"json first","extra":"not copied"},"JSON only":"first path"}"#,
    )
    .unwrap();
    fs::write(
        extra.join("en.json"),
        r#"{"Shared":{"first":"application"},"JSON only":"second path"}"#,
    )
    .unwrap();

    let localizer = Localizer::new(FileLoader::new(lang), "en".into(), "en".into()).unwrap();
    let args = BTreeMap::new();
    localizer.load(Some("*"), "messages", "EN").unwrap();
    localizer.add_path(extra);
    localizer.add_json_path(json);
    localizer.add_namespace("shop", package).unwrap();
    assert_eq!(
        localizer.string("messages.welcome", &args, None).unwrap(),
        "Hi :Name / :NAME"
    );
    assert_eq!(
        localizer.string("layered.labels.0", &args, None).unwrap(),
        "overlay"
    );
    assert_eq!(
        localizer.string("layered.labels.1", &args, None).unwrap(),
        "second"
    );
    assert_eq!(
        localizer.string("layered.view.note", &args, None).unwrap(),
        "keep"
    );
    assert_eq!(
        localizer.string("JSON only", &args, None).unwrap(),
        "second path"
    );
    assert_eq!(
        localizer.array("Shared", &args, None).unwrap(),
        BTreeMap::from([("first".into(), Line::Text("application".into()))])
    );

    localizer.load(Some("shop"), "cart", "en").unwrap();
    assert_eq!(
        localizer.string("shop::cart.title", &args, None).unwrap(),
        "local"
    );
    localizer
        .add_lines(
            [
                ("cart.title".into(), Line::Text("runtime".into())),
                ("cart.summary.stock".into(), Line::Text("ready".into())),
            ],
            "en",
            Some("shop"),
        )
        .unwrap();
    assert_eq!(
        localizer
            .string("shop::cart.summary.stock", &args, None)
            .unwrap(),
        "ready"
    );
    assert_eq!(
        localizer.string("shop::cart.title", &args, None).unwrap(),
        "runtime"
    );
    assert_eq!(
        localizer.string("shop::cart.detail", &args, None).unwrap(),
        "package detail"
    );

    localizer
        .add_lines(
            [
                (
                    "layered.labels.1".into(),
                    Line::Text("updated second".into()),
                ),
                ("layered.labels.7".into(), Line::Text("sparse".into())),
                ("cold.runtime".into(), Line::Text("registered".into())),
            ],
            "EN",
            None,
        )
        .unwrap();
    assert_eq!(
        localizer.string("layered.labels.0", &args, None).unwrap(),
        "overlay"
    );
    assert_eq!(
        localizer.string("layered.labels.1", &args, None).unwrap(),
        "updated second"
    );
    assert_eq!(
        localizer.string("layered.labels.7", &args, None).unwrap(),
        "sparse"
    );
    assert_eq!(
        localizer.string("cold.runtime", &args, None).unwrap(),
        "registered"
    );
    assert!(!localizer.has_for_locale("cold.existing", None).unwrap());

    let error = localizer
        .add_lines(
            [
                (
                    "layered.view.title".into(),
                    Line::Text("invalid batch".into()),
                ),
                ("bad-key".into(), Line::Text("invalid".into())),
            ],
            "en",
            None,
        )
        .unwrap_err();
    assert!(matches!(error, TranslationError::InvalidKey));
    assert_eq!(
        localizer.string("layered.view.title", &args, None).unwrap(),
        "overlay"
    );
}

#[tokio::test]
async fn asynchronous_source_loads_for_only_the_selected_http_application() {
    let first_root = Root::new("fr-FR");
    let second_root = Root::new("en-AU");
    let ((first, one), (second, two)) = tokio::join!(start(&first_root), start(&second_root));
    let before: Value = serde_json::from_str(&send(first, "/zh-Hant").await).unwrap();
    assert_eq!(before["greeting"], "您好，ada");
    assert_eq!(send(first, "/load-external-lines").await, "loaded");
    let (loaded, original) = tokio::join!(send(first, "/zh-Hant"), send(second, "/zh-Hant"));
    let loaded: Value = serde_json::from_str(&loaded).unwrap();
    let original: Value = serde_json::from_str(&original).unwrap();
    assert_eq!(loaded["greeting"], "外部 ada");
    assert_eq!(original["greeting"], "您好，ada");
    one.abort();
    two.abort();
}

#[tokio::test]
async fn asynchronous_translation_source_publishes_only_complete_batches() {
    use openrust::localization::{FileLoader, Localizer, TranslationResource};

    let root = Root::new("batch-loader");
    let source = root.0.join("source");
    fs::create_dir_all(source.join("zh-Hans")).unwrap();
    fs::write(
        source.join("zh-Hans/messages.json"),
        r#"{"welcome":"新 :name"}"#,
    )
    .unwrap();
    fs::write(source.join("zh-Hans.json"), "{invalid").unwrap();
    let loader = FileLoader::new(source.clone());
    let localizer = Localizer::new(
        FileLoader::new(root.0.join("lang")),
        "zh-Hans".into(),
        "en".into(),
    )
    .unwrap();
    let args = BTreeMap::from([("name".into(), "Ada".into())]);
    let resources = [
        TranslationResource {
            locale: "zh-Hans",
            group: "messages",
            namespace: Some("*"),
        },
        TranslationResource {
            locale: "zh-Hans",
            group: "*",
            namespace: None,
        },
    ];
    assert_eq!(
        localizer.string("messages.welcome", &args, None).unwrap(),
        "你好，Ada"
    );
    assert!(matches!(
        localizer.load_from(&loader, &resources).await,
        Err(TranslationError::Source(_))
    ));
    assert_eq!(
        localizer.string("messages.welcome", &args, None).unwrap(),
        "你好，Ada"
    );
    assert_eq!(localizer.string("Hello", &args, None).unwrap(), "你好");
    fs::write(source.join("zh-Hans.json"), r#"{"Hello":"新字面鍵"}"#).unwrap();
    localizer.load_from(&loader, &resources).await.unwrap();
    assert_eq!(
        localizer.string("messages.welcome", &args, None).unwrap(),
        "新 Ada"
    );
    assert_eq!(localizer.string("Hello", &args, None).unwrap(), "新字面鍵");
    assert_eq!(
        localizer.string("messages.fallback", &args, None).unwrap(),
        "English fallback"
    );
}

#[tokio::test]
async fn asynchronous_translation_source_preserves_concurrent_writes() {
    use openrust::{
        contracts::localization::Loader,
        localization::{
            FileLoader, Line, Lines, Localizer, TranslationFuture, TranslationResource,
            TranslationResult,
        },
    };
    use tokio::sync::Notify;

    #[derive(Default)]
    struct PausedLoader {
        started: Notify,
        resume: Notify,
    }

    impl Loader for PausedLoader {
        fn load<'a>(
            &'a self,
            locale: &'a str,
            group: &'a str,
            namespace: Option<&'a str>,
        ) -> TranslationFuture<'a, TranslationResult<Lines>> {
            Box::pin(async move {
                assert_eq!((locale, group, namespace), ("zh-Hans", "messages", None));
                self.started.notify_one();
                self.resume.notified().await;
                Ok(BTreeMap::from([(
                    "welcome".into(),
                    Line::Text("stale".into()),
                )]))
            })
        }
    }

    let root = Root::new("loader-concurrent");
    let localizer = Localizer::new(
        FileLoader::new(root.0.join("lang")),
        "zh-Hans".into(),
        "en".into(),
    )
    .unwrap();
    let resources = [
        TranslationResource {
            locale: "zh-Hans",
            group: "messages",
            namespace: None,
        },
        TranslationResource {
            locale: "zh-Hans",
            group: "messages",
            namespace: Some("*"),
        },
    ];
    let loader = PausedLoader::default();
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::join!(localizer.load_from(&loader, &resources), async {
            loader.started.notified().await;
            // An ordinary lazy read can race with a load without becoming a runtime write.
            assert_eq!(
                localizer
                    .string("messages.welcome", &BTreeMap::new(), None)
                    .unwrap(),
                "你好，:name"
            );
            localizer
                .add_lines(
                    [("messages.welcome".into(), Line::Text("concurrent".into()))],
                    "zh-Hans",
                    None,
                )
                .unwrap();
            loader.resume.notify_one();
        })
    })
    .await
    .unwrap();
    assert!(matches!(result.0, Err(TranslationError::CatalogChanged)));
    assert_eq!(
        localizer
            .string("messages.welcome", &BTreeMap::new(), None)
            .unwrap(),
        "concurrent"
    );
}

#[tokio::test]
async fn translation_replacements_are_isolated_across_http_applications() {
    let left = Root::new("en-CA");
    let right = Root::new("en-AU");
    let ((first, one), (second, two)) = tokio::join!(start(&left), start(&right));
    assert_eq!(send(first, "/install-replacements").await, "installed");
    let (changed, default) = tokio::join!(
        send(first, "/mixed-translation"),
        send(second, "/mixed-translation")
    );
    assert_eq!(
        serde_json::from_str::<Value>(&changed).unwrap(),
        json!({"missing": "[Ada] CUSTOM STATUS", "choice": "[2 custom status]", "label": "custom status"})
    );
    assert_eq!(
        serde_json::from_str::<Value>(&default).unwrap(),
        json!({"missing": "replacements.missing", "choice": "replacements.items", "label": "status"})
    );
    one.abort();
    two.abort();
}

#[test]
fn translation_replacements_follow_tag_order_and_walk_group_leaves() {
    use openrust::localization::{FileLoader, Localizer};
    use std::cell::Cell;

    let root = Root::new("replacement-order");
    let localizer = Localizer::new(
        FileLoader::new(root.0.join("lang")),
        "en".into(),
        "en".into(),
    )
    .unwrap();
    localizer
        .add_lines(
            [
                (
                    "render.leaves".into(),
                    Line::List(vec![
                        Line::Text("<mark>:name</mark><mark>x</mark>".into()),
                        Line::List(vec![Line::Text("<mark>a\nb</mark>:NAME :S".into())]),
                    ]),
                ),
                (
                    "render.items".into(),
                    Line::Text("{1} one|[2,*] :count :Count :COUNT".into()),
                ),
            ],
            "en",
            None,
        )
        .unwrap();
    let calls = Cell::new(0);
    let args = [
        (
            "mark",
            Replacement::tag(|raw| {
                calls.set(calls.get() + 1);
                format!("<tail>{raw}</tail>")
            }),
        ),
        ("tail", Replacement::tag(|raw| format!("[{raw}]"))),
        ("name", "ada".into()),
        ("s", "some name".into()),
    ];
    let rendered = localizer.array_with("render.leaves", &args, None).unwrap();
    assert_eq!(rendered["0"], Line::Text("[ada][x]".into()));
    assert_eq!(
        rendered["1"],
        Line::List(vec![Line::Text("<mark>a\nb</mark>ADA SOME NAME".into())])
    );
    assert_eq!(calls.get(), 2);
    assert_eq!(
        localizer
            .string_with(
                ":name-long :name <mark>tail</mark> <mark>X<mark>Y</mark>Z</mark>",
                &[
                    ("name", "<mark>literal</mark>".into()),
                    ("name-long", ":name".into()),
                    ("mark", Replacement::tag(|text| format!("[{text}]"))),
                ],
                None
            )
            .unwrap(),
        ":name <mark>literal</mark> [tail] [X<mark>Y]Z</mark>"
    );
    assert_eq!(
        localizer
            .choice_with("render.items", 2_i64, &[("count", "two".into())], None,)
            .unwrap(),
        "two Two TWO"
    );
    assert!(matches!(
        localizer.string_with(
            ":label",
            &[("label", Replacement::object(&NoticeLabel("x")))],
            None
        ),
        Err(TranslationError::MissingStringable(_))
    ));
    assert!(matches!(
        localizer.string_with("hello", &[("x.*", Replacement::tag(str::to_owned))], None),
        Err(TranslationError::InvalidTag)
    ));
}

#[test]
fn stringable_callbacks_can_reenter_the_translator() {
    use openrust::localization::{FileLoader, Localizer};

    let root = Root::new("replacement-reentry");
    let localizer = Arc::new(
        Localizer::new(
            FileLoader::new(root.0.join("lang")),
            "en".into(),
            "en".into(),
        )
        .unwrap(),
    );
    let owner = Arc::downgrade(&localizer);
    localizer.stringable(move |label: &NoticeLabel| {
        let owner = owner.upgrade().unwrap();
        owner.stringable(|_: &NoticeLabel| "new".into());
        owner.string(label.0, &BTreeMap::new(), None).unwrap()
    });
    let value = NoticeLabel("messages.fallback");
    let args = [("label", Replacement::object(&value))];
    assert_eq!(
        localizer.string_with(":label", &args, None).unwrap(),
        "English fallback"
    );
    assert_eq!(localizer.string_with(":label", &args, None).unwrap(), "new");
}
