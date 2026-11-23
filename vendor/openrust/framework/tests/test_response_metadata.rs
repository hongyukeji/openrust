use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::configuration::Routing,
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode, StreamedJson},
    routing::{RouteGroup, RouteParameter, SignatureMode},
    support::facades::{Route, Url},
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
            "back" => Url::previous(request, None)?,
            "route" => Url::route("target", &params, true)?,
            "action" => Url::action("TargetController@show", &params, true)?,
            "signed" => Url::signed_route("target", &params, None, SignatureMode::Absolute)?,
            "relative" => Url::signed_route("target", &params, None, SignatureMode::Relative)?,
            "tampered" => format!(
                "{}&changed=1",
                Url::signed_route("target", &params, None, SignatureMode::Absolute)?
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
            "json" => Ok(Response::stream_json(
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
                response.set_content("new");
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
    router.get("/target/{id}",target)?.name("target").set_action("TargetController@show")?;
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
            .assert_redirect_contains("/api/target/")
            .assert_redirect_to_route("target", &params)
            .assert_redirect_to_action("TargetController@show", &params)
            .assert_location("http://example.test/api/target/a%20b");
        for uri in ["", "/API/target/", "wrong"] {
            fails(&response, |response| {
                response.assert_redirect_contains(uri);
            });
        }
        fails(&response, |response| {
            response.assert_redirect_to_route("target", &[RouteParameter::named("id", "wrong")]);
        });
    }
    app.get("/api/redirect/signed")
        .await
        .unwrap()
        .assert_redirect_to_signed_route(Some("target"), &params, true);
    app.get("/api/redirect/relative")
        .await
        .unwrap()
        .assert_redirect_to_signed_route(Some("target"), &params, false);
    let response = app.get("/api/redirect/tampered").await.unwrap();
    fails(&response, |response| {
        response.assert_redirect_to_signed_route(None, &[], true);
    });
    let response = app.get("/api/target/1").await.unwrap();
    fails(&response, |response| {
        response.assert_redirect_to_route("target", &params);
    });
}

#[tokio::test]
async fn stream_metadata_distinguishes_buffered_and_streamed_bodies_and_propagates_errors() {
    let mut app = app();
    let response = app.get("/api/stream/plain").await.unwrap();
    response
        .assert_streamed()
        .assert_streamed_content("firstsecond")
        .assert_streamed_content("firstsecond");
    fails(&response, |response| {
        response.assert_not_streamed();
    });
    fails(&response, |response| {
        response.assert_streamed_content("wrong");
    });
    for mode in ["buffered", "replaced"] {
        let response = app.get(&format!("/api/stream/{mode}")).await.unwrap();
        response.assert_not_streamed();
        fails(&response, |response| {
            response.assert_streamed();
        });
        fails(&response, |response| {
            response.streamed_content();
        });
    }
    app.get("/api/stream/json")
        .await
        .unwrap()
        .assert_streamed()
        .assert_streamed_json_content(&json!({"name":"中文/路径","n":1.0}));
    assert!(app.get("/api/stream/error").await.is_err());
    app.get("/api/stream/plain")
        .await
        .unwrap()
        .assert_streamed_content("firstsecond");
}

#[tokio::test]
async fn cookie_expiration_checks_past_future_session_and_missing_cookies() {
    let mut app = app();
    let response = app.get("/api/cookies").await.unwrap();
    response
        .assert_cookie_expired("old")
        .assert_cookie_expired("gone")
        .assert_cookie_not_expired("future")
        .assert_cookie_not_expired("session")
        .assert_cookie_not_expired("short")
        .assert_cookie_missing("absent");
    for name in ["future", "session", "short", "absent"] {
        fails(&response, |response| {
            response.assert_cookie_expired(name);
        });
    }
    for name in ["old", "gone", "absent"] {
        fails(&response, |response| {
            response.assert_cookie_not_expired(name);
        });
    }
    fails(&response, |response| {
        response.assert_cookie_missing("session");
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
        .assert_ok()
        .assert_view_is("card")
        .assert_content("Ada creator composer shared")
        .assert_view_has("user.name", Some(&ViewValue::from("Ada")))
        .assert_view_has("user.nil", None)
        .assert_view_has("created", Some(&"creator".into()))
        .assert_view_has("composed", Some(&"composer".into()))
        .assert_view_has("shared", Some(&"shared".into()))
        .assert_view_has("number", Some(&f64::INFINITY.into()))
        .assert_view_missing("user.other");
    assert_eq!(response.view_data(Some("user"))["name"], "Ada");
    assert_eq!(response.view_data(None)["created"], "creator");
    response
        .assert_view_has_all(&ViewValue::from(json!(["created", "shared"])))
        .assert_view_has_all(&ViewValue::from(json!({"composed":"composer"})));
    fails(&response, |response| {
        response.assert_view_is("other");
    });
    fails(&response, |response| {
        response.assert_view_has("user.name", Some(&"Bob".into()));
    });
    fails(&response, |response| {
        response.assert_view_missing("user.nil");
    });
    let plain = app.get("/api/target/1").await.unwrap();
    fails(&plain, |response| {
        response.assert_view_is("card");
    });
}

fn encoding(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let document: serde_json::Value =
            serde_json::from_slice(&request.body_bytes().await?).unwrap();
        if request.route("mode") == Some("stream") {
            Ok(Response::stream_json(
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
                .assert_ok()
                .assert_content(case[mode].as_str().unwrap());
            if mode == "stream" {
                response.assert_streamed();
                let passed = catch_unwind(AssertUnwindSafe(|| {
                    response.assert_streamed_json_content(&case["document"])
                }))
                .is_ok();
                assert_eq!(
                    passed,
                    case["assert_streamed_json"].as_bool().unwrap(),
                    "{case}"
                );
            } else {
                response.assert_not_streamed();
            }
        }
    }
    app.get("/api/file")
        .await
        .unwrap()
        .assert_ok()
        .assert_not_streamed()
        .assert_streamed_content(include_bytes!("reference/json-schema.json"));
}

#[tokio::test]
async fn cookie_max_age_is_anchored_to_response_time_and_assertions_follow_the_live_test_clock() {
    let mut app = app();
    let instant = |text: &str| {
        openrust::support::Date::from_utc(
            text.parse::<jiff::Timestamp>().unwrap(),
            jiff::tz::TimeZone::UTC,
        )
    };
    app.travel_to(instant("2026-11-22T01:00:00Z"));
    let response = app.get("/api/cookies").await.unwrap();
    response.assert_cookie_not_expired("short");
    app.travel_to(instant("2026-11-22T01:03:00Z"));
    response.assert_cookie_expired("short");
    app.travel_to(instant("2099-12-31T23:59:00Z"));
    response.assert_cookie_not_expired("future");
    app.travel_to(instant("2100-01-01T00:01:00Z"));
    response
        .assert_cookie_expired("future")
        .assert_cookie_not_expired("session");
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
            .assert_redirect_back()
            .assert_redirect_back_without_errors();
        fails(&response, |response| {
            response.assert_redirect_back_with_errors(&json!(["email"]), None, "default");
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
            response.assert_redirect_back();
        });
    }
    app.get("/api/redirect/back")
        .await
        .unwrap()
        .assert_redirect_back()
        .assert_location("http://example.test");
}
