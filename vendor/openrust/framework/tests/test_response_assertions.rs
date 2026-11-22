use openrust::{
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Route,
    testing::{TestApp, TestResponse},
};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn response(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let status: u16 = request.route("status").unwrap().parse().unwrap();
        let body = request.route("body").unwrap_or("");
        let content = if matches!(body, "no-header" | "wrong-header") {
            ""
        } else {
            body
        };
        let mut response = Response::new(
            StatusCode::from_u16(status).unwrap(),
            axum::body::Body::from(content.to_owned()),
        )
        .header("x-probe".parse().unwrap(), "present".parse().unwrap());
        if body != "no-header" {
            response = response.header(
                "precognition-success".parse().unwrap(),
                if body == "wrong-header" {
                    "false"
                } else {
                    "true"
                }
                .parse()
                .unwrap(),
            );
        }
        Ok(response)
    })
}

fn app() -> TestApp {
    TestApp::create(|builder| {
        builder.withConfig(|_,_|Ok([
            ("app".into(),serde_json::json!({"env":"testing","debug":true,"key":"12345678901234567890123456789012","cipher":"AES-256-CBC"})),
            ("session".into(),serde_json::json!({"driver":"array","cookie":"assertion-session","lifetime":120}))
        ].into())).withMiddleware(|middleware| { middleware.preventRequestForgery(["fixture"], false, false); }).withRouting(Routing::default().web(|| {
            Route::post("/fixture",
                fixture_response,
            )?;
            Route::get("/flash", flash_session)?;
            Route::get("/redirect-errors/{bag}", redirect_errors)?;
            Route::get("/old-input", read_old_input)?;
            Route::get("/{status}", response)?;
            Route::get("/{status}/{body}", response)
        }))
    })
    .unwrap()
}

fn fails(response: &TestResponse, assertion: impl FnOnce(&TestResponse)) {
    assert!(
        catch_unwind(AssertUnwindSafe(|| assertion(response))).is_err(),
        "错误响应必须使断言失败"
    );
}

#[tokio::test]
async fn named_status_assertions_accept_only_their_exact_status() {
    type Assertion = fn(&TestResponse) -> &TestResponse;
    let assertions: &[(u16, Assertion)] = &[
        (200, TestResponse::assertOk),
        (201, TestResponse::assertCreated),
        (202, TestResponse::assertAccepted),
        (301, TestResponse::assertMovedPermanently),
        (302, TestResponse::assertFound),
        (304, TestResponse::assertNotModified),
        (307, TestResponse::assertTemporaryRedirect),
        (308, TestResponse::assertPermanentRedirect),
        (400, TestResponse::assertBadRequest),
        (401, TestResponse::assertUnauthorized),
        (402, TestResponse::assertPaymentRequired),
        (403, TestResponse::assertForbidden),
        (404, TestResponse::assertNotFound),
        (405, TestResponse::assertMethodNotAllowed),
        (406, TestResponse::assertNotAcceptable),
        (408, TestResponse::assertRequestTimeout),
        (409, TestResponse::assertConflict),
        (410, TestResponse::assertGone),
        (415, TestResponse::assertUnsupportedMediaType),
        (422, TestResponse::assertUnprocessable),
        (424, TestResponse::assertFailedDependency),
        (429, TestResponse::assertTooManyRequests),
        (500, TestResponse::assertInternalServerError),
        (503, TestResponse::assertServiceUnavailable),
    ];
    let mut app = app();
    for (status, assertion) in assertions {
        let response = app.get(&format!("/{status}")).await.unwrap();
        assert_eq!(response.status().as_u16(), *status, "{response:?}");
        assert!(std::ptr::eq(assertion(&response), &response));
        let wrong = app
            .get(if *status == 200 { "/201" } else { "/200" })
            .await
            .unwrap();
        fails(&wrong, |response| {
            assertion(response);
        });
    }
}

