use std::{fs, path::PathBuf};

use openrust::{
    contracts::{http::Kernel as _, validation::Validator as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{App, Lang, Route, Validator},
    validation::{Data, RuleSet},
};

#[path = "../app/http/mod.rs"]
pub(crate) mod http;
#[path = "../app/models/mod.rs"]
pub(crate) mod models;
mod app {
    pub(crate) use crate::{http, models};
}
#[path = "../config/mod.rs"]
mod config;
#[path = "../routes/web.rs"]
mod web;

struct Root(PathBuf);
impl Root {
    fn new(locale: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "openrust-welcome-{}-{}",
            std::process::id(),
            openrust::support::Str::random(12)
        ));
        fs::create_dir_all(path.join("resources/views")).unwrap();
        fs::write(
            path.join(".env"),
            format!("APP_LOCALE={locale}\nAPP_FALLBACK_LOCALE=en\n"),
        )
        .unwrap();
        fs::write(
            path.join("resources/views/welcome.blade.html"),
            include_str!("../resources/views/welcome.blade.html"),
        )
        .unwrap();
        for (locale, data, language_name) in [
            (
                "en",
                include_str!("../lang/en/welcome.json"),
                include_str!("../lang/en/languages.json"),
            ),
            (
                "zh-Hans",
                include_str!("../lang/zh-Hans/welcome.json"),
                include_str!("../lang/zh-Hans/languages.json"),
            ),
            (
                "zh-Hant",
                include_str!("../lang/zh-Hant/welcome.json"),
                include_str!("../lang/zh-Hant/languages.json"),
            ),
        ] {
            fs::create_dir_all(path.join("lang").join(locale)).unwrap();
            fs::write(path.join("lang").join(locale).join("welcome.json"), data).unwrap();
            fs::write(
                path.join("lang").join(locale).join("languages.json"),
                language_name,
            )
            .unwrap();
        }
        fs::write(
            path.join("resources/views/other.blade.html"),
            "{{ __('welcome.heading') }}|@include('partial')",
        )
        .unwrap();
        fs::write(
            path.join("resources/views/partial.blade.html"),
            "{{ trans('welcome.documentation') }}",
        )
        .unwrap();
        Self(path)
    }

    async fn kernel(&self) -> Kernel {
        self.kernel_with_browser(true).await
    }

    async fn kernel_with_browser(&self, browser: bool) -> Kernel {
        self.kernel_options(browser, None, None).await
    }

    async fn kernel_options(
        &self,
        browser: bool,
        parameter: Option<serde_json::Value>,
        candidates: Option<Vec<&'static str>>,
    ) -> Kernel {
        self.kernel_custom(browser, parameter, candidates, None)
            .await
    }

    async fn kernel_custom(
        &self,
        browser: bool,
        parameter: Option<serde_json::Value>,
        candidates: Option<Vec<&'static str>>,
        callback: Option<&'static str>,
    ) -> Kernel {
        let parameter_path = self.0.join("locale-query.json");
        if let Some(value) = parameter {
            fs::write(&parameter_path, serde_json::to_vec(&value).unwrap()).unwrap();
        } else {
            let _ = fs::remove_file(&parameter_path);
        }
        let application = Application::configure(self.0.clone())
            .withConfig(|base, env| {
                let mut values = config::load(base, env)?;
                values.get_mut("app").unwrap()["key"] = "a".repeat(32).into();
                values.get_mut("app").unwrap()["name"] = "Welcome <Test>".into();
                values.get_mut("session").unwrap()["driver"] = "array".into();
                if let Ok(bytes) = fs::read(base.join("locale-query.json")) {
                    values.get_mut("app").unwrap()["locale_query_parameter"] =
                        serde_json::from_slice(&bytes).unwrap();
                }
                Ok(values)
            })
            .withMiddleware(move |middleware| {
                if let Some(candidates) = candidates {
                    middleware.setLocale(&candidates).unwrap();
                }
                if let Some(locale) = callback {
                    middleware.localeUsing(move |_| Ok(Some(locale.into())));
                }
                if !browser {
                    middleware.setLocale(&[]).unwrap();
                }
            })
            .withRouting(Routing::default().web(|| {
                web::register()?;
                Route::get("/locale", default_locale)?;
                Route::get("/escaped-repository", || {
                    openrust::view::view("welcome")
                        .with("name", "App")
                        .with("version", "1.0.0")
                        .with("language", "en")
                        .with("repository", "https://example.test/a?x=1&y=\"two\"")
                })?;
                Route::get("/other", || openrust::view::view("other"))?;
                Route::get("/validation", validate)?;
                Route::get("/metadata", || {
                    let mut response = Response::from("metadata");
                    response
                        .parts_mut()
                        .headers
                        .insert("content-language", "fr".parse().unwrap());
                    response
                        .parts_mut()
                        .headers
                        .append("vary", "Accept-Encoding".parse().unwrap());
                    response
                        .parts_mut()
                        .headers
                        .append("vary", "Origin".parse().unwrap());
                    response
                })?;
                Route::get("/vary-star", || {
                    Response::from("star").header("vary".parse().unwrap(), "*".parse().unwrap())
                })?;
                Route::get("/vary-existing", || {
                    Response::from("existing").header(
                        "vary".parse().unwrap(),
                        "Origin, accept-language".parse().unwrap(),
                    )
                })?;
                Route::get("/switch", switched_locale)
            }))
            .create()
            .unwrap();
        let mut kernel = Kernel::new(application);
        kernel.bootstrap().await.unwrap();
        kernel
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn default_locale(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        tokio::task::yield_now().await;
        Ok(Response::new(
            StatusCode::OK,
            Body::from(Lang::getLocale().unwrap()),
        ))
    })
}
fn validate(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let mut validator = Validator::make(
            Data::new(serde_json::json!({}))?,
            RuleSet::parse([("name", "required")])?,
        )?;
        let _ = validator.validate().await;
        Ok(Response::from(validator.errors_mut().first(None, None)))
    })
}
fn switched_locale(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        App::setLocale("zh-Hant").await.unwrap();
        Response::view(
            StatusCode::OK,
            openrust::view::view("other"),
            Default::default(),
            request,
        )
        .await
    })
}
async fn send(
    kernel: &Kernel,
    path: &str,
    languages: &[&str],
) -> (openrust::http::HeaderMap, String) {
    send_status(kernel, path, languages, StatusCode::OK).await
}
async fn send_status(
    kernel: &Kernel,
    path: &str,
    languages: &[&str],
    status: StatusCode,
) -> (openrust::http::HeaderMap, String) {
    let raw = axum::extract::Request::builder()
        .uri(path)
        .body(Body::empty())
        .unwrap();
    let (parts, body) = raw.into_parts();
    let mut request = Request::from_parts(parts, body);
    for language in languages {
        request
            .parts_mut()
            .headers
            .append("accept-language", language.parse().unwrap());
    }
    let response = kernel.handle(&mut request).await;
    assert_eq!(response.status(), status);
    let (parts, body) = response.into_parts();
    let body = axum::body::to_bytes(body, 128 * 1024).await.unwrap();
    (parts.headers, String::from_utf8(body.to_vec()).unwrap())
}
fn assert_links(html: &str, locale: &str) {
    let repository = Application::REPOSITORY;
    for suffix in [
        format!("/blob/master/docs/{locale}/README.md"),
        format!("/blob/master/docs/{locale}/getting-started/deployment.md"),
        String::new(),
        "/blob/master/CHANGELOG.md".into(),
    ] {
        assert!(
            html.contains(&format!("href=\"{repository}{suffix}\"")),
            "缺少链接：{suffix}"
        );
    }
}

