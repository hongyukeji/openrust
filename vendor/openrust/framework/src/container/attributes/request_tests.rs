use super::*;
use crate::{
    Error,
    config::{FrameworkConfig, NoConfig},
    container::{ClassAttribute, ClassDescriptor, ConstructorParam},
    foundation::{ApplicationDefinition, ApplicationState, BootContext, ProviderBuilder},
    http::{
        HttpServiceProvider, Json, RequestInfo,
        axum::{
            body::Body,
            extract::{Request, State},
            http::StatusCode,
            middleware::Next,
            response::Response,
        },
    },
    routing::RouteGroup,
    support::{DefaultProviders, ServiceProvider},
    testing::TestApp,
};
use figment::{Figment, providers::Serialized};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(serde::Serialize)]
struct Probe {
    id: Option<String>,
    trusted: Option<String>,
}

#[tokio::test]
async fn copied_request_cannot_reuse_a_retired_routes_resolver()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    use crate::http::axum::extract::FromRequestParts as _;
    let mut app = TestApp::<ProbeApp>::builder(Figment::from(Serialized::defaults(
        FrameworkConfig::default(),
    )))
    .build()
    .await?;
    let (mut parts, body) = Request::get("/scope/original")
        .body(Body::empty())?
        .into_parts();
    let info = RequestInfo::from_request_parts(&mut parts, &()).await?;
    info.set_route_resolver(Arc::new(|| {
        Ok(Some(crate::routing::Route::new(
            None,
            "trusted",
            None,
            vec!["GET".to_owned()],
            vec![("id".into(), json!("prebound"))],
        )))
    }))?;
    let copied = parts.clone();
    let first = app.request(Request::from_parts(parts, body)).await?;
    first.assert_status(StatusCode::OK)?;
    let first: Value = serde_json::from_slice(first.body())?;
    assert_eq!(first["data"]["http"]["id"], "prebound");

    let second = app
        .request(Request::from_parts(copied, Body::empty()))
        .await?;
    second.assert_status(StatusCode::OK)?;
    let second: Value = serde_json::from_slice(second.body())?;
    assert_eq!(second["data"]["http"]["id"], "original");
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

fn optional(value: &ParameterValue) -> Result<Option<String>> {
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(value.downcast::<String>()?.as_ref().clone()))
}

fn descriptor() -> ClassDescriptor {
    ClassDescriptor::concrete::<Probe>(
        "test::RequestAttributeScopeProbe",
        vec![
            ConstructorParam::scalar::<String>("id")
                .nullable()
                .with_attribute(ClassAttribute::contextual(|| Ok(RouteParameter::default()))),
            ConstructorParam::scalar::<String>("trusted")
                .nullable()
                .with_attribute(ClassAttribute::contextual(|| {
                    Ok(RequestAttribute::new("trusted"))
                })),
        ],
        |args| {
            Ok(Arc::new(Probe {
                id: optional(args.value(0)?)?,
                trusted: optional(args.value(1)?)?,
            }))
        },
    )
}

struct ProbeProvider;
impl ServiceProvider for ProbeProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(|routes| {
                routes.group(
                    RouteGroup::new().middleware(crate::http::axum::middleware::from_fn(attach)),
                    |routes| routes.get("/scope/{id}", probe).get("/stream/{id}", stream),
                )
            })?;
            Ok(())
        })
    }
}

struct ProbeApp;
impl ApplicationDefinition for ProbeApp {
    type Config = NoConfig;

    fn default_providers() -> DefaultProviders {
        DefaultProviders::empty()
    }

    fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
        vec![Box::new(HttpServiceProvider), Box::new(ProbeProvider)]
    }

    fn configure_providers(_: &NoConfig, builder: &mut ProviderBuilder) {
        builder.class_descriptors([descriptor()]);
    }
}

async fn attach(info: RequestInfo, request: Request, next: Next) -> Result<Response> {
    let trusted = info
        .route_parameter("id")?
        .and_then(|value| value.as_str().map(|value| format!("服务端-{value}")))
        .ok_or_else(|| Error::custom("test_route_id_missing", "缺少测试路由参数"))?;
    info.attributes()
        .insert("trusted", ParameterValue::new(trusted))?;
    Ok(next.run(request).await)
}

async fn probe(State(state): State<ApplicationState>) -> Result<Json<Value>> {
    let http = state.resolve::<Probe>().await?;
    let container = state.container();
    let root = container.resolve::<Probe>().await?;
    let scope = container.scope()?;
    let other = scope.resolve::<Probe>().await?;
    Ok(Json(json!({
        "http": &*http,
        "root": &*root,
        "other": &*other,
    })))
}

async fn stream(State(state): State<ApplicationState>) -> Response {
    let frames = futures_util::stream::once(async move {
        let value = state.resolve::<Probe>().await?;
        serde_json::to_vec(&*value).map_err(Error::internal)
    });
    Response::new(Body::from_stream(frames))
}

#[tokio::test]
async fn request_attributes_match_only_the_physical_http_scope_including_body_poll()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut app = TestApp::<ProbeApp>::builder(Figment::from(Serialized::defaults(
        FrameworkConfig::default(),
    )))
    .build()
    .await?;
    let get = |path: &str| {
        Request::get(path)
            .header("trusted", "external")
            .body(Body::empty())
    };
    // HTTP 头必须是合法 ASCII；真实捕获的 Unicode 则从 URI 解码。
    let first = app
        .request(get("/scope/%E6%96%87?trusted=ignored")?)
        .await?;
    first.assert_status(StatusCode::OK)?;
    let body: Value = serde_json::from_slice(first.body())?;
    assert_eq!(
        body["data"]["http"],
        json!({"id":"文","trusted":"服务端-文"})
    );
    for key in ["root", "other"] {
        assert_eq!(body["data"][key], json!({"id":null,"trusted":null}));
    }
    let response = app.request(get("/stream/next")?).await?;
    response.assert_status(StatusCode::OK)?;
    assert_eq!(
        serde_json::from_slice::<Value>(response.body())?,
        json!({
            "id": "next", "trusted":"服务端-next"
        })
    );
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}
