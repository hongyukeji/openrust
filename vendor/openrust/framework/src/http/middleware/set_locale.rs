use std::sync::Arc;

use crate::{
    config::ConfigError,
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response},
    support::facades::{App, Config, Cookie},
    translation::{TranslationResult, canonical_locale},
};

#[derive(Clone)]
pub(crate) struct PrivateLocaleResponse;

type LocaleCallback = Arc<dyn Fn(&Request) -> Result<Option<String>, HttpError> + Send + Sync>;

/// 当前请求的语言切换信息，由实际执行的中间件提供给页面。
#[derive(Clone, Debug, serde::Serialize)]
pub struct LocaleSelection {
    /// 可接受 URL 选择的参数名；关闭识别或回调已决定语言时为空。
    pub query_parameter: Option<String>,
    /// 此实例实际配置的候选语言。
    pub supported: Vec<String>,
    /// 当前语言是否由 URL 参数显式选择。
    pub from_query: bool,
    /// 有效的持久手动偏好，独立于 URL 覆盖和当前实际语言。
    pub preference: Option<String>,
    /// 当前管线允许通过安全写入请求保存偏好。
    pub can_store: bool,
}

/// 默认语言中间件；由框架在路由管线自动装配，Web 在 Cookie 解密之后执行。
/// 按回调、URL、受保护 Cookie、app.locale_header 指定头及默认候选选择。
/// 自定义头缺失时不重试 Accept-Language；停用中间件后配置本身不会选择语言。
/// 默认实例的资源发现由 HTTP Kernel 初始化；独立注册路由中间件时用 `new` 指定候选。
#[derive(Clone)]
pub struct SetLocale {
    supported: Vec<String>,
    discover: bool,
    locale_callback: Option<LocaleCallback>,
}

impl Default for SetLocale {
    fn default() -> Self {
        Self {
            supported: Vec::new(),
            discover: true,
            locale_callback: None,
        }
    }
}

impl SetLocale {
    /// 覆盖候选语言；空列表关闭识别，不修改当前语言或响应头。
    pub fn new(locales: &[&str]) -> TranslationResult<Self> {
        let supported = locales
            .iter()
            .map(|locale| canonical_locale(locale))
            .collect::<TranslationResult<_>>()?;
        Ok(Self {
            supported,
            discover: false,
            locale_callback: None,
        })
    }

    /// 原子替换候选，保留已有回调；空列表关闭整个中间件。
    pub fn set_locale(&mut self, locales: &[&str]) -> TranslationResult<&mut Self> {
        self.supported = Self::new(locales)?.supported;
        self.discover = false;
        Ok(self)
    }

    /// 默认实例在 HTTP 引导后优先读取配置，否则发现资源；显式候选跳过两者。
    pub(crate) async fn configure(
        &mut self,
        app: &crate::foundation::Application,
    ) -> TranslationResult<()> {
        use crate::contracts::{container::Container as _, foundation::Application as _};
        if self.discover {
            let config = app
                .config()
                .map_err(crate::translation::TranslationError::Unavailable)?;
            let invalid = || {
                crate::translation::TranslationError::Source(Box::new(ConfigError::InvalidType {
                    key: "app.supported_locales".into(),
                    expected: "* 或 BCP 47 语言标签数组",
                }))
            };
            match config.get("app.supported_locales") {
                None => {}
                Some(serde_json::Value::String(value)) if value == "*" => {}
                Some(serde_json::Value::Array(locales)) => {
                    let mut supported = Vec::new();
                    for locale in locales {
                        let locale = canonical_locale(locale.as_str().ok_or_else(invalid)?)
                            .map_err(|_| invalid())?;
                        if !supported.contains(&locale) {
                            supported.push(locale);
                        }
                    }
                    self.supported = supported;
                    self.discover = false;
                    return Ok(());
                }
                Some(_) => return Err(invalid()),
            }
            let loader = app
                .container()
                .get::<crate::translation::FileLoader>("translation.loader")
                .map_err(crate::translation::TranslationError::Unavailable)?;
            self.supported = tokio::task::spawn_blocking(move || loader.availableLocales())
                .await
                .map_err(|source| {
                    crate::translation::TranslationError::Source(Box::new(source))
                })??;
            self.discover = false;
        }
        Ok(())
    }

