use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, SystemTime},
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{foundation::Application as _, http::Kernel as _, routing::UrlGenerator as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, RedirectResponse, Request, Response, StatusCode},
    routing::{
        RedirectOptions, RouteGroup, RouteParameter, Router, RoutingError, SignatureMode, UrlError,
        UrlGenerator, UrlValue,
    },
    support::facades::{Redirect, Url},
};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

const REFERENCE_KEY: &str = "base64:QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=";

fn article(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Response::json(StatusCode::OK, json!({"visited":true})) })
}

fn links(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let parameters = [RouteParameter::named("article", "42")];
        let mode = SignatureMode::Absolute;
        let expires = SystemTime::now() + Duration::from_secs(60);
        let absolute = Url::temporary_signed_route("article", expires, &parameters, mode)?;
        let relative = Url::signed_route("relative", &parameters, None, SignatureMode::Relative)?;
        let expired = Url::temporary_signed_route(
            "article",
            SystemTime::now() - Duration::from_secs(5),
            &parameters,
            mode,
        )?;
        Response::json(
            StatusCode::OK,
            json!({
                "absolute": absolute, "relative": relative, "expired": expired,
                "named": Url::route("ordinary", &[], true)?,
            "article": Url::route("article", &parameters, true)?,
            }),
        )
    })
}

fn redirect(_: &mut Request) -> HttpFuture<'_, Result<RedirectResponse, HttpError>> {
    Box::pin(async {
        Ok(Redirect::route(
            "ordinary",
            &[RouteParameter::named("slug", "a b")],
            RedirectOptions::default(),
        )?)
    })
}

const SHOW_ARTICLE: &str = "ArticleController@show";

fn action_links(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let parameters = [RouteParameter::named("article", "a b")];
        assert!(matches!(
            Url::action("OldController@show", &parameters, true),
            Err(UrlError::MissingAction)
        ));
        assert!(matches!(
            Url::action(SHOW_ARTICLE, &[], true),
            Err(UrlError::MissingParameter)
        ));
        Response::json(
            StatusCode::OK,
            json!({
                "absolute": Url::action(SHOW_ARTICLE, &parameters, true)?,
                "relative": Url::action(SHOW_ARTICLE, &parameters, false)?,
            }),
        )
    })
}

fn action_redirect(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Ok(Redirect::action(
            SHOW_ARTICLE,
            &[RouteParameter::named("article", "42")],
            RedirectOptions::default(),
        )?
        .into())
    })
}

fn request_links(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        if let Some(locale) = request.route("locale").map(str::to_owned) {
            Url::defaults(request, &[("locale", "replaced")]);
            Url::defaults(request, &[("locale", &locale), ("slug", "localized")]);
        }
        tokio::task::yield_now().await;
        let params = [RouteParameter::positional("a%?#")];
        let mode = SignatureMode::Absolute;
        Response::json(
            StatusCode::OK,
            json!({
                "full": Url::full()?,
                "current": Url::current()?,
                "previous_path": Url::previous_path(request, Some("/fallback/?x=1"))?,
                "default": Url::route("ordinary", &[], false)?,
                "positional_override": Url::route("localized", &[RouteParameter::positional("en"), params[0].clone()], false)?,
                "named_required": Url::route("localized", &[RouteParameter::named("article", "post"), RouteParameter::positional("en")], false)?,

                "action": Url::action("LocalizedController@show", &params, false).ok(),
                "explicit": Url::route("localized", &[RouteParameter::named("locale", "en"), params[0].clone()], false)?,
                "signed": Url::signed_route("localized", &params, None, mode).ok(),
            }),
        )
    })
}

fn redirect_variant(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mut options = RedirectOptions {
            status: StatusCode::TEMPORARY_REDIRECT,
            ..Default::default()
        };
        options
            .headers
            .insert("x-redirect-proof", "kept".parse().unwrap());
        let response = match request.route("variant").unwrap() {
            "refresh" => Redirect::refresh(options)?,
            "secure" => Redirect::secure("/article/42", options)?,
            "away" => Redirect::away("https://example.test/next?x=one&y=two", options)?,
            "relative" => Redirect::away("relative?x=one", options)?,
            "fragment-add" => Redirect::away("relative?x=%23", options)?.with_fragment("new")?,
            "fragment-replace" => {
                Redirect::away("https://example.test/next?x=one&y=%23#old", options)?
                    .with_fragment("ignored#new&more#inner")?
            }
            "fragment-remove" => Redirect::away("/next?x=%23#old", options)?.without_fragment()?,
            "fragment-empty" => Redirect::away("/next#old", options)?.with_fragment("")?,
            "fragment-unchanged" => Redirect::away("/next?x=%23", options)?.without_fragment()?,
            "fragment-unicode" => Redirect::away("/next", options)?.with_fragment("#章节")?,
            "fragment-invalid" => {
                assert!(matches!(
                    Redirect::away("/next", options)?.with_fragment("bad\r\nX: value"),
                    Err(UrlError::InvalidRedirect)
                ));
                assert!(matches!(
                    Redirect::away("#old", RedirectOptions::default())?.without_fragment(),
                    Err(UrlError::InvalidRedirect)
                ));
                return Ok(Response::no_content());
            }
            "temporary" => Redirect::temporary_signed_route(
                "article",
                SystemTime::now() + Duration::from_secs(60),
                &[RouteParameter::named("article", "42")],
                options,
            )?,
            "signed" => Redirect::signed_route(
                "article",
                &[RouteParameter::named("article", "42")],
                None,
                options,
            )?,
            "invalid" => {
                for target in [
                    "",
                    "//outside.test",
                    "javascript:alert(1)",
                    "/a\\b",
                    "/bad\r\nX: y",
                ] {
                    assert!(matches!(
                        Redirect::away(target, RedirectOptions::default()),
                        Err(UrlError::InvalidRedirect)
                    ));
                }
                options.status = StatusCode::NOT_MODIFIED;
                assert!(matches!(
                    Redirect::away("/safe", options),
                    Err(UrlError::InvalidRedirect)
                ));
                return Ok(Response::empty(StatusCode::NO_CONTENT));
            }
            _ => unreachable!(),
        };
        Ok(response.into())
    })
}

