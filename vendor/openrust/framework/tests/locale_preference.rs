use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc};

use openrust::{
    contracts::{foundation::Application as _, http::Kernel as _},
    cookie::{
        CookieOptions,
        middleware::{AddQueuedCookiesToResponse, EncryptCookies},
    },
    encryption::{Cipher, Encrypter},
    foundation::{
        Application,
        configuration::{Middleware, Routing},
        http::Kernel,
    },
    http::{
        Body, HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode,
        middleware::{LocaleSelection, SetLocale},
    },
    routing::MiddlewareEntry,
    support::facades::{App, Cookie, Lang, Route},
};
use serde_json::{Value, json};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "locale-preference-{}",
            openrust::support::Str::random(12)
        ));
        for (locale, text) in [("en", "Hello"), ("zh-Hans", "你好"), ("zh-Hant", "您好")] {
            fs::create_dir_all(path.join("lang").join(locale)).unwrap();
            fs::write(
                path.join("lang").join(locale).join("message.json"),
                json!({"hello":text}).to_string(),
            )
            .unwrap();
        }
        Self(path)
    }
    async fn kernel(
        &self,
        app: Value,
        key: u8,
        exempt: bool,
        configure: impl FnOnce(&mut Middleware) + Send + Sync + 'static,
    ) -> Kernel {
        let config_path = self.0.join("settings.json");
        fs::write(config_path, serde_json::to_vec(&app).unwrap()).unwrap();
        let app = Application::configure(self.0.clone())
            .withConfig(|base, _| {
                let mut app = json!({"locale":"en","fallback_locale":"en"});
                app.as_object_mut().unwrap().extend(
                    serde_json::from_slice::<Value>(&fs::read(base.join("settings.json")).unwrap())
                        .unwrap()
                        .as_object()
                        .unwrap()
                        .clone(),
                );
                Ok(BTreeMap::from([
                    ("app".into(), app),
                    (
                        "session".into(),
                        json!({"path":"/","same_site":"lax","cookie":"test-session"}),
                    ),
                ]))
            })
            .withMiddleware(move |middleware| {
                let mut encrypt = EncryptCookies::new(Arc::new(
                    Encrypter::new([key; 32], Cipher::Aes256Cbc).unwrap(),
                ));
                if exempt {
                    encrypt.disableFor(["locale"]);
                }
                // 有意不装配 Session，验证偏好只依赖安全 Cookie 管线。
                middleware.group(
                    "web",
                    [
                        MiddlewareEntry::Instance(Arc::new(encrypt)),
                        MiddlewareEntry::Instance(Arc::new(AddQueuedCookiesToResponse)),
                    ],
                );
                configure(middleware);
            })
            .withRouting(
                Routing::default()
                    .web(|| {
                        Route::get("/", inspect)?;
                        Route::get("/switch", switch)?;
                        Route::get("/unsafe-store", unsafe_store)?;
                        Route::post("/preference/{locale}", store)?;
                        Route::post("/seed/{locale}", seed)?;
                        Route::group(
                            openrust::routing::RouteGroup::new()
                                .middleware_name("cache.headers:public;max_age=60;etag"),
                            |router| {
                                router.get("/cache", inspect)?;
                                Ok(())
                            },
                        )?;
                        Ok(())
                    })
                    .api(|| {
                        Route::get("/probe", inspect)?;
                        Route::post("/preference/{locale}", store)?;
                        Ok(())
                    }),
            )
            .create()
            .unwrap();
        let mut kernel = Kernel::new(app);
        kernel.bootstrap().await.unwrap();
        kernel
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn inspect(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let first = request.getLanguages();
        tokio::task::yield_now().await;
        Response::json(
            StatusCode::OK,
            json!({
                "locale":App::currentLocale().unwrap(),"translation":Lang::get("message.hello",&BTreeMap::new(),None).unwrap(),
                "preference":request.extensions().get::<LocaleSelection>().and_then(|selection|selection.preference.clone()),
                "can_store":request.extensions().get::<LocaleSelection>().is_some_and(|selection|selection.can_store),
                "languages":first,"after":request.getLanguages(),"session":request.hasSession()
            }),
        )
    })
}
fn switch(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        App::setLocale("zh-Hant").await.unwrap();
        Ok(Response::from("changed"))
    })
}
fn store(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        SetLocale::storePreference(
            request,
            request.route("locale").filter(|locale| *locale != "auto"),
        )?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}
