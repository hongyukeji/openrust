//! 使用真实请求、Session及TestApp核对公开响应，而非检查静态配置。

use super::*;
use crate::core::NoConfig;
use crate::foundation::ApplicationDefinition;
use crate::foundation::BootContext;
use crate::http::HttpServiceProvider;
use crate::session::MemorySessionStore;
use crate::session::Session;
use crate::support::{DefaultProviders, ServiceProvider};
use crate::testing::TestApp;
use axum::{
    body::{Body, to_bytes},
    extract::FromRequest,
    http::{Request as HttpRequest, header},
};

async fn request(accept: &str, payload: &str, session: Option<Session>) -> Result<Request> {
    let mut incoming = HttpRequest::post("/current")
        .header(header::HOST, "example.test")
        .header(header::ACCEPT, accept)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::REFERER, "http://example.test/settings")
        .body(Body::from(payload.to_owned()))
        .map_err(Error::internal)?;
    if let Some(session) = session {
        incoming.extensions_mut().insert(session);
    }
    Request::from_request(incoming, &()).await
}
fn session() -> Result<Session> {
    Session::new(None, Arc::new(MemorySessionStore::default()), None)
}
async fn body(response: Response) -> Result<Vec<u8>> {
    Ok(to_bytes(response.into_body(), 1024 * 1024)
        .await
        .map_err(Error::internal)?
        .to_vec())
}

#[tokio::test]
async fn default_json_is_exact_and_never_wrapped() -> Result<()> {
    let request = request("application/json", "{}", None).await?;
    let responses = DefaultFortifyResponses::new(FortifyConfig::default());
    let response = LoginResponse::to_response(&responses, &request).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body(response).await?)
            .map_err(Error::internal)?,
        json!({"two_factor":false})
    );
    let response = PasswordConfirmedResponse::to_response(&responses, &request).await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(body(response).await?, b"\"\"");
    for response in [
        LogoutResponse::to_response(&responses, &request).await?,
        TwoFactorLoginResponse::to_response(&responses, &request).await?,
    ] {
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(body(response).await?, [] as [u8; 0]);
    }
    for response in [
        TwoFactorEnabledResponse::to_response(&responses, &request).await?,
        TwoFactorConfirmedResponse::to_response(&responses, &request).await?,
        TwoFactorDisabledResponse::to_response(&responses, &request).await?,
        RecoveryCodesGeneratedResponse::to_response(&responses, &request).await?,
    ] {
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body(response).await?, b"\"\"");
    }
    Ok(())
}

#[tokio::test]
async fn html_management_flashes_exact_status_and_redirects_back() -> Result<()> {
    let responses = DefaultFortifyResponses::new(FortifyConfig::default());
    for status in [
        "two-factor-authentication-enabled",
        "two-factor-authentication-confirmed",
        "two-factor-authentication-disabled",
        "recovery-codes-generated",
    ] {
        let session = session()?;
        let request = request("text/html", "{}", Some(session.clone())).await?;
        let response = match status {
            "two-factor-authentication-enabled" => {
                TwoFactorEnabledResponse::to_response(&responses, &request).await?
            }
            "two-factor-authentication-confirmed" => {
                TwoFactorConfirmedResponse::to_response(&responses, &request).await?
            }
            "two-factor-authentication-disabled" => {
                TwoFactorDisabledResponse::to_response(&responses, &request).await?
            }
            _ => RecoveryCodesGeneratedResponse::to_response(&responses, &request).await?,
        };
        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(
            response.headers()[header::LOCATION],
            "http://example.test/settings"
        );
        assert_eq!(session.get_json("status")?, Some(json!(status)));
    }
    Ok(())
}

#[tokio::test]
async fn intended_location_is_consumed_once_with_configured_fallback() -> Result<()> {
    let session = session()?;
    session.put_typed("url.intended", "/protected")?;
    let request = request("text/html", "{}", Some(session.clone())).await?;
    let mut config = FortifyConfig::default();
    config
        .redirects
        .insert("login".into(), Some("/dashboard".into()));
    config
        .redirects
        .insert("password-confirmation".into(), Some("/confirmed".into()));
    let responses = DefaultFortifyResponses::new(config);
    let first = LoginResponse::to_response(&responses, &request).await?;
    assert!(
        first.headers()[header::LOCATION]
            .to_str()
            .map_err(Error::internal)?
            .ends_with("/protected")
    );
    assert_eq!(session.get_json("url.intended")?, None);
    let second = TwoFactorLoginResponse::to_response(&responses, &request).await?;
    assert!(
        second.headers()[header::LOCATION]
            .to_str()
            .map_err(Error::internal)?
            .ends_with("/dashboard")
    );
    let confirmation = PasswordConfirmedResponse::to_response(&responses, &request).await?;
    assert!(
        confirmation.headers()[header::LOCATION]
            .to_str()
            .map_err(Error::internal)?
            .ends_with("/confirmed")
    );
    Ok(())
}

