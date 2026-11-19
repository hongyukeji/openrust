use openrust::{
    auth::AuthResult,
    contracts::{auth::Authenticatable, config::Repository},
    http::{HttpError, HttpFuture, Request, Response},
    pipeline::PipelinePipe,
};
use serde_json::Value;
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, Ordering},
};

pub type LoginPipe = PipelinePipe<(), Response, HttpError, Request>;
pub type AuthenticateCallback = dyn for<'a> Fn(&'a mut Request) -> HttpFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>>
    + Send
    + Sync;
pub type ConfirmPasswordCallback = dyn for<'a> Fn(&'a dyn Authenticatable, Option<&'a str>) -> HttpFuture<'a, AuthResult<bool>>
    + Send
    + Sync;
pub type LoginPipelineCallback =
    dyn Fn(&Request) -> Result<Vec<LoginPipe>, HttpError> + Send + Sync;

/// Application-owned Fortify configuration and replaceable callbacks.
pub struct Fortify {
    pub(crate) config: Arc<dyn Repository>,
    authenticate: RwLock<Option<Arc<AuthenticateCallback>>>,
    confirm_password: RwLock<Option<Arc<ConfirmPasswordCallback>>>,
    pipeline: RwLock<Option<Arc<LoginPipelineCallback>>>,
    login_view: RwLock<Option<Arc<crate::http::responses::SimpleViewResponse>>>,
    confirm_password_view: RwLock<Option<Arc<crate::http::responses::SimpleViewResponse>>>,
    registers_routes: AtomicBool,
}
#[allow(
    non_snake_case,
    reason = "Matches Fortify's application configuration API"
)]
impl Fortify {
    pub fn new(config: Arc<dyn Repository>) -> Self {
        Self {
            config,
            authenticate: RwLock::new(None),
            confirm_password: RwLock::new(None),
            pipeline: RwLock::new(None),
            login_view: RwLock::new(None),
            confirm_password_view: RwLock::new(None),
            registers_routes: AtomicBool::new(true),
        }
    }
    pub fn username(&self) -> String {
        self.string("fortify.username", "email")
    }
    pub fn email(&self) -> String {
        self.string("fortify.email", "email")
    }
    pub fn guard(&self) -> Option<String> {
        self.config.get("fortify.guard").and_then(|v| {
            v.as_str()
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
        })
    }
    pub fn redirects(&self, name: &str, default: Option<&str>) -> String {
        self.config
            .get(&format!("fortify.redirects.{name}"))
            .filter(|v| !v.is_null())
            .map(|v| v.as_str().unwrap_or_default().to_owned())
            .or_else(|| default.map(str::to_owned))
            .unwrap_or_else(|| self.string("fortify.home", "/home"))
    }
    pub fn authenticateUsing(&self, callback: Arc<AuthenticateCallback>) {
        *self.authenticate.write().expect("Fortify callbacks lock") = Some(callback);
    }
    pub fn confirmPasswordsUsing(&self, callback: Arc<ConfirmPasswordCallback>) {
        *self
            .confirm_password
            .write()
            .expect("Fortify callbacks lock") = Some(callback);
    }
    pub fn authenticateThrough(&self, callback: Arc<LoginPipelineCallback>) {
        *self.pipeline.write().expect("Fortify callbacks lock") = Some(callback);
    }
    pub fn loginView(&self, view: impl Into<crate::http::responses::SimpleViewResponse>) {
        *self.login_view.write().expect("Fortify views lock") = Some(Arc::new(view.into()));
    }
    pub fn confirmPasswordView(&self, view: impl Into<crate::http::responses::SimpleViewResponse>) {
        *self
            .confirm_password_view
            .write()
            .expect("Fortify views lock") = Some(Arc::new(view.into()));
    }
    pub(crate) fn view(
        &self,
        login: bool,
    ) -> Option<Arc<crate::http::responses::SimpleViewResponse>> {
        if login {
            self.login_view.read().expect("Fortify views lock").clone()
        } else {
            self.confirm_password_view
                .read()
                .expect("Fortify views lock")
                .clone()
        }
    }
    pub fn ignoreRoutes(&self) {
        self.registers_routes.store(false, Ordering::Relaxed);
    }
    pub fn registersRoutes(&self) -> bool {
        self.registers_routes.load(Ordering::Relaxed)
    }
    pub(crate) fn authenticate_callback(&self) -> Option<Arc<AuthenticateCallback>> {
        self.authenticate
            .read()
            .expect("Fortify callbacks lock")
            .clone()
    }
    pub(crate) fn confirm_password_callback(&self) -> Option<Arc<ConfirmPasswordCallback>> {
        self.confirm_password
            .read()
            .expect("Fortify callbacks lock")
            .clone()
    }
    pub(crate) fn pipeline_callback(&self) -> Option<Arc<LoginPipelineCallback>> {
        self.pipeline
            .read()
            .expect("Fortify callbacks lock")
            .clone()
    }
    pub(crate) fn string(&self, key: &str, default: &str) -> String {
        self.config
            .get(key)
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| default.to_owned())
    }
    pub(crate) fn truthy(&self, key: &str) -> bool {
        self.config.get(key).is_some_and(|value| match value {
            Value::Null => false,
            Value::Bool(v) => v,
            Value::String(v) => !v.is_empty() && v != "0",
            Value::Number(v) => v.as_f64() != Some(0.0),
            Value::Array(v) => !v.is_empty(),
            Value::Object(v) => !v.is_empty(),
        })
    }
}
