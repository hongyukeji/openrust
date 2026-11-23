#![allow(non_snake_case, reason = "保留既定公开接口及命名")]
use openrust::{
    contracts::session::Session as _,
    cookie::CookieOptions,
    foundation::configuration::Routing,
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Cookie, Route},
    testing::TestApp,
};
use serde_json::{Value, json};

fn inspect(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = json!({"ajax":request.ajax(),"pjax":request.pjax(),"prefetch":request.prefetch(),"agent":request.user_agent(),"fingerprint":request.fingerprint()?,"json":request.expects_json()});
        request.session_mut()?.put("cart.total", json!(7))?;
        Response::json(StatusCode::OK, data)?.cookie(
            &Cookie::make("token", "中文 + value", 0, &CookieOptions::default())?,
            request,
        )
    })
}
#[tokio::test]
async fn request_fingerprint_preserves_registration_order_root_uri_and_automatic_head() {
    let mut app=TestApp::create(|builder|builder.with_config(|_,_|Ok([
  ("app".into(),json!({"env":"testing","key":"12345678901234567890123456789012","cipher":"AES-256-CBC"})),
  ("session".into(),json!({"driver":"array","cookie":"alignment-session","lifetime":120}))
 ].into())).with_routing(Routing::default().web(|| {
  Route::r#match(&[http::Method::POST,http::Method::GET],"/items/{id}",inspect)?;
  Route::get("/",inspect)
 }))).unwrap();
    let response = app
        .call(
            http::Request::builder()
                .uri("http://example.test/items/7")
                .header("x-requested-with", "XMLHttpRequest")
                .header("x-pjax", "1")
                .header("sec-purpose", "PREFETCH")
                .header("user-agent", "Alignment/1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    response
        .assert_ok()
        .assert_json_path("ajax", &json!(true))
        .assert_json_path("pjax", &json!(true))
        .assert_json_path("prefetch", &json!(true))
        .assert_json_path("agent", &json!("Alignment/1"))
        .assert_session_has("cart.total", Some(&json!(7)))
        .assert_cookie("token", Some("中文 + value"));
    let data: Value = response.json().unwrap();
    let expected = aws_lc_rs::digest::digest(
        &aws_lc_rs::digest::SHA1_FOR_LEGACY_USE_ONLY,
        b"POST|GET|HEAD||items/{id}|",
    );
    let expected: String = expected
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(data["fingerprint"], expected);
    let response = app.get("http://example.test/").await.unwrap();
    response
        .assert_json_path("ajax", &json!(false))
        .assert_json_path("pjax", &json!(false))
        .assert_json_path("prefetch", &json!(false));
    let expected = aws_lc_rs::digest::digest(
        &aws_lc_rs::digest::SHA1_FOR_LEGACY_USE_ONLY,
        b"GET|HEAD||/|",
    );
    let expected: String = expected
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    response.assert_json_path("fingerprint", &json!(expected));
    let request = Request::from_parts(http::Request::new(()).into_parts().0, Body::empty());
    assert!(request.fingerprint().is_err());
    for failure in [
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            response.assert_cookie("token", Some("wrong"));
        })),
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            response.assert_session_has("missing", None);
        })),
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            response.assert_json_structure(&json!(["missing"]));
        })),
    ] {
        assert!(failure.is_err());
    }
}

