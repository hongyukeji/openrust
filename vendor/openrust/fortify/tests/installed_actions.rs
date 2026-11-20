// Compile the actual published actions and provider against the application's real User.
#[path = "../stubs/mod.rs"]
mod installed;
#[path = "../stubs/fortify_service_provider.rs"]
mod installed_provider;
mod app {
    pub(crate) mod models {
        mod user {
            include!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../app/models/user.rs"
            ));
        }
        pub(crate) use user::User;
    }
    pub(crate) mod actions {
        pub(crate) mod fortify {
            pub(crate) use crate::installed::*;
        }
    }
}

impl Clone for app::models::User {
    fn clone(&self) -> Self {
        use openrust::database::eloquent::Model;
        Self::from_state(self.state().clone())
    }
}
impl openrust_fortify::TwoFactorAuthenticatable for app::models::User {}
impl openrust_fortify::passkeys::PasskeyAuthenticatable for app::models::User {}

#[test]
fn installed_provider_and_actions_are_native_contracts() {
    fn provider(_: impl openrust::support::ServiceProvider) {}
    provider(installed_provider::FortifyServiceProvider);
}

fn update(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    use openrust_fortify::contracts::UpdatesUserPasswords as _;
    Box::pin(async move {
        let user = openrust::auth::Auth::user(request).await?.unwrap();
        let input = request.all().await?;
        installed::UpdateUserPassword
            .updateFromRequest(request, user.as_ref(), &input)
            .await?;
        Ok(openrust::http::Response::noContent())
    })
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::group(openrust::routing::RouteGroup::new(), |router| {
        router.r#match(&[openrust::http::Method::POST], "/update", update)?;
        Ok(())
    })
}

#[tokio::test]
async fn installed_password_action_checks_web_guard_independently_of_the_updated_user() {
    use openrust::{
        auth::AuthManager,
        contracts::auth::Authenticatable,
        contracts::http::Kernel as _,
        database::eloquent::{Model, ModelState},
        hashing::HashOptions,
    };
    use serde_json::json;
    let mut app=openrust::foundation::Application::configure(std::env::temp_dir())
        .withConfig(|_,_|Ok([
            ("hashing".into(),json!({"driver":"bcrypt","bcrypt":{"rounds":4}})),
            ("auth".into(),json!({"defaults":{"guard":"admin"},"guards":{"web":{"driver":"web-fixture"},"admin":{"driver":"admin-fixture"}}})),
        ].into()))
        .withRouting(openrust::foundation::configuration::Routing::default().api(routes)).create().unwrap();
    app.boot().await.unwrap();
    let hasher = app
        .make::<dyn openrust::contracts::hashing::Hasher>()
        .await
        .unwrap();
    let manager = app.make::<AuthManager>().await.unwrap();
    for (driver, password) in [
        ("web-fixture", "web-secret"),
        ("admin-fixture", "admin-secret"),
    ] {
        let hash = hasher
            .make(password.as_bytes(), &HashOptions::new())
            .await
            .unwrap();
        manager.viaRequest(driver, move |_| {
            let mut user = app::models::User::from_state(ModelState::default());
            user.setAttribute("password", hash.clone());
            Box::pin(async move { Ok(Some(Box::new(user) as Box<dyn Authenticatable>)) })
        });
    }
    let mut kernel = openrust::foundation::http::Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let kernel = std::sync::Arc::new(kernel);
    for (password, web_rejected) in [("admin-secret", true), ("web-secret", false)] {
        let input =
            json!({"current_password":password,"password":"short","password_confirmation":"short"});
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/api/update")
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(input.to_string()))
            .unwrap();
        let response = kernel.clone().handleTransport(request).await;
        assert_eq!(response.status(), 422);
        let body = axum::body::to_bytes(response.into_body(), 10240)
            .await
            .unwrap();
        let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            result["errors"].get("current_password").is_some(),
            web_rejected,
            "{result}"
        );
        assert!(result["errors"].get("password").is_some());
    }
}
