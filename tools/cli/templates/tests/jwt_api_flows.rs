//! 显式JWT API生成应用的真实Router回归；不借用Web Session流程。
#![cfg(feature = "auth-fortify")]
#[path = "../app/mod.rs"]
mod app;
#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "../config/mod.rs"]
mod config;
{{grpc_module}}
#[path = "support/fortify_api_http.rs"]
mod fortify_api_http;
#[path = "../modules/mod.rs"]
mod modules;
#[path = "../routes/mod.rs"]
mod routes;
#[path = "../database/seeders.rs"]
mod seed;
use bootstrap::OpenRustGeneratedApplication as GeneratedApplication;
use openrust::testing::TestDatabase;

#[tokio::test]
async fn generated_jwt_api_profile_has_real_factor_management_and_no_session_cookie()
-> Result<(), Box<dyn std::error::Error>> {
    {{database_setup}}

    let defaults = openrust::config::FrameworkConfig::default();
    let input = figment::Figment::from(figment::providers::Serialized::defaults(defaults))
        .merge(figment::providers::Serialized::defaults(serde_json::json!({
            "app":{"env":"local","url":"http://localhost","name":"generated-api-tests"},
            "database":{"url":database.url()},
            "cache":{"default":"test","stores":{"test":{"driver":"memory"}}},
            "auth":{"jwt":{"issuer":"http://localhost","audience":"generated-api-tests","active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}},"signed_links":{"issuer":"http://localhost","active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}},
            "mail":{"driver":"log"},"sms":{"driver":"log"}
        })));
    let outcome = fortify_api_http::exercise::<GeneratedApplication>(input).await;
    let closed = database.close().await;
    outcome?;
    closed?;
    Ok(())
}