fn redirect_same_origin(
    request: &mut Request,
) -> HttpFuture<'_, Result<RedirectResponse, HttpError>> {
    Box::pin(async move {
        let target = request.query(Some("target"))?.unwrap();
        let mode = request.route("mode").unwrap();
        let mut options = RedirectOptions {
            status: StatusCode::SEE_OTHER,
            ..Default::default()
        };
        options
            .headers
            .insert("x-redirect-proof", "kept".parse().unwrap());
        let fallback = if mode == "invalid-fallback" {
            ""
        } else {
            "/fallback?from=origin&retry=1"
        };
        Redirect::away(target.as_str().unwrap(), options)?.enforce_same_origin(
            request,
            fallback,
            mode != "skip-scheme",
            mode != "skip-port",
        )
    })
}

#[tokio::test]
async fn redirect_origin_uses_the_effective_trusted_proxy_origin() {
    use openrust::{http::TrustedProxyHeaders, support::facades::Route};

    let app = Application::configure(std::env::temp_dir())
        .with_middleware(|middleware| {
            middleware
                .trust_proxies(&["127.0.0.1"], TrustedProxyHeaders::DEFAULT)
                .unwrap();
        })
        .with_routing(
            Routing::default().web(|| Route::get("/redirect-origin/{mode}", redirect_same_origin)),
        )
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport
                .with_state(Arc::new(kernel))
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let response = exchange_with(
        address,
        "GET",
        "/redirect-origin/strict?target=https%3A%2F%2Fmalicious.invalid%3A8443%2Fnext",
        "internal.test",
        "",
        "X-Forwarded-Proto: https\r\nX-Forwarded-Port: 8443\r\n",
        303,
    )
    .await;
    assert!(
        response.contains("location: https://malicious.invalid:8443/next\r\n"),
        "{response}"
    );
    server.abort();
}

fn routes(router: &mut Router) -> Result<(), RoutingError> {
    router
        .get("/article/{article}", article)?
        .name("article")
        .middleware_name("signed");
    router
        .get("/relative/{article}", article)?
        .name("relative")
        .middleware_name("signed:relative,utm");
    router
        .get("/page/{slug}/{suffix?}", article)?
        .name("ordinary")
        .defaults("slug", "default");
    router
        .get("/action-articles/{article}", article)?
        .set_action("OldController@show")?;
    router
        .get("/action-articles/{article}", article)?
        .set_action(SHOW_ARTICLE)?;
    router
        .get("/action-alias/{article}", article)?
        .set_action(SHOW_ARTICLE)?;
    router
        .get("/account", article)?
        .set_action("AccountController@show")?;
    router.group(
        RouteGroup::new().domain("{tenant}.example.test"),
        |router| {
            router
                .get("/account", article)?
                .set_action("AccountController@show")?;
            Ok(())
        },
    )?;
    router.get("/action-links", action_links)?;
    router.get("/action-redirect", action_redirect)?;
    router.get("/links", links)?;
    router.get("/redirect", redirect)?;
    router
        .get("/localized/{locale}/{article}", article)?
        .name("localized")
        .set_action("LocalizedController@show")?
        .middleware_name("signed");
    router.get("/bound/{article:slug}", article)?.name("bound");
    router.get("/request-links/{locale?}", request_links)?;
    router.get("/redirect-variant/{variant}", redirect_variant)?;
    router.get("/redirect-origin/{mode}", redirect_same_origin)?;
    router.redirect("/optional-old/{id?}", "/new/{id?}", StatusCode::FOUND)?;
    assert!(
        router
            .redirect("/reserved/{status?}", "/next", StatusCode::FOUND)
            .is_err()
    );
    assert!(
        router
            .redirect("/reserved/{destination}", "/next", StatusCode::FOUND)
            .is_err()
    );
    router.permanent_redirect("/old/{id}", "/new/{id}")?;
    Ok(())
}

fn make() -> Application {
    Application::configure(std::env::temp_dir())
        .with_config(|_, _| {
            Ok([(
                "app".into(),
                json!({"url":"https://example.test","key":REFERENCE_KEY}),
            )]
            .into())
        })
        .with_routing(Routing::new(|_| {
            let mut router = Router::new();
            routes(&mut router)?;
            Ok(router)
        }))
        .create()
        .unwrap()
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}

