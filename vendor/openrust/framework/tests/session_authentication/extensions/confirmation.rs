use openrust::support::facades::Redirect;

use super::*;

async fn browser_get(address: SocketAddr, path: &str, cookie: &str) -> String {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let wire = format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: text/html\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n");
        stream.write_all(wire.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 302 "), "{response}");
        response
    })
    .await
    .unwrap()
}

fn confirm(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let input = request.input(Some("password")).await?.unwrap();
        if !Auth::confirmPassword(request, input.as_str().unwrap()).await? {
            return Err(openrust::http::abort(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Invalid password.",
                Default::default(),
            ));
        }
        Ok(Redirect::intended(request, "/settings", Default::default())?.into())
    })
}

fn age_confirmation(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let now = jiff::Timestamp::now().as_second();
        let age = request
            .query(Some("age"))?
            .unwrap()
            .as_str()
            .unwrap()
            .parse::<i64>()
            .unwrap();
        request
            .session_mut()?
            .put("auth.password_confirmed_at", json!(now - age))?;
        current(request).await
    })
}

#[tokio::test]
async fn password_confirmation_redirects_and_expires_over_http() {
    let (address, server) = start(
        || {
            Route::getWith("/login", current, |route| {
                route.middleware_name("auth.basic");
                Ok(())
            })?;
            Route::getWith("/confirmation", current, |route| {
                route.name("password.confirm").middleware_name("auth");
                Ok(())
            })?;
            Route::getWith("/confirmation/alternate", current, |route| {
                route.name("password.alternate").middleware_name("auth");
                Ok(())
            })?;
            Route::r#match(&[Method::POST], "/confirmation", confirm)?;
            Route::getWith("/settings", current, |route| {
                route
                    .middleware_name("auth")
                    .middleware_name("password.confirm");
                Ok(())
            })?;
            Route::getWith("/settings/strict", current, |route| {
                route
                    .middleware_name("auth")
                    .middleware_name("password.confirm:password.alternate,10");
                Ok(())
            })?;
            Route::get("/age", age_confirmation)?;
            Ok(())
        },
        || Ok(()),
        |app| {
            app.container()
                .get::<dyn Repository>("config")
                .unwrap()
                .set("auth.password_timeout", json!(60))
                .unwrap();
        },
    )
    .await;
    let login = send(
        address,
        "GET",
        "/login",
        &authorization("a@example.test", "good:secret"),
        "",
        200,
    )
    .await;
    let cookie = login.cookie("auth_session");
    let cookies = format!("Cookie: {cookie}\r\n");
    assert_eq!(
        get(address, "/settings", &cookie, 423).await.body["message"],
        "Password confirmation required."
    );
    let strict = browser_get(address, "/settings/strict", &cookie).await;
    assert!(strict.contains(&format!(
        "location: http://{address}/confirmation/alternate"
    )));
    let browser = browser_get(address, "/settings?tab=security", &cookie).await;
    assert!(browser.contains(&format!("location: http://{address}/confirmation\r\n")));
    send(
        address,
        "POST",
        "/confirmation",
        &cookies,
        r#"{"password":"wrong"}"#,
        422,
    )
    .await;
    get(address, "/settings", &cookie, 423).await;
    let confirmed = send(
        address,
        "POST",
        "/confirmation",
        &cookies,
        r#"{"password":"good:secret"}"#,
        302,
    )
    .await;
    assert!(
        confirmed
            .headers
            .contains(&format!("location: http://{address}/settings?tab=security"))
    );
    assert_eq!(
        get(address, "/settings", &cookie, 200).await.body["user"],
        "001"
    );
    get(address, "/settings/strict", &cookie, 200).await;

    get(address, "/age?age=30", &cookie, 200).await;
    get(address, "/settings", &cookie, 200).await;
    get(address, "/settings/strict", &cookie, 423).await;
    get(address, "/age?age=90", &cookie, 200).await;
    get(address, "/settings", &cookie, 423).await;
    server.abort();
}