    /// 为此实例选择语言；None 继续 URL 参数、受保护 Cookie 及请求头选择，错误交给 HTTP 异常处理。
    /// Some 的标签规范化后须精确属于候选，非法或不支持的值报错；空候选时不执行回调。
    pub fn locale_using(
        &mut self,
        callback: impl Fn(&Request) -> Result<Option<String>, HttpError> + Send + Sync + 'static,
    ) -> &mut Self {
        self.locale_callback = Some(Arc::new(callback));
        self
    }

    /// 保存或清除当前浏览器的手动偏好；须在 Cookie 解密、队列和写入请求保护之后调用。
    /// None 恢复自动，Some 仅接受当前候选中的 BCP 47 标签；不会再次切换本次语言。
    pub fn store_preference(request: &Request, locale: Option<&str>) -> Result<(), HttpError> {
        if matches!(
            *request.method(),
            http::Method::GET | http::Method::HEAD | http::Method::OPTIONS
        ) {
            return Err(HttpError::InvalidRequest);
        }
        let selection = request
            .extensions()
            .get::<LocaleSelection>()
            .filter(|selection| selection.can_store)
            .ok_or(HttpError::InvalidRequest)?;
        let name = locale_cookie()?.ok_or(HttpError::InvalidRequest)?;
        if !request.cookie_is_protected(&name) {
            return Err(HttpError::InvalidRequest);
        }
        let cookie = match locale {
            Some(locale) => {
                let locale = canonical_locale(locale).map_err(|_| HttpError::InvalidRequest)?;
                if !selection.supported.contains(&locale) {
                    return Err(HttpError::InvalidRequest);
                }
                Cookie::forever(&name, &locale, &Default::default())?
            }
            None => Cookie::forget(&name, None, None)?,
        };
        Cookie::queue(cookie)?;
        Ok(())
    }

    /// 从请求体的指定顶层字段保存偏好；不读取查询参数。
    /// null 或空字符串恢复自动，缺字段及其他类型报错；沿用 storePreference 的安全约束。
    pub async fn store_preference_from_request(
        request: &mut Request,
        field: &str,
    ) -> Result<(), HttpError> {
        let body = request.post(None).await?.ok_or(HttpError::InvalidRequest)?;
        let value = body.get(field).ok_or(HttpError::InvalidRequest)?;
        let locale = match value {
            serde_json::Value::Null => None,
            serde_json::Value::String(locale) if locale.is_empty() => None,
            serde_json::Value::String(locale) => Some(locale.as_str()),
            _ => return Err(HttpError::InvalidRequest),
        };
        Self::store_preference(request, locale)
    }

