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
#[derive(Clone)]
pub struct SetLocale {
    supported: Vec<String>,
    locale_callback: Option<LocaleCallback>,
}

impl Default for SetLocale {
    fn default() -> Self {
        Self {
            supported: ["en", "zh-Hans", "zh-Hant"].map(str::to_owned).to_vec(),
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
            locale_callback: None,
        })
    }

    /// 原子替换候选，保留已有回调；空列表关闭整个中间件。
    #[allow(non_snake_case, reason = "沿用中间件配置方法的命名风格")]
    pub fn setLocale(&mut self, locales: &[&str]) -> TranslationResult<&mut Self> {
        self.supported = Self::new(locales)?.supported;
        Ok(self)
    }

    /// 为此实例选择语言；None 继续 URL 参数、受保护 Cookie 及请求头选择，错误交给 HTTP 异常处理。
    /// Some 的标签规范化后须精确属于候选，非法或不支持的值报错；空候选时不执行回调。
    #[allow(non_snake_case, reason = "沿用项目回调配置方法的命名风格")]
    pub fn localeUsing(
        &mut self,
        callback: impl Fn(&Request) -> Result<Option<String>, HttpError> + Send + Sync + 'static,
    ) -> &mut Self {
        self.locale_callback = Some(Arc::new(callback));
        self
    }

    /// 保存或清除当前浏览器的手动偏好；须在 Cookie 解密、队列和写入请求保护之后调用。
    /// None 恢复自动，Some 仅接受当前候选中的 BCP 47 标签；不会再次切换本次语言。
    #[allow(non_snake_case, reason = "沿用中间件公开方法命名")]
    pub fn storePreference(request: &Request, locale: Option<&str>) -> Result<(), HttpError> {
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
            let locale = canonical_locale(&locale).map_err(internal)?;
            if !self.supported.contains(&locale) {
                return Err(internal(std::io::Error::new(
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
            let default = if Config::has("app.locale").map_err(internal)? {
                App::currentLocale().map_err(internal)?
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
            App::setLocale(&locale).await.map_err(internal)?;
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
            let language = if Config::has("app.locale").map_err(internal)? {
                App::currentLocale().map_err(internal)?
            } else {
                locale
            };
            if !response.headers().contains_key("content-language") {
                response
                    .parts_mut()
                    .headers
                    .insert("content-language", language.parse().map_err(internal)?);
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
        response
            .parts_mut()
            .headers
            .append("vary", name.parse().map_err(internal)?);
    }
    Ok(())
}

fn locale_cookie() -> Result<Option<String>, HttpError> {
    match Config::get("app.locale_cookie").map_err(internal)? {
        None => Ok(Some("locale".into())),
        Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(name))
            if !name.is_empty()
                && name != "0"
                && name != "XSRF-TOKEN"
                && !name.contains(['.', '[', ']'])
                && name.parse::<http::HeaderName>().is_ok()
                && Config::get("session.cookie")
                    .map_err(internal)?
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .as_deref()
                    .unwrap_or("openrust_session")
                    != name.as_str() =>
        {
            Ok(Some(name))
        }
        _ => Err(internal(ConfigError::InvalidType {
            key: "app.locale_cookie".into(),
            expected: "不与会话或 XSRF Cookie 冲突的非空顶层 Cookie 名称，或 null",
        })),
    }
}

fn locale_header() -> Result<Option<http::HeaderName>, HttpError> {
    let invalid = || {
        internal(ConfigError::InvalidType {
            key: "app.locale_header".into(),
            expected: "合法的非空 HTTP 请求头名称，或 null",
        })
    };
    match Config::get("app.locale_header").map_err(internal)? {
        None => Ok(Some(http::header::ACCEPT_LANGUAGE)),
        Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(name)) => http::HeaderName::from_bytes(name.as_bytes())
            .map(Some)
            .map_err(|_| invalid()),
        _ => Err(invalid()),
    }
}

fn query_parameter() -> Result<Option<String>, HttpError> {
    match Config::get("app.locale_query_parameter").map_err(internal)? {
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
        _ => Err(internal(ConfigError::InvalidType {
            key: "app.locale_query_parameter".into(),
            expected: "非空查询参数名（不含空白、控制字符、点或方括号），或 null",
        })),
    }
}

fn internal(source: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
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