fn unsafe_store(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        SetLocale::storePreference(request, Some("en"))?;
        Ok(Response::from("unsafe"))
    })
}
fn seed(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Cookie::queue(Cookie::forever(
            "locale",
            request.route("locale").unwrap(),
            &CookieOptions::default(),
        )?)?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}
async fn send(
    kernel: &Kernel,
    method: &str,
    path: &str,
    cookie: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, HeaderMap, Value) {
    let mut raw = http::Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost");
    if !cookie.is_empty() {
        raw = raw.header("cookie", cookie);
    }
    for (name, value) in headers {
        raw = raw.header(*name, *value);
    }
    let mut request = Request::from(raw.body(Body::empty()).unwrap());
    // 在 Kernel 之前缓存标准头，协商自定义头也不得污染它。
    let standard = request.getLanguages().to_vec();
    let (parts, body) = kernel.handle(&mut request).await.into_parts();
    assert_eq!(request.getLanguages(), standard);
    let bytes = axum::body::to_bytes(body, 128 * 1024).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (parts.status, parts.headers, value)
}
fn saved(headers: &HeaderMap) -> String {
    headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|h| h.to_str().ok())
        .find(|h| h.starts_with("locale="))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .into()
}
fn has_locale_cookie(headers: &HeaderMap) -> bool {
    headers
        .get_all("set-cookie")
        .iter()
        .any(|h| h.to_str().unwrap().starts_with("locale="))
}

#[tokio::test]
async fn protected_preference_is_distinct_from_actual_locale_and_never_needs_session() {
    let root = Root::new();
    let kernel = root.kernel(json!({}), 1, false, |_| {}).await;
    let (_, headers, value) = send(&kernel, "GET", "/", "", &[("accept-language", "zh-TW")]).await;
    assert_eq!(value["locale"], "zh-Hant");
    assert_eq!(value["preference"], Value::Null);
    assert_eq!(value["session"], false);
    assert_eq!(value["can_store"], true);
    assert!(!has_locale_cookie(&headers));
    let (status, headers, _) = send(&kernel, "POST", "/preference/zh-Hans", "", &[]).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let cookie = saved(&headers);
    for (path, expected) in [("/", "zh-Hans"), ("/?locale=zh-Hant", "zh-Hant")] {
        let (_, headers, value) =
            send(&kernel, "GET", path, &cookie, &[("accept-language", "en")]).await;
        assert_eq!(value["locale"], expected);
        assert_eq!(value["preference"], "zh-Hans");
        assert_eq!(value["session"], false);
        assert!(!has_locale_cookie(&headers));
        assert!(headers.get_all("vary").iter().any(|h| h == "Cookie"));
        assert!(
            headers["cache-control"]
                .to_str()
                .unwrap()
                .contains("no-store")
        );
    }
    let (_, headers, _) = send(&kernel, "GET", "/switch", &cookie, &[]).await;
    assert_eq!(headers["content-language"], "zh-Hant");
    let (status, headers, _) = send(&kernel, "GET", "/unsafe-store", &cookie, &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!has_locale_cookie(&headers));
    assert_eq!(kernel.get_application().getLocale().unwrap(), "en");
}

