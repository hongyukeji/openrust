use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::configuration::Routing,
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode, StreamedJson},
    routing::{RouteGroup, RouteParameter, SignatureMode},
    support::facades::{Route, URL},
    testing::{TestApp, TestResponse},
    view::ViewValue,
};
use serde_json::json;
use std::{
    convert::Infallible,
    panic::{AssertUnwindSafe, catch_unwind},
};

fn target(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::from("ok")) })
}
fn redirect(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mode = request.route("mode").unwrap();
        let params = [RouteParameter::named("id", "a b")];
        let location = match mode {
            "back" => URL::previous(request, None)?,
            "route" => URL::route("target", &params, true)?,
            "action" => URL::action("TargetController@show", &params, true)?,
            "signed" => URL::signedRoute("target", &params, None, SignatureMode::Absolute)?,
            "relative" => URL::signedRoute("target", &params, None, SignatureMode::Relative)?,
            "tampered" => format!(
                "{}&changed=1",
                URL::signedRoute("target", &params, None, SignatureMode::Absolute)?
            ),
            _ => "/api/target/other".into(),
        };
        Ok(Response::new(StatusCode::FOUND, Body::empty())
            .header(http::header::LOCATION, location.parse().unwrap()))
    })
}
fn stream(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        match request.route("mode").unwrap() {
            "plain" => Ok(Response::stream(
                StatusCode::OK,
                futures_util::stream::iter([Ok::<_, Infallible>("first"), Ok("second")]),
            )),
            "json" => Ok(Response::streamJson(
                StatusCode::OK,
                StreamedJson::value(json!({"name":"中文/路径","n":1.0}))?,
            )),
            "error" => Ok(Response::stream(
                StatusCode::OK,
                futures_util::stream::iter([Ok("first"), Err(std::io::Error::other("流读取失败"))]),
            )),
            "replaced" => {
                let mut response = Response::stream(
                    StatusCode::OK,
                    futures_util::stream::iter([Ok::<_, Infallible>("old")]),
                );
                response.setContent("new");
                Ok(response)
            }
            _ => Ok(Response::from("firstsecond")),
        }
    })
}
fn cookie_response(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let mut response = Response::from("cookies");
        for value in [
            "old=x; expires=Sat, 01 Jan 2000 00:00:00 GMT; Path=/",
            "future=x; expires=Fri, 01 Jan 2100 00:00:00 GMT; Path=/",
            "session=x; Path=/",
            "short=x; Max-Age=120; Path=/",
            "gone=x; Max-Age=-10; Path=/",
        ] {
            response
                .parts_mut()
                .headers
                .append(http::header::SET_COOKIE, value.parse().unwrap());
        }
        Ok(response)
    })
}
fn page() -> openrust::view::View {
    openrust::view::view("card")
        .with("user", json!({"name":"Ada","nil":null}))
        .with("number", f64::INFINITY)
}
fn app() -> TestApp {
    TestApp::create(|builder|builder.with_config(|_,_|Ok([
    ("app".into(),json!({"env":"testing","url":"http://example.test","key":"12345678901234567890123456789012"}))
].into())).with_routing(Routing::default().api(||Route::group(RouteGroup::new(),|router| {
    router.post("/encoding/{mode}",encoding)?;
    router.get("/file",file_response)?;
    router.get("/target/{id}",target)?.name("target").setAction("TargetController@show")?;
    router.get("/redirect/{mode}",redirect)?;
    router.get("/stream/{mode}",stream)?;
    router.get("/cookies",cookie_response)?;
    router.get("/view",page)?;
    Ok(())
})))).unwrap()
}
fn fails(response: &TestResponse, assertion: impl FnOnce(&TestResponse)) {
    assert!(catch_unwind(AssertUnwindSafe(|| assertion(response))).is_err());
}