async fn exchange(address: SocketAddr, path: &str, host: &str, status: u16) -> String {
    exchange_with(address, "GET", path, host, "", "", status).await
}

async fn exchange_with(
    address: SocketAddr,
    method: &str,
    path: &str,
    host: &str,
    body: &str,
    headers: &str,
    status: u16,
) -> String {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let request = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nX-Forwarded-Host: malicious.invalid\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}", body.len());
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with(&format!("HTTP/1.1 {status} ")), "{path} {response}");
        response
    }).await.unwrap()
}

#[tokio::test]
async fn named_routes_match_php_fixture_and_keep_ordered_parameters() {
    use openrust::contracts::container::Container as _;
    let mut app = make();
    app.boot().await.unwrap();
    let urls = app.container().make::<UrlGenerator>().unwrap();
    let action_parameters = [RouteParameter::named("article", "42")];
    assert_eq!(
        urls.action(SHOW_ARTICLE, &action_parameters, true).unwrap(),
        "https://example.test/action-articles/42"
    );
    assert_eq!(
        urls.action(
            r"\AccountController@show",
            &[RouteParameter::named("tenant", "demo")],
            true
        )
        .unwrap(),
        "https://demo.example.test/account"
    );
    let mut other = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    other.boot().await.unwrap();
    let other_urls = other.container().make::<UrlGenerator>().unwrap();
    assert!(matches!(
        other_urls.action(SHOW_ARTICLE, &action_parameters, true),
        Err(UrlError::MissingAction)
    ));
    let parameters = [
        RouteParameter::named("article", "42"),
        RouteParameter::named("z", "last"),
        RouteParameter::named("a", "first"),
    ];
    // Exported by the pinned PHP fixture in the v0.x URL acceptance suite.
    assert_eq!(
        urls.signed_route("article", &parameters, None, SignatureMode::Absolute)
            .unwrap(),
        "https://example.test/article/42?a=first&z=last&signature=3a50fabb90297c1b500f0effa8923b783232d03c06487bafb9e6c5fe59c1f497"
    );
    assert_eq!(
        urls.query(
            "/page?z=1&filters[]=old&q=old&q=new",
            &[
                RouteParameter::named(
                    "filters",
                    UrlValue::Array(vec![RouteParameter::positional("x y")])
                ),
                RouteParameter::named("z", "2"),
            ],
            &[],
            None
        )
        .unwrap(),
        "https://example.test/page?z=2&filters%5B0%5D=x%20y&q=new"
    );
    let parameters = [
        RouteParameter::named("suffix", "café"),
        RouteParameter::named(
            "order",
            UrlValue::Array(vec![RouteParameter::positional("a b")]),
        ),
        RouteParameter::named("include", "notes"),
    ];
    assert_eq!(
        urls.route("ordinary", &parameters, false).unwrap(),
        "/page/default/caf%C3%A9?order%5B0%5D=a%20b&include=notes"
    );
    assert!(matches!(
        urls.route("missing", &[], true),
        Err(UrlError::MissingRoute)
    ));
    assert!(matches!(
        urls.route("article", &[], true),
        Err(UrlError::MissingParameter)
    ));
    assert!(matches!(
        urls.signed_route(
            "article",
            &[RouteParameter::named("signature", "x")],
            None,
            SignatureMode::Absolute
        ),
        Err(UrlError::ReservedSignatureParameter)
    ));
    app.config()
        .unwrap()
        .set("app.url", json!("https://example.test:8443/base"))
        .unwrap();
    let with_defaults = urls
        .as_ref()
        .clone()
        .with_defaults(&[("article:slug", "default-slug")]);
    assert_eq!(
        with_defaults.route("bound", &[], false).unwrap(),
        "/bound/default-slug"
    );
    assert_eq!(
        with_defaults
            .route("bound", &[RouteParameter::positional("override")], true)
            .unwrap(),
        "https://example.test:8443/base/bound/override"
    );
    assert_eq!(
        urls.previous_path(Some("/fallback/?q=1#anchor")).unwrap(),
        "/fallback"
    );
    assert_eq!(
        urls.action(
            "AccountController@show",
            &[RouteParameter::named("tenant", "shop")],
            true
        )
        .unwrap(),
        "https://shop.example.test:8443/account"
    );
    app.config().unwrap().set("app.key", json!("")).unwrap();
    assert!(matches!(
        urls.signed_route("article", &parameters, None, SignatureMode::Absolute),
        Err(UrlError::MissingSigningKey)
    ));
}

