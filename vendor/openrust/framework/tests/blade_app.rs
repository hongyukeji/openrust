use openrust::{
    class_projection,
    container::{ClassDescriptor, ConstructorParam, ParameterValue, ServiceKey},
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Config,
    support::{ProviderResult, ServiceProvider},
    view::{ViewError, ViewFuture, ViewObject, ViewResult, ViewValue, view},
};
use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc};
struct Greeter {
    name: String,
    token: String,
}
impl Greeter {
    fn new(name: String) -> Self {
        Self {
            name,
            token: format!("{:032x}", fastrand::u128(..)),
        }
    }
}
impl ViewObject for Greeter {
    fn call(&self, method: &str, arguments: &[ViewValue]) -> ViewResult<ViewValue> {
        match (method, arguments) {
            ("label", []) => Ok(self.name.clone().into()),
            ("token", []) => Ok(self.token.clone().into()),
            ("child", []) => Ok(ViewValue::from_object(Arc::new(Self::new(
                "<child>".into(),
            )))),
            ("greet", [ViewValue::String(who)]) => Ok(format!("{}:{who}", self.name).into()),
            ("application", []) => Config::string("app.name")
                .map(Into::into)
                .map_err(|e| ViewError::RenderFailed(Box::new(e))),
            _ => Err(ViewError::RenderFailed(
                std::io::Error::other("服务方法或参数无效。").into(),
            )),
        }
    }
    fn callAsync<'a>(
        &'a self,
        method: &'a str,
        arguments: Vec<ViewValue>,
    ) -> ViewFuture<'a, ViewResult<ViewValue>> {
        Box::pin(async move {
            if method == "afterYield" {
                tokio::task::yield_now().await;
                return self.call("application", &[]);
            }
            self.call(method, &arguments)
        })
    }
}
struct Services;
impl ServiceProvider for Services {
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            openrust::support::facades::Event::listen(
                openrust::foundation::events::LocaleUpdated::NAME,
                Arc::new(FailingLocale),
                openrust::events::Delivery::Sync,
            )?;
            Ok(())
        })
    }

    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container();
            container.registerClass(
                ClassDescriptor::concrete::<Greeter, _>(
                    vec![
                        ConstructorParam::scalar::<String>("name")
                            .withDefault(|| Ok(ParameterValue::new("Default".to_owned()))),
                    ],
                    |args| Ok(Arc::new(Greeter::new((*args.get::<String>(0)?).clone()))),
                )?
                .withProjection(class_projection!(Greeter => dyn ViewObject)),
            )?;
            container.scoped::<Greeter, _>(|c| c.build::<Greeter>())?;
            container.alias(ServiceKey::typed::<Greeter>(), "greeter")?;
            container.alias("greeter", "greeting")?;
            container
                .scopedAsyncNamed::<dyn ViewObject, _>("async", |_| {
                    Box::pin(async {
                        tokio::task::yield_now().await;
                        Ok(Arc::new(Greeter::new("Async".into())) as Arc<dyn ViewObject>)
                    })
                })
                .await?;
            container.instanceNamed("opaque", Arc::new(42_i64))?;
            container.instanceNamed::<dyn ViewObject>(
                "keeper",
                Arc::new(Keeper(std::sync::Mutex::default())),
            )?;
            Ok(())
        })
    }
}
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("blade-app-{:032x}", fastrand::u128(..)));
        fs::create_dir_all(path.join("views/components")).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, source: &str) {
        fs::write(
            self.0.join("views").join(format!("{name}.blade.html")),
            source,
        )
        .unwrap();
    }
    async fn kernel(&self, name: &str) -> Kernel {
        fs::write(self.0.join("name"), name).unwrap();
        let app=Application::configure(self.0.clone()).withConfig(|base,_| { let name=fs::read_to_string(base.join("name")).unwrap(); Ok(BTreeMap::from([
            ("app".into(),serde_json::json!({"name":name,"locale":"en","fallback_locale":"en","debug":true,"env":"testing"})),
            ("view".into(),serde_json::json!({"paths":[base.join("views")],"compiled":base.join("compiled")})),
        ])) }).withProviders(vec![Box::new(Services), Box::new(Deferred)]).withRouting(Routing::new(|_| {
            let mut router=openrust::routing::Router::new();
            router.get("/",render)?; Ok(router)
        })).create().unwrap();
        let mut kernel = Kernel::new(app);
        kernel.bootstrap().await.unwrap();
        kernel
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn render(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Response::view(StatusCode::OK, view("main"), HeaderMap::new(), request).await
    })
}
async fn send(kernel: &Kernel) -> (StatusCode, String) {
    let (parts, body) = axum::extract::Request::builder()
        .uri("/")
        .header("host", "example.test")
        .body(Body::empty())
        .unwrap()
        .into_parts();
    let mut request = Request::from_parts(parts, body);
    let response = kernel.handle(&mut request).await;
    let (parts, body) = response.into_parts();
    (
        parts.status,
        String::from_utf8(
            axum::body::to_bytes(body, 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap(),
    )
}
#[tokio::test]
async fn named_services_aliases_constructor_parameters_and_object_chains_use_the_container() {
    let root = Root::new();
    root.write("main","{{ app('greeter')->label() }}|{{ app('greeting')->greet('<user>') }}|{{ app('greeter', ['name' => 'Override'])->label() }}|{{ app()->makeWith('greeter', ['name'=>'Other'])->label() }}|{{ app('greeter')->label() }}|{{ app('greeter')->child()->label() }}|{{ app('greeter') === app('greeting') }}|{{ app() === app('app') }}|{{ app(null) === app() }}|{{ app('config') === app()->make('config') }}");
    let kernel = root.kernel("A").await;
    assert_eq!(
        send(&kernel).await,
        (
            StatusCode::OK,
            "Default|Default:&lt;user&gt;|Override|Other|Default|&lt;child&gt;|1|1|1|1".into()
        )
    );
}
#[tokio::test]
async fn async_services_and_calls_run_on_the_existing_http_bridge() {
    let root = Root::new();
    root.write("main","{{ app('async')->label() }}|{{ app('async')->afterYield() }}|{{ app('async') === app('async') }}");
    let kernel = root.kernel("<Async App>").await;
    assert_eq!(
        send(&kernel).await,
        (StatusCode::OK, "Async|&lt;Async App&gt;|1".into())
    );
}
#[tokio::test]
async fn objects_survive_includes_components_cached_templates_and_concurrent_apps_without_leaks() {
    let roots = [Root::new(), Root::new()];
    for root in &roots {
        root.write("main","@include('part', ['service'=>app('greeter')])|<x-service :service=\"app('greeter')\" />|{{ app('greeter')->token() }}");
        root.write(
            "part",
            "{{ $service->application() }}:{{ $service->token() }}",
        );
        root.write(
            "components/service",
            "@props(['service']){{ $service->application() }}:{{ $service->token() }}",
        );
    }
    let kernels = [
        Arc::new(roots[0].kernel("Alpha").await),
        Arc::new(roots[1].kernel("Beta").await),
    ];
    let mut tasks = Vec::new();
    for i in 0..16 {
        let kernel = kernels[i % 2].clone();
        tasks.push(tokio::spawn(async move { (i, send(&kernel).await) }));
    }
    let mut tokens = std::collections::HashSet::new();
    for task in tasks {
        let (i, (status, body)) = task.await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        let parts: Vec<_> = body.split('|').collect();
        assert_eq!(parts.len(), 3);
        let name = if i % 2 == 0 { "Alpha" } else { "Beta" };
        assert_eq!(parts[0], format!("{name}:{}", parts[2]));
        assert_eq!(parts[0], parts[1]);
        assert!(tokens.insert(parts[2].to_owned()), "请求作用域实例被复用");
    }
}
#[tokio::test]
async fn invalid_names_types_methods_and_unexposed_services_fail_and_recover() {
    let root = Root::new();
    let kernel = root.kernel("App").await;
    for source in [
        "{{ app(42) }}",
        "{{ app('missing')->label() }}",
        "{{ app('opaque')->label() }}",
        "{{ app('greeter', false)->label() }}",
        "{{ app(null, false)->version() }}",
        "{{ app('greeter',['name'=>false])->label() }}",
        "{{ app('greeter')->unknown() }}",
        "{{ app()->unknown() }}",
        "{{ app('greeter')->greet(false) }}",
        "{{ app('greeter')->label()->unknown() }}",
        "{{ app()->version(1) }}",
        "{{ app()->isLocale() }}",
        "{{ app()->isLocale(false) }}",
        "{{ app('greeter') }}",
        "{{ app('config')->get(false) }}",
    ] {
        root.write("main", source);
        let (status, body) = send(&kernel).await;
        assert_eq!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{source}: {body}"
        );
    }
    root.write("main", "{{ app('greeter')->label() }}");
    assert_eq!(send(&kernel).await, (StatusCode::OK, "Default".into()));
}
#[tokio::test]
async fn template_language_mutation_and_configuration_reads_share_the_actual_request_scope() {
    let root = Root::new();
    root.write("main","{{ app()->getLocale() }}|{{ app()->setLocale('zh-Hans') }}{{ app()->currentLocale() }}|{{ app()->isLocale('zh-Hans') }}|{{ app('config')->get('app.locale') }}");
    let kernel = root.kernel("App").await;
    for _ in 0..2 {
        assert_eq!(
            send(&kernel).await,
            (StatusCode::OK, "en|zh-Hans|1|zh-Hans".into())
        );
    }
}
#[tokio::test]
async fn bare_objects_are_truthy_but_cannot_be_serialized_into_template_cache_or_json() {
    let root = Root::new();
    root.write(
        "main",
        "@if(app()){{ 'yes' }}@endif|@include('part',['instance'=>app()])",
    );
    root.write("part", "{{ $instance->getLocale() }}");
    let kernel = root.kernel("App").await;
    assert_eq!(send(&kernel).await, (StatusCode::OK, "yes|en".into()));
    assert!(
        ViewValue::from_object(Arc::new(Greeter::new("private".into())))
            .to_json()
            .is_err()
    );
}

struct Keeper(std::sync::Mutex<Option<ViewValue>>);
impl ViewObject for Keeper {
    fn call(&self, method: &str, arguments: &[ViewValue]) -> ViewResult<ViewValue> {
        match (method, arguments) {
            ("remember", [value]) => {
                *self.0.lock().unwrap() = Some(value.clone());
                Ok(ViewValue::Null)
            }
            ("recall", []) => Ok(self.0.lock().unwrap().clone().unwrap_or_default()),
            _ => Err(ViewError::InvalidName),
        }
    }
}
#[tokio::test]
async fn objects_retained_by_a_service_cannot_cross_request_boundaries() {
    let root = Root::new();
    root.write("main", "{{ app('keeper')->remember(app('greeter')) }}");
    let kernel = root.kernel("App").await;
    assert_eq!(send(&kernel).await, (StatusCode::OK, String::new()));
    root.write("main", "{{ app('keeper')->recall()->label() }}");
    let (status, body) = send(&kernel).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(body.contains("视图服务对象已离开所属应用或请求作用域"));
    root.write("main", "{{ app('greeter')->label() }}");
    assert_eq!(send(&kernel).await, (StatusCode::OK, "Default".into()));
}

struct Deferred;
impl ServiceProvider for Deferred {
    fn isDeferred(&self) -> bool {
        true
    }
    fn provides(&self) -> Vec<ServiceKey> {
        vec!["deferred-greeting".into()]
    }
    fn deferredFactory(&self) -> Option<Arc<openrust::support::ProviderFactory>> {
        Some(Arc::new(|| Box::new(Self)))
    }
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .singletonNamed::<dyn ViewObject, _>("deferred-greeting", |_| {
                    Ok(Arc::new(Greeter::new("Deferred".into())) as Arc<dyn ViewObject>)
                })?;
            Ok(())
        })
    }
}
#[tokio::test]
async fn deferred_services_and_application_queries_preserve_container_contracts() {
    let root = Root::new();
    root.write("main", "{{ app()->resolved('deferred-greeting') === false }}|{{ app('deferred-greeting')->label() }}|{{ app()->resolved('deferred-greeting') }}|{{ app()->isShared('deferred-greeting') }}|{{ app()->environment(['production','testing']) }}|{{ app()->getFallbackLocale() }}|{{ app()->basePath('/child') }}");
    let kernel = root.kernel("App").await;
    let (status, body) = send(&kernel).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        format!("1|Deferred|1|1|1|en|{}", root.0.join("child").display())
    );
}

