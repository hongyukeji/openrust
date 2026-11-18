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
    support::facades::{Redirect, URL},
};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

const LARAVEL_KEY: &str = "base64:QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=";

fn article(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Response::json(StatusCode::OK, json!({"visited":true})) })
}

fn links(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let parameters = [RouteParameter::named("article", "42")];
        let mode = SignatureMode::Absolute;
        let expires = SystemTime::now() + Duration::from_secs(60);
        let absolute = URL::temporarySignedRoute("article", expires, &parameters, mode)?;
        let relative = URL::signedRoute("relative", &parameters, None, SignatureMode::Relative)?;
        let expired = URL::temporarySignedRoute(
            "article",
            SystemTime::now() - Duration::from_secs(5),
            &parameters,
            mode,
        )?;
        Response::json(
            StatusCode::OK,
            json!({
                "absolute": absolute, "relative": relative, "expired": expired,
                "named": URL::route("ordinary", &[], true)?,
            "article": URL::route("article", &parameters, true)?,
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
            URL::action("OldController@show", &parameters, true),
            Err(UrlError::MissingAction)
        ));
        assert!(matches!(
            URL::action(SHOW_ARTICLE, &[], true),
            Err(UrlError::MissingParameter)
        ));
        Response::json(
            StatusCode::OK,
            json!({
                "absolute": URL::action(SHOW_ARTICLE, &parameters, true)?,
                "relative": URL::action(SHOW_ARTICLE, &parameters, false)?,
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
            URL::defaults(request, &[("locale", "replaced")]);
            URL::defaults(request, &[("locale", &locale), ("slug", "localized")]);
        }
        tokio::task::yield_now().await;
        let params = [RouteParameter::positional("a%?#")];
        let mode = SignatureMode::Absolute;
        Response::json(
            StatusCode::OK,
            json!({
                "full": URL::full()?,
                "current": URL::current()?,
                "previous_path": URL::previousPath(request, Some("/fallback/?x=1"))?,
                "default": URL::route("ordinary", &[], false)?,
                "positional_override": URL::route("localized", &[RouteParameter::positional("en"), params[0].clone()], false)?,
                "named_required": URL::route("localized", &[RouteParameter::named("article", "post"), RouteParameter::positional("en")], false)?,

                "action": URL::action("LocalizedController@show", &params, false).ok(),
                "explicit": URL::route("localized", &[RouteParameter::named("locale", "en"), params[0].clone()], false)?,
                "signed": URL::signedRoute("localized", &params, None, mode).ok(),
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
            "fragment-add" => Redirect::away("relative?x=%23", options)?.withFragment("new")?,
            "fragment-replace" => {
                Redirect::away("https://example.test/next?x=one&y=%23#old", options)?
                    .withFragment("ignored#new&more#inner")?
            }
            "fragment-remove" => Redirect::away("/next?x=%23#old", options)?.withoutFragment()?,
            "fragment-empty" => Redirect::away("/next#old", options)?.withFragment("")?,
            "fragment-unchanged" => Redirect::away("/next?x=%23", options)?.withoutFragment()?,
            "fragment-unicode" => Redirect::away("/next", options)?.withFragment("#章节")?,
            "fragment-invalid" => {
                assert!(matches!(
                    Redirect::away("/next", options)?.withFragment("bad\r\nX: value"),
                    Err(UrlError::InvalidRedirect)
                ));
                assert!(matches!(
                    Redirect::away("#old", RedirectOptions::default())?.withoutFragment(),
                    Err(UrlError::InvalidRedirect)
                ));
                return Ok(Response::noContent());
            }
            "temporary" => Redirect::temporarySignedRoute(
                "article",
                SystemTime::now() + Duration::from_secs(60),
                &[RouteParameter::named("article", "42")],
                options,
            )?,
            "signed" => Redirect::signedRoute(
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
        .setAction("OldController@show")?;
    router
        .get("/action-articles/{article}", article)?
        .setAction(SHOW_ARTICLE)?;
    router
        .get("/action-alias/{article}", article)?
        .setAction(SHOW_ARTICLE)?;
    router
        .get("/account", article)?
        .setAction("AccountController@show")?;
    router.group(
        RouteGroup::new().domain("{tenant}.example.test"),
        |router| {
            router
                .get("/account", article)?
                .setAction("AccountController@show")?;
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
        .setAction("LocalizedController@show")?
        .middleware_name("signed");
    router.get("/bound/{article:slug}", article)?.name("bound");
    router.get("/request-links/{locale?}", request_links)?;
    router.get("/redirect-variant/{variant}", redirect_variant)?;
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
    router.permanentRedirect("/old/{id}", "/new/{id}")?;
    Ok(())
}

fn make() -> Application {
    Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            Ok([(
                "app".into(),
                json!({"url":"https://example.test","key":LARAVEL_KEY}),
            )]
            .into())
        })
        .withRouting(Routing::new(|_| {
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
    kernel.handleTransport(request).await
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
        urls.signedRoute("article", &parameters, None, SignatureMode::Absolute)
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
        urls.signedRoute(
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
        .withDefaults(&[("article:slug", "default-slug")]);
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
        urls.previousPath(Some("/fallback/?q=1#anchor")).unwrap(),
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
        urls.signedRoute("article", &parameters, None, SignatureMode::Absolute),
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
        .set("app.previous_keys", json!([LARAVEL_KEY]))
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
        format!("http://{host}/request-links/fr/?z=%2b&a=1")
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
            disk.temporaryDownloadRequest(name, expiration, |_| panic!(
                "local disk must not invoke SDK configuration"
            ))
            .await,
            Err(openrust::filesystem::FilesystemError::UnsupportedOperation)
        ));
        let download = disk.temporaryUrl(name, expiration).await?;
        let upload = disk.temporaryUploadUrl(name, expiration).await?;
        let custom = openrust::support::facades::Storage::disk(Some("custom"))?;
        assert!(!custom.providesTemporaryUrls());
        assert!(!custom.providesTemporaryUploadUrls());
        custom.buildTemporaryUrlsUsing(|path, expiration, options| {
            Box::pin(async move {
                assert_eq!(options.response_content_type.as_deref(), Some("text/plain"));
                openrust::support::facades::Storage::temporaryUrl(path, expiration).await
            })
        });
        custom.buildTemporaryUploadUrlsUsing(|path, expiration, options| {
            Box::pin(async move {
                assert_eq!(options.content_type.as_deref(), Some("text/plain"));
                openrust::support::facades::Storage::temporaryUploadUrl(path, expiration).await
            })
        });
        assert!(custom.providesTemporaryUrls());
        assert!(custom.providesTemporaryUploadUrls());
        assert_eq!(
            custom
                .temporaryUrlWith(
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
                .temporaryUploadUrlWith(
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
            .temporaryUrl(name, SystemTime::now() - Duration::from_secs(5))
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
        .withConfig(|_, _| Ok([
            ("app".into(), json!({"url":"https://example.test","key":LARAVEL_KEY})),
            ("filesystems".into(), json!({"default":"local","disks":{"local":{"driver":"local","root":"private","serve":true},"custom":{"driver":"local","root":"custom"}}})),
        ].into()))
        .withRouting(Routing::default().web(|| {
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
            assert!(Redirect::getIntendedUrl(request)?.is_some());
            Redirect::setIntendedUrl(request, "/manually-set")?;
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
        let uri = session.previousUri().unwrap();
        Response::json(
            StatusCode::OK,
            json!({
                "url": uri.as_ref().map(|uri| uri.value()),
                "route": session.previousRoute(),
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
            .withErrors(request, &errors, "profile")?
            .withErrors(request, &errors, "default")?;
        let redirect = match request.route("mode") {
            Some("only") => {
                redirect
                    .onlyInput(request, &["profile.name", "query"])
                    .await?
            }
            Some("except") => redirect.exceptInput(request, &["profile.secret"]).await?,
            Some("supplied") => {
                redirect
                    .withInput(request, Some(json!({"manual": true})))
                    .await?
            }
            _ => redirect.withInput(request, None).await?,
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
        .withConfig(|_, _| {
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
        .withRouting(Routing::default().web(|| {
            use openrust::support::facades::Route;
            Route::getWith("/guarded", guest, |route| {
                route.name("guarded");
                Ok(())
            })?;
            Route::get("/previous-session", previous_session)?;
            Route::get("/intended", intended)?;
            Route::get("/back", back)?;
            Route::get("/flash", read_flash)?;
            Route::get("/flash-token", flash_token)?;
            Route::r#match(
                &[openrust::http::Method::POST],
                "/flash/{mode}",
                flash_redirect,
            )
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