#[tokio::test]
async fn redirect_assertions_use_actual_route_action_parameters_and_signatures() {
    let mut app = app();
    let params = [RouteParameter::named("id", "a b")];
    for mode in ["route", "action"] {
        let response = app.get(&format!("/api/redirect/{mode}")).await.unwrap();
        response
            .assertRedirectContains("/api/target/")
            .assertRedirectToRoute("target", &params)
            .assertRedirectToAction("TargetController@show", &params)
            .assertLocation("http://example.test/api/target/a%20b");
        for uri in ["", "/API/target/", "wrong"] {
            fails(&response, |response| {
                response.assertRedirectContains(uri);
            });
        }
        fails(&response, |response| {
            response.assertRedirectToRoute("target", &[RouteParameter::named("id", "wrong")]);
        });
    }
    app.get("/api/redirect/signed")
        .await
        .unwrap()
        .assertRedirectToSignedRoute(Some("target"), &params, true);
    app.get("/api/redirect/relative")
        .await
        .unwrap()
        .assertRedirectToSignedRoute(Some("target"), &params, false);
    let response = app.get("/api/redirect/tampered").await.unwrap();
    fails(&response, |response| {
        response.assertRedirectToSignedRoute(None, &[], true);
    });
    let response = app.get("/api/target/1").await.unwrap();
    fails(&response, |response| {
        response.assertRedirectToRoute("target", &params);
    });
}

#[tokio::test]
async fn stream_metadata_distinguishes_buffered_and_streamed_bodies_and_propagates_errors() {
    let mut app = app();
    let response = app.get("/api/stream/plain").await.unwrap();
    response
        .assertStreamed()
        .assertStreamedContent("firstsecond")
        .assertStreamedContent("firstsecond");
    fails(&response, |response| {
        response.assertNotStreamed();
    });
    fails(&response, |response| {
        response.assertStreamedContent("wrong");
    });
    for mode in ["buffered", "replaced"] {
        let response = app.get(&format!("/api/stream/{mode}")).await.unwrap();
        response.assertNotStreamed();
        fails(&response, |response| {
            response.assertStreamed();
        });
        fails(&response, |response| {
            response.streamedContent();
        });
    }
    app.get("/api/stream/json")
        .await
        .unwrap()
        .assertStreamed()
        .assertStreamedJsonContent(&json!({"name":"中文/路径","n":1.0}));
    assert!(app.get("/api/stream/error").await.is_err());
    app.get("/api/stream/plain")
        .await
        .unwrap()
        .assertStreamedContent("firstsecond");
}

#[tokio::test]
async fn cookie_expiration_checks_past_future_session_and_missing_cookies() {
    let mut app = app();
    let response = app.get("/api/cookies").await.unwrap();
    response
        .assertCookieExpired("old")
        .assertCookieExpired("gone")
        .assertCookieNotExpired("future")
        .assertCookieNotExpired("session")
        .assertCookieNotExpired("short")
        .assertCookieMissing("absent");
    for name in ["future", "session", "short", "absent"] {
        fails(&response, |response| {
            response.assertCookieExpired(name);
        });
    }
    for name in ["old", "gone", "absent"] {
        fails(&response, |response| {
            response.assertCookieNotExpired(name);
        });
    }
    fails(&response, |response| {
        response.assertCookieMissing("session");
    });
}

#[tokio::test]
async fn view_assertions_retain_actual_creator_composer_shared_and_native_values() {
    let mut app = app();
    app.boot().await.unwrap();
    let root = app.app().base_path();
    std::fs::create_dir_all(root.join("resources/views")).unwrap();
    std::fs::write(
        root.join("resources/views/card.blade.html"),
        "{{ $user['name'] }} {{ $created }} {{ $composed }} {{ $shared }}",
    )
    .unwrap();
    let factory = app
        .app()
        .container()
        .make::<openrust::view::Factory>()
        .unwrap();
    factory.share("shared", "shared");
    factory.creator("card", |view| {
        view.data_mut().insert("created".into(), "creator".into());
        Ok(())
    });
    factory.composer("card", |view| {
        view.data_mut().insert("composed".into(), "composer".into());
        Ok(())
    });
    let response = app.get("/api/view").await.unwrap();
    response
        .assertOk()
        .assertViewIs("card")
        .assertContent("Ada creator composer shared")
        .assertViewHas("user.name", Some(&ViewValue::from("Ada")))
        .assertViewHas("user.nil", None)
        .assertViewHas("created", Some(&"creator".into()))
        .assertViewHas("composed", Some(&"composer".into()))
        .assertViewHas("shared", Some(&"shared".into()))
        .assertViewHas("number", Some(&f64::INFINITY.into()))
        .assertViewMissing("user.other");
    assert_eq!(response.viewData(Some("user"))["name"], "Ada");
    assert_eq!(response.viewData(None)["created"], "creator");
    response
        .assertViewHasAll(&ViewValue::from(json!(["created", "shared"])))
        .assertViewHasAll(&ViewValue::from(json!({"composed":"composer"})));
    fails(&response, |response| {
        response.assertViewIs("other");
    });
    fails(&response, |response| {
        response.assertViewHas("user.name", Some(&"Bob".into()));
    });
    fails(&response, |response| {
        response.assertViewMissing("user.nil");
    });
    let plain = app.get("/api/target/1").await.unwrap();
    fails(&plain, |response| {
        response.assertViewIs("card");
    });
}

