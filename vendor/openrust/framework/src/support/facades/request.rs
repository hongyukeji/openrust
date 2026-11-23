use super::Facade;
use crate::{
    http::{
        HttpError, HttpFuture, Request as HttpRequest, RequestValue, UploadedFile,
        UploadedFileValue,
    },
    support::{ArrayKey, BackedEnum, Collection, Fluent, InputDate, Stringable},
};
use serde_json::Value;
use std::{net::IpAddr, sync::Arc};

use crate::{contracts::container::BindingResolutionError, http::request_context};

/// 通过安全借用操作当前 HTTP 请求，不复制输入或缓存跨请求状态。
pub struct Request;

impl Request {
    /// 无传输层地址时返回 None；不在 HTTP 请求作用域时返回错误。
    pub fn ip() -> Result<Option<IpAddr>, BindingResolutionError> {
        if request_context::current_request().is_some() {
            return Self::get_facade_root()?.with(|request| request.ip());
        }
        request_context::with_client_ips(|ips| ips.first().copied())
            .ok_or(BindingResolutionError::FacadeNotSet)
    }

    /// 使用与请求实例相同的可信代理解析结果。
    pub fn ips() -> Result<Vec<IpAddr>, BindingResolutionError> {
        if request_context::current_request().is_some() {
            return Self::get_facade_root()?.with(|request| request.ips().to_vec());
        }
        request_context::with_client_ips(<[IpAddr]>::to_vec)
            .ok_or(BindingResolutionError::FacadeNotSet)
    }

    pub fn get_client_ip() -> Result<Option<IpAddr>, BindingResolutionError> {
        Self::ip()
    }

    pub fn get_client_ips() -> Result<Vec<IpAddr>, BindingResolutionError> {
        Self::ips()
    }
}

/// 当前请求的安全访问句柄；不能把可变引用带出一次访问或跨请求使用。
/// 使用 `from_request` 构造测试替身，再通过 `Request::swap` 安装。
pub struct RequestHandle {
    request: Arc<tokio::sync::Mutex<HttpRequest>>,
    identity: Option<request_context::RequestIdentity>,
}
impl RequestHandle {
    pub(crate) fn for_current_request() -> Result<Arc<Self>, BindingResolutionError> {
        let request = request_context::current_request().ok_or_else(|| {
            if request_context::with_client_ips(|_| ()).is_some() {
                BindingResolutionError::FactoryFailed { service: "request", source: std::io::Error::other("当前处理器已借用可变请求：请使用请求实例，或先通过 request.with_container(...) 借出请求再调用门面").into() }
            } else { BindingResolutionError::FacadeNotSet }
        })?;
        let container = super::current_container().ok_or(BindingResolutionError::FacadeNotSet)?;
        let identity = request_context::identity(container.as_ref())
            .ok_or(BindingResolutionError::FacadeNotSet)?;
        Ok(Arc::new(Self {
            request,
            identity: Some(identity),
        }))
    }
    pub fn from_request(request: HttpRequest) -> Self {
        Self {
            request: Arc::new(tokio::sync::Mutex::new(request)),
            identity: None,
        }
    }
    fn borrow(&self) -> Result<tokio::sync::OwnedMutexGuard<HttpRequest>, BindingResolutionError> {
        let source = if self.identity.is_some() {
            request_context::current_request().ok_or(BindingResolutionError::FacadeNotSet)?
        } else {
            self.request.clone()
        };
        let request =
            source
                .try_lock_owned()
                .map_err(|_| BindingResolutionError::FactoryFailed {
                    service: "request",
                    source: std::io::Error::other(
                        "请求正在被独占访问：请在回调内使用收到的请求，勿再次借用 Request 门面",
                    )
                    .into(),
                })?;
        if self
            .identity
            .as_ref()
            .is_some_and(|identity| !identity.matches(&request))
        {
            return Err(BindingResolutionError::FacadeNotSet);
        }
        Ok(request)
    }
    pub fn with<T>(
        &self,
        callback: impl FnOnce(&HttpRequest) -> T,
    ) -> Result<T, BindingResolutionError> {
        let request = self.borrow()?;
        Ok(callback(&request))
    }
    pub fn with_mut<T>(
        &self,
        callback: impl FnOnce(&mut HttpRequest) -> T,
    ) -> Result<T, BindingResolutionError> {
        let mut request = self.borrow()?;
        Ok(callback(&mut request))
    }
    pub async fn with_async<T>(
        &self,
        callback: impl for<'a> FnOnce(&'a mut HttpRequest) -> HttpFuture<'a, Result<T, HttpError>>,
    ) -> Result<T, HttpError> {
        let mut request = self.borrow()?;
        callback(&mut request).await
    }
}
impl Facade for Request {
    type Service = RequestHandle;
    const CACHED: bool = false;
    fn get_facade_accessor() -> crate::container::ServiceKey {
        "request".into()
    }
    fn swap(instance: Arc<RequestHandle>) -> Result<(), BindingResolutionError> {
        let current = Self::get_facade_root()?;
        if Arc::ptr_eq(&current.request, &instance.request)
            || current
                .identity
                .as_ref()
                .zip(instance.identity.as_ref())
                .is_some_and(|(left, right)| left.same_scope(right))
        {
            return Ok(());
        }
        let mut replacement = instance.borrow()?;
        let mut request = current.borrow()?;
        std::mem::swap(&mut *request, &mut *replacement);
        request_context::refresh_metadata(&request);
        Ok(())
    }
}