#[tokio::test]
async fn failed_confirmation_returns_validation_and_html_session_errors() -> Result<()> {
    let responses = DefaultFortifyResponses::new(FortifyConfig::default());
    let json = request("application/json", "{}", None).await?;
    assert!(matches!(
        FailedPasswordConfirmationResponse::to_response(&responses, &json).await,
        Err(Error::Validation(_))
    ));
    let session = session()?;
    let html = request("text/html", "{}", Some(session.clone())).await?;
    let response = FailedPasswordConfirmationResponse::to_response(&responses, &html).await?;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert!(session.get_json("errors")?.is_some());
    Ok(())
}

#[tokio::test]
async fn failed_challenge_selects_filled_recovery_field_and_preserves_default_bag() -> Result<()> {
    let responses = DefaultFortifyResponses::new(FortifyConfig::default());
    for (payload, field) in [
        ("{}", "code"),
        (r#"{"recovery_code":""}"#, "code"),
        (r#"{"recovery_code":"wrong"}"#, "recovery_code"),
    ] {
        let request = request("application/json", payload, None).await?;
        let result = FailedTwoFactorLoginResponse::to_response(&responses, &request).await;
        if let Err(Error::Validation(errors)) = result {
            assert!(
                serde_json::to_value(errors)
                    .map_err(Error::internal)?
                    .get(field)
                    .is_some()
            );
        } else {
            return Err(Error::custom(
                "test_expected_validation",
                "应返回字段校验错误",
            ));
        }
    }
    Ok(())
}

struct ResponsesProvider;
impl ServiceProvider for ResponsesProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(|routes| {
                routes.get("/fortify-response", |request: Request| async move {
                    LoginResponse::to_response(
                        &DefaultFortifyResponses::new(FortifyConfig::default()),
                        &request,
                    )
                    .await
                })
            })?;
            Ok(())
        })
    }
}
struct ResponsesApp;
impl ApplicationDefinition for ResponsesApp {
    type Config = NoConfig;
    fn default_providers() -> DefaultProviders {
        DefaultProviders::empty()
    }
    fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
        vec![Box::new(HttpServiceProvider), Box::new(ResponsesProvider)]
    }
}
#[tokio::test]
async fn test_app_serves_exact_default_json_through_real_router() -> Result<()> {
    let fixture = TestApp::<ResponsesApp>::builder(figment::Figment::from(
        figment::providers::Serialized::defaults(crate::config::FrameworkConfig::default()),
    ))
    .build()
    .await?;
    let response = fixture
        .raw_response(
            HttpRequest::get("/fortify-response")
                .header(header::ACCEPT, "application/json")
                .body(Body::empty())
                .map_err(Error::internal)?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body(response).await?)
            .map_err(Error::internal)?,
        json!({"two_factor":false})
    );
    Ok(())
}

struct CustomLogin;
impl LoginResponse for CustomLogin {
    fn to_response<'a>(&'a self, _: &'a Request) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async { Ok(Json(json!({"custom":true})).into_response()) })
    }
}
struct BindingProvider;
impl ServiceProvider for BindingProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.config_section(
                "fortify",
                FortifyConfig::default(),
                crate::config::repository::SectionSchema::Any,
                |_| Ok(()),
            )?;
            app.instance::<dyn LoginResponse>(Arc::new(CustomLogin))
                .await?;
            register(app).await
        })
    }
}
#[tokio::test]
async fn registration_preserves_custom_binding_and_scopes_defaults_without_inventing_views()
-> Result<()> {
    let mut builder = crate::foundation::ProviderBuilder::new();
    builder.provider(BindingProvider);
    let runtime = builder.start(figment::Figment::new()).await?;
    let first = runtime.container().scope()?;
    let second = runtime.container().scope()?;
    let a = first.resolve::<dyn LogoutResponse>().await?;
    let b = first.resolve::<dyn LogoutResponse>().await?;
    let other = second.resolve::<dyn LogoutResponse>().await?;
    assert!(Arc::ptr_eq(&a, &b));
    assert!(!Arc::ptr_eq(&a, &other));
    let custom = first.resolve::<dyn LoginResponse>().await?;
    let request = request("application/json", "{}", None).await?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &body(custom.to_response(&request).await?).await?
        )
        .map_err(Error::internal)?,
        json!({"custom":true})
    );
    assert!(first.resolve::<dyn LoginViewResponse>().await.is_err());
    assert!(
        first
            .resolve::<dyn ConfirmPasswordViewResponse>()
            .await
            .is_err()
    );
    assert!(
        first
            .resolve::<dyn TwoFactorChallengeViewResponse>()
            .await
            .is_err()
    );
    Ok(())
}
