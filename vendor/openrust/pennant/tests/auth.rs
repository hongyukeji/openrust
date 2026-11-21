#![allow(non_snake_case, reason = "认证与队列公开契约沿用 Laravel 原名")]

use openrust::{
    auth::{AuthIdentifier, AuthManager},
    contracts::auth::Authenticatable,
    database::{
        DatabaseManager,
        eloquent::{
            Attributes, Model, ModelMetadata, ModelState,
            relations::{MorphMap, MorphMapEntry},
        },
    },
    foundation::{Application, configuration::Routing},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    support::{ProviderResult, ServiceProvider, facades::Route},
    testing::TestApp,
};
use openrust_pennant::{
    Feature, FeatureManager, PennantServiceProvider, Scope, middleware::EnsureFeaturesAreActive,
};
use serde_json::json;
use std::sync::Arc;

struct Account(ModelState);
impl Model for Account {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("pennant_users");
        &META
    }
    fn from_state(state: ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}
impl Authenticatable for Account {
    fn getAuthIdentifierName(&self) -> &str {
        "id"
    }
    fn getAuthIdentifier(&self) -> AuthIdentifier {
        7_u64.into()
    }
    fn getAuthPasswordName(&self) -> &str {
        "password"
    }
    fn getAuthPassword(&self) -> Option<&str> {
        None
    }
    fn getRememberToken(&self) -> Option<&str> {
        None
    }
    fn setRememberToken(&mut self, _: Option<String>) {}
    fn getRememberTokenName(&self) -> &str {
        "remember_token"
    }
}
struct Register;
impl ServiceProvider for Register {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let auth = app.make::<AuthManager>().await?;
            auth.registerModel::<Account>("accounts");
            MorphMap::enforceMorphMap(app, [MorphMapEntry::model::<Account>("staff")], false)?;
            let db = app.make::<DatabaseManager>().await?.connection(None)?;
            auth.viaRequest("pennant-header", move |request| {
                let guest = request.uri().path().contains("guest");
                let user = (!guest).then(|| {
                    Box::new(Account::queryOn(&db).make(Attributes::new()).unwrap())
                        as Box<dyn Authenticatable>
                });
                Box::pin(async move { Ok(user) })
            });
            app.make::<FeatureManager>().await?.store(None)?.define(
                "signed",
                Arc::new(|scope| Box::pin(async move { Ok(json!(scope != Scope::Null)) })),
            )?;
            Ok(())
        })
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/signed", identity)?;
    Route::get("/guest", identity)?;
    Route::group(
        openrust::routing::RouteGroup::new()
            .middleware(Arc::new(EnsureFeaturesAreActive::using(["signed"]))),
        |router| {
            router.get("/protected", identity)?;
            router.get("/protected-guest", identity)?;
            Ok(())
        },
    )?;
    Ok(())
}
fn identity(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let _features = Feature::forRequest(request).await.map_err(internal)?;
        let manager = Feature::manager().map_err(internal)?;
        let value = manager.serializeScope(&manager.defaultScope("array").map_err(internal)?);
        Ok(Response::new(StatusCode::OK, Body::from(value)))
    })
}
fn internal(source: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}
#[tokio::test]
async fn lazy_native_guard_concrete_class_morph_and_guest_scopes_are_request_local() {
    let mut app = TestApp::create(|builder| builder.withConfig(|_, _| Ok([
        ("pennant".into(), json!({"default":"array"})),
        ("database".into(), json!({"default":"test","connections":{"test":{"driver":"sqlite","database":":memory:"}}})),
        ("auth".into(), json!({"defaults":{"guard":"test"},"guards":{"test":{"driver":"pennant-header"}}})),
    ].into())).withProviders(vec![Box::new(PennantServiceProvider), Box::new(Register)]).withRouting(Routing::default().api(routes))).unwrap();
    assert_eq!(
        app.get("/api/signed").await.unwrap().assertOk().body(),
        format!("{}|7", std::any::type_name::<Account>()).as_bytes()
    );
    assert_eq!(
        app.get("/api/guest").await.unwrap().assertOk().body(),
        b"__laravel_null"
    );
    let manager = app.app().make::<FeatureManager>().await.unwrap();
    manager.useMorphMap(true);
    assert_eq!(
        app.get("/api/protected").await.unwrap().assertOk().body(),
        b"staff|7"
    );
    app.get("/api/protected-guest")
        .await
        .unwrap()
        .assertStatus(StatusCode::BAD_REQUEST);
    assert_eq!(
        app.get("/api/guest").await.unwrap().assertOk().body(),
        b"__laravel_null"
    );
}