#[tokio::test]
async fn signs_and_redirects_over_real_http() {
    let mut app = make();
    app.boot().await.unwrap();
    let config = app.config().unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    let host = address.to_string();
    let response = exchange(address, "/links", &host, 200).await;
    let links: serde_json::Value =
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    let target = |key: &str| {
        links[key]
            .as_str()
            .unwrap()
            .strip_prefix(&format!("http://{address}"))
            .unwrap()
            .to_owned()
    };
    assert_eq!(links["named"], format!("http://{address}/page/default"));
    assert_eq!(links["article"], format!("http://{address}/article/42"));
    let absolute = target("absolute");
    let expired = target("expired");
    let relative = links["relative"].as_str().unwrap();
    exchange(address, &absolute, &host, 200).await;
    exchange(address, &absolute, "other.invalid", 403).await;
    exchange(address, &absolute.replace("/42?", "/43?"), &host, 403).await;
    exchange(address, &expired, &host, 403).await;
    exchange(
        address,
        &format!("{relative}&utm=source"),
        "other.invalid",
        200,
    )
    .await;
    exchange(address, &format!("{relative}&other=source"), &host, 403).await;
    config.set("app.key", json!("new-key")).unwrap();
    config
        .set("app.previous_keys", json!([REFERENCE_KEY]))
        .unwrap();
    exchange(address, &absolute, &host, 200).await;
    config.set("app.previous_keys", json!([])).unwrap();
    exchange(address, &absolute, &host, 403).await;
    let response = exchange(address, "/action-links", &host, 200).await;
    let actions: serde_json::Value =
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(
        actions["absolute"],
        format!("http://{host}/action-articles/a%20b")
    );
    assert_eq!(actions["relative"], "/action-articles/a%20b");
    let response = exchange(address, "/action-redirect", &host, 302).await;
    assert!(response.contains(&format!("location: http://{host}/action-articles/42\r\n")));
    exchange(address, "/action-articles/42", &host, 200).await;
    let response = exchange(address, "/redirect", &host, 302).await;
    assert!(response.contains(&format!("location: http://{address}/page/a%20b\r\n")));
    let response = exchange(address, "/old/a%20b", &host, 301).await;
    assert!(response.contains("location: /new/a%20b\r\n"));
    let observe = |path| {
        let host = &host;
        async move {
            let response = exchange(address, path, host, 200).await;
            serde_json::from_str::<serde_json::Value>(response.split_once("\r\n\r\n").unwrap().1)
                .unwrap()
        }
    };
    let (fr, tw) = tokio::join!(
        observe("/request-links/fr/?z=%2b&a=1"),
        observe("/request-links/tw")
    );
    assert_eq!(
        fr["full"],
        format!("http://{host}/request-links/fr?a=1&z=%2B")
    );
    assert_eq!(fr["current"], format!("http://{host}/request-links/fr"));
    assert_eq!(fr["default"], "/page/localized");
    assert_eq!(fr["previous_path"], "/fallback");
    assert_eq!(fr["positional_override"], "/localized/en/a%25%3F%23");
    assert_eq!(fr["named_required"], "/localized/en/post");
    assert_eq!(fr["action"], "/localized/fr/a%25%3F%23");
    assert_eq!(tw["action"], "/localized/tw/a%25%3F%23");
    assert_eq!(tw["explicit"], "/localized/en/a%25%3F%23");
    let signed = fr["signed"].as_str().unwrap();
    let origin = format!("http://{host}");
    let signed = signed.strip_prefix(&origin).unwrap();
    exchange(address, signed, &host, 200).await;
    exchange(address, &signed.replace("/fr/", "/en/"), &host, 403).await;
    let next = observe("/request-links").await;
    assert_eq!(next["default"], "/page/default");
    assert!(next["action"].is_null());
    for (variant, expected) in [
        ("refresh", format!("{origin}/redirect-variant/refresh")),
        ("secure", format!("https://{host}/article/42")),
        ("away", "https://example.test/next?x=one&y=two".into()),
        ("relative", "relative?x=one".into()),
    ] {
        let response = exchange(
            address,
            &format!("/redirect-variant/{variant}?ignored=1"),
            &host,
            307,
        )
        .await;
        assert!(
            response.contains(&format!("location: {expected}\r\n")),
            "{response}"
        );
        assert!(response.contains("x-redirect-proof: kept\r\n"));
        if variant == "away" {
            assert!(response.contains("x=one&amp;y=two"));
        }
    }
    for variant in ["signed", "temporary"] {
        let response = exchange(address, &format!("/redirect-variant/{variant}"), &host, 307).await;
        let location = response
            .lines()
            .find_map(|line| line.strip_prefix("location: "))
            .unwrap();
        exchange(address, location.strip_prefix(&origin).unwrap(), &host, 200).await;
    }
    exchange(address, "/redirect-variant/invalid", &host, 204).await;
    for (variant, target, escaped) in [
        ("fragment-add", "relative?x=%23#new", "relative?x=%23#new"),
        (
            "fragment-replace",
            "https://example.test/next?x=one&y=%23#new&more#inner",
            "https://example.test/next?x=one&amp;y=%23#new&amp;more#inner",
        ),
        ("fragment-remove", "/next?x=%23", "/next?x=%23"),
        ("fragment-empty", "/next#", "/next#"),
        ("fragment-unchanged", "/next?x=%23", "/next?x=%23"),
        ("fragment-unicode", "/next#章节", "/next#章节"),
    ] {
        let response = exchange(address, &format!("/redirect-variant/{variant}"), &host, 307).await;
        assert!(
            response.contains(&format!("location: {target}\r\n")),
            "{response}"
        );
        assert!(response.contains("x-redirect-proof: kept\r\n"));
        assert!(
            response.contains(&format!("href=\"{escaped}\"")),
            "{response}"
        );
    }
    exchange(address, "/redirect-variant/fragment-invalid", &host, 204).await;
    for (mode, request_host, target, keep) in [
        (
            "strict",
            "site.test",
            "http://SITE.TEST:80/next?x=1&y=2#section",
            true,
        ),
        (
            "strict",
            "site.test:8080",
            "http://site.test:8080/next",
            true,
        ),
        ("strict", "site.test:8080", "http://site.test/next", false),
        ("strict", "site.test", "http://site.test:8080/next", false),
        ("strict", "site.test", "https://site.test/next", false),
        ("strict", "site.test", "/next", false),
        ("skip-scheme", "site.test", "https://site.test/next", true),
        (
            "skip-scheme",
            "site.test",
            "https://site.test:8080/next",
            false,
        ),
        ("skip-port", "site.test", "http://site.test:8080/next", true),
        (
            "skip-port",
            "site.test",
            "https://site.test:8080/next",
            false,
        ),
        (
            "skip-port",
            "site.test",
            "http://malicious.invalid/next",
            false,
        ),
        ("strict", "[::1]:8080", "http://[::1]:8080/next", true),
    ] {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("target", target)
            .finish();
        let path = format!("/redirect-origin/{mode}?{query}");
        let response = exchange(address, &path, request_host, 303).await;
        let expected = if keep {
            target
        } else {
            "/fallback?from=origin&retry=1"
        };
        assert!(
            response.contains(&format!("location: {expected}\r\n")),
            "{path} {response}"
        );
        assert!(response.contains("x-redirect-proof: kept\r\n"));
        let escaped = expected.replace('&', "&amp;");
        assert!(
            response.contains(&format!("href=\"{escaped}\"")),
            "{response}"
        );
    }
    exchange(
        address,
        "/redirect-origin/invalid-fallback?target=https%3A%2F%2Foutside.test%2Fnext",
        "site.test",
        500,
    )
    .await;
    let response = exchange(address, "/optional-old", &host, 302).await;
    assert!(response.contains("location: /new\r\n"));
    let response = exchange(address, "/optional-old/a%3F%23", &host, 302).await;
    assert!(response.contains("location: /new/a%3F%23\r\n"));

    config
        .set("app.url", json!("http://canonical.test"))
        .unwrap();
    config.set("app.url_explicit", json!(true)).unwrap();
    let response = exchange(address, "/links", &host, 200).await;
    let links: serde_json::Value =
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(links["named"], "http://canonical.test/page/default");

    config.set("app.prefer_app_url", json!(false)).unwrap();
    let response = exchange(address, "/links", &host, 200).await;
    let links: serde_json::Value =
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(links["named"], format!("http://{host}/page/default"));

    config.set("app.prefer_app_url", json!(true)).unwrap();
    let response = exchange(address, "/links", &host, 200).await;
    let links: serde_json::Value =
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(links["named"], "http://canonical.test/page/default");
    let signed = links["absolute"]
        .as_str()
        .unwrap()
        .strip_prefix("http://canonical.test")
        .unwrap();
    exchange(address, signed, "canonical.test", 200).await;
    exchange(address, signed, &host, 403).await;
    server.abort();
}