fn assert_page(headers: &openrust::http::HeaderMap, html: &str, locale: &str) {
    assert_links(html, locale);
    for path in [
        format!("docs/{locale}/README.md"),
        format!("docs/{locale}/getting-started/deployment.md"),
        "CHANGELOG.md".into(),
    ] {
        assert!(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(path)
                .is_file()
        );
    }
    let (tag, heading, documentation, source, deployment, changelog) = match locale {
        "zh-Hans" => (
            "zh-Hans",
            "开始使用",
            "开发文档",
            "项目源码",
            "部署指南",
            "查看更新日志",
        ),
        "zh-Hant" => (
            "zh-Hant",
            "開始使用",
            "開發文件",
            "專案原始碼",
            "部署指南",
            "查看更新日誌",
        ),
        _ => (
            "en",
            "Let’s get started",
            "Documentation",
            "Source code",
            "Deployment guide",
            "View changelog",
        ),
    };
    assert_eq!(headers["content-language"], tag);
    assert!(
        headers["vary"]
            .to_str()
            .unwrap()
            .split(',')
            .any(|s| s.trim().eq_ignore_ascii_case("Accept-Language"))
    );
    for content in [
        format!("<html lang=\"{tag}\">"),
        format!("docs/{locale}/README.md"),
        format!("docs/{locale}/getting-started/deployment.md"),
        "Welcome &lt;Test&gt;".into(),
        format!("v{}", env!("CARGO_PKG_VERSION")),
    ] {
        assert!(html.contains(&content), "missing {content}");
    }
    for content in [heading, documentation, source, deployment, changelog] {
        assert!(html.contains(content), "missing {content}");
    }
    assert!(!html.contains("welcome.heading"));
    if locale != "en" {
        assert!(!html.contains("Let’s get started"));
    }
}

