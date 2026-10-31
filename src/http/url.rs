//! 命名路由与控制器动作的共享 URL 生成。

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::core::{Error, Result};
use crate::http::domain::DomainPattern;
use crate::http::routes::{ActionRouteDefinition, NamedRouteDefinition};
use axum::extract::FromRequestParts;
use axum::http::{StatusCode, request::Parts};

#[derive(Clone)]
struct NamedRoute {
    path: String,
    domain: Option<String>,
}

/// 根据 Laravel 式命名路由或动作身份生成绝对 URL。
///
/// 在 HTTP handler 中可直接提取。域参数按单个 DNS 标签校验，路径参数按路径段编码；
/// 未用于域或路径的参数成为 query 参数。scheme 和端口来自 `app.url`；
/// host 使用已登记的域模板，无域模板时来自 `app.url`，不采信当前请求的 `Host`。
#[derive(Clone)]
pub struct RouteUrlGenerator {
    inner: Arc<RouteUrlInner>,
    root_controller_namespace: Option<String>,
}

struct RouteUrlInner {
    base_url: Option<::url::Url>,
    routes: BTreeMap<String, NamedRoute>,
    actions: BTreeMap<String, NamedRoute>,
}

impl RouteUrlGenerator {
    pub(crate) fn empty() -> Self {
        Self {
            inner: Arc::new(RouteUrlInner {
                base_url: None,
                routes: BTreeMap::new(),
                actions: BTreeMap::new(),
            }),
            root_controller_namespace: None,
        }
    }

    pub(crate) fn new(base_url: &str, routes: Vec<NamedRouteDefinition>) -> Result<Self> {
        Self::new_with_actions(base_url, routes, Vec::new())
    }

    pub(crate) fn new_with_actions(
        base_url: &str,
        routes: Vec<NamedRouteDefinition>,
        actions: Vec<ActionRouteDefinition>,
    ) -> Result<Self> {
        let base_url = ::url::Url::parse(base_url).map_err(|_| {
            Error::custom("url_base_invalid", "app.url must be an absolute HTTP URL")
        })?;
        if !matches!(base_url.scheme(), "http" | "https")
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(Error::custom(
                "url_base_invalid",
                "app.url must use HTTP or HTTPS without credentials, query or fragment",
            ));
        }

        let mut named = BTreeMap::new();
        for NamedRouteDefinition { name, path, domain } in routes {
            if name.trim().is_empty() || name.chars().any(char::is_whitespace) {
                return Err(Error::custom(
                    "http_route_name_invalid",
                    "route names must be non-empty and contain no whitespace",
                ));
            }
            validate_url_domain(&path, domain.as_deref())?;
            if named
                .insert(
                    name.clone(),
                    NamedRoute {
                        path: path.clone(),
                        domain,
                    },
                )
                .is_some()
            {
                return Err(Error::custom(
                    "http_route_name_duplicate",
                    "route names must be unique",
                ));
            }
            validate_url_path(&path)?;
        }

        let mut indexed = BTreeMap::new();
        for ActionRouteDefinition {
            action,
            path,
            domain,
        } in actions
        {
            crate::http::action::validate_identity(&action)?;
            validate_url_domain(&path, domain.as_deref())?;
            validate_url_path(&path)?;
            // 上游用原始 controller 检查存在，却按 trim 后的键保存。
            // 因此带首尾反斜线的后项可覆盖同一规范键，不能简化为 entry.or_insert。
            if !indexed.contains_key(&action) {
                indexed.insert(
                    action.trim_matches('\\').to_owned(),
                    NamedRoute { path, domain },
                );
            }
        }