impl Request {
    pub fn instance() -> Result<Arc<RequestHandle>, BindingResolutionError> {
        Self::get_facade_root()
    }
    // 原生传输层显式提供请求，不读取 PHP 全局环境。
    pub fn capture(request: ::http::Request<crate::http::Body>) -> HttpRequest {
        request.into()
    }
    pub fn create_from_base(request: ::http::Request<crate::http::Body>) -> HttpRequest {
        request.into()
    }
    pub fn method() -> Result<crate::http::Method, HttpError> {
        Ok(Self::instance()?.with(|request| request.method().clone())?)
    }
    pub fn uri() -> Result<crate::support::Uri, HttpError> {
        crate::support::Uri::of(&Self::full_url()?).map_err(|_| HttpError::InvalidRequest)
    }
    pub fn path() -> Result<String, HttpError> {
        Ok(Self::instance()?.with(|request| request.path().to_owned())?)
    }
    pub fn decoded_path() -> Result<String, HttpError> {
        Ok(Self::instance()?.with(|request| request.decoded_path().into_owned())?)
    }
    pub fn segments() -> Result<Vec<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.segments())?)
    }
    pub fn ajax() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.ajax())?)
    }
    pub fn pjax() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.pjax())?)
    }
    pub fn prefetch() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.prefetch())?)
    }
    pub fn secure() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.secure())?)
    }
    pub fn user_agent() -> Result<Option<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.user_agent().map(str::to_owned))?)
    }
    pub fn has_session() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.has_session())?)
    }
    pub fn get_base_url() -> Result<String, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_base_url().to_owned())?)
    }
    pub fn get_path_info() -> Result<String, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_path_info().to_owned())?)
    }
    pub fn get_real_method() -> Result<crate::http::Method, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_real_method().clone())?)
    }
    pub fn get_acceptable_content_types() -> Result<Vec<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_acceptable_content_types().to_vec())?)
    }
    pub fn get_languages() -> Result<Vec<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_languages().to_vec())?)
    }
    pub fn is_attempting_precognition() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.is_attempting_precognition())?)
    }
    pub fn is_precognitive() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.is_precognitive())?)
    }
    pub fn is_json() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.is_json())?)
    }
    pub fn expects_json() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.expects_json())?)
    }
    pub fn wants_json() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.wants_json())?)
    }
    pub fn wants_markdown() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.wants_markdown())?)
    }
    pub fn accepts_any_content_type() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.accepts_any_content_type())?)
    }
    pub fn accepts_json() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.accepts_json())?)
    }
    pub fn accepts_markdown() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.accepts_markdown())?)
    }
    pub fn accepts_html() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.accepts_html())?)
    }
    pub fn root() -> Result<String, HttpError> {
        Self::instance()?.with(|request| request.root())?
    }
    pub fn url() -> Result<String, HttpError> {
        Self::instance()?.with(|request| request.url())?
    }
    pub fn full_url() -> Result<String, HttpError> {
        Self::instance()?.with(|request| request.full_url())?
    }
    pub fn host() -> Result<String, HttpError> {
        Self::instance()?.with(|request| request.host())?
    }
    pub fn http_host() -> Result<String, HttpError> {
        Self::instance()?.with(|request| request.http_host())?
    }
    pub fn scheme_and_http_host() -> Result<String, HttpError> {
        Self::instance()?.with(|request| request.scheme_and_http_host())?
    }
    pub fn fingerprint() -> Result<String, HttpError> {
        Self::instance()?.with(|request| request.fingerprint())?
    }
    pub fn segment(index: usize) -> Result<Option<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.segment(index))?)
    }
    pub fn is(pattern: &str) -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.is(pattern))?)
    }
    pub fn is_method(pattern: &str) -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.is_method(pattern))?)
    }
    pub fn route_is(patterns: &[&str]) -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.route_is(patterns))?)
    }
    pub fn full_url_is(patterns: &[&str]) -> Result<bool, HttpError> {
        Self::instance()?.with(|request| request.full_url_is(patterns))?
    }
    pub fn full_url_with_query(query: &Value) -> Result<String, HttpError> {
        Self::instance()?.with(|request| request.full_url_with_query(query))?
    }
    pub fn full_url_without_query(keys: &[&str]) -> Result<String, HttpError> {
        Self::instance()?.with(|request| request.full_url_without_query(keys))?
    }
    pub fn header(key: &str) -> Result<Option<String>, HttpError> {
        Self::instance()?
            .with(|request| request.header(key).map(|value| value.map(str::to_owned)))?
    }
    pub fn bearer_token() -> Result<Option<String>, HttpError> {
        Self::instance()?
            .with(|request| request.bearer_token().map(|value| value.map(str::to_owned)))?
    }
    pub fn has_header(key: &str) -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.has_header(key))?)
    }
    pub fn has_cookie(key: &str) -> Result<bool, HttpError> {
        Self::instance()?.with(|request| request.has_cookie(key))?
    }
    pub fn query(key: Option<&str>) -> Result<Option<Value>, HttpError> {
        Self::instance()?.with(|request| request.query(key))?
    }
    pub fn cookie(key: Option<&str>) -> Result<Option<Value>, HttpError> {
        Self::instance()?.with(|request| request.cookie(key))?
    }
    pub fn old(key: Option<&str>) -> Result<Option<Value>, HttpError> {
        Ok(Self::instance()?.with(|request| request.old(key).cloned())?)
    }
    pub fn route() -> Result<Option<crate::http::ResolvedRoute>, HttpError> {
        Ok(Self::instance()?.with(|request| request.resolved_route())?)
    }
    pub fn route_parameter(key: &str) -> Result<Option<crate::http::RequestField>, HttpError> {
        Ok(Self::instance()?.with(|request| {
            let route = request.resolved_route()?;
            if let Some(model) = route.models.get(key) {
                return Some(crate::http::RequestField::RouteModel(model.clone()));
            }
            route.parameters.get(key).map(|value| {
                crate::http::RequestField::Value(RequestValue::Value(value.clone().into()))
            })
        })?)
    }
    pub fn accepts(types: &[&str]) -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.accepts(types))?)
    }
    pub fn prefers(types: &[&str]) -> Result<Option<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.prefers(types).map(str::to_owned))?)
    }
    pub fn format(default: &str) -> Result<String, HttpError> {
        Ok(Self::instance()?.with(|request| request.format(default).to_owned())?)
    }
    pub fn matches_type(actual: &str, content_type: &str) -> bool {
        HttpRequest::matches_type(actual, content_type)
    }
    pub async fn input(key: Option<&str>) -> Result<Option<Value>, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.input(key).await
    }
    pub async fn post(key: Option<&str>) -> Result<Option<Value>, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.post(key).await
    }
    pub async fn json(key: Option<&str>) -> Result<Option<Value>, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.json(key).await
    }
    pub async fn keys() -> Result<Vec<ArrayKey>, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.keys().await
    }
    pub async fn all() -> Result<RequestValue, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.all().await
    }
    pub async fn all_files() -> Result<UploadedFileValue, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.all_files().await
    }
    pub async fn flash() -> Result<(), HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.flash().await
    }
    pub async fn has(keys: &[&str]) -> Result<bool, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.has(keys).await
    }
    pub async fn exists(keys: &[&str]) -> Result<bool, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.exists(keys).await
    }
    pub async fn has_any(keys: &[&str]) -> Result<bool, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.has_any(keys).await
    }
    pub async fn missing(keys: &[&str]) -> Result<bool, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.missing(keys).await
    }
    pub async fn filled(keys: &[&str]) -> Result<bool, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.filled(keys).await
    }
    pub async fn is_not_filled(keys: &[&str]) -> Result<bool, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.is_not_filled(keys).await
    }
    pub async fn any_filled(keys: &[&str]) -> Result<bool, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.any_filled(keys).await
    }
    pub async fn only(keys: &[&str]) -> Result<RequestValue, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.only(keys).await
    }
    pub async fn except(keys: &[&str]) -> Result<RequestValue, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.except(keys).await
    }
    pub async fn all_only(keys: &[&str]) -> Result<RequestValue, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.all_only(keys).await
    }
    pub async fn flash_only(keys: &[&str]) -> Result<(), HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.flash_only(keys).await
    }
    pub async fn flash_except(keys: &[&str]) -> Result<(), HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.flash_except(keys).await
    }
    pub async fn fluent(key: Option<&str>) -> Result<Fluent, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.fluent(key).await
    }
    pub async fn array(key: Option<&str>) -> Result<Value, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.array(key).await
    }
    pub async fn collect(key: Option<&str>) -> Result<Collection, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.collect(key).await
    }
    pub async fn fluent_or(key: &str, default: Value) -> Result<Fluent, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.fluent_or(key, default).await
    }
    pub async fn integer(key: &str, default: i64, strict: bool) -> Result<i64, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.integer(key, default, strict).await
    }
    pub async fn float(key: &str, default: f64, strict: bool) -> Result<f64, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.float(key, default, strict).await
    }
    pub async fn boolean(key: &str, default: bool, strict: bool) -> Result<bool, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.boolean(key, default, strict).await
    }
    pub async fn string(key: &str, default: &str, strict: bool) -> Result<Stringable, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.string(key, default, strict).await
    }
    pub async fn str(key: &str, default: &str, strict: bool) -> Result<Stringable, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.str(key, default, strict).await
    }
    pub async fn r#enum<E: BackedEnum>(
        key: &str,
        default: Option<E>,
    ) -> Result<Option<E>, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.r#enum(key, default).await
    }
    pub async fn enums<E: BackedEnum>(key: &str) -> Result<Collection<E>, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.enums(key).await
    }
    pub async fn date(
        key: &str,
        format: Option<&str>,
        timezone: Option<&str>,
    ) -> Result<Option<InputDate>, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.date(key, format, timezone).await
    }
    pub async fn interval(
        key: &str,
        unit: Option<&str>,
    ) -> Result<Option<crate::support::InputInterval>, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.interval(key, unit).await
    }
    pub async fn file(key: &str) -> Result<Option<UploadedFile>, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.file(key).await
    }
    pub async fn file_value(key: &str) -> Result<Option<UploadedFileValue>, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.file_value(key).await
    }
    pub async fn image(key: &str) -> Result<Option<crate::image::Image>, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.image(key).await
    }
    pub async fn has_file(key: &str) -> Result<bool, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.has_file(key).await
    }
    pub async fn merge(input: Value) -> Result<(), HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.merge(input).await.map(|_| ())
    }
    pub async fn merge_if_missing(input: Value) -> Result<(), HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.merge_if_missing(input).await.map(|_| ())
    }
    pub async fn replace(input: Value) -> Result<(), HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.replace(input).await.map(|_| ())
    }
    pub async fn body_bytes() -> Result<axum::body::Bytes, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.body_bytes().await
    }
    pub fn flush() -> Result<(), HttpError> {
        Self::instance()?.with_mut(|request| request.flush())?
    }
    pub async fn user(
        guard: Option<&str>,
    ) -> crate::auth::AuthResult<Option<Arc<dyn crate::contracts::auth::Authenticatable>>> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.user(guard).await
    }
    pub fn get_preferred_language(locales: Option<&[&str]>) -> Result<Option<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_preferred_language(locales))??)
    }
    pub fn filter_precognitive_rules(
        rules: crate::validation::RuleSet,
    ) -> Result<crate::validation::RuleSet, HttpError> {
        Ok(Self::instance()?.with(|request| request.filter_precognitive_rules(rules))??)
    }
    pub fn with_session<T>(
        callback: impl FnOnce(&mut crate::session::Store) -> T,
    ) -> Result<T, HttpError> {
        Self::instance()?.with_mut(|request| Ok(callback(request.session_mut()?)))?
    }
}

