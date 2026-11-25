use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::{HttpFuture, Response, StatusCode},
};
use openrust_webhook_client::{
    Error, RespondsToWebhook, Result, SignatureValidator, VerifiedWebhook, WebhookConfig,
    WebhookConfigRepository, WebhookJobDispatcher, WebhookProcessor, WebhookProfile,
    WebhookRequest, WebhookRuntime, WebhookStore, models::WebhookCall,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
type Trace = Arc<Mutex<Vec<&'static str>>>;
struct Validator(Trace, bool);
impl SignatureValidator for Validator {
    fn is_valid(&self, _: &WebhookRequest, _: &WebhookConfig) -> Result<bool> {
        self.0.lock().unwrap().push("validate");
        Ok(self.1)
    }
}
struct Profile(Trace, bool);
impl WebhookProfile for Profile {
    fn should_process(&self, _: &WebhookRequest, _: &WebhookConfig) -> Result<bool> {
        self.0.lock().unwrap().push("profile");
        Ok(self.1)
    }
}
struct Respond(Trace);
impl RespondsToWebhook for Respond {
    fn respond_to_valid_webhook(&self, _: &WebhookRequest, _: &WebhookConfig) -> Result<Response> {
        self.0.lock().unwrap().push("response");
        Ok(Response::new(StatusCode::ACCEPTED, "accepted".into()))
    }
}
struct Store(Trace);
impl WebhookStore for Store {
    fn store_webhook<'a>(
        &'a self,
        event: &'a VerifiedWebhook<'_>,
    ) -> HttpFuture<'a, Result<WebhookCall>> {
        Box::pin(async move {
            self.0.lock().unwrap().push("store");
            Ok(WebhookCall {
                id: "fixture".into(),
                scope: "tenant".into(),
                name: event.config().name().into(),
                revision: event.config().revision().into(),
                url: event.request().uri().to_string(),
                method: event.request().method().clone(),
                headers: Default::default(),
                raw_body: event.request().body().to_vec(),
                created_at: 1,
                exception: None,
            })
        })
    }
    fn find<'a>(&'a self, _: &'a str) -> HttpFuture<'a, Result<Option<WebhookCall>>> {
        Box::pin(async { Ok(None) })
    }
    fn set_exception<'a>(
        &'a self,
        _: &'a WebhookCall,
        error: Option<&'a Value>,
    ) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(if error.is_some() {
                "exception"
            } else {
                "clear"
            });
            Ok(())
        })
    }
}
struct Job(Trace);
impl WebhookJobDispatcher for Job {
    fn prepare(&self, _: &WebhookCall) -> Result<()> {
        self.0.lock().unwrap().push("job-prepare");
        Ok(())
    }
    fn dispatch<'a>(&'a self, call: &'a WebhookCall) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move {
            assert_eq!(call.name(), "one");
            self.0.lock().unwrap().push("dispatch");
            Ok(())
        })
    }
}
async fn application() -> (tempfile::TempDir, Arc<Application>) {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().to_path_buf())
        .with_providers(vec![Box::new(
            openrust_webhook_client::WebhookClientServiceProvider,
        )])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    (root, Arc::new(app))
}
async fn config(app: &Application, overrides: Value) -> Arc<WebhookConfig> {
    let mut value = json!({"name":"one","signing_secret":null,"signature_validator":"vendor","webhook_profile":"vendor","webhook_response":"vendor","webhook_model":"vendor","process_webhook_job":"vendor"});
    value
        .as_object_mut()
        .unwrap()
        .extend(overrides.as_object().unwrap().clone());
    Arc::new(
        WebhookConfig::from_config(
            &value,
            &app.make::<openrust::config::Environment>().await.unwrap(),
        )
        .unwrap(),
    )
}
fn register(app: &Application, trace: &Trace, valid: bool, process: bool, job_fails: bool) {
    let t = trace.clone();
    WebhookRuntime::register_signature_validator(app.container(), "vendor", move |cfg, _| {
        assert_eq!(cfg.name(), "one");
        t.lock().unwrap().push("validator-created");
        Ok(Arc::new(Validator(t.clone(), valid)))
    })
    .unwrap();
    let t = trace.clone();
    WebhookRuntime::register_webhook_profile(app.container(), "vendor", move |_, _| {
        t.lock().unwrap().push("profile-created");
        Ok(Arc::new(Profile(t.clone(), process)))
    })
    .unwrap();
    let t = trace.clone();
    WebhookRuntime::register_webhook_response(app.container(), "vendor", move |_, _| {
        t.lock().unwrap().push("response-created");
        Ok(Arc::new(Respond(t.clone())))
    })
    .unwrap();
    let t = trace.clone();
    WebhookRuntime::register_webhook_store(app.container(), "vendor", move |_, _| {
        t.lock().unwrap().push("store-created");
        Ok(Arc::new(Store(t.clone())))
    })
    .unwrap();
    let t = trace.clone();
    WebhookRuntime::register_job_dispatcher(app.container(), "vendor", move |_, _| {
        t.lock().unwrap().push("job-created");
        if job_fails {
            Err(Error::InvalidConfig("job factory fixture"))
        } else {
            Ok(Arc::new(Job(t.clone())))
        }
    })
    .unwrap();
}
fn request() -> WebhookRequest {
    WebhookRequest::new(http::Method::POST, Default::default(), b"original".to_vec()).unwrap()
}
#[tokio::test]
async fn typed_factories_preserve_constructor_and_processor_order() {
    let (_root, app) = application().await;
    let trace = Trace::default();
    register(&app, &trace, true, true, false);
    let cfg = config(&app, json!({})).await;
    let runtime = WebhookRuntime::from_application(app, cfg.clone())
        .await
        .unwrap();
    assert_eq!(
        *trace.lock().unwrap(),
        ["validator-created", "profile-created", "response-created"]
    );
    assert_eq!(
        WebhookProcessor::new(&request(), &cfg, &runtime)
            .process()
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(
        *trace.lock().unwrap(),
        [
            "validator-created",
            "profile-created",
            "response-created",
            "validate",
            "profile",
            "store-created",
            "store",
            "job-created",
            "job-prepare",
            "clear",
            "dispatch",
            "response"
        ]
    );
}
#[tokio::test]
async fn valid_ignored_requests_do_not_resolve_an_unknown_storage_or_construct_the_job() {
    let (_root, app) = application().await;
    let trace = Trace::default();
    register(&app, &trace, true, false, false);
    let cfg = config(&app, json!({"webhook_model":"missing"})).await;
    let runtime = WebhookRuntime::from_application(app, cfg.clone())
        .await
        .unwrap();
    assert_eq!(
        WebhookProcessor::new(&request(), &cfg, &runtime)
            .process()
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(
        *trace.lock().unwrap(),
        [
            "validator-created",
            "profile-created",
            "response-created",
            "validate",
            "profile",
            "response"
        ]
    );
}
#[tokio::test]
async fn constructor_failure_is_saved_before_clearing_any_old_exception() {
    let (_root, app) = application().await;
    let trace = Trace::default();
    register(&app, &trace, true, true, true);
    let cfg = config(&app, json!({})).await;
    let runtime = WebhookRuntime::from_application(app, cfg.clone())
        .await
        .unwrap();
    assert!(matches!(
        WebhookProcessor::new(&request(), &cfg, &runtime)
            .process()
            .await,
        Err(Error::InvalidConfig("job factory fixture"))
    ));
    assert_eq!(
        *trace.lock().unwrap(),
        [
            "validator-created",
            "profile-created",
            "response-created",
            "validate",
            "profile",
            "store-created",
            "store",
            "job-created",
            "exception"
        ]
    );
}
#[tokio::test]
async fn invalid_signature_never_constructs_storage_or_a_job() {
    let (_root, app) = application().await;
    let trace = Trace::default();
    register(&app, &trace, false, true, false);
    let cfg = config(&app, json!({"webhook_model":"missing"})).await;
    let runtime = WebhookRuntime::from_application(app, cfg.clone())
        .await
        .unwrap();
    assert!(matches!(
        WebhookProcessor::new(&request(), &cfg, &runtime)
            .process()
            .await,
        Err(Error::InvalidSignature)
    ));
    assert_eq!(
        *trace.lock().unwrap(),
        [
            "validator-created",
            "profile-created",
            "response-created",
            "validate"
        ]
    );
}
#[tokio::test]
async fn factories_are_application_local_and_unknown_or_wrong_typed_bindings_fail() {
    let (_a, first) = application().await;
    let (_b, second) = application().await;
    let trace = Trace::default();
    register(&first, &trace, true, true, false);
    let cfg = config(&first, json!({})).await;
    assert!(
        WebhookRuntime::from_application(first.clone(), cfg.clone())
            .await
            .is_ok()
    );
    assert!(matches!(
        WebhookRuntime::from_application(second, cfg.clone()).await,
        Err(Error::InvalidImplementation {
            field: "signature_validator",
            ..
        })
    ));
    first
        .container()
        .instance_named(
            "webhook-client.signature_validator.vendor",
            Arc::new("wrong-type".to_owned()),
        )
        .unwrap();
    assert!(matches!(
        WebhookRuntime::from_application(first, cfg).await,
        Err(Error::InvalidImplementation {
            field: "signature_validator",
            ..
        })
    ));
}
#[tokio::test]
async fn unknown_job_is_rejected_before_signature_but_missing_store_is_deferred() {
    let (_root, app) = application().await;
    let trace = Trace::default();
    register(&app, &trace, false, true, false);
    let cfg = config(&app, json!({"process_webhook_job":"missing"})).await;
    assert!(matches!(
        WebhookRuntime::from_application(app, cfg).await,
        Err(Error::InvalidImplementation {
            field: "process_webhook_job",
            ..
        })
    ));
    assert_eq!(
        *trace.lock().unwrap(),
        ["validator-created", "profile-created", "response-created"]
    );
}
#[tokio::test]
async fn missing_profile_and_response_bindings_identify_the_config_field() {
    let (_root, app) = application().await;
    let trace = Trace::default();
    register(&app, &trace, true, true, false);
    for (field, expected_trace) in [
        ("webhook_profile", &["validator-created"][..]),
        (
            "webhook_response",
            &["validator-created", "profile-created"][..],
        ),
    ] {
        let cfg = config(&app, json!({(field):"unregistered"})).await;
        let Err(error) = WebhookRuntime::from_application(app.clone(), cfg).await else {
            panic!("未注册的 {field} 应当阻止运行时装配");
        };
        assert!(std::error::Error::source(&error).is_some());
        assert!(
            matches!(error, Error::InvalidImplementation { field: actual, .. } if actual == field)
        );
        let mut actual = trace.lock().unwrap();
        assert_eq!(actual.as_slice(), expected_trace);
        actual.clear();
    }
}
#[test]
fn route_name_mapping_preserves_source_prefix_and_first_dot_rules() {
    let configs = WebhookConfigRepository::new();
    for name in ["one", "one.with.dot", "plain", "shadow"] {
        configs.add_config(WebhookConfig::new(name, "").unwrap());
    }
    for (route, unique, expected) in [
        ("webhook-client-one", false, "one"),
        ("team.webhook-client-one.abcdefgh", true, "one"),
        ("webhook-client-one.with.dot", false, "one.with.dot"),
        ("webhook-client-one.with.dot", true, "one"),
        ("plain", false, "plain"),
        ("webhook-client-shadow.webhook-client-one", true, "shadow"),
    ] {
        assert_eq!(
            configs
                .get_config_for_route(Some(route), unique)
                .unwrap()
                .name(),
            expected
        );
    }
    assert!(
        matches!(configs.get_config_for_route(None,false),Err(Error::ConfigNotFound{name}) if name.is_empty())
    );
}

#[tokio::test]
async fn immutable_runtime_configuration_builders_use_the_same_typed_factories() {
    let (_root, app) = application().await;
    let trace = Trace::default();
    register(&app, &trace, true, false, false);
    let config = WebhookConfig::new("one", "runtime-secret")
        .unwrap()
        .with_signature_validator("vendor")
        .unwrap()
        .with_webhook_profile("vendor")
        .unwrap()
        .with_webhook_response("vendor")
        .unwrap()
        .with_webhook_model("vendor")
        .unwrap()
        .with_process_webhook_job("vendor")
        .unwrap()
        .with_scope("stable-app-tenant")
        .unwrap()
        .with_connection(None)
        .unwrap();
    assert_eq!(config.options()["scope"], "stable-app-tenant");
    assert!(!config.options().to_string().contains("runtime-secret"));
    let config = Arc::new(config);
    let runtime = WebhookRuntime::from_application(app, config.clone())
        .await
        .unwrap();
    assert_eq!(
        WebhookProcessor::new(&request(), &config, &runtime)
            .process()
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    assert!(
        WebhookConfig::new("one", "")
            .unwrap()
            .with_signature_validator("bad name")
            .is_err()
    );
    assert!(
        WebhookConfig::new("one", "")
            .unwrap()
            .with_scope("")
            .is_err()
    );
    assert!(
        WebhookConfig::new("one", "")
            .unwrap()
            .with_connection(Some("bad\nconnection"))
            .is_err()
    );
}

struct ContextValidator;
impl SignatureValidator for ContextValidator {
    fn is_valid(&self, _: &WebhookRequest, _: &WebhookConfig) -> Result<bool> {
        assert_eq!(
            openrust::support::facades::Config::string("fixture.tag").unwrap(),
            "second"
        );
        Ok(true)
    }
}
#[tokio::test]
async fn constructing_and_running_a_second_application_restores_the_first_facade_context() {
    let (_a, first) = application().await;
    let (_b, second) = application().await;
    let trace = Trace::default();
    register(&second, &trace, true, false, false);
    first
        .get::<dyn openrust::contracts::config::Repository>("config")
        .await
        .unwrap()
        .set("fixture.tag", json!("first"))
        .unwrap();
    second
        .get::<dyn openrust::contracts::config::Repository>("config")
        .await
        .unwrap()
        .set("fixture.tag", json!("second"))
        .unwrap();
    WebhookRuntime::register_signature_validator(second.container(), "context", |_, _| {
        assert_eq!(
            openrust::support::facades::Config::string("fixture.tag").unwrap(),
            "second"
        );
        Ok(Arc::new(ContextValidator))
    })
    .unwrap();
    let cfg = config(&second, json!({"signature_validator":"context"})).await;
    first
        .call(
            openrust::container::Callback::new(&[], move |_| {
                let app = second.clone();
                let config = cfg.clone();
                async move {
                    assert_eq!(
                        openrust::support::facades::Config::string("fixture.tag").unwrap(),
                        "first"
                    );
                    let runtime = WebhookRuntime::from_application(app, config.clone())
                        .await
                        .unwrap();
                    assert_eq!(
                        openrust::support::facades::Config::string("fixture.tag").unwrap(),
                        "first"
                    );
                    assert_eq!(
                        WebhookProcessor::new(&request(), &config, &runtime)
                            .process()
                            .await
                            .unwrap()
                            .status(),
                        StatusCode::ACCEPTED
                    );
                    assert_eq!(
                        openrust::support::facades::Config::string("fixture.tag").unwrap(),
                        "first"
                    );
                    Ok(())
                }
            }),
            Default::default(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn required_implementation_fields_are_not_silently_defaulted() {
    let (_root, app) = application().await;
    let trace = Trace::default();
    register(&app, &trace, true, false, false);
    let config = WebhookConfig::new("one", "")
        .unwrap()
        .with_process_webhook_job("vendor")
        .unwrap();
    assert!(matches!(
        WebhookRuntime::from_application(app.clone(), Arc::new(config)).await,
        Err(Error::InvalidConfig("signature_validator"))
    ));
    assert!(trace.lock().unwrap().is_empty());
    let config = WebhookConfig::new("one", "")
        .unwrap()
        .with_signature_validator("vendor")
        .unwrap()
        .with_process_webhook_job("vendor")
        .unwrap();
    assert!(matches!(
        WebhookRuntime::from_application(app.clone(), Arc::new(config)).await,
        Err(Error::InvalidConfig("webhook_profile"))
    ));
    trace.lock().unwrap().clear();
    let config = WebhookConfig::new("one", "")
        .unwrap()
        .with_signature_validator("vendor")
        .unwrap()
        .with_webhook_profile("vendor")
        .unwrap()
        .with_process_webhook_job("vendor")
        .unwrap();
    assert!(matches!(
        WebhookRuntime::from_application(app.clone(), Arc::new(config)).await,
        Err(Error::InvalidConfig("webhook_model"))
    ));
    let config = WebhookConfig::new("one", "")
        .unwrap()
        .with_signature_validator("vendor")
        .unwrap()
        .with_webhook_profile("vendor")
        .unwrap()
        .with_webhook_model("vendor")
        .unwrap()
        .with_process_webhook_job("vendor")
        .unwrap();
    let config = Arc::new(config);
    let runtime = WebhookRuntime::from_application(app, config.clone())
        .await
        .unwrap();
    assert_eq!(
        WebhookProcessor::new(&request(), &config, &runtime)
            .process()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}
#[tokio::test]
async fn ignored_profiles_defer_even_an_empty_model_name_like_the_fixed_source() {
    let (_root, app) = application().await;
    let trace = Trace::default();
    register(&app, &trace, true, false, false);
    let config = config(&app, json!({"webhook_model":""})).await;
    let runtime = WebhookRuntime::from_application(app, config.clone())
        .await
        .unwrap();
    assert_eq!(
        WebhookProcessor::new(&request(), &config, &runtime)
            .process()
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
}
#[tokio::test]
async fn current_route_name_is_none_in_an_artisan_style_context() {
    let (_root, app) = application().await;
    assert_eq!(
        app.call(
            openrust::container::Callback::new(&[], |_| async {
                openrust::support::facades::Route::current_route_name()
            }),
            Default::default()
        )
        .await
        .unwrap(),
        None
    );
}
fn without_provider_routes() -> std::result::Result<(), openrust::routing::RoutingError> {
    use openrust_webhook_client::{WebhookRouteOptions, WebhookRoutes};
    openrust::support::facades::Route::webhooks("/callback", WebhookRouteOptions::default())
}
#[tokio::test]
async fn a_missing_provider_is_rejected_during_route_registration() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().to_path_buf())
        .with_routing(
            openrust::foundation::configuration::Routing::default().api(without_provider_routes),
        )
        .create()
        .unwrap();
    let error = app.boot().await.unwrap_err();
    let mut cursor: Option<&(dyn std::error::Error + 'static)> = Some(&error);
    let mut found = false;
    while let Some(source) = cursor {
        if matches!(
            source.downcast_ref::<Error>(),
            Some(Error::InvalidConfig("WebhookClientServiceProvider"))
        ) {
            found = true;
        }
        cursor = source.source();
    }
    assert!(found, "应当因缺少接收 Provider 而失败：{error}");
}

#[test]
fn invalid_route_method_keeps_the_original_token_and_source() {
    use openrust_webhook_client::{WebhookRouteOptions, WebhookRoutes};

    for method in ["POST", "options", "head"] {
        let error = openrust::support::facades::Route::webhooks(
            "/callback",
            WebhookRouteOptions::default().methods(["post", method]),
        )
        .unwrap_err();
        assert!(matches!(
            std::error::Error::source(&error).and_then(|source| source.downcast_ref::<Error>()),
            Some(Error::InvalidMethod { method: actual }) if actual == method
        ));
    }
}

#[tokio::test]
async fn nullable_response_uses_the_same_default_as_an_omitted_response() {
    let (_root, app) = application().await;
    let trace = Trace::default();
    register(&app, &trace, true, false, false);
    let config = config(&app, json!({"webhook_response":null})).await;
    let runtime = WebhookRuntime::from_application(app, config.clone())
        .await
        .unwrap();
    assert_eq!(
        WebhookProcessor::new(&request(), &config, &runtime)
            .process()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}