#[tokio::test]
async fn browser_preferences_localize_the_real_welcome_view_and_links() {
    let root = Root::new("en");
    let kernel = root.kernel().await;
    for (language, expected) in [
        ("en", "en"),
        ("en-US", "en"),
        ("en-GB", "en"),
        ("zh", "zh-Hans"),
        ("zh-CN", "zh-Hans"),
        ("zh-SG", "zh-Hans"),
        ("zh-Hans", "zh-Hans"),
        ("zh-TW", "zh-Hant"),
        ("zh-HK", "zh-Hant"),
        ("zh-MO", "zh-Hant"),
        ("zh-Hant", "zh-Hant"),
        ("zh-Hant-CN", "zh-Hant"),
        ("zh-Latn-TW", "en"),
        ("zh-Hans-TW-u-nu-hanidec", "zh-Hans"),
        ("zh-Hans-TW", "zh-Hans"),
        ("zh-1901", "zh-Hans"),
        ("zh-CN-1901", "zh-Hans"),
        ("zh-TW-1901", "zh-Hant"),
        ("ZH-hk", "zh-Hant"),
        ("en;q=0.2, zh-TW;q=0.9, zh-CN;q=0.5", "zh-Hant"),
        ("fr-FR, zh-CN;q=0.8, en;q=0.7", "zh-Hans"),
        ("zh-TW;q=0, en;q=0.5", "en"),
        ("zh-CN;q=0.5, zh-TW;q=0.5", "zh-Hans"),
        ("zh-TW;q=1.000, en;q=0.999", "zh-Hant"),
    ] {
        let (headers, html) = send(&kernel, "/", &[language]).await;
        assert_page(&headers, &html, expected);
    }
    let (headers, html) = send(&kernel, "/", &["en;q=0.2", "zh-HK;q=0.9"]).await;
    assert_page(&headers, &html, "zh-Hant");
}

