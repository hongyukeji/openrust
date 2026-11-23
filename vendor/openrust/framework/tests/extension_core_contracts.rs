#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use openrust::{
    auth::{AuthIdentifier, AuthManager},
    console::confirmToProceed,
    contracts::{auth::Authenticatable, container::Container as _, foundation::Application as _},
    database::{
        DatabaseManager,
        eloquent::{
            Attributes, Model, ModelMetadata, ModelState,
            relations::{MorphMap, MorphMapEntry},
        },
    },
    foundation::Application,
};

struct Account(ModelState);
impl Model for Account {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("accounts")
            .fillable(&["name"])
            .guarded(&["password"])
            .softDeletes("removed_at");
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

async fn application() -> Application {
    let mut app = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            Ok([(
                "database".into(),
                serde_json::json!({"default":"test", "connections":{
                    "test":{"driver":"sqlite", "database":":memory:"}
                }}),
            )]
            .into())
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    app
}

#[tokio::test]
async fn registered_auth_model_uses_its_own_morph_map_and_preserves_errors() {
    let first = application().await;
    let second = application().await;
    let first_auth = first.container().make::<AuthManager>().unwrap();
    let second_auth = second.container().make::<AuthManager>().unwrap();
    first_auth.registerModel::<Account>("accounts");
    second_auth.registerModel::<Account>("accounts");
    MorphMap::enforceMorphMap(&first, [MorphMapEntry::model::<Account>("staff")], false).unwrap();
    MorphMap::enforceMorphMap(&second, [MorphMapEntry::model::<Account>("member")], false).unwrap();
    let db = first
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let other = second
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let account = Account::queryOn(&db).make(Attributes::new()).unwrap();
    let member = Account::queryOn(&other).make(Attributes::new()).unwrap();
    assert_eq!(account.getFillable(), &["name"]);
    assert_eq!(account.getGuarded(), &["password"]);
    assert_eq!(account.getDeletedAtColumn(), Some("removed_at"));
    assert_eq!(first_auth.userMorphClass(&account).unwrap(), Some("staff"));
    assert_eq!(
        first_auth.userClass(&account),
        Some(std::any::type_name::<Account>())
    );
    assert_eq!(second_auth.userMorphClass(&member).unwrap(), Some("member"));
    // Resolution follows the actual model connection, even through another manager.
    assert_eq!(first_auth.userMorphClass(&member).unwrap(), Some("member"));
    MorphMap::enforceMorphMap(&first, [], false).unwrap();
    assert!(first_auth.userMorphClass(&account).is_err());
    let unregistered = application().await;
    assert_eq!(
        unregistered
            .container()
            .make::<AuthManager>()
            .unwrap()
            .userClass(&member),
        None
    );
    assert_eq!(
        unregistered
            .container()
            .make::<AuthManager>()
            .unwrap()
            .userMorphClass(&member)
            .unwrap(),
        None
    );
}

#[test]
fn production_confirmation_accepts_optional_flags_and_refuses_non_interactive() {
    let command = || {
        clap::Command::new("clean").arg(
            clap::Arg::new("no-interaction")
                .long("no-interaction")
                .action(clap::ArgAction::SetTrue),
        )
    };
    let quiet = command()
        .try_get_matches_from(["clean", "--no-interaction"])
        .unwrap();
    assert!(confirmToProceed(false, &quiet).unwrap());
    assert!(!confirmToProceed(true, &quiet).unwrap());
    let forced = command()
        .arg(
            clap::Arg::new("force")
                .long("force")
                .action(clap::ArgAction::SetTrue),
        )
        .try_get_matches_from(["clean", "--force", "--no-interaction"])
        .unwrap();
    assert!(confirmToProceed(true, &forced).unwrap());
    let plain = clap::Command::new("plain")
        .try_get_matches_from(["plain"])
        .unwrap();
    assert!(confirmToProceed(false, &plain).unwrap());
}

fn cached_identity(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::{contracts::auth::Factory as _, support::facades::Auth};
        use std::sync::Arc;
        let auth = Auth::manager().unwrap();
        assert!(auth.resolvedUser(None).is_none());
        assert!(Auth::resolvedUser(Some("first")).unwrap().is_none());
        // No log listeners are installed. Reading cached identities must still work.
        assert!(openrust::log::context::Origin::current().is_none());
        if request.path() == "guest" {
            return Ok(openrust::http::Response::from("guest"));
        }
        auth.guard(request, Some("first"))
            .unwrap()
            .set_user(Box::new(Account::from_state(Default::default())));
        let first = Auth::resolvedUser(None).unwrap().unwrap();
        auth.shouldUse(request, "second").unwrap();
        assert!(Auth::resolvedUser(None).unwrap().is_none());
        assert!(Arc::ptr_eq(
            &first,
            &auth.resolvedUser(Some("first")).unwrap()
        ));
        auth.guard(request, Some("second"))
            .unwrap()
            .set_user(Box::new(Account::from_state(Default::default())));
        let second = Auth::resolvedUser(None).unwrap().unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
        assert!(auth.resolvedUser(Some("unknown")).is_none());
        let foreign = request
            .attributes()
            .get("foreign")
            .unwrap()
            .downcast::<Arc<AuthManager>>()
            .unwrap();
        assert!(foreign.resolvedUser(None).is_none());
        let child = auth.clone();
        assert!(
            tokio::spawn(async move { child.resolvedUser(None).is_none() })
                .await
                .unwrap()
        );
        if request.path() == "cancel" {
            std::future::pending::<()>().await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        assert!(Arc::ptr_eq(
            &second,
            &Auth::resolvedUser(None).unwrap().unwrap()
        ));
        assert!(Arc::ptr_eq(
            &first,
            &auth.resolvedUser(Some("first")).unwrap()
        ));
        Ok(openrust::http::Response::from("authenticated"))
    })
}

