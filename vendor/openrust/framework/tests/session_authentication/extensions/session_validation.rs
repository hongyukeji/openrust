use super::*;
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicBool, Ordering},
};

fn remember(req: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let credentials = Credentials::new([("email", "a@example.test".into())], "good:secret");
        if !Auth::session_guard(req, None)?
            .attempt(&credentials, true)
            .await?
        {
            return Err(HttpError::Unauthenticated);
        }
        current(req).await
    })
}
fn forget_login(req: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        req.session_mut()?.flush();
        Response::json(StatusCode::OK, json!({"forgot":true}))
    })
}
fn web() -> Result<(), RoutingError> {
    Route::post("/remember", remember)?;
    Route::post("/forget-login", forget_login)?;
    Route::get_with("/secure", current, |route| {
        route.middleware_name("auth");
        Ok(())
    })?;
    Ok(())
}

#[tokio::test]
async fn session_validators_gate_real_http_and_remember_before_id_rotation_without_cross_app_leaks()
{
    let revoked = Arc::new(Mutex::new(BTreeSet::<String>::new()));
    let captured = Arc::new(Mutex::new(Vec::<String>::new()));
    let unavailable = Arc::new(AtomicBool::new(false));
    let deny = revoked.clone();
    let observed = captured.clone();
    let failure = unavailable.clone();
    let (address, server) = start(
        web,
        || Ok(()),
        move |app| {
            app.container()
                .make::<AuthManager>()
                .unwrap()
                .add_session_validator("persistent-device-policy", move |guard, user, session| {
                    let deny = deny.clone();
                    let observed = observed.clone();
                    let failure = failure.clone();
                    Box::pin(async move {
                        tokio::task::yield_now().await;
                        assert_eq!(guard, "web");
                        assert_eq!(user.get_auth_identifier(), AuthIdentifier::from("001"));
                        observed.lock().unwrap().push(session.get_id().to_owned());
                        if failure.load(Ordering::SeqCst) {
                            return Err(AuthError::Configuration("设备存储不可用"));
                        }
                        Ok(!deny.lock().unwrap().contains(session.get_id()))
                    })
                })
                .unwrap();
        },
    )
    .await;
    let (other_address, other_server) = start(web, || Ok(()), |_| {}).await;
    let login = send(address, "POST", "/remember", "", "", 200).await;
    let session = login.cookie("auth_session");
    let remember = login.cookie("remember_web");
    get(address, "/secure", &session, 200).await;
    let original = captured.lock().unwrap().last().unwrap().clone();
    let other_login = send(other_address, "POST", "/remember", "", "", 200).await;
    let other_session = other_login.cookie("auth_session");
    send(
        address,
        "POST",
        "/forget-login",
        &format!("Cookie: {session}\r\n"),
        "",
        200,
    )
    .await;
    revoked.lock().unwrap().insert(original.clone());
    send(
        address,
        "GET",
        "/secure",
        &format!("Cookie: {session}; {remember}\r\n"),
        "",
        401,
    )
    .await;
    assert_eq!(
        captured.lock().unwrap().last().unwrap(),
        &original,
        "remember恢复策略须先检查原ID"
    );
    get(other_address, "/secure", &other_session, 200).await;
    revoked.lock().unwrap().clear();
    let restored = send(
        address,
        "GET",
        "/secure",
        &format!("Cookie: {session}; {remember}\r\n"),
        "",
        200,
    )
    .await;
    let renewed = restored.cookie("auth_session");
    assert_ne!(renewed, session, "允许原生remember恢复后仍轮换会话ID");
    get(address, "/secure", &renewed, 200).await;
    unavailable.store(true, Ordering::SeqCst);
    get(address, "/secure", &renewed, 500).await;
    get(other_address, "/secure", &other_session, 200).await;
    unavailable.store(false, Ordering::SeqCst);
    get(address, "/secure", &renewed, 200).await;
    server.abort();
    other_server.abort();
}