fn local_links(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let disk = openrust::support::facades::Storage::disk(None)?;
        let expiration = SystemTime::now() + Duration::from_secs(60);
        let name = "nested/report + 100%?#.txt";
        assert!(matches!(
            disk.temporary_download_request(name, expiration, |_| panic!(
                "local disk must not invoke SDK configuration"
            ))
            .await,
            Err(openrust::filesystem::FilesystemError::UnsupportedOperation)
        ));
        let download = disk.temporary_url(name, expiration).await?;
        let upload = disk.temporary_upload_url(name, expiration).await?;
        let custom = openrust::support::facades::Storage::disk(Some("custom"))?;
        assert!(!custom.provides_temporary_urls());
        assert!(!custom.provides_temporary_upload_urls());
        custom.build_temporary_urls_using(|path, expiration, options| {
            Box::pin(async move {
                assert_eq!(options.response_content_type.as_deref(), Some("text/plain"));
                openrust::support::facades::Storage::temporary_url(path, expiration).await
            })
        });
        custom.build_temporary_upload_urls_using(|path, expiration, options| {
            Box::pin(async move {
                assert_eq!(options.content_type.as_deref(), Some("text/plain"));
                openrust::support::facades::Storage::temporary_upload_url(path, expiration).await
            })
        });
        assert!(custom.provides_temporary_urls());
        assert!(custom.provides_temporary_upload_urls());
        assert_eq!(
            custom
                .temporary_url_with(
                    name,
                    expiration,
                    openrust::filesystem::TemporaryUrlOptions {
                        response_content_type: Some("text/plain".into()),
                        ..Default::default()
                    }
                )
                .await?,
            download
        );
        assert_eq!(
            custom
                .temporary_upload_url_with(
                    name,
                    expiration,
                    openrust::filesystem::TemporaryUploadUrlOptions {
                        content_type: Some("text/plain".into()),
                        ..Default::default()
                    }
                )
                .await?
                .url,
            upload.url
        );
        let expired = disk
            .temporary_url(name, SystemTime::now() - Duration::from_secs(5))
            .await?;
        Response::json(
            StatusCode::OK,
            json!({"download": download, "upload": upload.url, "expired": expired}),
        )
    })
}