fn identity_routes(
    _: &Application,
) -> Result<openrust::routing::Router, openrust::routing::RoutingError> {
    let mut routes = openrust::routing::Router::new();
    for path in ["/identity", "/guest", "/cancel"] {
        routes.get(path, cached_identity)?;
    }
    Ok(routes)
}

#[tokio::test]
async fn resolved_identity_is_guard_aware_without_logging_and_cannot_escape_its_request() {
    use openrust::{
        container::ParameterValue,
        contracts::http::Kernel as _,
        foundation::{configuration::Routing, http::Kernel},
        http::{Body, Request, StatusCode},
    };
    let foreign_app = application().await;
    let foreign = foreign_app.container().make::<AuthManager>().unwrap();
    let app = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            Ok([(
                "auth".into(),
                serde_json::json!({
                    "defaults":{"guard":"first"},
                    "guards":{"first":{"driver":"cached"},"second":{"driver":"cached"}}
                }),
            )]
            .into())
        })
        .withRouting(Routing::new(identity_routes))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let auth = kernel
        .get_application()
        .container()
        .make::<AuthManager>()
        .unwrap();
    auth.viaRequest("cached", |_| {
        panic!("cached identity lookup must not invoke a resolver")
    });
    let request = |path: &str| {
        let (parts, body) = http::Request::builder()
            .uri(path)
            .body(Body::empty())
            .unwrap()
            .into_parts();
        let request = Request::from_parts(parts, body);
        request
            .attributes()
            .insert("foreign", ParameterValue::new(foreign.clone()));
        request
    };
    let mut first = request("/identity");
    let mut second = request("/identity");
    let (one, two) = tokio::join!(kernel.handle(&mut first), kernel.handle(&mut second));
    assert_eq!(one.status(), StatusCode::OK);
    assert_eq!(two.status(), StatusCode::OK);
    assert!(auth.resolvedUser(None).is_none());
    let mut cancelled = request("/cancel");
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(20),
            kernel.handle(&mut cancelled)
        )
        .await
        .is_err()
    );
    assert!(auth.resolvedUser(None).is_none());
    let mut guest = request("/guest");
    assert_eq!(kernel.handle(&mut guest).await.status(), StatusCode::OK);
    assert!(auth.resolvedUser(None).is_none());
}