    /// 生成应用指定的本地返回 URL，移除实际语言查询参数并保留其他查询值。
    /// 目标必须是以单个 / 开头的应用内路径；不接受外部返回地址，也不创建响应或保存偏好。
    pub fn url_without_locale(request: &Request, path: &str) -> Result<String, HttpError> {
        use crate::{contracts::routing::UrlGenerator as _, support::facades::URL};

        if !path.starts_with('/')
            || path.starts_with("//")
            || path.contains('\\')
            || path.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(HttpError::InvalidRequest);
        }
        let parameter = request
            .extensions()
            .get::<LocaleSelection>()
            .and_then(|selection| selection.query_parameter.as_deref());
        let filtered = |mut query: serde_json::Value| -> Result<_, HttpError> {
            if let Some(parameter) = parameter {
                crate::support::Arr::forget(&mut query, parameter)
                    .map_err(|_| HttpError::InvalidRequest)?;
            }
            Ok(crate::routing::json_parameters(query))
        };
        let (path, fragment) = path.split_once('#').unwrap_or((path, ""));
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        let urls = URL::for_request(request)?;
        let target = urls.query(
            path,
            &filtered(crate::http::parse_query(query)?)?,
            &[],
            None,
        )?;
        let mut target = urls.query(
            &target,
            &filtered(request.query(None)?.unwrap_or_default())?,
            &[],
            None,
        )?;
        if !fragment.is_empty() {
            target.push('#');
            target.push_str(fragment);
        }
        Ok(target)
    }

    fn selected_locale(
        &self,
        request: &Request,
        default: &str,
        query_parameter: Option<&str>,
        header: Option<&http::HeaderName>,
        preference: Option<&str>,
    ) -> Result<(String, bool, bool), HttpError> {
        if let Some(callback) = &self.locale_callback
            && let Some(locale) = callback(request)?
        {
            let locale = canonical_locale(&locale)?;
            if !self.supported.contains(&locale) {
                return Err(HttpError::from(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("语言选择回调返回了不在候选列表中的语言：{locale}"),
                )));
            }
            return Ok((locale, false, true));
        }
        if let Some(parameter) = query_parameter
            && let Some(value) = request.query(Some(parameter))?
        {
            let locale = value.as_str().ok_or(HttpError::InvalidRequest)?;
            let locale = canonical_locale(locale).map_err(|_| HttpError::InvalidRequest)?;
            if !self.supported.contains(&locale) {
                return Err(HttpError::InvalidRequest);
            }
            return Ok((locale, true, false));
        }
        Ok((
            preference
                .map(str::to_owned)
                .unwrap_or_else(|| self.preferred_locale(request, default, header)),
            false,
            false,
        ))
    }

    fn preferred_locale(
        &self,
        request: &Request,
        default: &str,
        header: Option<&http::HeaderName>,
    ) -> String {
        header
            .map(|header| request.languages_from_header(header))
            .unwrap_or_default()
            .iter()
            .find_map(|language| {
                if language == "*" {
                    supported_locale(default, &self.supported)
                        .or_else(|| self.supported.first().map(String::as_str))
                } else {
                    supported_locale(language, &self.supported)
                }
            })
            .or_else(|| supported_locale(default, &self.supported))
            .unwrap_or_else(|| self.supported.first().expect("候选非空时才选择语言"))
            .to_owned()
    }
}

impl Middleware for SetLocale {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if self.supported.is_empty() {
                return Ok(next.run(request).await);
            }
            // 最小应用可不提供 app.locale，沿用译器已有的英文默认值。
            let default = if Config::has("app.locale")? {
                App::current_locale()?
            } else {
                "en".to_owned()
            };
            let parameter = query_parameter()?;
            let header = locale_header()?;
            let cookie = locale_cookie()?;
            // 只信任经过现有 EncryptCookies 验证的值，普通 API 不从原始 Cookie 选择语言。
            let cookie_active = cookie
                .as_ref()
                .is_some_and(|name| request.cookie_is_protected(name));
            let preference = if cookie_active {
                request
                    .cookie(cookie.as_deref())?
                    .and_then(|value| {
                        value
                            .as_str()
                            .and_then(|value| canonical_locale(value).ok())
                    })
                    .filter(|value| self.supported.contains(value))
            } else {
                None
            };
            let (locale, from_query, from_callback) = self.selected_locale(
                request,
                &default,
                parameter.as_deref(),
                header.as_ref(),
                preference.as_deref(),
            )?;
            App::set_locale(&locale).await?;
            request.parts_mut().extensions.insert(LocaleSelection {
                // 回调已决定语言时，URL 控件无法覆盖它，因此不对页面提供可切换入口。
                query_parameter: if from_callback { None } else { parameter },
                supported: self.supported.clone(),
                from_query,
                preference,
                can_store: cookie_active && !from_callback && Cookie::getQueuedCookies().is_ok(),
            });
            let mut response = next.run(request).await;
            // 下游可以主动切换语言，已有响应头仍由响应提供者决定。
            let language = if Config::has("app.locale")? {
                App::current_locale()?
            } else {
                locale
            };
            if !response.headers().contains_key("content-language") {
                response
                    .parts_mut()
                    .headers
                    .insert("content-language", language.parse()?);
            }
            if let Some(header) = header {
                add_vary(
                    &mut response,
                    if header == http::header::ACCEPT_LANGUAGE {
                        "Accept-Language"
                    } else {
                        header.as_str()
                    },
                )?;
            }
            if cookie_active {
                request.parts_mut().extensions.insert(PrivateLocaleResponse);
                add_vary(&mut response, "Cookie")?;
                super::set_cache_headers::set_private_locale_cache_control(
                    &mut response.parts_mut().headers,
                );
            }
            Ok(response)
        })
    }
}

