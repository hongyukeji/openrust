use crate::{Error, Result, http::controllers::WebhookController};
use openrust::{
    contracts::{config::Repository, container::Container as _, foundation::Application as _},
    http::Method,
    routing::{Route, Router, RoutingError},
    support::{
        Str,
        facades::{App, Config, Facade, Route as FacadeRoute},
    },
};

/// Rust 不提供缺省参数；默认值与上游宏的 default／post 一致。
pub struct WebhookRouteOptions {
    name: String,
    methods: Vec<String>,
}
impl Default for WebhookRouteOptions {
    fn default() -> Self {
        Self {
            name: "default".into(),
            methods: vec!["post".into()],
        }
    }
}
impl WebhookRouteOptions {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
    pub fn methods(mut self, methods: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.methods = methods.into_iter().map(Into::into).collect();
        self
    }
}
pub trait WebhookRoutes {
    fn webhooks(uri: &str, options: WebhookRouteOptions) -> std::result::Result<(), RoutingError>;
    fn webhooks_with(
        uri: &str,
        options: WebhookRouteOptions,
        configure: impl FnOnce(&mut Route) -> std::result::Result<(), RoutingError>,
    ) -> std::result::Result<(), RoutingError>;
}
pub trait WebhookRouter {
    fn webhooks(
        &mut self,
        uri: &str,
        options: WebhookRouteOptions,
    ) -> std::result::Result<&mut Route, RoutingError>;
}
pub(crate) fn unique_token(config: &dyn Repository) -> Result<bool> {
    match config.get("webhook-client.add_unique_token_to_route_name") {
        None | Some(serde_json::Value::Null) => Ok(false),
        Some(serde_json::Value::Bool(value)) => Ok(value),
        _ => Err(Error::InvalidConfig("add_unique_token_to_route_name")),
    }
}
fn prepare(options: WebhookRouteOptions) -> Result<(Vec<Method>, String)> {
    let mut methods = Vec::with_capacity(options.methods.len());
    for method in options.methods {
        // 固定源码确实包含 QUERY；不擅自扩大为 HEAD、OPTIONS 或接受大写注册参数。
        methods.push(match method.as_str() {
            "get" => Method::GET,
            "post" => Method::POST,
            "put" => Method::PUT,
            "patch" => Method::PATCH,
            "delete" => Method::DELETE,
            "query" => Method::from_bytes(b"QUERY").expect("有效的 QUERY 方法"),
            _ => return Err(Error::InvalidMethod { method }),
        });
    }
    if options.name.is_empty()
        || options.name.len() > 128
        || options.name.chars().any(char::is_control)
    {
        return Err(Error::InvalidConfig("name"));
    }
    let app = App::get_facade_root().map_err(Error::binding)?;
    if !app
        .container()
        .has(openrust::container::ServiceKey::typed::<
            crate::WebhookConfigRepository,
        >())
    {
        return Err(Error::InvalidConfig("WebhookClientServiceProvider"));
    }
    let config = Config::get_facade_root().map_err(Error::binding)?;
    let suffix = if unique_token(config.as_ref())? {
        format!(".{}", Str::random(8))
    } else {
        String::new()
    };
    Ok((methods, format!("webhook-client-{}{suffix}", options.name)))
}
fn registration(source: Error) -> RoutingError {
    RoutingError::ExtensionRegistration {
        source: Box::new(source),
    }
}
impl WebhookRoutes for FacadeRoute {
    #[track_caller]
    fn webhooks(uri: &str, options: WebhookRouteOptions) -> std::result::Result<(), RoutingError> {
        Self::webhooks_with(uri, options, |_| Ok(()))
    }
    #[track_caller]
    fn webhooks_with(
        uri: &str,
        options: WebhookRouteOptions,
        configure: impl FnOnce(&mut Route) -> std::result::Result<(), RoutingError>,
    ) -> std::result::Result<(), RoutingError> {
        let (methods, name) = prepare(options).map_err(registration)?;
        Self::match_with(&methods, uri, WebhookController, |route| {
            route.name(&name).set_action(
                "openrust_webhook_client::http::controllers::WebhookController::invoke",
            )?;
            configure(route)
        })
    }
}
impl WebhookRouter for Router {
    #[track_caller]
    fn webhooks(
        &mut self,
        uri: &str,
        options: WebhookRouteOptions,
    ) -> std::result::Result<&mut Route, RoutingError> {
        let (methods, name) = prepare(options).map_err(registration)?;
        let route = self.r#match(&methods, uri, WebhookController)?;
        route
            .name(&name)
            .set_action("openrust_webhook_client::http::controllers::WebhookController::invoke")?;
        Ok(route)
    }
}