#[tokio::test]
async fn invalid_cookie_values_and_removed_candidates_safely_fall_back() {
    let root = Root::new();
    let kernel = root.kernel(json!({}), 2, false, |_| {}).await;
    for cookie in ["locale=zh-Hant", "locale=broken", "locale=", "locale[]=bad"] {
        let (status, headers, value) = send(
            &kernel,
            "GET",
            "/",
            cookie,
            &[("accept-language", "zh-Hans")],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{cookie}");
        assert_eq!(value["locale"], "zh-Hans");
        assert_eq!(value["preference"], Value::Null);
        assert!(!has_locale_cookie(&headers));
    }
    for locale in ["bad_tag", "fr", "auto"] {
        let (_, headers, _) = send(&kernel, "POST", &format!("/seed/{locale}"), "", &[]).await;
        let (_, _, value) = send(
            &kernel,
            "GET",
            "/",
            &saved(&headers),
            &[("accept-language", "zh-TW")],
        )
        .await;
        assert_eq!(value["locale"], "zh-Hant");
        assert_eq!(value["preference"], Value::Null);
    }
    let (_, headers, _) = send(&kernel, "POST", "/preference/zh-Hant", "", &[]).await;
    let cookie = saved(&headers);
    let changed = root
        .kernel(json!({}), 2, false, |m| {
            m.setLocale(&["en", "zh-Hans"]).unwrap();
        })
        .await;
    let (_, _, value) = send(
        &changed,
        "GET",
        "/",
        &cookie,
        &[("accept-language", "zh-Hans")],
    )
    .await;
    assert_eq!(value["locale"], "zh-Hans");
    assert_eq!(value["preference"], Value::Null);
}

#[tokio::test]
async fn stateless_api_ignores_cookie_and_does_not_create_browser_state() {
    let root = Root::new();
    let kernel = root
        .kernel(json!({"locale_header":"X-Language"}), 3, false, |_| {})
        .await;
    let (_, headers, _) = send(&kernel, "POST", "/preference/zh-Hant", "", &[]).await;
    let cookie = saved(&headers);
    let (_, headers, value) = send(
        &kernel,
        "GET",
        "/api/probe",
        &cookie,
        &[("x-language", "zh-Hans"), ("accept-language", "en")],
    )
    .await;
    assert_eq!(value["locale"], "zh-Hans");
    assert_eq!(value["translation"], "你好");
    assert_eq!(value["languages"], json!(["en"]));
    assert_eq!(value["after"], json!(["en"]));
    assert_eq!(value["preference"], Value::Null);
    assert_eq!(value["can_store"], false);
    assert_eq!(value["session"], false);
    assert!(!headers.contains_key("set-cookie"));
    assert_eq!(headers["vary"], "x-language");
    assert!(!headers.contains_key("cache-control"));
    let (status, headers, _) = send(&kernel, "POST", "/api/preference/en", &cookie, &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!headers.contains_key("set-cookie"));
}

#[tokio::test]
async fn callback_query_cookie_header_default_priority_and_disabled_sources() {
    let root = Root::new();
    let kernel = root
        .kernel(
            json!({"locale_header":"X-Language","locale_query_parameter":"language"}),
            4,
            false,
            |m| {
                m.localeUsing(|request| {
                    Ok(request
                        .headers()
                        .get("x-user-language")
                        .map(|h| h.to_str().unwrap().into()))
                });
            },
        )
        .await;
    let (_, headers, _) = send(&kernel, "POST", "/preference/zh-Hans", "", &[]).await;
    let cookie = saved(&headers);
    for (path, cookie, headers, expected, store) in [
        (
            "/?language=en",
            cookie.as_str(),
            vec![("x-user-language", "zh-Hant"), ("x-language", "en")],
            "zh-Hant",
            false,
        ),
        (
            "/?language=zh-Hant",
            cookie.as_str(),
            vec![("x-language", "en")],
            "zh-Hant",
            true,
        ),
        (
            "/",
            cookie.as_str(),
            vec![("x-language", "en")],
            "zh-Hans",
            true,
        ),
        ("/", "", vec![("x-language", "zh-Hant")], "zh-Hant", true),
        ("/", "", vec![("accept-language", "zh-Hant")], "en", true),
    ] {
        let (status, _, value) = send(&kernel, "GET", path, cookie, &headers).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["locale"], expected);
        assert_eq!(value["can_store"], store);
    }
    let disabled = root
        .kernel(
            json!({"locale_header":null,"locale_query_parameter":null}),
            4,
            false,
            |_| {},
        )
        .await;
    let (_, headers, value) = send(
        &disabled,
        "GET",
        "/?locale=bad_tag",
        &cookie,
        &[("accept-language", "en")],
    )
    .await;
    assert_eq!(value["locale"], "zh-Hans");
    assert_eq!(headers["vary"], "Cookie");
    let off = root
        .kernel(json!({"locale_cookie":null}), 4, false, |_| {})
        .await;
    let (_, headers, value) = send(&off, "GET", "/", &cookie, &[("accept-language", "en")]).await;
    assert_eq!(value["locale"], "en");
    assert_eq!(value["can_store"], false);
    assert_eq!(headers["vary"], "Accept-Language");
    let empty = root
        .kernel(json!({"locale_cookie":false}), 4, false, |m| {
            m.setLocale(&[]).unwrap();
        })
        .await;
    let (status, headers, value) = send(&empty, "GET", "/?locale=bad_tag", &cookie, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["locale"], "en");
    assert!(!headers.contains_key("vary"));
}