#[tokio::test]
async fn serves_private_local_files_only_with_the_matching_temporary_method() {
    let root =
        std::env::temp_dir().join(format!("openrust-signed-files-{:016x}", fastrand::u64(..)));
    let root_guard = TestRoot(root.clone());
    std::fs::create_dir_all(root.join("public/storage")).unwrap();
    std::fs::write(root.join("public/storage/public.txt"), b"public data").unwrap();
    let app = Application::configure(root)
        .with_config(|_, _| Ok([
            ("app".into(), json!({"url":"https://example.test","key":REFERENCE_KEY})),
            ("filesystems".into(), json!({"default":"local","disks":{"local":{"driver":"local","root":"private","serve":true},"custom":{"driver":"local","root":"custom"}}})),
        ].into()))
        .with_routing(Routing::default().web(|| {
            use openrust::support::facades::Route;
            Route::get("/local-links", local_links)?;
            Route::r#static("/", "public")
        }))
        .create().unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    let host = address.to_string();
    let response = exchange(address, "/local-links", &host, 200).await;
    let links: serde_json::Value =
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    let target = |name: &str| {
        links[name]
            .as_str()
            .unwrap()
            .strip_prefix(&format!("http://{address}"))
            .unwrap()
    };
    let download = target("download");
    let upload = target("upload");
    exchange(address, download, &host, 404).await;
    exchange(address, upload, &host, 404).await;
    exchange_with(address, "PUT", download, &host, "unexpected", "", 404).await;
    exchange_with(address, "PUT", upload, &host, "private data", "", 204).await;
    let response = exchange(address, download, &host, 200).await;
    assert!(response.contains(
        "content-security-policy: default-src 'none'; style-src 'unsafe-inline'; sandbox"
    ));
    assert!(response.ends_with("private data"));
    exchange(address, download.split('?').next().unwrap(), &host, 404).await;
    exchange(address, target("expired"), &host, 404).await;
    let response = exchange(address, "/storage/public.txt", &host, 200).await;
    assert!(response.ends_with("public data"));
    server.abort();
    drop(root_guard);
}

struct TestRoot(std::path::PathBuf);

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn guest(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let response = Redirect::guest(request, "/login", RedirectOptions::default())?;
        if request.uri().query() == Some("replace=1") {
            assert!(Redirect::get_intended_url(request)?.is_some());
            Redirect::set_intended_url(request, "/manually-set")?;
        }
        Ok(response.into())
    })
}

fn intended(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Ok(Redirect::intended(request, "/fallback", RedirectOptions::default())?.into())
    })
}

fn back(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let result = Redirect::back(request, Some("/fallback"), RedirectOptions::default());
    Box::pin(std::future::ready(
        result.map(Into::into).map_err(Into::into),
    ))
}

fn previous_session(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let session = request.session()?;
        let uri = session.previous_uri().unwrap();
        Response::json(
            StatusCode::OK,
            json!({
                "url": uri.as_ref().map(|uri| uri.value()),
                "route": session.previous_route(),
            }),
        )
    })
}

fn flash_redirect(request: &mut Request) -> HttpFuture<'_, Result<RedirectResponse, HttpError>> {
    Box::pin(async {
        let mut errors = openrust::support::MessageBag::default();
        errors.add("name", "Name is required.");
        let redirect = Redirect::to("/flash", RedirectOptions::default())?
            .with(request, "status", json!("saved"))?
            .with_errors(request, &errors, "profile")?
            .with_errors(request, &errors, "default")?;
        let redirect = match request.route("mode") {
            Some("only") => {
                redirect
                    .only_input(request, &["profile.name", "query"])
                    .await?
            }
            Some("except") => redirect.except_input(request, &["profile.secret"]).await?,
            Some("supplied") => {
                redirect
                    .with_input(request, Some(json!({"manual": true})))
                    .await?
            }
            _ => redirect.with_input(request, None).await?,
        };
        Ok(redirect)
    })
}

fn read_flash(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        use openrust::contracts::session::Session as _;
        let session = request.session().unwrap();
        Response::json(
            StatusCode::OK,
            json!({
                "status": session.get("status"),
                "errors": session.get("errors"),
                "input": request.old(None),
            }),
        )
    })
}

fn flash_token(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        use openrust::contracts::session::Session as _;
        Response::json(StatusCode::OK, json!({"token": request.session()?.token()}))
    })
}