fn encoding(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let document: serde_json::Value =
            serde_json::from_slice(&request.body_bytes().await?).unwrap();
        if request.route("mode") == Some("stream") {
            Ok(Response::streamJson(
                StatusCode::OK,
                StreamedJson::value(document)?,
            ))
        } else {
            Response::json(StatusCode::OK, document)
        }
    })
}
fn file_response(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Response::file(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/reference/json-schema.json"),
            http::HeaderMap::new(),
        )
        .await
    })
}
#[tokio::test]
async fn json_response_encoding_matches_actual_fixed_json_and_streamed_json_responses() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("reference/test-response.json")).unwrap();
    let mut app = app();
    for case in fixture["encoding_cases"].as_array().unwrap() {
        for mode in ["default", "stream"] {
            let response = app
                .call(
                    http::Request::post(format!("/api/encoding/{mode}"))
                        .header("content-type", "application/json")
                        .body(Body::from(case["document"].to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            response
                .assertOk()
                .assertContent(case[mode].as_str().unwrap());
            if mode == "stream" {
                response.assertStreamed();
                let passed = catch_unwind(AssertUnwindSafe(|| {
                    response.assertStreamedJsonContent(&case["document"])
                }))
                .is_ok();
                assert_eq!(
                    passed,
                    case["assert_streamed_json"].as_bool().unwrap(),
                    "{case}"
                );
            } else {
                response.assertNotStreamed();
            }
        }
    }
    app.get("/api/file")
        .await
        .unwrap()
        .assertOk()
        .assertNotStreamed()
        .assertStreamedContent(include_bytes!("reference/json-schema.json"));
}

#[tokio::test]
async fn cookie_max_age_is_anchored_to_response_time_and_assertions_follow_the_live_test_clock() {
    let mut app = app();
    let instant = |text: &str| {
        openrust::support::Date::fromUtc(
            text.parse::<jiff::Timestamp>().unwrap(),
            jiff::tz::TimeZone::UTC,
        )
    };
    app.travelTo(instant("2026-11-22T01:00:00Z"));
    let response = app.get("/api/cookies").await.unwrap();
    response.assertCookieNotExpired("short");
    app.travelTo(instant("2026-11-22T01:03:00Z"));
    response.assertCookieExpired("short");
    app.travelTo(instant("2099-12-31T23:59:00Z"));
    response.assertCookieNotExpired("future");
    app.travelTo(instant("2100-01-01T00:01:00Z"));
    response
        .assertCookieExpired("future")
        .assertCookieNotExpired("session");
}

#[tokio::test]
async fn redirect_back_uses_request_referer_and_rejects_wrong_destinations() {
    let mut app = app();
    for previous in ["http://example.test/from?x=1", "/relative"] {
        let response = app
            .call(
                http::Request::get("/api/redirect/back")
                    .header("referer", previous)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        response
            .assertRedirectBack()
            .assertRedirectBackWithoutErrors();
        fails(&response, |response| {
            response.assertRedirectBackWithErrors(&json!(["email"]), None, "default");
        });
        let wrong = app
            .call(
                http::Request::get("/api/redirect/other")
                    .header("referer", previous)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        fails(&wrong, |response| {
            response.assertRedirectBack();
        });
    }
    app.get("/api/redirect/back")
        .await
        .unwrap()
        .assertRedirectBack()
        .assertLocation("http://example.test");
}