#[tokio::test]
async fn status_groups_preserve_boundaries_and_header_assertions_reject_missing_or_wrong_values() {
    let mut app = app();
    for status in [199, 200, 299, 300, 399, 400, 499, 500, 599, 600] {
        let response = app.get(&format!("/{status}")).await.unwrap();
        let success = catch_unwind(AssertUnwindSafe(|| response.assertSuccessful())).is_ok();
        let client = catch_unwind(AssertUnwindSafe(|| response.assertClientError())).is_ok();
        let server = catch_unwind(AssertUnwindSafe(|| response.assertServerError())).is_ok();
        assert_eq!(
            (success, client, server),
            (
                (200..300).contains(&status),
                (400..500).contains(&status),
                (500..600).contains(&status)
            )
        );
    }
    let response = app.get("/200").await.unwrap();
    response
        .assertHeader("X-Probe", "present")
        .assertHeader("X-Probe", "PRESENT")
        .assertHeaderContains("X-Probe", "rese")
        .assertHeader("x-probe", &String::from("present"))
        .assertHeader("x-probe", None)
        .assertHeaderMissing("x-missing");
    fails(&response, |response| {
        response.assertHeader("missing", None);
    });
    fails(&response, |response| {
        response.assertHeader("x-probe", "wrong");
    });
    for value in ["", "PRESENT", "absent"] {
        fails(&response, |response| {
            response.assertHeaderContains("x-probe", value);
        });
    }
    fails(&response, |response| {
        response.assertHeaderMissing("X-Probe");
    });
}

#[tokio::test]
async fn no_content_and_precognition_require_status_body_and_success_header() {
    let mut app = app();
    app.get("/204")
        .await
        .unwrap()
        .assertNoContent(None)
        .assertSuccessfulPrecognition();
    app.get("/200/0")
        .await
        .unwrap()
        .assertNoContent(Some(StatusCode::OK));
    for path in ["/204/no-header", "/204/wrong-header"] {
        let response = app.get(path).await.unwrap();
        response.assertNoContent(None);
        fails(&response, |response| {
            response.assertSuccessfulPrecognition();
        });
    }
    let text = app.get("/200/text").await.unwrap();
    fails(&text, |response| {
        response.assertNoContent(Some(StatusCode::OK));
    });
    let wrong_status = app.get("/200").await.unwrap();
    fails(&wrong_status, |response| {
        response.assertNoContent(None);
    });
    fails(&wrong_status, |response| {
        response.assertSuccessfulPrecognition();
    });
}

fn fixture_response(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let body: serde_json::Value =
            serde_json::from_slice(&request.body_bytes().await?).expect("夹具必须发送有效 JSON");
        use openrust::contracts::session::Session as _;
        if let Some(session) = body.get("session").and_then(serde_json::Value::as_object) {
            request.session_mut()?.flush();
            for (key, value) in session {
                let value = if key == "errors" && !value.is_null() {
                    let mut bags = openrust::support::ViewErrorBag::new();
                    for (name, messages) in value.as_object().expect("错误包必须为对象") {
                        let mut bag = openrust::support::MessageBag::default();
                        if let Some(messages) = messages.as_object() {
                            for (field, values) in messages {
                                for value in values.as_array().unwrap() {
                                    bag.add(field, value.as_str().unwrap());
                                }
                            }
                        }
                        bags.put(name, bag);
                    }
                    serde_json::to_value(bags).unwrap()
                } else {
                    value.clone()
                };
                request.session_mut()?.put(key, value)?;
            }
        }
        Ok(Response::new(
            StatusCode::OK,
            axum::body::Body::from(body["body"].as_str().unwrap().to_owned()),
        )
        .header(
            "content-type".parse().unwrap(),
            body.get("content_type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("text/html")
                .parse()
                .unwrap(),
        ))
    })
}