#[tokio::test]
async fn invalid_or_unsupported_preferences_use_the_supported_default() {
    for (default, expected) in [("zh-Hant", "zh-Hant"), ("zh-Hans", "zh-Hans"), ("fr", "en")] {
        let root = Root::new(default);
        let kernel = root.kernel().await;
        for language in [
            None,
            Some(""),
            Some("fr, de;q=0.8"),
            Some("*"),
            Some("en;q=NaN"),
            Some("en;q=1.1"),
            Some("en;q=-1"),
            Some("en;q=0.1234"),
            Some("en;q=0"),
            Some("en;q=1;q=0"),
            Some("en;broken"),
            Some("../../en"),
            Some("zh-Latn"),
        ] {
            let languages: Vec<_> = language.into_iter().collect();
            let (headers, html) = send(&kernel, "/", &languages).await;
            assert_page(&headers, &html, expected);
        }
    }
}

#[tokio::test]
async fn concurrent_languages_do_not_change_request_or_application_defaults() {
    let english = Root::new("en");
    let traditional = Root::new("zh-Hant");
    let (first, second) = tokio::join!(english.kernel(), traditional.kernel());
    for _ in 0..8 {
        let (en, cn, tw, fallback) = tokio::join!(
            send(&first, "/", &["en"]),
            send(&first, "/", &["zh-CN"]),
            send(&first, "/", &["zh-TW"]),
            send(&second, "/", &[])
        );
        for ((headers, html), locale) in [
            (en, "en"),
            (cn, "zh-Hans"),
            (tw, "zh-Hant"),
            (fallback, "zh-Hant"),
        ] {
            assert_page(&headers, &html, locale);
        }
    }
    assert_eq!(send(&first, "/locale", &[]).await.1, "en");
    assert_eq!(send(&first, "/locale", &["zh-CN"]).await.1, "zh-Hans");
    assert_eq!(send(&first, "/locale", &["zh-TW"]).await.1, "zh-Hant");
    assert_eq!(send(&first, "/locale", &[]).await.1, "en");
    let (headers, html) = send(&first, "/other", &["zh-TW"]).await;
    assert_eq!(headers["content-language"], "zh-Hant");
    assert_eq!(html, "開始使用|開發文件");
    assert_eq!(send(&second, "/locale", &[]).await.1, "zh-Hant");
}

#[tokio::test]
async fn missing_translation_uses_the_existing_english_fallback() {
    let root = Root::new("en");
    let path = root.0.join("lang/zh-Hant/welcome.json");
    let mut copy: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    copy.as_object_mut().unwrap().remove("changelog");
    fs::write(path, serde_json::to_vec(&copy).unwrap()).unwrap();
    let kernel = root.kernel().await;
    let (headers, html) = send(&kernel, "/", &["zh-TW"]).await;
    assert_eq!(headers["content-language"], "zh-Hant");
    assert!(html.contains("開始使用"));
    assert!(html.contains("View changelog"));
    assert!(!html.contains("welcome.changelog"));
}

#[tokio::test]
async fn language_middleware_handles_other_consumers_errors_and_response_metadata() {
    let root = Root::new("en");
    for (locale, message) in [
        ("en", "EN required"),
        ("zh-Hans", "简体必填"),
        ("zh-Hant", "繁體必填"),
    ] {
        fs::write(
            root.0.join("lang").join(locale).join("validation.json"),
            serde_json::to_vec(&serde_json::json!({"required":message})).unwrap(),
        )
        .unwrap();
    }
    let kernel = root.kernel().await;
    for (locale, message) in [
        ("en", "EN required"),
        ("zh-Hans", "简体必填"),
        ("zh-Hant", "繁體必填"),
    ] {
        let (headers, body) = send(&kernel, "/validation", &[locale]).await;
        assert_eq!(headers["content-language"], locale);
        assert_eq!(body, message);
    }
    let (headers, _) = send(&kernel, "/metadata", &["zh-CN"]).await;
    assert_eq!(headers["content-language"], "fr");
    assert_eq!(
        headers
            .get_all("vary")
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect::<Vec<_>>(),
        ["Accept-Encoding", "Origin", "Accept-Language"]
    );
    for (path, expected) in [
        ("/vary-star", "*"),
        ("/vary-existing", "Origin, accept-language"),
    ] {
        let (headers, _) = send(&kernel, path, &["zh-CN"]).await;
        assert_eq!(headers.get_all("vary").iter().count(), 1);
        assert_eq!(headers["vary"], expected);
    }
    let (headers, body) = send(&kernel, "/switch", &["en"]).await;
    assert_eq!(headers["content-language"], "zh-Hant");
    assert_eq!(body, "開始使用|開發文件");
    assert_eq!(send(&kernel, "/locale", &[]).await.1, "en");
    let (headers, _) = send_status(&kernel, "/not-found", &["zh-TW"], StatusCode::NOT_FOUND).await;
    assert_eq!(headers["content-language"], "zh-Hant");
}