struct FailingLocale;
impl openrust::contracts::events::Listener for FailingLocale {
    fn handle<'a>(
        &'a self,
        event: &'a dyn openrust::contracts::events::Event,
        _: &'a mut openrust::events::EventContext<'_>,
    ) -> openrust::events::EventFuture<
        'a,
        openrust::events::EventResult<Option<openrust::events::ListenerResponse>>,
    > {
        Box::pin(async move {
            let event = event
                .downcast_ref::<openrust::foundation::events::LocaleUpdated>()
                .unwrap();
            assert_eq!(Config::string("app.locale").unwrap(), event.locale);
            assert_eq!(
                openrust::support::facades::Lang::getLocale().unwrap(),
                event.locale
            );
            if event.locale == "fr" {
                return Err(openrust::events::EventError::Listener {
                    source: std::io::Error::other("模板语言监听器失败").into(),
                });
            }
            Ok(None)
        })
    }
}
#[tokio::test]
async fn async_locale_events_propagate_errors_after_update_and_next_request_recovers() {
    let root = Root::new();
    root.write("main", "{{ app()->setLocale('fr') }}");
    let kernel = root.kernel("App").await;
    let (status, body) = send(&kernel).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(body.contains("模板语言监听器失败"));
    root.write("main", "{{ app()->currentLocale() }}");
    assert_eq!(send(&kernel).await, (StatusCode::OK, "en".into()));
}