impl Request {
    pub fn get_script_name() -> Result<String, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_script_name().to_owned())?)
    }
    pub fn get_base_path() -> Result<String, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_base_path().to_owned())?)
    }
    pub fn get_scheme() -> Result<String, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_scheme().to_owned())?)
    }
    pub fn get_request_uri() -> Result<String, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_request_uri().to_owned())?)
    }
    pub fn get_protocol_version() -> Result<String, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_protocol_version().to_owned())?)
    }
    pub fn get_locale() -> Result<String, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_locale().to_owned())?)
    }
    pub fn get_default_locale() -> Result<String, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_default_locale().to_owned())?)
    }
    pub fn get_user() -> Result<Option<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_user())?)
    }
    pub fn get_password() -> Result<Option<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_password())?)
    }
    pub fn get_user_info() -> Result<Option<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_user_info())?)
    }
    pub fn get_e_tags() -> Result<Vec<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_e_tags())?)
    }
    pub fn get_charsets() -> Result<Vec<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_charsets())?)
    }
    pub fn get_encodings() -> Result<Vec<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_encodings())?)
    }
    pub fn get_content_type_format() -> Result<Option<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_content_type_format())?)
    }
    pub fn has_previous_session() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.has_previous_session())?)
    }
    pub fn is_no_cache() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.is_no_cache())?)
    }
    pub fn is_method_safe() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.is_method_safe())?)
    }
    pub fn is_method_idempotent() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.is_method_idempotent())?)
    }
    pub fn is_method_cacheable() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.is_method_cacheable())?)
    }
    pub fn prefer_safe_content() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.prefer_safe_content())?)
    }
    pub fn is_from_trusted_proxy() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.is_from_trusted_proxy())?)
    }
    pub fn is_xml_http_request() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| request.is_xml_http_request())?)
    }
    pub fn get_host() -> Result<String, HttpError> {
        Self::instance()?.with(|request| request.get_host())?
    }
    pub fn get_http_host() -> Result<String, HttpError> {
        Self::instance()?.with(|request| request.get_http_host())?
    }
    pub fn get_scheme_and_http_host() -> Result<String, HttpError> {
        Self::instance()?.with(|request| request.get_scheme_and_http_host())?
    }
    pub fn get_uri() -> Result<String, HttpError> {
        Self::instance()?.with(|request| request.get_uri())?
    }
    pub fn get_query_string() -> Result<Option<String>, HttpError> {
        Self::instance()?.with(|request| request.get_query_string())?
    }
    pub fn get_port() -> Result<u16, HttpError> {
        Self::instance()?.with(|request| request.get_port())?
    }
    pub fn get_uri_for_path(path: &str) -> Result<String, HttpError> {
        Self::instance()?.with(|request| request.get_uri_for_path(path))?
    }
    pub fn get_relative_uri_for_path(path: &str) -> Result<String, HttpError> {
        Self::instance()?.with(|request| request.get_relative_uri_for_path(path))?
    }
    pub fn set_locale(locale: &str) -> Result<(), HttpError> {
        Ok(Self::instance()?.with_mut(|request| request.set_locale(locale))??)
    }
    pub fn set_default_locale(locale: &str) -> Result<(), HttpError> {
        Ok(Self::instance()?.with_mut(|request| request.set_default_locale(locale))??)
    }
    pub fn set_request_locale(locale: &str) -> Result<(), HttpError> {
        Ok(Self::instance()?.with_mut(|request| request.set_request_locale(locale))??)
    }
    pub fn set_default_request_locale(locale: &str) -> Result<(), HttpError> {
        Ok(Self::instance()?.with_mut(|request| request.set_default_request_locale(locale))??)
    }
    pub fn set_method(method: crate::http::Method) -> Result<(), HttpError> {
        Ok(Self::instance()?.with_mut(|request| request.set_method(method))?)
    }
    pub fn get_method() -> Result<crate::http::Method, HttpError> {
        Self::method()
    }
    pub fn is_secure() -> Result<bool, HttpError> {
        Self::secure()
    }
    pub fn get_request_format(default: Option<&str>) -> Result<Option<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_request_format(default))?)
    }
    pub fn get_preferred_format(default: Option<&str>) -> Result<Option<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_preferred_format(default))?)
    }
    pub fn get_mime_types(format: &str) -> Result<Vec<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_mime_types(format))?)
    }
    pub fn get_mime_type(format: &str) -> Result<Option<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_mime_type(format))?)
    }
    pub fn get_format(
        mime_type: &str,
        subtype_fallback: bool,
    ) -> Result<Option<String>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_format(mime_type, subtype_fallback))?)
    }
    pub fn set_format(format: &str, mime_types: Vec<String>) -> Result<(), HttpError> {
        Ok(Self::instance()?.with_mut(|request| request.set_format(format, mime_types))?)
    }
    pub fn set_request_format(format: Option<&str>) -> Result<(), HttpError> {
        Ok(Self::instance()?.with_mut(|request| request.set_request_format(format))?)
    }
    pub fn set_json(json: Value) -> Result<(), HttpError> {
        Self::instance()?.with_mut(|request| request.set_json(json))?
    }
    pub fn set_user_resolver(resolver: crate::http::UserResolver) -> Result<(), HttpError> {
        Ok(Self::instance()?.with_mut(|request| request.set_user_resolver(resolver))?)
    }
    pub fn get_user_resolver() -> Result<Option<crate::http::UserResolver>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_user_resolver())?)
    }
    pub fn set_route_resolver(resolver: crate::http::RouteResolver) -> Result<(), HttpError> {
        Ok(Self::instance()?.with_mut(|request| request.set_route_resolver(resolver))?)
    }
    pub fn get_route_resolver() -> Result<Option<crate::http::RouteResolver>, HttpError> {
        Ok(Self::instance()?.with(|request| request.get_route_resolver())?)
    }
    pub fn set_laravel_session(session: crate::session::Store) -> Result<(), HttpError> {
        Ok(Self::instance()?.with_mut(|request| request.set_laravel_session(session))?)
    }
    pub fn set_session(session: crate::session::Store) -> Result<(), HttpError> {
        Ok(Self::instance()?.with_mut(|request| request.set_session(session))?)
    }
    pub fn server(key: Option<&str>) -> Result<Value, HttpError> {
        Ok(Self::instance()?.with(|request| request.server(key))?)
    }

    pub fn normalize_query_string(query: &str) -> Result<String, HttpError> {
        HttpRequest::normalize_query_string(query)
    }
    pub async fn to_array() -> Result<RequestValue, HttpError> {
        Self::all().await
    }
    pub async fn get_payload() -> Result<Value, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        request.get_payload().await
    }
    pub async fn get_content() -> Result<axum::body::Bytes, HttpError> {
        Self::body_bytes().await
    }
    pub fn session<T>(
        callback: impl FnOnce(&mut crate::session::Store) -> T,
    ) -> Result<T, HttpError> {
        Self::with_session(callback)
    }
    pub fn get_session<T>(
        callback: impl FnOnce(&mut crate::session::Store) -> T,
    ) -> Result<T, HttpError> {
        Self::with_session(callback)
    }
    pub async fn clamp(key: &str, min: f64, max: f64, default: f64) -> Result<f64, HttpError> {
        if min > max || !min.is_finite() || !max.is_finite() {
            return Err(HttpError::InvalidRequest);
        }
        Ok(Self::float(key, default, false).await?.clamp(min, max))
    }
    pub async fn when_has<T>(
        key: &str,
        callback: impl AsyncFnOnce(Option<RequestValue>) -> T,
    ) -> Result<Option<T>, HttpError> {
        let value = {
            let root = Self::instance()?;
            let mut request = root.borrow()?;
            let values = request.all().await?;
            if values.get(key).is_none() {
                return Ok(None);
            }
            values.get(key).cloned()
        };
        Ok(Some(callback(value).await))
    }
    pub async fn when_filled<T>(
        key: &str,
        callback: impl AsyncFnOnce(Option<RequestValue>) -> T,
    ) -> Result<Option<T>, HttpError> {
        if !Self::filled(&[key]).await? {
            return Ok(None);
        }
        Self::when_has(key, callback).await
    }
    pub async fn when_missing<T>(
        key: &str,
        callback: impl AsyncFnOnce() -> T,
    ) -> Result<Option<T>, HttpError> {
        if Self::missing(&[key]).await? {
            Ok(Some(callback().await))
        } else {
            Ok(None)
        }
    }
    pub async fn when_enum<E: BackedEnum, T>(
        key: &str,
        callback: impl AsyncFnOnce(E) -> T,
    ) -> Result<Option<T>, HttpError> {
        if !Self::filled(&[key]).await? {
            return Ok(None);
        }
        match Self::r#enum::<E>(key, None).await? {
            Some(value) => Ok(Some(callback(value).await)),
            None => Ok(None),
        }
    }
    pub fn when<T>(condition: bool, callback: impl FnOnce() -> T) -> Option<T> {
        condition.then(callback)
    }
    pub fn unless<T>(condition: bool, callback: impl FnOnce() -> T) -> Option<T> {
        Self::when(!condition, callback)
    }
    pub async fn validate(
        rules: crate::validation::RuleSet,
    ) -> Result<crate::validation::ValidatedInput, HttpError> {
        Self::validate_with_bag("default", rules).await
    }
    pub async fn validate_with_bag(
        bag: &str,
        rules: crate::validation::RuleSet,
    ) -> Result<crate::validation::ValidatedInput, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        let data = crate::validation::Data::from_request(&mut request).await?;
        let mut validator = crate::validation::facade::Validator::make(data, rules)?;
        validator
            .validate_request(&mut request)
            .await
            .map_err(|mut error| {
                if let crate::validation::ValidationError::Invalid(exception) = &mut error {
                    exception.error_bag(bag);
                }
                HttpError::from(error)
            })
    }
    pub fn has_valid_signature(absolute: bool) -> Result<bool, HttpError> {
        Self::has_valid_signature_while_ignoring(&[], absolute)
    }
    pub fn has_valid_relative_signature() -> Result<bool, HttpError> {
        Self::has_valid_signature(false)
    }
    pub fn has_valid_relative_signature_while_ignoring(ignore: &[&str]) -> Result<bool, HttpError> {
        Self::has_valid_signature_while_ignoring(ignore, false)
    }
    pub fn has_valid_signature_while_ignoring(
        ignore: &[&str],
        absolute: bool,
    ) -> Result<bool, HttpError> {
        Self::instance()?
            .with(|request| {
                super::Url::has_valid_signature(
                    request,
                    if absolute {
                        crate::routing::SignatureMode::Absolute
                    } else {
                        crate::routing::SignatureMode::Relative
                    },
                    crate::routing::SignatureQueryFilter::Keys(ignore),
                )
            })?
            .map_err(|source| HttpError::Internal {
                source: source.into(),
            })
    }
    pub async fn dump(keys: &[&str]) -> Result<RequestValue, HttpError> {
        let value = if keys.is_empty() {
            Self::all().await?
        } else {
            Self::only(keys).await?
        };
        eprintln!(
            "{}",
            crate::validation::Data::from_request_value(&value)?.as_value()
        );
        Ok(value)
    }
    pub async fn dd(keys: &[&str]) -> Result<(), HttpError> {
        let value = Self::dump(keys).await?;
        Err(HttpError::DumpAndDie(
            crate::validation::Data::from_request_value(&value)?
                .as_value()
                .to_string(),
        ))
    }
}

