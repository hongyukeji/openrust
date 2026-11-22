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
        loader.availableLocales().unwrap(),
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
    loader.addPath(root.path().join("extra"));
    loader.addPath(root.path().join("app"));
    loader.addJsonPath(root.path().join("json"));
    loader.addNamespace("shop", root.path().join("package"));
    assert_eq!(loader.availableLocales().unwrap(), ["de", "en", "fr"]);
}

#[test]
fn missing_sources_are_empty_but_unreadable_sources_are_errors() {
    let root = tempfile::tempdir().unwrap();
    let loader = FileLoader::new(root.path().join("absent"));
    assert!(loader.availableLocales().unwrap().is_empty());
    fs::write(root.path().join("file"), "not a directory").unwrap();
    loader.addPath(root.path().join("file"));
    assert!(loader.availableLocales().is_err());
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
            .availableLocales()
            .unwrap(),
        ["en"]
    );
}

fn inspect(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Ok(Response::from(serde_json::json!({
            "locale": App::currentLocale().unwrap(),
            "supported": request.extensions().get::<LocaleSelection>().map(|selection| &selection.supported),
        }).to_string()))
    })
}

fn application(root: &Path, candidates: Option<Vec<&'static str>>) -> Application {
    Application::configure(root.to_owned())
        .withConfig(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                serde_json::json!({"locale":"en", "fallback_locale":"en"}),
            )]))
        })
        .withMiddleware(move |middleware| {
            if let Some(candidates) = candidates {
                middleware.setLocale(&candidates).unwrap();
            }
        })
        .withRouting(Routing::default().web(|| Route::get("/", inspect)))
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

#[tokio::test]
async fn default_candidates_are_a_per_kernel_startup_snapshot_and_callback_uses_them() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "lang/en/messages.json");
    write(root.path(), "lang/ja/messages.json");
    let mut first = kernel(root.path(), None).await;
    assert_eq!(
        request(&first, "/?locale=ja", "en").await,
        serde_json::json!({"locale":"ja", "supported":["en", "ja"]})
    );
    write(root.path(), "lang/fr/messages.json");
    first.bootstrap().await.unwrap();
    assert_eq!(request(&first, "/", "fr").await["locale"], "en");
    let second = kernel(root.path(), None).await;
    let (old, new) = tokio::join!(request(&first, "/", "fr"), request(&second, "/", "fr"));
    assert_eq!(old["supported"], serde_json::json!(["en", "ja"]));
    assert_eq!(new["locale"], "fr");
    assert_eq!(new["supported"], serde_json::json!(["en", "fr", "ja"]));
    fs::remove_dir_all(root.path().join("lang")).unwrap();
    assert_eq!(request(&second, "/", "ja").await["locale"], "ja");
    let third = kernel(root.path(), None).await;
    let value = request(&third, "/?locale=ignored", "ja").await;
    assert_eq!(value, serde_json::json!({"locale":"en", "supported":null}));
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
            loader?.addPath(path);
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
        .withProviders(vec![Box::new(ExtraSource(root.path().join("extra")))])
        .withMiddleware(|middleware| {
            middleware.localeUsing(|_| Ok(Some("ja".into())));
        })
        .withRouting(Routing::default().web(|| Route::get("/", inspect)))
        .create()
        .unwrap();
    let mut selected = Kernel::new(app);
    selected.bootstrap().await.unwrap();
    assert_eq!(
        request(&selected, "/", "de").await,
        serde_json::json!({"locale":"ja", "supported":["en", "ja"]})
    );
    let other = tempfile::tempdir().unwrap();
    write(other.path(), "lang/fr.json");
    let independent = kernel(other.path(), None).await;
    assert_eq!(
        request(&independent, "/", "ja").await,
        serde_json::json!({"locale":"fr", "supported":["fr"]})
    );
}

#[tokio::test]
async fn failed_explicit_configuration_retains_automatic_discovery() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "lang/ja/messages.json");
    let app = Application::configure(root.path().to_owned())
        .withMiddleware(|middleware| {
            assert!(middleware.setLocale(&["zh_CN"]).is_err());
        })
        .withRouting(Routing::default().web(|| Route::get("/", inspect)))
        .create()
        .unwrap();
    let mut selected = Kernel::new(app);
    selected.bootstrap().await.unwrap();
    assert_eq!(
        request(&selected, "/", "ja").await["supported"],
        serde_json::json!(["ja"])
    );
}