        Ok(Self {
            inner: Arc::new(RouteUrlInner {
                base_url: Some(base_url),
                routes: named,
                actions: indexed,
            }),
            root_controller_namespace: None,
        })
    }

    /// 按名称生成 URL；缺失的域或路径参数报错，额外参数按字典序加入 query。
    pub fn route(&self, name: &str, parameters: &BTreeMap<String, String>) -> Result<String> {
        let route = self.inner.routes.get(name).ok_or_else(|| {
            generation_error(
                "http_named_route_not_found",
                "named route was not registered",
            )
        })?;

        self.render(route, parameters)
    }

    /// 按动作身份生成绝对 URL；参数、域编码及错误与命名路由共用同一内核。
    ///
    /// 无首反斜线的字符串使用本实例根命名空间；绝对字符串和二元组绕过它。
    /// 裸 invokable 身份不会自动追加 `@__invoke`，也不实例化控制器。
    pub fn action(
        &self,
        action: impl Into<crate::http::RouteAction>,
        parameters: &BTreeMap<String, String>,
    ) -> Result<String> {
        let action = action
            .into()
            .query(self.root_controller_namespace.as_deref())?;
        let route = self.inner.actions.get(&action).ok_or_else(|| {
            generation_error(
                "http_route_action_not_found",
                format!("动作 `{action}` 未登记"),
            )
        })?;
        self.render(route, parameters)
    }

    /// 设置本实例查询动作的根命名空间；空字符串清空，原样保留其余合法输入。
    ///
    /// 已有 clone 与请求快照不受后续修改影响；不可变路由索引仍共享同一 Arc。
    /// 这是 Rust 所有权适配，不模拟 PHP 跨引用共享可变对象。
    pub fn set_root_controller_namespace(
        &mut self,
        namespace: impl Into<String>,
    ) -> Result<&mut Self> {
        let namespace = namespace.into();
        crate::http::action::validate_namespace(&namespace)?;
        self.root_controller_namespace = (!namespace.is_empty()).then_some(namespace);
        Ok(self)
    }

    fn render(&self, route: &NamedRoute, parameters: &BTreeMap<String, String>) -> Result<String> {
        let mut remaining = parameters.clone();
        let mut url = self.inner.base_url.clone().ok_or_else(|| {
            Error::custom(
                "url_base_invalid",
                "app.url is unavailable for named routes",
            )
        })?;
        if let Some(domain) = &route.domain {
            let mut generated = Vec::new();
            for label in domain.split('.') {
                if let Some(parameter) = label
                    .strip_prefix('{')
                    .and_then(|part| part.strip_suffix('}'))
                {
                    let value = remaining.remove(parameter).ok_or_else(|| {
                        generation_error(
                            "http_route_parameter_missing",
                            format!("required route parameter `{parameter}` is missing"),
                        )
                    })?;
                    if !valid_domain_label(&value) {
                        return Err(generation_error(
                            "http_route_parameter_invalid",
                            format!("domain route parameter `{parameter}` must be one DNS label"),
                        ));
                    }
                    generated.push(value.to_ascii_lowercase());
                } else {
                    generated.push(label.to_owned());
                }
            }
            let host = generated.join(".");
            if DomainPattern::parse(&host).is_err() {
                return Err(generation_error(
                    "http_route_parameter_invalid",
                    "domain route parameter is not a valid DNS label",
                ));
            }
            url.set_host(Some(&host)).map_err(|_| {
                generation_error(
                    "http_route_parameter_invalid",
                    "domain route host is invalid",
                )
            })?;
        }
        let mut path_segments = url.path_segments_mut().map_err(|()| {
            Error::custom(
                "url_base_invalid",
                "app.url cannot be used as a hierarchical URL",
            )
        })?;
        path_segments.pop_if_empty();
        for segment in route
            .path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
        {
            if let Some(parameter) = route_parameter(segment) {
                let value = remaining.remove(parameter).ok_or_else(|| {
                    generation_error(
                        "http_route_parameter_missing",
                        format!("required route parameter `{parameter}` is missing"),
                    )
                })?;
                if matches!(value.as_str(), "." | "..") {
                    return Err(generation_error(
                        "http_route_parameter_invalid",
                        format!("route parameter `{parameter}` is not a valid path segment"),
                    ));
                }
                path_segments.push(&value);
            } else if segment.contains('{') || segment.contains('}') {
                return Err(generation_error(
                    "http_route_pattern_unsupported",
                    "named route parameters must occupy a complete path segment",
                ));
            } else {
                path_segments.push(segment);
            }
        }
        if route.path.ends_with('/') && route.path != "/" {
            path_segments.push("");
        }
        drop(path_segments);

        if !remaining.is_empty() {
            let mut query = url.query_pairs_mut();
            for (key, value) in remaining {
                query.append_pair(&key, &value);
            }
        }
        Ok(url.to_string())
    }
}