#[tokio::test]
async fn preferences_are_isolated_between_requests_apps_and_encryption_keys() {
    let root = Root::new();
    let first = root.kernel(json!({}), 5, false, |_| {}).await;
    let (_, h, _) = send(&first, "POST", "/preference/zh-Hans", "", &[]).await;
    let hans = saved(&h);
    let (_, h, _) = send(&first, "POST", "/preference/zh-Hant", "", &[]).await;
    let hant = saved(&h);
    let (a, b, c) = tokio::join!(
        send(&first, "GET", "/", &hans, &[]),
        send(&first, "GET", "/", &hant, &[]),
        send(&first, "GET", "/", "", &[("accept-language", "en")])
    );
    assert_eq!(a.2["locale"], "zh-Hans");
    assert_eq!(b.2["locale"], "zh-Hant");
    assert_eq!(c.2["locale"], "en");
    let other = Root::new();
    let second = other.kernel(json!({}), 6, false, |_| {}).await;
    let (_, _, value) = send(&second, "GET", "/", &hans, &[("accept-language", "en")]).await;
    assert_eq!(value["locale"], "en");
    assert_eq!(value["preference"], Value::Null);
    let exempt = root.kernel(json!({}), 5, true, |_| {}).await;
    let (_, headers, value) = send(
        &exempt,
        "GET",
        "/",
        "locale=zh-Hant",
        &[("accept-language", "en")],
    )
    .await;
    assert_eq!(value["locale"], "en");
    assert_eq!(value["can_store"], false);
    assert_eq!(headers["vary"], "Accept-Language");
    let (status, _, _) = send(&exempt, "POST", "/preference/en", "", &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn cookie_configuration_and_cache_snapshot_are_validated() {
    let root = Root::new();
    for value in [
        json!(""),
        json!(false),
        json!([]),
        json!("a b"),
        json!("locale[x]"),
        json!("XSRF-TOKEN"),
        json!("test-session"),
    ] {
        let kernel = root
            .kernel(json!({"locale_cookie":value}), 7, false, |_| {})
            .await;
        assert_eq!(
            send(&kernel, "GET", "/", "", &[]).await.0,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{value}"
        );
    }
    let custom = root
        .kernel(
            json!({"locale_cookie":"language-preference"}),
            7,
            false,
            |_| {},
        )
        .await;
    let (status, headers, _) = send(&custom, "POST", "/preference/zh-Hant", "", &[]).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let cookie = headers
        .get_all("set-cookie")
        .iter()
        .find(|h| h.to_str().unwrap().starts_with("language-preference="))
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    assert_eq!(
        send(&custom, "GET", "/", cookie, &[]).await.2["locale"],
        "zh-Hant"
    );
    let mut config =
        serde_json::to_value(custom.get_application().config().unwrap().all()).unwrap();
    config["app"]["locale_cookie"] = Value::Null;
    fs::create_dir_all(root.0.join("storage/framework")).unwrap();
    fs::write(
        root.0.join("storage/framework/config.json"),
        config.to_string(),
    )
    .unwrap();
    let cached = root
        .kernel(json!({"locale_cookie":false}), 7, false, |_| {})
        .await;
    let (_, _, value) = send(&cached, "GET", "/", cookie, &[("accept-language", "en")]).await;
    assert_eq!(value["locale"], "en");
    assert_eq!(value["can_store"], false);
    assert_eq!(
        send(&custom, "GET", "/", cookie, &[]).await.2["locale"],
        "zh-Hant"
    );
}

#[tokio::test]
async fn route_cache_directives_cannot_publish_cookie_variants() {
    let root = Root::new();
    let kernel = root.kernel(json!({}), 8, false, |_| {}).await;
    let (_, headers, _) = send(&kernel, "POST", "/preference/zh-Hans", "", &[]).await;
    let (_, headers, value) = send(&kernel, "GET", "/cache", &saved(&headers), &[]).await;
    assert_eq!(value["locale"], "zh-Hans");
    let cache = headers["cache-control"].to_str().unwrap();
    assert!(
        cache.contains("private") && cache.contains("no-store") && cache.contains("max-age=60")
    );
    assert!(!cache.split(',').any(|part| part.trim() == "public"));
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn stateful_browser_api_uses_protected_preference_only_for_trusted_origins() {
    let root = Root::new();
    let app=Application::configure(root.0.clone())
        .withConfig(|_,_| Ok(BTreeMap::from([
            ("app".into(),json!({"locale":"en","fallback_locale":"en","key":"a".repeat(32)})),
            ("session".into(),json!({"driver":"array","cookie":"stateful-session","path":"/","lottery":[0,100]})),
            ("sanctum".into(),json!({"stateful":["localhost"],"middleware":{"authenticate_session":false}})),
        ])))
        .withMiddleware(|m|{m.statefulApi();})
        .withRouting(Routing::default().api(|| {Route::get("/probe",inspect)?;Route::post("/preference/{locale}",store)?;Ok(())})).create().unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let (status, headers, _) = send(
        &kernel,
        "POST",
        "/api/preference/zh-Hant",
        "",
        &[
            ("origin", "http://localhost"),
            ("sec-fetch-site", "same-origin"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let cookie = saved(&headers);
    let (_, headers, value) = send(
        &kernel,
        "GET",
        "/api/probe",
        &cookie,
        &[("origin", "http://localhost"), ("accept-language", "en")],
    )
    .await;
    assert_eq!(value["locale"], "zh-Hant");
    assert_eq!(value["preference"], "zh-Hant");
    assert_eq!(value["session"], true);
    assert_eq!(value["can_store"], true);
    assert!(
        headers["cache-control"]
            .to_str()
            .unwrap()
            .contains("no-store")
    );
    for origin in [None, Some("http://other.example")] {
        let mut headers = vec![("accept-language", "en")];
        if let Some(origin) = origin {
            headers.push(("origin", origin));
        }
        let (_, headers, value) = send(&kernel, "GET", "/api/probe", &cookie, &headers).await;
        assert_eq!(value["locale"], "en");
        assert_eq!(value["session"], false);
        assert_eq!(value["can_store"], false);
        assert!(!headers.contains_key("set-cookie"));
    }
}

#[tokio::test]
async fn locale_defaults_ignore_legacy_sources_but_explicit_lang_remains_supported() {
    let root = Root::new();
    let default = root.kernel(json!({}), 19, false, |_| {}).await;
    let (_, headers, _) = send(&default, "POST", "/preference/zh-Hans", "", &[]).await;
    let cookie = headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|h| h.to_str().ok())
        .find(|h| h.starts_with("locale="))
        .expect("默认偏好 Cookie 名应为 locale")
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let legacy = root
        .kernel(
            json!({"locale_cookie":"lang","locale_query_parameter":"lang"}),
            19,
            false,
            |_| {},
        )
        .await;
    let (_, headers, _) = send(&legacy, "POST", "/preference/zh-Hant", "", &[]).await;
    let old = headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|h| h.to_str().ok())
        .find(|h| h.starts_with("lang="))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    for path in ["/?lang=zh-Hant", "/?lang[]=invalid", "/?lang=bad_tag"] {
        let (status, headers, value) =
            send(&default, "GET", path, &old, &[("accept-language", "en")]).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["locale"], "en");
        assert_eq!(value["preference"], Value::Null);
        assert_eq!(headers["content-language"], "en");
        assert_eq!(value["translation"], "Hello");
    }
    let both = format!("{old}; {cookie}");
    assert_eq!(
        send(&default, "GET", "/?lang=zh-Hant", &both, &[]).await.2["locale"],
        "zh-Hans"
    );
    assert_eq!(
        send(&default, "GET", "/?locale=zh-Hant", &both, &[])
            .await
            .2["locale"],
        "zh-Hant"
    );
    // 旧名称仅在显式配置后生效，两个来源可分别配置。
    for config in [
        json!({"locale_cookie":"lang"}),
        json!({"locale_query_parameter":"lang"}),
        json!({"locale_cookie":"lang","locale_query_parameter":"lang"}),
    ] {
        let kernel = root.kernel(config.clone(), 19, false, |_| {}).await;
        let cookie_name = config["locale_cookie"].as_str().unwrap_or("locale");
        let query_name = config["locale_query_parameter"]
            .as_str()
            .unwrap_or("locale");
        let stored = if cookie_name == "lang" { &old } else { &cookie };
        assert_eq!(
            send(&kernel, "GET", "/", stored, &[("accept-language", "en")])
                .await
                .2["locale"],
            if cookie_name == "lang" {
                "zh-Hant"
            } else {
                "zh-Hans"
            }
        );
        assert_eq!(
            send(&kernel, "GET", &format!("/?{query_name}=en"), stored, &[])
                .await
                .2["locale"],
            "en"
        );
        let (status, headers, _) = send(&kernel, "POST", "/preference/auto", stored, &[]).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(headers.get_all("set-cookie").iter().any(|h| {
            let cookie = h.to_str().unwrap();
            cookie.starts_with(&format!("{cookie_name}="))
                && cookie.to_lowercase().contains("max-age=0")
        }));
    }
}

#[tokio::test]
async fn cached_locale_names_override_live_configuration_without_affecting_other_apps() {
    let root = Root::new();
    fs::create_dir_all(root.0.join("storage/framework")).unwrap();
    fs::write(root.0.join("storage/framework/config.json"),json!({
        "app":{"locale":"en","fallback_locale":"en","locale_cookie":"lang","locale_query_parameter":"lang"},
        "session":{"path":"/","same_site":"lax","cookie":"test-session"}
    }).to_string()).unwrap();
    let cached = root
        .kernel(
            json!({"locale_cookie":false,"locale_query_parameter":false}),
            20,
            false,
            |_| {},
        )
        .await;
    let (status, headers, _) = send(&cached, "POST", "/preference/zh-Hant", "", &[]).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let cookie = headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|h| h.to_str().ok())
        .find(|h| h.starts_with("lang="))
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    assert_eq!(
        send(&cached, "GET", "/", cookie, &[]).await.2["locale"],
        "zh-Hant"
    );
    assert_eq!(
        send(&cached, "GET", "/?lang=zh-Hans", cookie, &[]).await.2["locale"],
        "zh-Hans"
    );
    assert_eq!(
        send(&cached, "GET", "/?locale=zh-Hans", cookie, &[])
            .await
            .2["locale"],
        "zh-Hant"
    );
    let other_root = Root::new();
    let default = other_root.kernel(json!({}), 20, false, |_| {}).await;
    assert_eq!(
        send(&default, "GET", "/?lang=zh-Hans", cookie, &[]).await.2["locale"],
        "en"
    );
}
