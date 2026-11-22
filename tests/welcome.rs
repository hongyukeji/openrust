use std::{fs, path::PathBuf};

use openrust::{
    contracts::{foundation::Application as _, http::Kernel as _, validation::Validator as _},
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

    async fn temporary_kernel(&self) -> Kernel {
        fs::write(self.0.join("locale-cookie.json"), "null").unwrap();
        self.kernel().await
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
                values.get_mut("app").unwrap()["debug"] = true.into();
                values.get_mut("app").unwrap()["key"] = "a".repeat(32).into();
                values.get_mut("app").unwrap()["name"] = "Welcome <Test>".into();
                values.get_mut("session").unwrap()["driver"] = "array".into();
                if let Ok(bytes) = fs::read(base.join("locale-query.json")) {
                    values.get_mut("app").unwrap()["locale_query_parameter"] =
                        serde_json::from_slice(&bytes).unwrap();
                }
                if let Ok(bytes) = fs::read(base.join("locale-cookie.json")) {
                    values.get_mut("app").unwrap()["locale_cookie"] =
                        serde_json::from_slice(&bytes).unwrap();
                }
                if let Ok(bytes) = fs::read(base.join("locale-header.json")) {
                    values.get_mut("app").unwrap()["locale_header"] =
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
    let (parts, body) = response.into_parts();
    let body = axum::body::to_bytes(body, 128 * 1024).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert_eq!(parts.status, status, "{body}");
    (parts.headers, body)
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
    assert!(
        headers["vary"]
            .to_str()
            .unwrap()
            .split(',')
            .any(|s| s.trim().eq_ignore_ascii_case("Accept-Language"))
    );
    assert_page_content(headers, html, locale);
}

fn assert_page_content(headers: &openrust::http::HeaderMap, html: &str, locale: &str) {
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
        ["Accept-Encoding", "Origin", "Accept-Language", "Cookie"]
    );
    for (path, expected) in [
        ("/vary-star", "*"),
        ("/vary-existing", "Origin, accept-language"),
    ] {
        let (headers, _) = send(&kernel, path, &["zh-CN"]).await;
        assert_eq!(
            headers.get_all("vary").iter().count(),
            if expected == "*" { 1 } else { 2 }
        );
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
    fs::write(root.0.join("locale-cookie.json"), "null").unwrap();
    let kernel = root.temporary_kernel().await;
    for locale in ["en", "zh-Hans", "zh-Hant"] {
        let (headers, html) = send(
            &kernel,
            &format!("/?locale={locale}&search=a%26b"),
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
        assert!(selected.contains(&format!("locale={locale}")));
        assert!(selected.contains("search=a%26b"));
    }
    let (_, html) = send(&kernel, "/", &["zh-TW"]).await;
    assert!(html.contains("href=\"/\" aria-current=\"true\""));
    assert!(html.contains("data-locale=\"zh-Hant\""));
}

#[tokio::test]
async fn switcher_tracks_custom_parameter_null_and_actual_candidates() {
    let root = Root::new("en");
    fs::write(root.0.join("locale-cookie.json"), "null").unwrap();
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
    assert!(html.contains("language=fr"));
    for (browser, parameter, candidates) in [
        (true, Some(serde_json::Value::Null), None),
        (false, None, None),
    ] {
        let path = if browser && candidates.is_some() {
            "/"
        } else {
            "/?locale[]=bad"
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
    let (headers, html) = send(&kernel, "/?locale[]=invalid", &["en"]).await;
    assert_page(&headers, &html, "zh-Hant");
    assert!(!html.contains("class=\"language-switcher\""));
}

#[tokio::test]
async fn header_shows_native_language_names_and_default_system_theme_controls() {
    let root = Root::new("en");
    let kernel = root.kernel().await;
    for locale in ["en", "zh-Hans", "zh-Hant"] {
        let (headers, html) = send(&kernel, &format!("/?locale={locale}"), &["en-US"]).await;
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
            assert!(header.contains(&format!(">{name}</button>")));
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
    fs::write(root.0.join("locale-cookie.json"), "null").unwrap();
    let kernel = root.kernel().await;
    let (_, html) = send(
        &kernel,
        "/?locale=zh-Hant&filter[tags][]=a%26b&filter[tags][]=%E7%94%B2&search=%3Cscript%3E",
        &["en"],
    )
    .await;
    assert!(html.contains("filter%5Btags%5D%5B0%5D=a%26b"));
    assert!(html.contains("filter%5Btags%5D%5B1%5D=%E7%94%B2"));
    assert!(html.contains("search=%3Cscript%3E"));
    assert!(html.contains("href=\"/?locale=en&amp;"));
    assert!(!html.contains("search=<script>"));
    let (_, next) = send(&kernel, "/?locale=en", &["zh-TW"]).await;
    assert!(!next.contains("filter%5Btags%5D"));
    assert!(!next.contains("search=%3Cscript%3E"));
    assert!(next.contains("href=\"/\""));
}

#[tokio::test]
async fn blade_switcher_preserves_numeric_keys_for_custom_language_parameter() {
    let root = Root::new("en");
    fs::write(root.0.join("locale-cookie.json"), "null").unwrap();
    let kernel = root.kernel_options(true, Some("5".into()), None).await;
    let (headers, html) = send(&kernel, "/?5=zh-Hant&9=keep", &["en"]).await;
    assert_page(&headers, &html, "zh-Hant");
    assert!(html.contains("href=\"/?5=en&amp;9=keep\""));
    assert!(html.contains("href=\"/?9=keep\""));
    assert!(!html.contains("0=zh-Hant"));
}

#[tokio::test]
async fn welcome_links_and_translations_follow_configured_header_and_query() {
    for header in [serde_json::json!("X-Language"), serde_json::Value::Null] {
        let root = Root::new("en");
        fs::write(root.0.join("locale-cookie.json"), "null").unwrap();
        fs::write(
            root.0.join("locale-header.json"),
            serde_json::to_vec(&header).unwrap(),
        )
        .unwrap();
        let kernel = root
            .kernel_options(true, Some("language".into()), None)
            .await;
        for (uri, expected) in [
            (
                "/?search=a%26b",
                if header.is_null() { "en" } else { "zh-Hant" },
            ),
            ("/?language=zh-Hans&search=a%26b", "zh-Hans"),
            ("/?language=en", "en"),
        ] {
            let mut request = Request::from(
                axum::extract::Request::builder()
                    .uri(uri)
                    .header("host", "localhost")
                    .header("accept-language", "zh-CN")
                    .header("x-language", "zh-TW")
                    .body(Body::empty())
                    .unwrap(),
            );
            let response = kernel.handle(&mut request).await;
            assert_eq!(response.status(), StatusCode::OK);
            let (parts, body) = response.into_parts();
            let html = String::from_utf8(
                axum::body::to_bytes(body, 1024 * 1024)
                    .await
                    .unwrap()
                    .to_vec(),
            )
            .unwrap();
            assert_eq!(parts.headers["content-language"], expected);
            if header.is_null() {
                assert!(!parts.headers.contains_key("vary"));
            } else {
                assert_eq!(parts.headers["vary"], "x-language");
            }
            assert!(html.contains(&format!("<html lang=\"{expected}\">")));
            for suffix in [
                format!("/blob/master/docs/{expected}/README.md"),
                format!("/blob/master/docs/{expected}/getting-started/deployment.md"),
                String::new(),
                "/blob/master/CHANGELOG.md".into(),
            ] {
                assert!(html.contains(&format!("href=\"{}{suffix}\"", Application::REPOSITORY)));
                if let Some(relative) = suffix.strip_prefix("/blob/master/") {
                    assert!(
                        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                            .join(relative)
                            .is_file()
                    );
                }
            }
            let expected_heading = match expected {
                "zh-Hans" => "开始使用",
                "zh-Hant" => "開始使用",
                _ => "Let’s get started",
            };
            assert!(html.contains(expected_heading));
            assert!(html.contains("language=zh-Hant"));
            if uri.contains("search=") {
                assert!(html.contains("href=\"/?search=a%26b\""));
            }
        }
    }
}

#[tokio::test]
async fn environment_header_selection_and_cached_configuration_reach_real_welcome() {
    let root = Root::new("en");
    let env_path = root.0.join(".env");
    let environment = |header: Option<&str>| {
        fs::write(
            &env_path,
            format!(
                "APP_LOCALE=en\nAPP_FALLBACK_LOCALE=en\n{}",
                header
                    .map(|value| format!("APP_LOCALE_HEADER={value}\n"))
                    .unwrap_or_default()
            ),
        )
        .unwrap();
    };
    async fn render(kernel: &Kernel) -> (axum::http::response::Parts, String) {
        let mut request = Request::from(
            axum::extract::Request::builder()
                .uri("/")
                .header("host", "localhost")
                .header("accept-language", "zh-Hans")
                .header("x-language", "zh-Hant")
                .body(Body::empty())
                .unwrap(),
        );
        let (parts, body) = kernel.handle(&mut request).await.into_parts();
        (
            parts,
            String::from_utf8(
                axum::body::to_bytes(body, 1024 * 1024)
                    .await
                    .unwrap()
                    .to_vec(),
            )
            .unwrap(),
        )
    }
    environment(None);
    let default = root.kernel().await;
    let (parts, html) = render(&default).await;
    assert_page_content(&parts.headers, &html, "zh-Hans");
    environment(Some("X-LaNgUaGe"));
    let custom = root.kernel().await;
    let (parts, html) = render(&custom).await;
    assert_page_content(&parts.headers, &html, "zh-Hant");
    assert_eq!(parts.headers["vary"], "x-language");
    let cache = root.0.join("storage/framework/config.json");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    // 缓存采用实际配置加载结果，避免手写不完整的应用配置掩盖消费链路。
    let mut cached =
        serde_json::to_value(custom.get_application().config().unwrap().all()).unwrap();
    cached["app"]["locale_header"] = serde_json::Value::Null;
    fs::write(&cache, serde_json::to_vec(&cached).unwrap()).unwrap();
    environment(Some(""));
    let disabled = root.kernel().await;
    let (parts, html) = render(&disabled).await;
    assert_page_content(&parts.headers, &html, "en");
    assert_eq!(parts.headers["vary"], "Cookie");
    // 已启动应用保留先前配置；磁盘缓存与环境变化不会按请求重载。
    let (parts, html) = render(&custom).await;
    assert_page_content(&parts.headers, &html, "zh-Hant");
    fs::remove_file(&cache).unwrap();
    let invalid = root.kernel().await;
    assert_eq!(
        render(&invalid).await.0.status,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    environment(Some("null"));
    let literal = root.kernel().await;
    let (parts, html) = render(&literal).await;
    assert_page_content(&parts.headers, &html, "en");
    assert_eq!(parts.headers["vary"], "null");
    environment(Some("X-Language"));
    let inactive = root.kernel_with_browser(false).await;
    let (parts, html) = render(&inactive).await;
    assert!(html.contains("<html lang=\"en\">"));
    assert!(!parts.headers.contains_key("vary"));
}

async fn preference_request(
    kernel: &Kernel,
    method: &str,
    path: &str,
    header: &str,
    cookies: &str,
    body: &str,
) -> (StatusCode, openrust::http::HeaderMap, String) {
    let mut raw = axum::extract::Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost")
        .header("accept-language", header);
    if !cookies.is_empty() {
        raw = raw.header("cookie", cookies);
    }
    if method == "POST" {
        raw = raw.header("content-type", "application/x-www-form-urlencoded");
    }
    let mut request = Request::from(raw.body(Body::from(body.to_owned())).unwrap());
    let (parts, body) = kernel.handle(&mut request).await.into_parts();
    let body = axum::body::to_bytes(body, 256 * 1024).await.unwrap();
    (
        parts.status,
        parts.headers,
        String::from_utf8(body.to_vec()).unwrap(),
    )
}
fn response_cookies(headers: &openrust::http::HeaderMap) -> String {
    headers
        .get_all("set-cookie")
        .iter()
        .map(|header| header.to_str().unwrap().split(';').next().unwrap())
        .collect::<Vec<_>>()
        .join("; ")
}
fn csrf_token(html: &str) -> &str {
    html.split("name=\"_token\" value=\"")
        .nth(1)
        .expect("表单包含 CSRF token")
        .split('"')
        .next()
        .unwrap()
}
fn locale_cookie(headers: &openrust::http::HeaderMap) -> &str {
    headers
        .get_all("set-cookie")
        .iter()
        .map(|header| header.to_str().unwrap())
        .find(|header| header.starts_with("locale="))
        .unwrap_or_else(|| panic!("未写入语言 Cookie：{headers:?}"))
}
fn assert_mode(html: &str, mode: &str, choice: &str) {
    assert!(html.contains(&format!("data-mode=\"{mode}\"")));
    let selected = html
        .split("<button ")
        .map(|part| part.split('>').next().unwrap())
        .find(|part| part.contains("name=\"locale\"") && part.contains("aria-current=\"true\""))
        .expect("一个偏好模式被选中");
    assert!(
        selected.contains(&format!("value=\"{choice}\"")),
        "{selected}"
    );
}

#[tokio::test]
async fn browser_preference_is_explicit_encrypted_persistent_and_resettable() {
    let root = Root::new("en");
    let kernel = root.kernel().await;
    let (status, headers, html) =
        preference_request(&kernel, "GET", "/?search=a%26b", "zh-TW", "", "").await;
    assert_eq!(status, StatusCode::OK);
    assert_page(&headers, &html, "zh-Hant");
    assert_mode(&html, "auto", "");
    assert!(
        !headers
            .get_all("set-cookie")
            .iter()
            .any(|v| v.to_str().unwrap().starts_with("locale="))
    );
    assert!(html.contains("method=\"post\" action=\"/locale?search=a%26b\""));
    let body = format!("_token={}&locale=zh-Hans", csrf_token(&html));
    let (status, saved, _) = preference_request(
        &kernel,
        "POST",
        "/locale?locale=en&search=a%26b",
        "en",
        &response_cookies(&headers),
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(saved["location"], "http://localhost?search=a%26b");
    let cookie = locale_cookie(&saved);
    assert!(!cookie.starts_with("locale=zh-Hans"));
    for attribute in ["path=/", "expires=", "httponly", "samesite=lax"] {
        assert!(cookie.to_lowercase().contains(attribute), "{cookie}");
    }
    assert!(!cookie.to_lowercase().contains("secure"));
    let cookies = response_cookies(&saved);
    for header in ["en", "zh-TW", "fr"] {
        let (status, headers, html) =
            preference_request(&kernel, "GET", "/", header, &cookies, "").await;
        assert_eq!(status, StatusCode::OK);
        assert_page(&headers, &html, "zh-Hans");
        assert_mode(&html, "fixed", "zh-Hans");
        let policy = headers["cache-control"].to_str().unwrap();
        assert!(policy.contains("private") && policy.contains("no-store"));
        assert!(!policy.split(',').any(|part| part.trim() == "public"));
    }
    let (_, headers, other) =
        preference_request(&kernel, "GET", "/other", "en", &cookies, "").await;
    assert!(other.contains("开始使用"));
    assert_eq!(headers["content-language"], "zh-Hans");
    let (_, headers, html) =
        preference_request(&kernel, "GET", "/?locale=zh-Hant", "en", &cookies, "").await;
    assert_page(&headers, &html, "zh-Hant");
    assert_mode(&html, "fixed", "zh-Hans");
    assert!(
        !headers
            .get_all("set-cookie")
            .iter()
            .any(|v| v.to_str().unwrap().starts_with("locale="))
    );
    let body = format!("_token={}&locale=zh-Hant", csrf_token(&html));
    let (status, updated, _) = preference_request(
        &kernel,
        "POST",
        "/locale",
        "en",
        &response_cookies(&headers),
        &body,
    )
    .await;
    // POST 必须携带固定偏好并非必要；会话 token 与加密 Cookie 仍须正确。
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, headers, html) =
        preference_request(&kernel, "GET", "/", "en", &response_cookies(&updated), "").await;
    assert_page(&headers, &html, "zh-Hant");
    assert_mode(&html, "fixed", "zh-Hant");
    let body = format!("_token={}&locale=", csrf_token(&html));
    let (status, cleared, _) = preference_request(
        &kernel,
        "POST",
        "/locale?locale=zh-Hans",
        "en",
        &response_cookies(&headers),
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(cleared["location"], "http://localhost");
    assert!(locale_cookie(&cleared).to_lowercase().contains("max-age=0"));
    let (_, headers, html) = preference_request(&kernel, "GET", "/", "zh-TW", "", "").await;
    assert_page(&headers, &html, "zh-Hant");
    assert_mode(&html, "auto", "");
}

#[tokio::test]
async fn preference_submission_rejects_bad_input_and_requires_csrf() {
    let root = Root::new("en");
    let kernel = root.kernel().await;
    let (_, headers, html) = preference_request(&kernel, "GET", "/", "en", "", "").await;
    let cookies = response_cookies(&headers);
    let token = csrf_token(&html);
    for value in [
        "locale=fr",
        "locale=auto",
        "locale=bad_tag",
        "locale[]=en",
        "",
    ] {
        let (status, headers, _) = preference_request(
            &kernel,
            "POST",
            "/locale",
            "en",
            &cookies,
            &format!("_token={token}&{value}"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{value}");
        assert!(
            !headers
                .get_all("set-cookie")
                .iter()
                .any(|v| v.to_str().unwrap().starts_with("locale="))
        );
    }
    for body in ["locale=en", "_token=invalid&locale=en"] {
        let (status, headers, _) =
            preference_request(&kernel, "POST", "/locale", "en", &cookies, body).await;
        assert_eq!(status.as_u16(), 419);
        assert!(
            !headers
                .get_all("set-cookie")
                .iter()
                .any(|v| v.to_str().unwrap().starts_with("locale="))
        );
    }
    let (status, headers, _) =
        preference_request(&kernel, "GET", "/locale?locale=zh-Hant", "en", &cookies, "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !headers
            .get_all("set-cookie")
            .iter()
            .any(|v| v.to_str().unwrap().starts_with("locale="))
    );
}

#[tokio::test]
async fn query_disabled_still_allows_cookie_controls_and_cookie_disabled_preserves_temporary_links()
{
    let root = Root::new("en");
    let kernel = root
        .kernel_options(true, Some(serde_json::Value::Null), None)
        .await;
    let (_, headers, html) =
        preference_request(&kernel, "GET", "/?lang=keep&search=x", "zh-Hans", "", "").await;
    assert_page(&headers, &html, "zh-Hans");
    assert_mode(&html, "auto", "");
    assert!(html.contains("action=\"/locale?lang=keep&amp;search=x\""));
    let body = format!("_token={}&locale=zh-Hant", csrf_token(&html));
    let (status, saved, _) = preference_request(
        &kernel,
        "POST",
        "/locale?lang=keep",
        "en",
        &response_cookies(&headers),
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(saved["location"], "http://localhost?lang=keep");
    fs::write(root.0.join("locale-cookie.json"), "null").unwrap();
    let disabled = root.kernel().await;
    let (_, _, html) = preference_request(
        &disabled,
        "GET",
        "/?locale=zh-Hans",
        "en",
        &response_cookies(&saved),
        "",
    )
    .await;
    assert!(html.contains("href=\"/?locale=zh-Hant\""));
    assert!(!html.contains("name=\"locale\""));
}

#[tokio::test]
async fn renamed_cookie_reuses_https_path_domain_and_same_site_on_save_and_delete() {
    let root = Root::new("en");
    fs::write(root.0.join(".env"),"APP_LOCALE=en\nAPP_FALLBACK_LOCALE=en\nAPP_LOCALE_COOKIE=language-preference\nSESSION_PATH=/portal\nSESSION_DOMAIN=example.test\nSESSION_SECURE_COOKIE=true\nSESSION_SAME_SITE=strict\n").unwrap();
    let kernel = root.kernel().await;
    let (_, headers, html) =
        preference_request(&kernel, "GET", "https://example.test/", "en", "", "").await;
    let body = format!("_token={}&locale=zh-Hant", csrf_token(&html));
    let (status, saved, _) = preference_request(
        &kernel,
        "POST",
        "https://example.test/locale",
        "en",
        &response_cookies(&headers),
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = saved
        .get_all("set-cookie")
        .iter()
        .map(|h| h.to_str().unwrap())
        .find(|h| h.starts_with("language-preference="))
        .unwrap();
    for attribute in [
        "path=/portal",
        "domain=example.test",
        "secure",
        "httponly",
        "samesite=strict",
    ] {
        assert!(cookie.contains(attribute), "{cookie}");
    }
    let age: u64 = cookie
        .split("Max-Age=")
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!((34_559_990..=34_560_000).contains(&age));
    let cookies = response_cookies(&saved);
    let (_, headers, html) =
        preference_request(&kernel, "GET", "https://example.test/", "en", &cookies, "").await;
    assert_mode(&html, "fixed", "zh-Hant");
    let body = format!("_token={}&locale=", csrf_token(&html));
    let (status, deleted, _) = preference_request(
        &kernel,
        "POST",
        "https://example.test/locale",
        "en",
        &response_cookies(&headers),
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = deleted
        .get_all("set-cookie")
        .iter()
        .map(|h| h.to_str().unwrap())
        .find(|h| h.starts_with("language-preference="))
        .unwrap();
    for attribute in [
        "path=/portal",
        "domain=example.test",
        "secure",
        "httponly",
        "samesite=strict",
        "Max-Age=0",
    ] {
        assert!(cookie.contains(attribute), "{cookie}");
    }
}

#[tokio::test]
async fn locale_post_keeps_csrf_session_and_preferences_over_tcp() {
    use axum::{extract::State, handler::Handler as _};
    use std::{net::SocketAddr, sync::Arc};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

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

    async fn send(
        address: SocketAddr,
        method: &str,
        path: &str,
        cookies: &str,
        body: &str,
    ) -> (u16, openrust::http::HeaderMap, String) {
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAccept-Language: en\r\nCookie: {cookies}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();
        let (head, body) = response.split_once("\r\n\r\n").unwrap();
        let mut lines = head.split("\r\n");
        let status = lines
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        let mut headers = openrust::http::HeaderMap::new();
        for line in lines {
            let (name, value) = line.split_once(':').unwrap();
            headers.append(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                value.trim().parse().unwrap(),
            );
        }
        assert!(!headers.contains_key("transfer-encoding"));
        (status, headers, body.to_owned())
    }

    let root = Root::new("en");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport
        .with_state(Arc::new(root.kernel().await))
        .into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    let (status, headers, html) = send(address, "GET", "/?search=a%26b", "", "").await;
    assert_eq!(status, 200);
    let cookies = response_cookies(&headers);
    let token = csrf_token(&html);
    let (status, headers, _) = send(address, "POST", "/locale", &cookies, "locale=zh-Hans").await;
    assert_eq!(status, 419);
    assert!(
        !headers
            .get_all("set-cookie")
            .iter()
            .any(|value| value.to_str().unwrap().starts_with("locale="))
    );
    let body = format!("_token={token}&locale=zh-Hans");
    let (status, headers, _) = send(
        address,
        "POST",
        "/locale?locale=en&search=a%26b",
        &cookies,
        &body,
    )
    .await;
    assert_eq!(status, 303);
    assert_eq!(headers["location"], "http://localhost?search=a%26b");
    assert!(!locale_cookie(&headers).starts_with("locale=zh-Hans"));
    let cookies = response_cookies(&headers);
    let (status, headers, html) = send(address, "GET", "/", &cookies, "").await;
    assert_eq!(status, 200);
    assert_eq!(headers["content-language"], "zh-Hans");
    assert_mode(&html, "fixed", "zh-Hans");
    let body = format!("_token={}&locale=", csrf_token(&html));
    let (status, headers, _) = send(address, "POST", "/locale", &cookies, &body).await;
    assert_eq!(status, 303);
    assert!(locale_cookie(&headers).contains("Max-Age=0"));
    server.abort();
}

#[tokio::test]
async fn default_locale_ignores_old_query_and_explicit_lang_configuration_still_works() {
    let root = Root::new("en");
    let default = root.kernel().await;
    let (status, headers, html) = preference_request(
        &default,
        "GET",
        "/?lang=zh-Hant&locale=zh-Hans",
        "en",
        "lang=zh-Hant",
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_page(&headers, &html, "zh-Hans");
    assert_mode(&html, "auto", "");
    assert!(html.contains("action=\"/locale?lang=zh-Hant\""));
    let (status, headers, html) =
        preference_request(&default, "GET", "/?lang[]=bad", "en", "", "").await;
    assert_eq!(status, StatusCode::OK);
    assert_page(&headers, &html, "en");
    fs::write(
        root.0.join(".env"),
        "APP_LOCALE=en\nAPP_LOCALE_COOKIE=lang\nAPP_LOCALE_QUERY_PARAMETER=lang\n",
    )
    .unwrap();
    let custom = root.kernel().await;
    let (status, headers, html) =
        preference_request(&custom, "GET", "/?lang=zh-Hant&locale=keep", "en", "", "").await;
    assert_eq!(status, StatusCode::OK);
    assert_page(&headers, &html, "zh-Hant");
    assert!(html.contains("action=\"/locale?locale=keep\""));
    let body = format!("_token={}&locale=zh-Hans", csrf_token(&html));
    let (status, saved, _) = preference_request(
        &custom,
        "POST",
        "/locale?lang=en&locale=keep",
        "en",
        &response_cookies(&headers),
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(saved["location"], "http://localhost?locale=keep");
    assert!(
        saved
            .get_all("set-cookie")
            .iter()
            .any(|h| h.to_str().unwrap().starts_with("lang="))
    );
    let (_, headers, html) =
        preference_request(&custom, "GET", "/", "zh-TW", &response_cookies(&saved), "").await;
    assert_page(&headers, &html, "zh-Hans");
    assert_mode(&html, "fixed", "zh-Hans");
}

#[tokio::test]
async fn discovered_languages_render_native_names_and_support_eighteen_choices() {
    let root = Root::new("en");
    let extra = [
        ("ar", "العربية"),
        ("de", "Deutsch"),
        ("es", ""),
        ("fr", "Français"),
        ("he", "עברית"),
        ("id", "Bahasa Indonesia"),
        ("it", "Italiano"),
        ("ja", "日本語"),
        ("ko", "한국어"),
        ("nl", "Nederlands"),
        ("pl", "Polski"),
        ("pt", "Português"),
        ("ru", "Русский"),
        ("th", "ไทย"),
        ("tr", "Türkçe"),
    ];
    for (locale, name) in extra {
        let path = root.0.join("lang").join(locale);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("welcome.json"), "{}").unwrap();
        if !name.is_empty() {
            fs::write(path.join("languages.json"),
                serde_json::json!({"name":name, "direction": if matches!(locale,"ar"|"he") { "rtl" } else { "ltr" }}).to_string()).unwrap();
        }
    }
    let kernel = root.kernel().await;
    let (headers, html) = send(&kernel, "/?locale=ja", &["en"]).await;
    assert_eq!(headers["content-language"], "ja");
    assert_eq!(html.matches("name=\"locale\" value=").count(), 19);
    for (locale, name) in extra {
        assert!(html.contains(&format!("value=\"{locale}\"")));
        assert!(html.contains(&format!(
            ">{}</button>",
            if name.is_empty() { locale } else { name }
        )));
    }
    // 西语缺少名称时显示代码，不能使用英文资源中的 English。
    assert!(html.contains("lang=\"es\" dir=\"auto\">es</button>"));
    assert!(!html.contains("languages.name"));
    for locale in ["ar", "he"] {
        let (headers, html) = send(&kernel, &format!("/?locale={locale}"), &["en"]).await;
        assert_eq!(headers["content-language"], locale);
        assert!(html.contains(&format!("<html lang=\"{locale}\" dir=\"rtl\">")));
    }
    // 自动候选也直接供 GET 模式使用，无需额外维护页面列表。
    fs::write(root.0.join("locale-cookie.json"), "null").unwrap();
    let temporary = root.kernel().await;
    let (_, html) = send(&temporary, "/", &["ja"]).await;
    assert_eq!(html.matches("<a href=\"/?locale=").count(), 18);
    assert!(html.contains("lang=\"ja\" dir=\"auto\">日本語</a>"));
}

#[tokio::test]
async fn a_new_language_uses_existing_csrf_preference_and_explicit_candidate_controls() {
    let root = Root::new("en");
    fs::create_dir_all(root.0.join("lang/ja")).unwrap();
    fs::write(
        root.0.join("lang/ja/languages.json"),
        r#"{"name":"日本語"}"#,
    )
    .unwrap();
    let kernel = root.kernel().await;
    let (_, headers, html) = preference_request(&kernel, "GET", "/", "en", "", "").await;
    let body = format!("_token={}&locale=ja", csrf_token(&html));
    let (status, saved, _) = preference_request(
        &kernel,
        "POST",
        "/locale",
        "en",
        &response_cookies(&headers),
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (status, headers, html) =
        preference_request(&kernel, "GET", "/", "en", &response_cookies(&saved), "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-language"], "ja");
    assert_mode(&html, "fixed", "ja");
    let restricted = root.kernel_options(true, None, Some(vec!["en"])).await;
    let (_, html) = send(&restricted, "/", &["ja"]).await;
    assert!(!html.contains("value=\"ja\""));
    let inactive = root.kernel_with_browser(false).await;
    let (_, html) = send(&inactive, "/?locale[]=invalid", &["ja"]).await;
    assert!(!html.contains("class=\"language-switcher\""));
}

#[tokio::test]
async fn preference_redirect_removes_custom_locale_override_and_preserves_nested_query() {
    let root = Root::new("en");
    let kernel = root
        .kernel_options(true, Some("language".into()), None)
        .await;
    let (_, headers, html) = preference_request(&kernel, "GET", "/", "en", "", "").await;
    let body = format!("_token={}&locale=zh-Hant", csrf_token(&html));
    let (status, saved, _) = preference_request(
        &kernel,
        "POST",
        "/locale?language=en&filter%5Bterm%5D=a%26b&locale=keep&plus=%2B",
        "en",
        &response_cookies(&headers),
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        saved["location"],
        "http://localhost?filter%5Bterm%5D=a%26b&locale=keep&plus=%2B"
    );
    let (_, headers, html) =
        preference_request(&kernel, "GET", "/", "en", &response_cookies(&saved), "").await;
    assert_page(&headers, &html, "zh-Hant");
    assert_mode(&html, "fixed", "zh-Hant");
}

#[tokio::test]
async fn environment_candidates_control_menu_and_preference_without_bootstrap_code() {
    let root = Root::new("en");
    fs::create_dir_all(root.0.join("lang/ja")).unwrap();
    fs::write(
        root.0.join("lang/ja/languages.json"),
        r#"{"name":"日本語"}"#,
    )
    .unwrap();
    fs::write(
        root.0.join(".env"),
        "APP_LOCALE=en\nAPP_SUPPORTED_LOCALES=ja,en\n",
    )
    .unwrap();
    let configured = root.kernel().await;
    let (_, headers, html) = preference_request(&configured, "GET", "/", "ja", "", "").await;
    assert_eq!(headers["content-language"], "ja");
    assert_eq!(html.matches("name=\"locale\" value=").count(), 3);
    assert!(!html.contains("value=\"zh-Hans\""));
    assert!(html.find("value=\"ja\"").unwrap() < html.find("value=\"en\"").unwrap());
    let body = format!("_token={}&locale=en", csrf_token(&html));
    let (status, saved, _) = preference_request(
        &configured,
        "POST",
        "/locale",
        "ja",
        &response_cookies(&headers),
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, headers, html) =
        preference_request(&configured, "GET", "/", "ja", &response_cookies(&saved), "").await;
    assert_eq!(headers["content-language"], "en");
    assert_mode(&html, "fixed", "en");
    let restricted = root.kernel_options(true, None, Some(vec!["en"])).await;
    let (_, html) = send(&restricted, "/", &["ja"]).await;
    assert!(!html.contains("value=\"ja\""));
    fs::write(
        root.0.join(".env"),
        "APP_LOCALE=en\nAPP_SUPPORTED_LOCALES=\n",
    )
    .unwrap();
    let disabled = root.kernel().await;
    let (_, html) = send(&disabled, "/", &["ja"]).await;
    assert!(!html.contains("class=\"language-switcher\""));
    fs::write(
        root.0.join(".env"),
        "APP_LOCALE=en\nAPP_SUPPORTED_LOCALES=null\n",
    )
    .unwrap();
    let automatic = root.kernel().await;
    let (_, html) = send(&automatic, "/", &["ja"]).await;
    assert_eq!(html.matches("name=\"locale\" value=").count(), 5);
}

#[tokio::test]
async fn preference_redirect_uses_a_body_local_target_before_saving() {
    let root = Root::new("en");
    let kernel = root.kernel().await;
    let (_, headers, html) = preference_request(&kernel, "GET", "/", "en", "", "").await;
    assert!(html.contains("name=\"redirect\" value=\"/\""));
    let cookie = response_cookies(&headers);
    let token = csrf_token(&html);
    for (field, expected) in [
        (
            "redirect=%2Faccount%3Flocale%3Den%26tab%3Da%2526b%23preferences",
            "http://localhost/account?tab=a%26b&search=%2B#preferences",
        ),
        ("redirect=", "http://localhost?search=%2B"),
        ("", "http://localhost?search=%2B"),
    ] {
        let body = format!("_token={token}&locale=zh-Hans&{field}");
        let (status, saved, _) = preference_request(
            &kernel,
            "POST",
            "/locale?locale=en&search=%2B",
            "en",
            &cookie,
            &body,
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{field}");
        assert_eq!(saved["location"], expected);
        let (_, next, html) =
            preference_request(&kernel, "GET", "/", "en", &response_cookies(&saved), "").await;
        assert_page(&next, &html, "zh-Hans");
    }
    for field in [
        "redirect=https%3A%2F%2Fevil.example",
        "redirect=%2F%2Fevil.example",
        "redirect=%2F%5Cevil.example",
        "redirect=relative",
        "redirect%5B%5D=%2Faccount",
    ] {
        let body = format!("_token={token}&locale=zh-Hant&{field}");
        let (status, headers, _) =
            preference_request(&kernel, "POST", "/locale", "en", &cookie, &body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{field}");
        assert!(!headers.contains_key("location"));
        assert!(
            !headers
                .get_all("set-cookie")
                .iter()
                .any(|v| v.to_str().unwrap().starts_with("locale="))
        );
    }
    // 查询参数不能充当返回目标；保存成功后使用应用默认目标。
    let body = format!("_token={token}&locale=zh-Hant");
    let (status, headers, _) = preference_request(
        &kernel,
        "POST",
        "/locale?redirect=%2Faccount",
        "en",
        &cookie,
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers["location"], "http://localhost?redirect=%2Faccount");
}