impl Request {
    fn macros() -> Result<Arc<crate::http::RequestMacros>, BindingResolutionError> {
        use crate::contracts::container::Container as _;
        super::current_container()
            .ok_or(BindingResolutionError::FacadeNotSet)?
            .get::<crate::http::RequestMacros>(crate::container::ServiceKey::typed::<
                crate::http::RequestMacros,
            >())
    }
    pub fn r#macro<Fut>(
        name: impl Into<String>,
        callback: impl Fn(Vec<Value>) -> Fut + Send + Sync + 'static,
    ) -> Result<(), BindingResolutionError>
    where
        Fut: std::future::Future<Output = Result<Value, HttpError>> + Send + 'static,
    {
        Self::mixin([(name, crate::http::RequestMacro::new(callback))], true)
    }
    pub fn mixin<K: Into<String>>(
        macros: impl IntoIterator<Item = (K, crate::http::RequestMacro)>,
        replace: bool,
    ) -> Result<(), BindingResolutionError> {
        Self::macros()?.mixin(macros, replace);
        Ok(())
    }
    pub fn has_macro(name: &str) -> Result<bool, BindingResolutionError> {
        Ok(Self::macros()?.has(name))
    }
    pub fn flush_macros() -> Result<(), BindingResolutionError> {
        Self::macros()?.clear();
        Ok(())
    }
    pub async fn call(name: &str, arguments: Vec<Value>) -> Result<Value, HttpError> {
        Self::macros()?.call(name, arguments).await
    }
}

