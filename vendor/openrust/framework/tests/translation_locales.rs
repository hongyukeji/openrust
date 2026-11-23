use std::{collections::BTreeMap, fs, path::Path};

use openrust::{
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{
        Body, HttpError, HttpFuture, Request, Response, StatusCode, middleware::LocaleSelection,
    },
    support::{
        ProviderResult, ServiceProvider,
        facades::{App, Route},
    },
    translation::FileLoader,
};

fn write(root: &Path, path: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "{}").unwrap();
}

#[test]
fn file_loader_discovers_exact_resource_names_and_nested_groups_without_loading_contents() {
    let root = tempfile::tempdir().unwrap();
    for path in [
        "en/welcome.json",
        "en.json",
        "ja/nested/messages.json",
        "fr.json",
        "zh-Hant/messages.json",
        "vendor/shop/de/messages.json",
        ".git/it/messages.json",
        "zh_CN/messages.json",
        "EN/messages.json",
        "pt/readme.md",
        "es/.hidden.json",
    ] {
        write(root.path(), path);
    }
    fs::create_dir_all(root.path().join("de")).unwrap();
    // 发现只枚举文件名；翻译内容仍由正常加载路径校验。
    fs::write(root.path().join("fr.json"), "invalid JSON").unwrap();
    let loader = FileLoader::new(root.path().to_owned());
    assert_eq!(
        loader.available_locales().unwrap(),
        ["en", "fr", "ja", "zh-Hant"]
    );
}

#[test]
fn additional_application_and_json_paths_respect_their_existing_loader_semantics() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "app/en/messages.json");
    write(root.path(), "extra/de/messages.json");
    write(root.path(), "json/fr.json");
    write(root.path(), "json/it/messages.json");
    write(root.path(), "package/ja/messages.json");
    let mut loader = FileLoader::new(root.path().join("app"));
    loader.add_path(root.path().join("extra"));
    loader.add_path(root.path().join("app"));
    loader.add_json_path(root.path().join("json"));
    loader.add_namespace("shop", root.path().join("package"));
    assert_eq!(loader.available_locales().unwrap(), ["de", "en", "fr"]);
}

#[test]
fn missing_sources_are_empty_but_unreadable_sources_are_errors() {
    let root = tempfile::tempdir().unwrap();
    let loader = FileLoader::new(root.path().join("absent"));
    assert!(loader.available_locales().unwrap().is_empty());
    fs::write(root.path().join("file"), "not a directory").unwrap();
    loader.add_path(root.path().join("file"));
    assert!(loader.available_locales().is_err());
}

#[cfg(unix)]
#[test]
fn discovery_does_not_follow_resource_or_group_symlinks() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "external/messages.json");
    write(root.path(), "lang/en/messages.json");
    fs::create_dir_all(root.path().join("lang/de")).unwrap();
    symlink(root.path().join("external"), root.path().join("lang/fr")).unwrap();
    symlink(
        root.path().join("external/messages.json"),
        root.path().join("lang/ja.json"),
    )
    .unwrap();
    symlink(
        root.path().join("external"),
        root.path().join("lang/de/nested"),
    )
    .unwrap();
    assert_eq!(
        FileLoader::new(root.path().join("lang"))
            .available_locales()
            .unwrap(),
        ["en"]
    );
}

fn inspect(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Ok(Response::from(serde_json::json!({
            "locale": App::current_locale().unwrap(),
            "supported": request.extensions().get::<LocaleSelection>().map(|selection| &selection.supported),
        }).to_string()))
    })
}

fn application(root: &Path, candidates: Option<Vec<&'static str>>) -> Application {
    Application::configure(root.to_owned())
        .with_config(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                serde_json::json!({"locale":"en", "fallback_locale":"en"}),
            )]))
        })
        .with_middleware(move |middleware| {
            if let Some(candidates) = candidates {
                middleware.set_locale(&candidates).unwrap();
            }
        })
        .with_routing(Routing::default().web(|| Route::get("/", inspect)))
        .create()
        .unwrap()
}

