//! 示例 API 路由；路由定义和处理器分开。

use crate::app::example::http::controllers;
#[cfg(feature = "auth")]
use controllers::auth::{
    auth_login, auth_logout, auth_refresh, auth_register, auth_request_email_verification,
    auth_request_password_reset, auth_reset_password, auth_verify_email, current_user,
};
use openrust::prelude::*;

pub(super) fn register(routes: Routes, database_enabled: bool) -> Routes {
    let routes = routes.get("/api/example", controllers::example::show);
    #[cfg(feature = "auth")]
    let routes = if database_enabled {
        auth_routes(routes)
    } else {
        routes
    };
    #[cfg(feature = "queue")]
    let routes = if database_enabled {
        routes.post(
            "/api/jobs/example-heartbeat",
            controllers::jobs::enqueue_example_heartbeat,
        )
    } else {
        routes
    };
    #[cfg(not(any(feature = "auth", feature = "queue")))]
    let _ = database_enabled;
    routes
}

#[cfg(feature = "auth")]
fn auth_routes(routes: Routes) -> Routes {
    routes.nest("/api", |api| {
        api.get("/me", current_user)
            .layer(openrust::auth::AuthLayer::required(
                openrust::auth::AuthMode::Jwt,
            ))
            .post("/auth/register", auth_register)
            .post(
                "/auth/request-email-verification",
                auth_request_email_verification,
            )
            .post("/auth/verify-email", auth_verify_email)
            .post("/auth/login", auth_login)
            .post("/auth/refresh", auth_refresh)
            .post("/auth/logout", auth_logout)
            .post("/auth/password-reset", auth_request_password_reset)
            .post("/auth/password-reset/confirm", auth_reset_password)
            .merge(Routes::new().get("/me/pat", current_user).layer(
                openrust::auth::AuthLayer::required(openrust::auth::AuthMode::PersonalAccessToken),
            ))
    })
}