#[tokio::test]
async fn browser_language_policy_does_not_require_extra_application_configuration() {
    let root = Root::new("zh-Hant");
    let kernel = root.kernel().await;
    for (header, expected) in [
        ("fr-CA", "zh-Hant"),
        ("zh-CN", "zh-Hans"),
        ("zh-TW", "zh-Hant"),
        ("de", "zh-Hant"),
        ("en-US", "en"),
    ] {
        assert_eq!(send(&kernel, "/locale", &[header]).await.1, expected);
    }
}

#[tokio::test]
async fn welcome_without_browser_detection_uses_configured_translation_fallback() {
    for (locale, fallback, title) in [
        ("fr", "en", "Let’s get started"),
        ("fr", "zh-Hant", "開始使用"),
        ("zh-Hans", "en", "开始使用"),
    ] {
        let root = Root::new(locale);
        fs::write(
            root.0.join(".env"),
            format!("APP_LOCALE={locale}\nAPP_FALLBACK_LOCALE={fallback}\n"),
        )
        .unwrap();
        let kernel = root.kernel_with_browser(false).await;
        let (headers, html) = send(&kernel, "/", &["zh-CN"]).await;
        assert!(html.contains(title));
        assert!(html.contains(&format!("<html lang=\"{locale}\">")));
        assert_links(&html, locale);
        if locale == "fr" {
            assert!(
                !PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("docs/fr")
                    .exists()
            );
        }
        assert_eq!(headers["content-language"], locale);
        assert!(!headers.contains_key("vary"));
        assert_eq!(send(&kernel, "/locale", &[]).await.1, locale);
    }
}

#[tokio::test]
async fn custom_welcome_translation_keeps_current_language_links() {
    for fallback in ["en", "zh-Hant"] {
        let root = Root::new("fr");
        fs::write(
            root.0.join(".env"),
            format!("APP_LOCALE=fr\nAPP_FALLBACK_LOCALE={fallback}\n"),
        )
        .unwrap();
        fs::create_dir_all(root.0.join("lang/fr")).unwrap();
        fs::write(
            root.0.join("lang/fr/welcome.json"),
            serde_json::json!({"heading":"Bienvenue"}).to_string(),
        )
        .unwrap();
        let kernel = root.kernel_with_browser(false).await;
        let (headers, html) = send(&kernel, "/", &["zh-TW"]).await;
        assert_eq!(headers["content-language"], "fr");
        assert!(html.contains("<html lang=\"fr\">"));
        assert!(html.contains("Bienvenue"));
        assert_links(&html, "fr");
        assert!(html.contains(if fallback == "zh-Hant" {
            "開發文件"
        } else {
            "Documentation"
        }));
        assert_eq!(send(&kernel, "/locale", &[]).await.1, "fr");
    }
}

#[tokio::test]
async fn partial_translations_do_not_guess_the_page_language_from_the_heading() {
    let root = Root::new("fr");
    fs::create_dir_all(root.0.join("lang/fr")).unwrap();
    fs::write(
        root.0.join("lang/fr/welcome.json"),
        serde_json::json!({
            "description":"Une application Rust."
        })
        .to_string(),
    )
    .unwrap();
    let kernel = root.kernel_with_browser(false).await;
    let (headers, html) = send(&kernel, "/", &[]).await;
    assert_eq!(headers["content-language"], "fr");
    assert!(html.contains("<html lang=\"fr\">"));
    assert!(html.contains("Let’s get started"));
    assert!(html.contains("Une application Rust."));
    assert_links(&html, "fr");
    assert_eq!(send(&kernel, "/locale", &[]).await.1, "fr");
}