async fn kernel(root: &Path, candidates: Option<Vec<&'static str>>) -> Kernel {
    let mut kernel = Kernel::new(application(root, candidates));
    kernel.bootstrap().await.unwrap();
    kernel
}

async fn request(kernel: &Kernel, path: &str, language: &str) -> serde_json::Value {
    let mut request = Request::from(
        http::Request::builder()
            .uri(path)
            .header("accept-language", language)
            .body(Body::empty())
            .unwrap(),
    );
    let response = kernel.handle(&mut request).await;
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_parts().1, 65536)
            .await
            .unwrap(),
    )
    .unwrap()
}

fn bundled_candidates(extra: &[&str]) -> serde_json::Value {
    let mut locales = [
        "en", "zh-Hans", "zh-Hant", "ja", "ko", "fr", "de", "es", "pt", "it", "ru", "ar", "hi",
        "id", "vi", "th", "tr", "nl",
    ]
    .into_iter()
    .collect::<std::collections::BTreeSet<_>>();
    locales.extend(extra.iter().copied());
    serde_json::json!(locales)
}

#[tokio::test]
async fn default_candidates_are_a_per_kernel_startup_snapshot_and_callback_uses_them() {
    let root = tempfile::tempdir().unwrap();
    let mut first = kernel(root.path(), None).await;
    assert_eq!(
        request(&first, "/?locale=ja", "en").await,
        serde_json::json!({"locale":"ja", "supported":bundled_candidates(&[])})
    );
    // 使用不在框架默认集合中的语言，验证应用新增来源仍只在启动时发现。
    write(root.path(), "lang/sv/messages.json");
    first.bootstrap().await.unwrap();
    assert_eq!(request(&first, "/", "sv").await["locale"], "en");
    let second = kernel(root.path(), None).await;
    let (old, new) = tokio::join!(request(&first, "/", "sv"), request(&second, "/", "sv"));
    assert_eq!(old["supported"], bundled_candidates(&[]));
    assert_eq!(new["locale"], "sv");
    assert_eq!(new["supported"], bundled_candidates(&["sv"]));
    fs::remove_dir_all(root.path().join("lang")).unwrap();
    assert_eq!(request(&second, "/", "sv").await["locale"], "sv");
    let third = kernel(root.path(), None).await;
    assert_eq!(
        request(&third, "/", "ja").await,
        serde_json::json!({"locale":"ja", "supported":bundled_candidates(&[])})
    );
}

#[tokio::test]
async fn explicit_candidates_preserve_order_include_non_file_languages_and_empty_disables() {
    let root = tempfile::tempdir().unwrap();
    // 即使来源无法扫描，显式列表仍适用于数据库等非文件来源。
    fs::write(root.path().join("lang"), "not a directory").unwrap();
    let explicit = kernel(root.path(), Some(vec!["fr", "en", "ja"])).await;
    assert_eq!(
        request(&explicit, "/", "fr").await,
        serde_json::json!({"locale":"fr", "supported":["fr", "en", "ja"]})
    );
    let disabled = kernel(root.path(), Some(vec![])).await;
    assert_eq!(
        request(&disabled, "/?locale[]=invalid", "fr").await,
        serde_json::json!({"locale":"en", "supported":null})
    );
    let mut automatic = Kernel::new(application(root.path(), None));
    assert!(automatic.bootstrap().await.is_err());
}

struct ExtraSource(std::path::PathBuf);
impl ServiceProvider for ExtraSource {
    fn boot(&mut self, app: &Application) -> HttpFuture<'_, ProviderResult> {
        let loader = app.container().get::<FileLoader>("translation.loader");
        let path = self.0.clone();
        Box::pin(async move {
            loader?.add_path(path);
            Ok(())
        })
    }
}