fn validate_url_domain(path: &str, domain: Option<&str>) -> Result<()> {
    if let Some(pattern) = domain {
        let pattern = DomainPattern::parse(pattern)?;
        for label in pattern.as_str().split('.') {
            if let Some(parameter) = label
                .strip_prefix('{')
                .and_then(|part| part.strip_suffix('}'))
                && path
                    .split('/')
                    .any(|segment| route_parameter(segment) == Some(parameter))
            {
                return Err(Error::custom(
                    "http_route_parameter_duplicate",
                    "domain and path parameter names must differ",
                ));
            }
        }
    }
    Ok(())
}

fn validate_url_path(path: &str) -> Result<()> {
    let mut normalized_path = path;
    if let Some(path) = normalized_path.strip_prefix('/') {
        normalized_path = path;
    }
    if let Some(path) = normalized_path.strip_suffix('/') {
        normalized_path = path;
    }
    if path != "/" && normalized_path.split('/').any(str::is_empty) {
        return Err(Error::custom(
            "http_route_pattern_unsupported",
            "named routes cannot contain empty path segments",
        ));
    }
    if path.split('/').any(|segment| {
        (segment.contains('{') || segment.contains('}') || segment.contains('*'))
            && route_parameter(segment).is_none()
    }) {
        return Err(Error::custom(
            "http_route_pattern_unsupported",
            "named route parameters must occupy a complete path segment",
        ));
    }
    Ok(())
}

fn generation_error(code: impl Into<String>, message: impl Into<String>) -> Error {
    Error::custom_with_status(StatusCode::INTERNAL_SERVER_ERROR, code, message)
}

impl<S> FromRequestParts<S> for RouteUrlGenerator
where
    S: Send + Sync,
{
    type Rejection = Error;

    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl std::future::Future<Output = Result<Self>> + Send {
        std::future::ready(
            parts
                .extensions
                .get::<Self>()
                .cloned()
                .ok_or_else(route_url_unavailable),
        )
    }
}

pub(crate) fn route_url_unavailable() -> Error {
    Error::custom(
        "http_route_url_unavailable",
        "route URL generation is unavailable outside the application HTTP router",
    )
}

fn route_parameter(segment: &str) -> Option<&str> {
    let parameter = segment.strip_prefix('{')?.strip_suffix('}')?;
    (!parameter.is_empty()
        && !parameter
            .chars()
            .any(|c| matches!(c, '{' | '}' | '?' | '*')))
    .then_some(parameter)
}

fn valid_domain_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value.is_ascii()
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(test)]
mod action_tests {
    use super::*;
    use crate::http::RequestInfo;
    use axum::body::Body;
    use axum::http::Request;

    fn definition(action: &str, path: &str, domain: Option<&str>) -> ActionRouteDefinition {
        ActionRouteDefinition {
            action: action.into(),
            path: path.into(),
            domain: domain.map(str::to_owned),
        }
    }

    #[test]
    fn action_and_named_route_share_domain_path_query_and_parameter_errors() -> Result<()> {
        // 动作不能另做一套漏编码、漏缺参检查的URL拼接器。
        let urls = RouteUrlGenerator::new_with_actions(
            "https://configured.test:8443/base",
            vec![NamedRouteDefinition {
                name: "draft".into(),
                path: "/draft/{id}/".into(),
                domain: Some("{tenant}.example.test".into()),
            }],
            vec![definition(
                "Draft@show",
                "/draft/{id}/",
                Some("{tenant}.example.test"),
            )],
        )?;
        let parameters = BTreeMap::from([
            ("tenant".into(), "ACME".into()),
            ("id".into(), "a/b ü".into()),
            ("a".into(), "x&y".into()),
            ("z".into(), "last".into()),
        ]);
        assert_eq!(
            urls.action("Draft@show", &parameters)?,
            urls.route("draft", &parameters)?
        );
        assert_eq!(
            urls.action("Draft@show", &parameters)?,
            "https://acme.example.test:8443/base/draft/a%2Fb%20%C3%BC/?a=x%26y&z=last"
        );
        for (key, value, code) in [
            ("id", ".", "http_route_parameter_invalid"),
            ("tenant", "not.one.label", "http_route_parameter_invalid"),
        ] {
            let mut invalid = parameters.clone();
            invalid.insert(key.into(), value.into());
            assert_eq!(
                urls.action("Draft@show", &invalid)
                    .err()
                    .map(|error| error.code().to_owned())
                    .as_deref(),
                Some(code)
            );
        }
        let mut missing = parameters;
        missing.remove("id");
        assert_eq!(
            urls.action("Draft@show", &missing)
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("http_route_parameter_missing")
        );
        assert_eq!(
            urls.action("unknown", &missing)
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("http_route_action_not_found")
        );
        Ok(())
    }