#[tokio::test]
async fn redirects_preserve_intended_and_flash_data_in_the_encrypted_http_session() {
    let app = Application::configure(std::env::temp_dir())
        .with_config(|_, _| {
            Ok([
                (
                    "app".into(),
                    json!({"url":"https://example.test","key":"a".repeat(32)}),
                ),
                (
                    "session".into(),
                    json!({"driver":"array","cookie":"probe_session","lottery":[0,100]}),
                ),
            ]
            .into())
        })
        .with_routing(Routing::default().web(|| {
            use openrust::support::facades::Route;
            Route::get_with("/guarded", guest, |route| {
                route.name("guarded");
                Ok(())
            })?;
            Route::get("/previous-session", previous_session)?;
            Route::get("/intended", intended)?;
            Route::get("/back", back)?;
            Route::get("/flash", read_flash)?;
            Route::get("/flash-token", flash_token)?;
            Route::post("/flash/{mode}", flash_redirect)
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    let host = address.to_string();
    let response = exchange(address, "/guarded?tab=details", &host, 302).await;
    assert!(response.contains(&format!("location: http://{address}/login\r\n")));
    let cookie = response
        .lines()
        .find_map(|line| line.strip_prefix("set-cookie: probe_session="))
        .and_then(|value| value.split(';').next())
        .unwrap();
    let headers = format!("Cookie: probe_session={cookie}\r\n");
    let prefetch = format!("{headers}Purpose: prefetch\r\n");
    let response = exchange_with(
        address,
        "GET",
        "/previous-session",
        &host,
        "",
        &prefetch,
        200,
    )
    .await;
    let history: serde_json::Value =
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(history["route"], "guarded");
    assert_eq!(history["url"], format!("http://{host}/guarded?tab=details"));
    let response = exchange_with(address, "GET", "/back", &host, "", &prefetch, 302).await;
    assert!(response.contains(&format!("location: http://{host}/guarded?tab=details\r\n")));
    let same_origin_referer = format!("{prefetch}Referer: http://{host}/specific/?x=1\r\n");
    let response = exchange_with(
        address,
        "GET",
        "/back",
        &host,
        "",
        &same_origin_referer,
        302,
    )
    .await;
    assert!(response.contains(&format!("location: http://{host}/specific/?x=1\r\n")));
    let response =
        exchange_with(address, "GET", "/back", "other.invalid", "", &prefetch, 302).await;
    assert!(response.contains("location: http://other.invalid/fallback\r\n"));
    let response = exchange_with(address, "GET", "/intended", &host, "", &headers, 302).await;
    assert!(response.contains(&format!(
        "location: http://{address}/guarded?tab=details\r\n"
    )));
    let response = exchange_with(address, "GET", "/intended", &host, "", &headers, 302).await;
    assert!(response.contains(&format!("location: http://{address}/fallback\r\n")));
    let response = exchange_with(
        address,
        "GET",
        "/back",
        &host,
        "",
        "Referer: https://foreign.invalid/\r\n",
        302,
    )
    .await;
    assert!(response.contains(&format!("location: http://{address}/fallback\r\n")));
    let response = exchange_with(
        address,
        "GET",
        "/guarded?replace=1",
        &host,
        "",
        &headers,
        302,
    )
    .await;
    assert!(response.contains(&format!("location: http://{host}/login\r\n")));
    let response = exchange_with(address, "GET", "/intended", &host, "", &headers, 302).await;
    assert!(response.contains(&format!("location: http://{host}/manually-set\r\n")));
    let token = exchange_with(address, "GET", "/flash-token", &host, "", &headers, 200).await;
    let token: serde_json::Value =
        serde_json::from_str(token.split_once("\r\n\r\n").unwrap().1).unwrap();
    let fields = format!(
        "Content-Type: application/x-www-form-urlencoded\r\nX-CSRF-TOKEN: {}\r\n",
        token["token"].as_str().unwrap()
    );
    for (mode, input) in [
        ("only", json!({"profile":{"name":"Ada"},"query":"value"})),
        (
            "except",
            json!({"profile":{"name":"Ada"},"query":"value","ignored":"x"}),
        ),
        ("supplied", json!({"manual":true})),
        (
            "all",
            json!({"profile":{"name":"Ada","secret":"drop"},"query":"value","ignored":"x"}),
        ),
    ] {
        let response = exchange_with(
            address,
            "POST",
            &format!("/flash/{mode}?query=value"),
            &host,
            "profile[name]=Ada&profile[secret]=drop&ignored=x",
            &format!("{headers}{fields}"),
            302,
        )
        .await;
        assert!(response.contains(&format!("location: http://{address}/flash\r\n")));
        let empty = json!({"status":null,"errors":null,"input":null});
        let read = |headers: String| {
            let host = &host;
            async move {
                let response =
                    exchange_with(address, "GET", "/flash", host, "", &headers, 200).await;
                serde_json::from_str::<serde_json::Value>(
                    response.split_once("\r\n\r\n").unwrap().1,
                )
                .unwrap()
            }
        };
        assert_eq!(read(String::new()).await, empty);
        let flashed = read(headers.clone()).await;
        assert_eq!(flashed["status"], "saved");
        assert_eq!(flashed["input"], input);
        assert_eq!(
            flashed["errors"],
            json!({
                "default": {"name":["Name is required."]},
                "profile": {"name":["Name is required."]},
            })
        );
        assert_eq!(read(headers.clone()).await, empty);
    }
    let multipart = concat!(
        "--probe\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nAda\r\n",
        "--probe\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"a.txt\"\r\n",
        "Content-Type: text/plain\r\n\r\nfile bytes\r\n--probe--\r\n"
    );
    let upload_headers = format!(
        "{headers}Content-Type: multipart/form-data; boundary=probe\r\nX-CSRF-TOKEN: {}\r\n",
        token["token"].as_str().unwrap(),
    );
    exchange_with(
        address,
        "POST",
        "/flash/all",
        &host,
        multipart,
        &upload_headers,
        302,
    )
    .await;
    let response = exchange_with(address, "GET", "/flash", &host, "", &headers, 200).await;
    let flashed: serde_json::Value =
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(flashed["input"], json!({"name": "Ada"}));
    server.abort();
}

#[tokio::test]
async fn asset_origins_secure_assets_and_defaults_are_instance_scoped() {
    use openrust::contracts::container::Container as _;
    let mut app = make();
    app.boot().await.unwrap();
    app.config()
        .unwrap()
        .set("app.url", json!("http://origin.test/base/index.php"))
        .unwrap();
    app.config()
        .unwrap()
        .set("app.asset_url", json!("https://configured-cdn.test/static"))
        .unwrap();
    let service = app.container().make::<UrlGenerator>().unwrap();
    let mut urls = service.as_ref().clone().with_defaults(&[("locale", "en")]);
    assert_eq!(
        urls.get_default_parameters()
            .get("locale")
            .map(String::as_str),
        Some("en")
    );
    assert!(service.get_default_parameters().is_empty());
    assert_eq!(
        urls.secure_asset("/logo.svg").unwrap(),
        "https://configured-cdn.test/static/logo.svg"
    );
    urls.use_asset_origin(Some("http://cdn.test/assets/"));
    assert_eq!(
        urls.asset("/logo.svg/", Some(true)).unwrap(),
        "http://cdn.test/assets/logo.svg"
    );
    urls.use_asset_origin(None);
    assert_eq!(
        urls.secure_asset("logo.svg").unwrap(),
        "https://origin.test/base/logo.svg"
    );
    urls.use_origin(Some("http://override.test/sub/"));
    assert_eq!(
        urls.asset("logo.svg", None).unwrap(),
        "http://override.test/sub/logo.svg"
    );
    assert_eq!(
        urls.asset_from("https://custom.test/sub/index.php/", "/logo.svg/", None)
            .unwrap(),
        "http://custom.test/sub/logo.svg"
    );
    assert_eq!(
        urls.asset_from("http://custom.test/", "logo.svg", Some(true))
            .unwrap(),
        "https://custom.test/logo.svg"
    );
    urls.force_scheme(Some("https"));
    assert_eq!(
        urls.asset_from("http://custom.test/", "logo.svg", None)
            .unwrap(),
        "https://custom.test/logo.svg"
    );
    assert_eq!(
        urls.secure_asset("https://external.test/logo.svg").unwrap(),
        "https://external.test/logo.svg"
    );
    assert!(
        urls.asset_from("javascript:alert(1)", "logo.svg", None)
            .is_err()
    );
    assert!(
        urls.asset_from("https://custom.test/", "a\nb", None)
            .is_err()
    );
    urls.use_origin(None);
    assert_eq!(
        urls.asset("logo.svg", None).unwrap(),
        "https://origin.test/base/logo.svg"
    );
    assert_eq!(
        service.asset("logo.svg", None).unwrap(),
        "https://configured-cdn.test/static/logo.svg"
    );
}

#[tokio::test]
async fn signature_expiration_uses_owning_application_clock_even_in_another_scope() {
    use openrust::support::facades::Facade as _;
    let mut first = make();
    first.boot().await.unwrap();
    let mut second = make();
    second.boot().await.unwrap();
    let a = first.make::<UrlGenerator>().await.unwrap();
    let b = second.make::<UrlGenerator>().await.unwrap();
    let ca = first
        .make::<openrust::support::DateFactory>()
        .await
        .unwrap();
    let cb = second
        .make::<openrust::support::DateFactory>()
        .await
        .unwrap();
    let at = "2030-01-01T00:00:00Z".parse::<jiff::Timestamp>().unwrap();
    ca.set_test_now(Some(openrust::support::Date::from_utc(
        at,
        jiff::tz::TimeZone::UTC,
    )));
    cb.set_test_now(Some(openrust::support::Date::from_utc(
        at + jiff::SignedDuration::from_secs(61),
        jiff::tz::TimeZone::UTC,
    )));
    let link = a
        .temporary_signed_route(
            "article",
            (at + jiff::SignedDuration::from_secs(60)).into(),
            &[RouteParameter::named("article", "42")],
            SignatureMode::Absolute,
        )
        .unwrap();
    let request = Request::from(
        http::Request::builder()
            .uri(link)
            .body(openrust::http::Body::empty())
            .unwrap(),
    );
    openrust::support::facades::App::set_facade_application(Some(&second), async {
        assert!(
            a.has_valid_signature(&request, SignatureMode::Absolute, (&[] as &[&str]).into())
                .unwrap()
        );
        assert!(
            !b.has_valid_signature(&request, SignatureMode::Absolute, (&[] as &[&str]).into())
                .unwrap()
        );
    })
    .await;
    ca.set_test_now(Some(openrust::support::Date::from_utc(
        at + jiff::SignedDuration::from_secs(61),
        jiff::tz::TimeZone::UTC,
    )));
    assert!(
        !a.has_valid_signature(&request, SignatureMode::Absolute, (&[] as &[&str]).into())
            .unwrap()
    );
}