#[tokio::test]
async fn discovery_uses_selected_language_root_and_provider_registered_sources() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "lang/de/messages.json");
    write(root.path(), "resources/lang/en/messages.json");
    write(root.path(), "extra/ja/messages.json");
    let app = Application::configure(root.path().to_owned())
        .with_providers(vec![Box::new(ExtraSource(root.path().join("extra")))])
        .with_middleware(|middleware| {
            middleware.locale_using(|_| Ok(Some("ja".into())));
        })
        .with_routing(Routing::default().web(|| Route::get("/", inspect)))
        .create()
        .unwrap();
    let mut selected = Kernel::new(app);
    selected.bootstrap().await.unwrap();
    assert_eq!(
        request(&selected, "/", "de").await,
        serde_json::json!({"locale":"ja", "supported":bundled_candidates(&[])})
    );
    let other = tempfile::tempdir().unwrap();
    write(other.path(), "lang/fr.json");
    let independent = kernel(other.path(), None).await;
    assert_eq!(
        request(&independent, "/", "ja").await,
        serde_json::json!({"locale":"ja", "supported":bundled_candidates(&[])})
    );
}

#[tokio::test]
async fn failed_explicit_configuration_retains_automatic_discovery() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "lang/ja/messages.json");
    let app = Application::configure(root.path().to_owned())
        .with_middleware(|middleware| {
            assert!(middleware.set_locale(&["zh_CN"]).is_err());
        })
        .with_routing(Routing::default().web(|| Route::get("/", inspect)))
        .create()
        .unwrap();
    let mut selected = Kernel::new(app);
    selected.bootstrap().await.unwrap();
    assert_eq!(
        request(&selected, "/", "ja").await["supported"],
        bundled_candidates(&[])
    );
}

fn configured_application(
    root: &Path,
    value: serde_json::Value,
    explicit: Option<Vec<&'static str>>,
) -> Application {
    fs::write(root.join("supported.json"), value.to_string()).unwrap();
    Application::configure(root.to_owned())
        .with_config(|base, _| {
            let value: serde_json::Value = serde_json::from_slice(&fs::read(base.join("supported.json")).unwrap()).unwrap();
            Ok(BTreeMap::from([("app".into(), serde_json::json!({"locale":"en", "fallback_locale":"en", "supported_locales":value}))]))
        })
        .with_middleware(move |middleware| {
            if let Some(candidates) = explicit { middleware.set_locale(&candidates).unwrap(); }
        })
        .with_routing(Routing::default().web(|| Route::get("/", inspect)))
        .create().unwrap()
}

#[tokio::test]
async fn configured_candidates_override_discovery_preserve_order_and_normalize_once() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "lang/de/messages.json");
    let mut selected = Kernel::new(configured_application(
        root.path(),
        serde_json::json!(["JA", "en", "ja"]),
        None,
    ));
    selected.bootstrap().await.unwrap();
    assert_eq!(
        request(&selected, "/", "de").await,
        serde_json::json!({"locale":"en", "supported":["ja","en"]})
    );
    assert_eq!(
        request(&selected, "/?locale=ja", "en").await["locale"],
        "ja"
    );
    selected
        .get_application()
        .config()
        .unwrap()
        .set("app.supported_locales", serde_json::json!(["de"]))
        .unwrap();
    selected.bootstrap().await.unwrap();
    assert_eq!(
        request(&selected, "/", "ja").await["supported"],
        serde_json::json!(["ja", "en"])
    );
}

#[tokio::test]
async fn wildcard_discovers_but_configured_lists_and_explicit_candidates_skip_sources() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "lang/ja/messages.json");
    let mut automatic = Kernel::new(configured_application(
        root.path(),
        serde_json::json!("*"),
        None,
    ));
    automatic.bootstrap().await.unwrap();
    assert_eq!(
        request(&automatic, "/", "ja").await["supported"],
        bundled_candidates(&[])
    );
    fs::remove_dir_all(root.path().join("lang")).unwrap();
    fs::write(root.path().join("lang"), "not a directory").unwrap();
    for (value, explicit, expected) in [
        (
            serde_json::json!(["fr", "en"]),
            None,
            serde_json::json!(["fr", "en"]),
        ),
        (serde_json::json!([]), None, serde_json::Value::Null),
        (
            serde_json::json!(["fr"]),
            Some(vec!["ja"]),
            serde_json::json!(["ja"]),
        ),
        (
            serde_json::json!(["fr"]),
            Some(vec![]),
            serde_json::Value::Null,
        ),
        (
            serde_json::json!(false),
            Some(vec!["en"]),
            serde_json::json!(["en"]),
        ),
    ] {
        let mut selected = Kernel::new(configured_application(root.path(), value, explicit));
        selected.bootstrap().await.unwrap();
        assert_eq!(request(&selected, "/", "fr").await["supported"], expected);
    }
}