    #[tokio::test]
    async fn action_namespace_mutation_is_local_to_clones_and_captured_request() -> Result<()> {
        // 请求捕获与clone共享不可变索引，而非共享可变查询namespace。
        let mut urls = RouteUrlGenerator::new_with_actions(
            "https://configured.test",
            Vec::new(),
            vec![
                definition("App\\Draft@show", "/app", None),
                definition("Draft@show", "/absolute", None),
                definition("App\\Invokable", "/invoke", None),
            ],
        )?;
        let before = urls.clone();
        urls.set_root_controller_namespace("App")?;
        let after = urls.clone();
        assert!(Arc::ptr_eq(&before.inner, &urls.inner));
        assert!(Arc::ptr_eq(&after.inner, &urls.inner));
        let (mut parts, _) = Request::get("/captured")
            .header("host", "malicious.test")
            .body(Body::empty())
            .map_err(|error| Error::custom("test_request_invalid", error.to_string()))?
            .into_parts();
        parts.extensions.insert(urls.clone());
        let info = RequestInfo::from_request_parts(&mut parts, &()).await?;
        urls.set_root_controller_namespace("")?;
        let parameters = BTreeMap::new();
        assert_eq!(
            before.action("Draft@show", &parameters)?,
            "https://configured.test/absolute"
        );
        assert_eq!(
            urls.action("Draft@show", &parameters)?,
            "https://configured.test/absolute"
        );
        assert_eq!(
            after.action("Draft@show", &parameters)?,
            "https://configured.test/app"
        );
        assert_eq!(
            info.route_urls()?.action("Draft@show", &parameters)?,
            "https://configured.test/app"
        );
        assert!(Arc::ptr_eq(&info.route_urls()?.inner, &urls.inner));
        assert_eq!(
            after.action(("Draft", "show"), &parameters)?,
            "https://configured.test/absolute"
        );
        assert_eq!(
            after.action("\\Draft@show", &parameters)?,
            "https://configured.test/absolute"
        );
        assert_eq!(
            after.action("Invokable", &parameters)?,
            "https://configured.test/invoke"
        );
        assert_eq!(
            after
                .action("App\\Draft@show", &parameters)
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("http_route_action_not_found")
        );
        urls.set_root_controller_namespace("App")?;
        assert!(urls.set_root_controller_namespace("Bad\n").is_err());
        assert_eq!(
            urls.action("Draft@show", &parameters)?,
            "https://configured.test/app"
        );
        Ok(())
    }

    #[test]
    fn action_lookup_preserves_raw_index_asymmetry_and_query_validation() -> Result<()> {
        let urls = RouteUrlGenerator::new_with_actions(
            "http://configured.test",
            Vec::new(),
            vec![
                definition("Draft@show", "/first", None),
                definition("\\Draft@show\\", "/second", None),
                definition("Draft@show", "/third", None),
                definition("Invokable", "/invoke", None),
            ],
        )?;
        let parameters = BTreeMap::new();
        assert_eq!(
            urls.action("\\Draft@show\\", &parameters)?,
            "http://configured.test/second"
        );
        assert_eq!(
            urls.action(("Draft", "show"), &parameters)?,
            "http://configured.test/second"
        );
        assert_eq!(
            urls.action("Invokable", &parameters)?,
            "http://configured.test/invoke"
        );
        for action in [
            crate::http::RouteAction::from(""),
            crate::http::RouteAction::from(("Draft", "")),
            crate::http::RouteAction::from("Draft\r@show"),
        ] {
            assert_eq!(
                urls.action(action, &parameters)
                    .err()
                    .map(|error| error.code().to_owned())
                    .as_deref(),
                Some("http_route_action_invalid")
            );
        }
        assert_eq!(
            RouteUrlGenerator::empty()
                .action("Draft@show", &parameters)
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("http_route_action_not_found")
        );
        Ok(())
    }
}