#[tokio::test]
async fn body_and_text_assertions_follow_real_fixed_test_response_and_phpunit() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("reference/test-response.json")).unwrap();
    let mut app = app();
    for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        let body = case["body"].as_str().unwrap();
        let response = app
            .call(
                ::http::Request::post("/fixture")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({"body":body}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        response.assertStatus(StatusCode::OK).assertContent(body);
        let method = case["method"].as_str().unwrap();
        let arguments = &case["arguments"];
        let passed = catch_unwind(AssertUnwindSafe(|| match method {
            "assertSee" => {
                response.assertSee(
                    arguments[0].as_str().unwrap(),
                    arguments[1].as_bool().unwrap(),
                );
            }
            "assertDontSee" => {
                response.assertDontSee(
                    arguments[0].as_str().unwrap(),
                    arguments[1].as_bool().unwrap(),
                );
            }
            "assertSeeText" => {
                response.assertSeeText(
                    arguments[0].as_str().unwrap(),
                    arguments[1].as_bool().unwrap(),
                );
            }
            "assertDontSeeText" => {
                response.assertDontSeeText(
                    arguments[0].as_str().unwrap(),
                    arguments[1].as_bool().unwrap(),
                );
            }
            _ => {
                let values: Vec<_> = arguments[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap())
                    .collect();
                match method {
                    "assertSeeInOrder" => {
                        response.assertSeeInOrder(&values, arguments[1].as_bool().unwrap());
                    }
                    "assertSeeHtmlInOrder" => {
                        response.assertSeeHtmlInOrder(&values);
                    }
                    "assertSeeTextInOrder" => {
                        response.assertSeeTextInOrder(&values, arguments[1].as_bool().unwrap());
                    }
                    _ => panic!("未知参考方法 {method}"),
                }
            }
        }))
        .is_ok();
        assert_eq!(
            passed,
            case["passes"].as_bool().unwrap(),
            "case {index}: {case}"
        );
    }
}

#[test]
fn shared_tag_removal_matches_fixed_php_for_quotes_comments_and_allowed_tags() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("reference/test-response.json")).unwrap();
    for case in fixture["stripped_allowed"].as_array().unwrap() {
        let allowed: Vec<_> = case["allowed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(
            openrust::support::Stringable::from(case["body"].as_str().unwrap())
                .stripTags(&allowed)
                .value(),
            case["stripped"].as_str().unwrap(),
            "{case}"
        );
    }
}

#[tokio::test]
async fn json_assertions_follow_real_fixed_test_response_and_phpunit() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("reference/test-response.json")).unwrap();
    let mut app = app();
    for (index, case) in fixture["json_cases"].as_array().unwrap().iter().enumerate() {
        let body = case["body"].to_string();
        let response = app
            .call(
                ::http::Request::post("/fixture")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({"body":body}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        response.assertOk().assertContent(&body);
        let arguments = &case["arguments"];
        let optional_key = || arguments.get(0).and_then(serde_json::Value::as_str);
        let passed = catch_unwind(AssertUnwindSafe(|| {
            match case["method"].as_str().unwrap() {
                "assertJson" => {
                    response.assertJson(&arguments[0], arguments[1].as_bool().unwrap());
                }
                "assertJsonPath" => {
                    response.assertJsonPath(arguments[0].as_str().unwrap(), &arguments[1]);
                }
                "assertJsonPathCanonicalizing" => {
                    response.assertJsonPathCanonicalizing(
                        arguments[0].as_str().unwrap(),
                        &arguments[1],
                    );
                }
                "assertJsonFragment" => {
                    response.assertJsonFragment(&arguments[0]);
                }
                "assertJsonMissing" => {
                    response.assertJsonMissing(&arguments[0], arguments[1].as_bool().unwrap());
                }
                "assertJsonMissingExact" => {
                    response.assertJsonMissingExact(&arguments[0]);
                }
                "assertJsonMissingPath" => {
                    response.assertJsonMissingPath(arguments[0].as_str().unwrap());
                }
                "assertJsonCount" => {
                    response.assertJsonCount(
                        arguments[0].as_u64().unwrap().try_into().unwrap(),
                        arguments.get(1).and_then(serde_json::Value::as_str),
                    );
                }
                "assertJsonStructure" => {
                    response.assertJsonStructure(&arguments[0]);
                }
                "assertExactJsonStructure" => {
                    response.assertExactJsonStructure(&arguments[0]);
                }
                "assertSimilarJson" => {
                    response.assertSimilarJson(&arguments[0]);
                }
                "assertExactJson" => {
                    response.assertExactJson(&arguments[0]);
                }
                "assertJsonPaths" => {
                    response.assertJsonPaths(arguments[0].as_object().unwrap());
                }
                "assertJsonPathsCanonicalizing" => {
                    response.assertJsonPathsCanonicalizing(arguments[0].as_object().unwrap());
                }
                "assertJsonFragments" => {
                    response.assertJsonFragments(arguments[0].as_array().unwrap());
                }
                "assertJsonMissingPaths" => {
                    let paths: Vec<_> = arguments[0]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|path| path.as_str().unwrap())
                        .collect();
                    response.assertJsonMissingPaths(&paths);
                }
                "assertJsonIsArray" => {
                    response.assertJsonIsArray(optional_key());
                }
                "assertJsonIsObject" => {
                    response.assertJsonIsObject(optional_key());
                }
                method => panic!("未知 JSON 参考方法 {method}"),
            }
        }))
        .is_ok();
        assert_eq!(
            passed,
            case["passes"].as_bool().unwrap(),
            "JSON case {index}: {case}"
        );
    }
}