fn add_vary(response: &mut Response, name: &str) -> Result<(), HttpError> {
    let present = response.headers().get_all("vary").iter().any(|value| {
        value.to_str().is_ok_and(|value| {
            value
                .split(',')
                .any(|part| part.trim() == "*" || part.trim().eq_ignore_ascii_case(name))
        })
    });
    if !present {
        response.parts_mut().headers.append("vary", name.parse()?);
    }
    Ok(())
}

fn locale_cookie() -> Result<Option<String>, HttpError> {
    match Config::get("app.locale_cookie")? {
        None => Ok(Some("locale".into())),
        Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(name))
            if !name.is_empty()
                && name != "0"
                && name != "XSRF-TOKEN"
                && !name.contains(['.', '[', ']'])
                && name.parse::<http::HeaderName>().is_ok()
                && Config::get("session.cookie")?
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .as_deref()
                    .unwrap_or("openrust_session")
                    != name.as_str() =>
        {
            Ok(Some(name))
        }
        _ => Err(HttpError::from(ConfigError::InvalidType {
            key: "app.locale_cookie".into(),
            expected: "不与会话或 XSRF Cookie 冲突的非空顶层 Cookie 名称，或 null",
        })),
    }
}

fn locale_header() -> Result<Option<http::HeaderName>, HttpError> {
    let invalid = || {
        HttpError::from(ConfigError::InvalidType {
            key: "app.locale_header".into(),
            expected: "合法的非空 HTTP 请求头名称，或 null",
        })
    };
    match Config::get("app.locale_header")? {
        None => Ok(Some(http::header::ACCEPT_LANGUAGE)),
        Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(name)) => http::HeaderName::from_bytes(name.as_bytes())
            .map(Some)
            .map_err(|_| invalid()),
        _ => Err(invalid()),
    }
}

fn query_parameter() -> Result<Option<String>, HttpError> {
    match Config::get("app.locale_query_parameter")? {
        None => Ok(Some("locale".into())),
        Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(name))
            if !name.is_empty()
                && !name.chars().any(|c| {
                    c.is_whitespace() || c.is_control() || matches!(c, '.' | '[' | ']')
                }) =>
        {
            Ok(Some(name))
        }
        _ => Err(HttpError::from(ConfigError::InvalidType {
            key: "app.locale_query_parameter".into(),
            expected: "非空查询参数名（不含空白、控制字符、点或方括号），或 null",
        })),
    }
}

fn supported_locale<'a>(locale: &str, supported: &'a [String]) -> Option<&'a str> {
    let locale = canonical_locale(locale).ok()?;
    let base = locale
        .split('-')
        .take_while(|part| part.len() != 1)
        .collect::<Vec<_>>();
    let language = *base.first()?;
    let script = base
        .iter()
        .skip(1)
        .find(|part| part.len() == 4 && part.bytes().all(|byte| byte.is_ascii_alphabetic()));
    // 显式文字体系优先；未知文字体系不再通过地区推断中文资源。
    let chinese = if language == "zh" {
        match script.copied() {
            Some("Hant") => Some("zh-Hant"),
            Some("Hans") => Some("zh-Hans"),
            Some(_) => return None,
            None if base.iter().any(|part| matches!(*part, "TW" | "HK" | "MO")) => Some("zh-Hant"),
            None => Some("zh-Hans"),
        }
    } else {
        None
    };
    let candidate = chinese.unwrap_or(&locale);
    supported
        .iter()
        .find(|item| item.as_str() == candidate)
        .or_else(|| supported.iter().find(|item| **item == locale))
        .or_else(|| supported.iter().find(|item| item.as_str() == language))
        .map(String::as_str)
}