#[tokio::test]
async fn missing_display_text_uses_fallback_without_changing_links() {
    let root = Root::new("zh-Hans");
    let path = root.0.join("lang/zh-Hans/welcome.json");
    let mut data: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    data.as_object_mut().unwrap().remove("documentation");
    fs::write(path, serde_json::to_vec(&data).unwrap()).unwrap();
    let kernel = root.kernel().await;
    let (headers, html) = send(&kernel, "/", &["zh-CN"]).await;
    assert_eq!(headers["content-language"], "zh-Hans");
    assert!(html.contains("开始使用"));
    assert!(html.contains("Documentation"));
    assert_links(&html, "zh-Hans");
    assert!(html.contains("docs/zh-Hans/getting-started/deployment.md"));
}

#[tokio::test]
async fn missing_display_text_does_not_prevent_link_rendering() {
    let root = Root::new("fr");
    fs::write(
        root.0.join(".env"),
        "APP_LOCALE=fr\nAPP_FALLBACK_LOCALE=de\n",
    )
    .unwrap();
    let kernel = root.kernel_with_browser(false).await;
    let (headers, html) = send(&kernel, "/", &[]).await;
    assert_eq!(headers["content-language"], "fr");
    assert!(html.contains("welcome.heading"));
    assert_links(&html, "fr");
    assert!(!html.contains("docs/en/README.md"));
}

#[tokio::test]
async fn repository_is_escaped_in_all_welcome_links() {
    let root = Root::new("en");
    let kernel = root.kernel().await;
    let (_, html) = send(&kernel, "/escaped-repository", &[]).await;
    let escaped = "https://example.test/a?x=1&amp;y=&quot;two&quot;";
    for suffix in [
        "",
        "/blob/master/docs/en/README.md",
        "/blob/master/docs/en/getting-started/deployment.md",
        "/blob/master/CHANGELOG.md",
    ] {
        assert!(html.contains(&format!("href=\"{escaped}{suffix}\"")));
    }
    assert!(!html.contains("href=\"https://example.test/a?x=1&y="));
}

#[tokio::test]
async fn manual_switching_uses_selected_language_and_auto_removes_only_its_parameter() {
    let root = Root::new("en");
    let kernel = root.kernel().await;
    for locale in ["en", "zh-Hans", "zh-Hant"] {
        let (headers, html) = send(
            &kernel,
            &format!("/?lang={locale}&search=a%26b"),
            &["en-US"],
        )
        .await;
        assert_page(&headers, &html, locale);
        assert!(html.contains("class=\"language-switcher\""));
        assert!(html.contains("href=\"/?search=a%26b\""));
        let selected = html
            .split("<a ")
            .map(|part| part.split('>').next().unwrap())
            .find(|tag| tag.contains("aria-current=\"true\""))
            .unwrap();
        assert!(selected.contains(&format!("lang={locale}")));
        assert!(selected.contains("search=a%26b"));
    }
    let (_, html) = send(&kernel, "/", &["zh-TW"]).await;
    assert!(html.contains("href=\"/\" aria-current=\"true\""));
    assert!(html.contains("data-locale=\"zh-Hant\""));
}