#[tokio::test]
async fn session_and_validation_assertions_follow_real_fixed_store_and_phpunit() {
    use serde_json::Value;
    let fixture: Value =
        serde_json::from_str(include_str!("reference/test-response.json")).unwrap();
    let mut app = app();
    for (index, case) in fixture["session_cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let response = app
            .call(
                ::http::Request::post("/fixture")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(case.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        response
            .assertOk()
            .assertContent(case["body"].as_str().unwrap());
        let args = case["arguments"].as_array().unwrap();
        let bag = |index: usize| args.get(index).and_then(Value::as_str).unwrap_or("default");
        let response_key =
            |index: usize| args.get(index).and_then(Value::as_str).unwrap_or("errors");
        let keys = |index: usize| {
            args.get(index)
                .filter(|value| !value.is_null())
                .map(|value| {
                    value
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|value| value.as_str().unwrap())
                        .collect::<Vec<_>>()
                })
        };
        let passed = catch_unwind(AssertUnwindSafe(|| {
            match case["method"].as_str().unwrap() {
                "assertSessionHas" => {
                    response.assertSessionHas(args[0].as_str().unwrap(), args.get(1));
                }
                "assertSessionMissing" => {
                    response.assertSessionMissing(args[0].as_str().unwrap(), args.get(1));
                }
                "assertSessionHasInput" => {
                    response.assertSessionHasInput(args[0].as_str().unwrap(), args.get(1));
                }
                "assertSessionMissingInput" => {
                    response.assertSessionMissingInput(args[0].as_str().unwrap());
                }
                "assertSessionHasAll" => {
                    response.assertSessionHasAll(&args[0]);
                }
                "assertSessionHasNoErrors" => {
                    response.assertSessionHasNoErrors();
                }
                "assertSessionHasErrors" => {
                    response.assertSessionHasErrors(&args[0], args[1].as_str(), bag(2));
                }
                "assertSessionDoesntHaveErrors" => {
                    response.assertSessionDoesntHaveErrors(&args[0], args[1].as_str(), bag(2));
                }
                "assertValid" => {
                    let keys = keys(0);
                    response.assertValid(keys.as_deref(), bag(1), response_key(2));
                }
                "assertInvalid" => {
                    response.assertInvalid(args.first(), bag(1), response_key(2));
                }
                "assertOnlyInvalid" => {
                    response.assertOnlyInvalid(args.first(), bag(1), response_key(2));
                }
                "assertJsonValidationErrors" => {
                    response.assertJsonValidationErrorsAt(&args[0], response_key(1));
                }
                "assertOnlyJsonValidationErrors" => {
                    response.assertOnlyJsonValidationErrors(&args[0], response_key(1));
                }
                "assertJsonMissingValidationErrors" => {
                    let keys = keys(0);
                    response.assertJsonMissingValidationErrors(keys.as_deref(), response_key(1));
                }
                "assertJsonValidationErrorFor" => {
                    response
                        .assertJsonValidationErrorFor(args[0].as_str().unwrap(), response_key(1));
                }
                method => panic!("未知 Session 参考方法 {method}"),
            }
        }))
        .is_ok();
        assert_eq!(
            passed,
            case["passes"].as_bool().unwrap(),
            "Session case {index}: {case}"
        );
    }
}

fn flash_session(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use openrust::contracts::session::Session as _;
    Box::pin(async move {
        request
            .session_mut()?
            .flashInput(serde_json::json!({"email":"ada@example.test","nullable":null}))?;
        let mut errors = openrust::support::ViewErrorBag::new();
        let mut bag = openrust::support::MessageBag::default();
        bag.add("email", "Invalid email.");
        errors.put("login", bag);
        request
            .session_mut()?
            .flash("errors", serde_json::to_value(errors).unwrap())?;
        Ok(Response::from("flashed"))
    })
}
fn read_old_input(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Response::json(
            StatusCode::OK,
            serde_json::json!({"email":request.session()?.getOldInput(Some("email"))}),
        )
    })
}
#[tokio::test]
async fn assertions_do_not_consume_flashed_input_and_named_errors_across_real_session_requests() {
    let mut app = app();
    let first = app.get("/flash").await.unwrap();
    first
        .assertOk()
        .assertSessionHasInput("email", Some(&serde_json::json!("ada@example.test")))
        .assertSessionHasInput("email", None)
        .assertSessionMissingInput("nullable")
        .assertSessionHasErrorsIn("login", &serde_json::json!(["email"]), None)
        .assertInvalid(
            Some(&serde_json::json!({"email":"Invalid"})),
            "login",
            "errors",
        )
        .assertValid(None, "default", "errors");
    let cookie = first
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|value| value.starts_with("assertion-session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let request = || {
        ::http::Request::get("/old-input")
            .header("cookie", cookie)
            .body(axum::body::Body::empty())
            .unwrap()
    };
    app.call(request())
        .await
        .unwrap()
        .assertOk()
        .assertJsonPath("email", &serde_json::json!("ada@example.test"))
        .assertSessionMissingInput("email")
        .assertSessionHasNoErrors();
    app.call(request())
        .await
        .unwrap()
        .assertJsonPath("email", &serde_json::Value::Null)
        .assertSessionMissingInput("email");
}