#[tokio::test]
async fn review_empty_environment_patterns_are_not_an_environment_read() {
    let root = Root::new();
    root.write("main", "{{ app()->environment() }}|{{ app()->environment([]) === false }}|{{ app()->environment(['missing']) === false }}|{{ app()->environment('testing') }}");
    let kernel = root.kernel("Review").await;
    assert_eq!(
        send(&kernel).await,
        (StatusCode::OK, "testing|1|1|1".into())
    );
}

#[tokio::test]
async fn review_parenthesized_object_receivers_and_chains_render_normally() {
    let root = Root::new();
    root.write("main", "{{ (app())->currentLocale() }}|{{ ((app('greeter')))->child()->label() }}|{{ (app('config'))->get('app.name') }}|{{ (1 + 2) * 3 }}");
    let kernel = root.kernel("<Review>").await;
    assert_eq!(
        send(&kernel).await,
        (StatusCode::OK, "en|&lt;child&gt;|&lt;Review&gt;|9".into())
    );
}

#[tokio::test]
async fn review_same_service_is_equal_in_both_comparison_forms() {
    let root = Root::new();
    root.write("main", "{{ app() == app() }}|{{ app('greeter') == app('greeting') }}|{{ app('greeter') === app('greeting') }}|{{ app('greeter') != app('greeting') }}|{{ app('greeter') == null }}");
    let kernel = root.kernel("Review").await;
    assert_eq!(send(&kernel).await, (StatusCode::OK, "1|1|1||".into()));
}