#[tokio::test]
async fn switcher_tracks_custom_parameter_null_and_actual_candidates() {
    let root = Root::new("en");
    let custom = root
        .kernel_options(
            true,
            Some("language".into()),
            Some(vec!["en", "zh-Hant", "fr"]),
        )
        .await;
    let (headers, html) = send(&custom, "/?language=zh-Hant&lang=keep", &["en"]).await;
    assert_page(&headers, &html, "zh-Hant");
    assert!(html.contains("/?language=en&amp;lang=keep"));
    assert!(html.contains("href=\"/?lang=keep\""));
    assert!(!html.contains("language=zh-Hans"));
    assert!(!html.contains("language=fr"));
    for (browser, parameter, candidates) in [
        (true, Some(serde_json::Value::Null), None),
        (false, None, None),
        (true, None, Some(vec!["fr"])),
    ] {
        let path = if browser && candidates.is_some() {
            "/"
        } else {
            "/?lang[]=bad"
        };
        let kernel = root.kernel_options(browser, parameter, candidates).await;
        let (_, html) = send(&kernel, path, &["en"]).await;
        assert!(!html.contains("class=\"language-switcher\""));
    }
}

#[tokio::test]
async fn callback_selected_language_hides_controls_that_cannot_override_it() {
    let root = Root::new("en");
    let kernel = root.kernel_custom(true, None, None, Some("zh-Hant")).await;
    let (headers, html) = send(&kernel, "/?lang[]=invalid", &["en"]).await;
    assert_page(&headers, &html, "zh-Hant");
    assert!(!html.contains("class=\"language-switcher\""));
}

#[tokio::test]
async fn header_shows_native_language_names_and_default_system_theme_controls() {
    let root = Root::new("en");
    let kernel = root.kernel().await;
    for locale in ["en", "zh-Hans", "zh-Hant"] {
        let (headers, html) = send(&kernel, &format!("/?lang={locale}"), &["en-US"]).await;
        assert_page(&headers, &html, locale);
        let header = html
            .split("<header class=\"page-header\">")
            .nth(1)
            .expect("页头存在")
            .split("</header>")
            .next()
            .unwrap();
        assert!(header.contains("Welcome &lt;Test&gt;"));
        for (tag, name) in [
            ("en", "English"),
            ("zh-Hans", "简体中文"),
            ("zh-Hant", "繁體中文"),
        ] {
            assert!(header.contains(&format!("lang=\"{tag}\"")));
            assert!(header.contains(&format!(">{name}</a>")));
        }
        assert_eq!(header.matches("<summary ").count(), 2);
        for appearance in ["light", "dark", "system"] {
            assert!(header.contains(&format!("data-theme-choice=\"{appearance}\"")));
        }
        assert!(!html.contains("welcome.english"));
        assert!(!html.contains("languages.name"));
    }
}

#[tokio::test]
async fn blade_switcher_keeps_nested_encoded_query_data_and_does_not_retain_previous_request() {
    let root = Root::new("en");
    let kernel = root.kernel().await;
    let (_, html) = send(
        &kernel,
        "/?lang=zh-Hant&filter[tags][]=a%26b&filter[tags][]=%E7%94%B2&search=%3Cscript%3E",
        &["en"],
    )
    .await;
    assert!(html.contains("filter%5Btags%5D%5B0%5D=a%26b"));
    assert!(html.contains("filter%5Btags%5D%5B1%5D=%E7%94%B2"));
    assert!(html.contains("search=%3Cscript%3E"));
    assert!(html.contains("href=\"/?lang=en&amp;"));
    assert!(!html.contains("search=<script>"));
    let (_, next) = send(&kernel, "/?lang=en", &["zh-TW"]).await;
    assert!(!next.contains("filter%5Btags%5D"));
    assert!(!next.contains("search=%3Cscript%3E"));
    assert!(next.contains("href=\"/\""));
}

#[tokio::test]
async fn blade_switcher_preserves_numeric_keys_for_custom_language_parameter() {
    let root = Root::new("en");
    let kernel = root.kernel_options(true, Some("5".into()), None).await;
    let (headers, html) = send(&kernel, "/?5=zh-Hant&9=keep", &["en"]).await;
    assert_page(&headers, &html, "zh-Hant");
    assert!(html.contains("href=\"/?5=en&amp;9=keep\""));
    assert!(html.contains("href=\"/?9=keep\""));
    assert!(!html.contains("0=zh-Hant"));
}