#[tokio::test]
async fn malformed_candidate_configuration_fails_at_startup_and_can_be_retried() {
    let root = tempfile::tempdir().unwrap();
    for value in [
        serde_json::Value::Null,
        serde_json::json!("null"),
        serde_json::json!("en,ja"),
        serde_json::json!(false),
        serde_json::json!({"en":true}),
        serde_json::json!(["en", 42]),
        serde_json::json!(["*"]),
        serde_json::json!(["en", "*"]),
        serde_json::json!([""]),
        serde_json::json!(["zh_CN"]),
    ] {
        let mut selected = Kernel::new(configured_application(root.path(), value.clone(), None));
        let error = selected
            .bootstrap()
            .await
            .expect_err("错误配置必须阻止启动");
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&error);
        let mut invalid_candidates = false;
        while let Some(error) = source {
            if let Some(openrust::config::ConfigError::InvalidType { key, .. }) =
                error.downcast_ref::<openrust::config::ConfigError>()
            {
                invalid_candidates = key == "app.supported_locales";
                break;
            }
            source = error.source();
        }
        assert!(invalid_candidates, "{value}: {error:?}");
        selected
            .get_application()
            .config()
            .unwrap()
            .set("app.supported_locales", serde_json::json!(["ja"]))
            .unwrap();
        selected.bootstrap().await.unwrap();
        assert_eq!(
            request(&selected, "/", "ja").await["supported"],
            serde_json::json!(["ja"])
        );
    }
}

#[tokio::test]
async fn cached_candidates_win_over_live_configuration_and_keep_applications_isolated() {
    let root = tempfile::tempdir().unwrap();
    let mut first = Kernel::new(configured_application(
        root.path(),
        serde_json::json!(["ja", "en"]),
        None,
    ));
    first.bootstrap().await.unwrap();
    let cache = serde_json::to_value(first.get_application().config().unwrap().all()).unwrap();
    fs::create_dir_all(root.path().join("storage/framework")).unwrap();
    fs::write(
        root.path().join("storage/framework/config.json"),
        cache.to_string(),
    )
    .unwrap();
    let mut cached = Kernel::new(configured_application(
        root.path(),
        serde_json::json!(["fr"]),
        None,
    ));
    cached.bootstrap().await.unwrap();
    assert_eq!(
        request(&cached, "/", "ja").await["supported"],
        serde_json::json!(["ja", "en"])
    );
    let independent_root = tempfile::tempdir().unwrap();
    let mut independent = Kernel::new(configured_application(
        independent_root.path(),
        serde_json::json!(["fr"]),
        None,
    ));
    independent.bootstrap().await.unwrap();
    let (cached, other) = tokio::join!(
        request(&cached, "/", "fr"),
        request(&independent, "/", "fr")
    );
    assert_eq!(cached["locale"], "en");
    assert_eq!(other["locale"], "fr");
}

#[tokio::test]
async fn wildcard_enables_all_bundled_and_application_languages_and_respects_explicit_override() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "lang/sv/messages.json");
    let mut all = Kernel::new(configured_application(
        root.path(),
        serde_json::json!("*"),
        None,
    ));
    all.bootstrap().await.unwrap();
    assert_eq!(
        request(&all, "/", "sv").await,
        serde_json::json!({"locale":"sv", "supported":bundled_candidates(&["sv"])})
    );
    let mut explicit = Kernel::new(configured_application(
        root.path(),
        serde_json::json!("*"),
        Some(vec!["en"]),
    ));
    explicit.bootstrap().await.unwrap();
    assert_eq!(
        request(&explicit, "/", "sv").await,
        serde_json::json!({"locale":"en", "supported":["en"]})
    );
}