impl Request {
    /// 转移已有请求的所有权，保留已解析输入、上传和 Session；不复制独占资源。
    pub fn create_from(request: HttpRequest) -> HttpRequest {
        request
    }
    /// 构造原生请求，查询、Cookie、服务端元数据由传输层显式提供。
    pub fn create(request: ::http::Request<crate::http::Body>) -> HttpRequest {
        request.into()
    }
    /// 导出独立的传输快照。会话、路由模型及中间件状态仍归原始请求所有。
    pub async fn duplicate() -> Result<::http::Request<axum::body::Bytes>, HttpError> {
        let root = Self::instance()?;
        let mut request = root.borrow()?;
        let mut snapshot = request.to_http_request().await?;
        snapshot.extensions_mut().clear();
        if let Some(peer) = request
            .extensions()
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        {
            snapshot.extensions_mut().insert(*peer);
        }
        if let Some(security) = request.extensions().get::<crate::http::RequestSecurity>() {
            snapshot.extensions_mut().insert(*security);
        }
        Ok(snapshot)
    }
    /// 替换当前独占借用的请求，而不是修改进程全局变量。
    pub fn initialize(
        request: ::http::Request<crate::http::Body>,
    ) -> Result<(), BindingResolutionError> {
        Self::swap(Arc::new(RequestHandle::from_request(request.into())))
    }
}

impl Request {
    pub fn get_http_method_parameter_override() -> Result<bool, HttpError> {
        Ok(Self::instance()?.with(|request| {
            request
                .extensions()
                .get::<crate::http::MethodOverride>()
                .cloned()
                .unwrap_or_default()
                .parameter
        })?)
    }
    pub fn get_allowed_http_method_override() -> Result<Option<Vec<crate::http::Method>>, HttpError>
    {
        Ok(Self::instance()?.with(|request| {
            request
                .extensions()
                .get::<crate::http::MethodOverride>()
                .and_then(|policy| policy.allowed().map(<[crate::http::Method]>::to_vec))
        })?)
    }
}

impl Request {
    pub fn headers() -> Result<crate::http::HeaderMap, HttpError> {
        Ok(Self::instance()?.with(|request| request.headers().clone())?)
    }
}