fn redirect_errors(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use openrust::contracts::session::Session as _;
    Box::pin(async move {
        let bag_name = request.route("bag").unwrap().to_owned();
        let previous = openrust::support::facades::URL::previous(request, None)?;
        let mut errors = openrust::support::ViewErrorBag::new();
        let mut bag = openrust::support::MessageBag::default();
        bag.add("email", "邮箱无效");
        errors.put(&bag_name, bag);
        request
            .session_mut()?
            .flash("errors", serde_json::to_value(errors).unwrap())?;
        Ok(Response::new(StatusCode::FOUND, axum::body::Body::empty())
            .header(http::header::LOCATION, previous.parse().unwrap()))
    })
}

#[tokio::test]
async fn redirect_back_with_errors_uses_real_session_and_named_bags() {
    let mut app = app();
    for bag in ["default", "login"] {
        let response = app
            .call(
                http::Request::get(format!("/redirect-errors/{bag}"))
                    .header("referer", "http://localhost/from?step=2")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        response.assertRedirectBackWithErrors(&serde_json::json!({"email":"邮箱无效"}), None, bag);
        fails(&response, |r| {
            r.assertRedirectBackWithErrors(&serde_json::json!(["missing"]), None, bag);
        });
        fails(&response, |r| {
            r.assertRedirectBackWithErrors(&serde_json::json!(["email"]), None, "other");
        });
        fails(&response, |r| {
            r.assertRedirectBackWithoutErrors();
        });
        response.assertSessionHasErrors(&serde_json::json!(["email"]), None, bag);
    }
}
