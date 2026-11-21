use std::{fs, path::PathBuf};

use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Lang, Route},
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
        for (locale, data) in [
            ("en", include_str!("../lang/en/welcome.json")),
            ("zh_CN", include_str!("../lang/zh_CN/welcome.json")),
            ("zh_TW", include_str!("../lang/zh_TW/welcome.json")),
        ] {
            fs::create_dir_all(path.join("lang").join(locale)).unwrap();
            fs::write(path.join("lang").join(locale).join("welcome.json"), data).unwrap();
        }
        Self(path)
    }

    async fn kernel(&self) -> Kernel {
        let application = Application::configure(self.0.clone())
            .withConfig(|base, env| {
                let mut values = config::load(base, env)?;
                values.get_mut("app").unwrap()["key"] = "a".repeat(32).into();
                values.get_mut("app").unwrap()["name"] = "Welcome <Test>".into();
                values.get_mut("session").unwrap()["driver"] = "array".into();
                Ok(values)
            })
            .withRouting(Routing::default().web(|| {
                web::register()?;
                Route::get("/locale", default_locale)
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
        Ok(Response::new(
            StatusCode::OK,
            Body::from(Lang::getLocale().unwrap()),
        ))
    })
}
async fn send(
    kernel: &Kernel,
    path: &str,
    languages: &[&str],
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
    assert_eq!(response.status(), StatusCode::OK);
    let (parts, body) = response.into_parts();
    let body = axum::body::to_bytes(body, 128 * 1024).await.unwrap();
    (parts.headers, String::from_utf8(body.to_vec()).unwrap())
}
fn assert_page(headers: &openrust::http::HeaderMap, html: &str, locale: &str) {
    let (tag, heading, documentation, source, deployment, changelog) = match locale {
        "zh_CN" => (
            "zh-CN",
            "开始使用",
            "开发文档",
            "项目源码",
            "部署指南",
            "查看更新日志",
        ),
        "zh_TW" => (
            "zh-TW",
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
        ("zh", "zh_CN"),
        ("zh-CN", "zh_CN"),
        ("zh-SG", "zh_CN"),
        ("zh-Hans", "zh_CN"),
        ("zh-TW", "zh_TW"),
        ("zh-HK", "zh_TW"),
        ("zh-MO", "zh_TW"),
        ("zh-Hant", "zh_TW"),
        ("zh-Hant-CN", "zh_TW"),
        ("zh-Hans-TW", "zh_CN"),
        ("ZH-hk", "zh_TW"),
        ("en;q=0.2, zh-TW;q=0.9, zh-CN;q=0.5", "zh_TW"),
        ("fr-FR, zh-CN;q=0.8, en;q=0.7", "zh_CN"),
        ("zh-TW;q=0, en;q=0.5", "en"),
        ("zh-CN;q=0.5, zh-TW;q=0.5", "zh_CN"),
        ("zh-TW;q=1.000, en;q=0.999", "zh_TW"),
    ] {
        let (headers, html) = send(&kernel, "/", &[language]).await;
        assert_page(&headers, &html, expected);
    }
    let (headers, html) = send(&kernel, "/", &["en;q=0.2", "zh-HK;q=0.9"]).await;
    assert_page(&headers, &html, "zh_TW");
}

#[tokio::test]
async fn invalid_or_unsupported_preferences_use_the_supported_default() {
    for (default, expected) in [("zh_TW", "zh_TW"), ("zh_CN", "zh_CN"), ("fr", "en")] {
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
    let traditional = Root::new("zh_TW");
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
            (cn, "zh_CN"),
            (tw, "zh_TW"),
            (fallback, "zh_TW"),
        ] {
            assert_page(&headers, &html, locale);
        }
    }
    assert_eq!(send(&first, "/locale", &[]).await.1, "en");
    assert_eq!(send(&second, "/locale", &[]).await.1, "zh_TW");
}

#[tokio::test]
async fn missing_translation_uses_the_existing_english_fallback() {
    let root = Root::new("en");
    let path = root.0.join("lang/zh_TW/welcome.json");
    let mut copy: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    copy.as_object_mut().unwrap().remove("changelog");
    fs::write(path, serde_json::to_vec(&copy).unwrap()).unwrap();
    let kernel = root.kernel().await;
    let (headers, html) = send(&kernel, "/", &["zh-TW"]).await;
    assert_eq!(headers["content-language"], "zh-TW");
    assert!(html.contains("開始使用"));
    assert!(html.contains("View changelog"));
    assert!(!html.contains("welcome.changelog"));
}