fn seeded(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        request
            .session_mut()?
            .set_previous_url("http://seed.test/old")?;
        request.session_mut()?.set_previous_route(Some("old"))?;
        Response::json(
            StatusCode::OK,
            json!({"prefetch":request.prefetch(),"json":request.expects_json(),"original":request.headers().get("x-original-accept").and_then(|v|v.to_str().ok())}),
        )
    })
}
#[tokio::test]
async fn prefetch_filters_previous_url_and_route_using_exact_case_insensitive_headers() {
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|_, _| {
                Ok([
                    (
                        "app".into(),
                        json!({"env":"testing","key":"12345678901234567890123456789012"}),
                    ),
                    (
                        "session".into(),
                        json!({"driver":"array","cookie":"alignment-session"}),
                    ),
                ]
                .into())
            })
            .with_routing(Routing::default().web(|| {
                Route::group(openrust::routing::RouteGroup::new(), |router| {
                    router
                        .r#match(&[http::Method::GET], "/check", seeded)?
                        .name("check");
                    Ok(())
                })
            }))
    })
    .unwrap();
    for (header, value, prefetch) in [
        ("x-moz", "prefetch", true),
        ("purpose", "PREFETCH", true),
        ("sec-purpose", "not-prefetch", false),
        ("x-purpose", "prefetch", false),
    ] {
        let response = app
            .call(
                http::Request::get("http://example.test/check")
                    .header(header, value)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        response
            .assert_ok()
            .assert_json_path("prefetch", &json!(prefetch))
            .assert_session_has(
                "_previous.url",
                Some(&json!(if prefetch {
                    "http://seed.test/old"
                } else {
                    "http://example.test/check"
                })),
            )
            .assert_session_has(
                "_previous.route",
                Some(&json!(if prefetch { "old" } else { "check" })),
            );
    }
}
fn assertions(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        match request.route("kind") {
            Some("redirect") => Ok(Response::new(StatusCode::FOUND, Body::empty()).header(
                http::header::LOCATION,
                http::HeaderValue::from_static("/destination"),
            )),
            Some("download") => Ok(Response::from("file").header(
                http::header::CONTENT_DISPOSITION,
                http::HeaderValue::from_static("attachment; filename=\"report.txt\""),
            )),
            _ => Response::json(
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({"errors":{"email":["invalid"]},"rows":[{"id":1},{"id":2}]}),
            ),
        }
    })
}
#[tokio::test]
async fn response_assertions_check_real_redirect_download_and_validation_responses() {
    let mut app = TestApp::create(|builder| {
        builder
            .with_routing(Routing::default().api(|| Route::get("/{kind}", assertions).map(|_| ())))
    })
    .unwrap();
    let redirect = app.get("/api/redirect").await.unwrap();
    redirect.assert_redirect(Some("/destination"));
    let download = app.get("/api/download").await.unwrap();
    download.assert_download(Some("report.txt"));
    let errors = app.get("/api/errors").await.unwrap();
    errors
        .assert_json_validation_errors(&["email"])
        .assert_json_structure(&json!({"rows":{"*":["id"]}}))
        .assert_json_path("rows.1.id", &json!(2));
    for result in [
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            redirect.assert_redirect(Some("/wrong"));
        })),
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            download.assert_download(Some("wrong.txt"));
        })),
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            errors.assert_json_validation_errors(&["missing"]);
        })),
    ] {
        assert!(result.is_err());
    }
}
#[tokio::test]
async fn broad_accept_prefers_json_and_preserves_explicit_media_types() {
    let mut app = TestApp::create(|builder| {
        builder
            .prefers_json_responses(true)
            .with_config(|_, _| {
                Ok([
                    (
                        "app".into(),
                        json!({"env":"testing","key":"12345678901234567890123456789012"}),
                    ),
                    (
                        "session".into(),
                        json!({"driver":"array","cookie":"alignment-session"}),
                    ),
                ]
                .into())
            })
            .with_routing(Routing::default().web(|| Route::get("/check", seeded).map(|_| ())))
    })
    .unwrap();
    for (accept, expected) in [
        ("", true),
        ("*/*", true),
        ("application/*;q=0.5, */*", true),
        ("text/html", false),
        ("application/json", true),
        ("text/html, */*", false),
    ] {
        app.call(
            http::Request::get("http://example.test/check")
                .header("accept", accept)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .assert_ok()
        .assert_json_path("json", &json!(expected));
    }
}

fn excluded_upload(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use openrust::contracts::validation::Validator as _;
    Box::pin(async move {
        let data = openrust::validation::Data::from_request(request).await?;
        assert!(data.file("attachment").is_some());
        let rules = openrust::validation::RuleSet::parse([
            ("mode", "required"),
            ("attachment", "exclude_if:mode,skip|file"),
        ])?;
        let mut validator = openrust::support::facades::Validator::make(data, rules)?;
        for _ in 0..2 {
            let validated = validator.validate().await?;
            assert!(validated.file("attachment").is_none());
            assert!(validated.get("attachment").is_none());
        }
        Response::json(StatusCode::OK, validator.safe()?.into_value())
    })
}
#[tokio::test]
async fn excluded_multipart_file_is_removed_from_both_validated_data_and_file_projection() {
    let mut app = TestApp::create(|builder| {
        builder.with_routing(
            Routing::default()
                .api(|| Route::r#match(&[http::Method::POST], "/upload", excluded_upload)),
        )
    })
    .unwrap();
    let body = "--upload\r\nContent-Disposition: form-data; name=\"mode\"\r\n\r\nskip\r\n--upload\r\nContent-Disposition: form-data; name=\"attachment\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nfile bytes\r\n--upload--\r\n";
    app.call(
        http::Request::post("http://example.test/api/upload")
            .header("accept", "application/json")
            .header("content-type", "multipart/form-data; boundary=upload")
            .body(Body::from(body))
            .unwrap(),
    )
    .await
    .unwrap()
    .assert_ok()
    .assert_exact_json(&json!({"mode":"skip"}));
}

#[tokio::test]
async fn explicit_exception_policy_overrides_json_preference_after_middleware_replacement() {
    let mut app = TestApp::create(|builder| {
        builder
            .prefers_json_responses(true)
            .with_middleware(|middleware| {
                *middleware = Default::default();
            })
            .with_exceptions(|exceptions| {
                exceptions.shouldRenderJsonWhen(|_, _| false);
            })
            .with_routing(Routing::default().api(|| Route::get("/fail", reject_request)))
    })
    .unwrap();
    let response = app.get("http://example.test/api/fail").await.unwrap();
    response.assert_status(StatusCode::FORBIDDEN);
    assert!(response.json::<serde_json::Value>().is_err());
}

fn reject_request(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Err(HttpError::AuthorizationDenied("拒绝".into())) })
}

fn scoped_view() -> openrust::view::View {
    openrust::support::facades::View::share("shared", "request").unwrap();
    openrust::view::view("row")
}
fn default_view() -> openrust::view::View {
    openrust::view::view("row")
}
#[tokio::test]
async fn shared_view_data_reaches_blocking_renderer_and_does_not_leak_to_next_request() {
    use openrust::contracts::{container::Container as _, foundation::Application as _};
    let mut app = TestApp::create(|builder| {
        builder.with_routing(Routing::default().api(|| {
            Route::get("/one", scoped_view)?;
            Route::get("/default", default_view)
        }))
    })
    .unwrap();
    app.boot().await.unwrap();
    let root = app.app().base_path();
    std::fs::create_dir_all(root.join("resources/views")).unwrap();
    std::fs::write(root.join("resources/views/row.blade.html"), "{{ $shared }}").unwrap();
    app.app()
        .container()
        .make::<openrust::view::Factory>()
        .unwrap()
        .share("shared", "boot");
    app.get("http://example.test/api/one")
        .await
        .unwrap()
        .assert_ok()
        .assert_content("request");
    app.get("http://example.test/api/default")
        .await
        .unwrap()
        .assert_ok()
        .assert_content("boot");
}
