use std::sync::Arc;

use crate::{
    config::ConfigError,
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response},
    support::facades::{App, Config},
    translation::{TranslationResult, canonical_locale},
};

type LocaleCallback = Arc<dyn Fn(&Request) -> Result<Option<String>, HttpError> + Send + Sync>;

/// 当前请求的语言切换信息，由实际执行的中间件提供给页面。
#[derive(Clone, Debug)]
pub struct LocaleSelection {
    /// 可接受 URL 选择的参数名；关闭识别或回调已决定语言时为空。
    pub query_parameter: Option<String>,
    /// 此实例实际配置的候选语言。
    pub supported: Vec<String>,
    /// 当前语言是否由 URL 参数显式选择。
    pub from_query: bool,
}

/// 默认全局语言中间件；支持语言仅属于当前应用的装配实例。
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

    /// 为此实例选择语言；None 继续 URL 参数及浏览器选择，错误交给 HTTP 异常处理。
    /// Some 的标签规范化后须精确属于候选，非法或不支持的值报错；空候选时不执行回调。
    #[allow(non_snake_case, reason = "沿用项目回调配置方法的命名风格")]
    pub fn localeUsing(
        &mut self,
        callback: impl Fn(&Request) -> Result<Option<String>, HttpError> + Send + Sync + 'static,
    ) -> &mut Self {
        self.locale_callback = Some(Arc::new(callback));
        self
    }

    fn selected_locale(
        &self,
        request: &Request,
        default: &str,
        query_parameter: Option<&str>,
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
        Ok((self.preferred_locale(request, default), false, false))
    }

    fn preferred_locale(&self, request: &Request, default: &str) -> String {
        request
            .getLanguages()
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
            let (locale, from_query, from_callback) =
                self.selected_locale(request, &default, parameter.as_deref())?;
            App::setLocale(&locale).await.map_err(internal)?;
            request.parts_mut().extensions.insert(LocaleSelection {
                // 回调已决定语言时，URL 控件无法覆盖它，因此不对页面提供可切换入口。
                query_parameter: if from_callback { None } else { parameter },
                supported: self.supported.clone(),
                from_query,
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
            let varies = response.headers().get_all("vary").iter().any(|value| {
                value.to_str().is_ok_and(|value| {
                    value.split(',').any(|part| {
                        part.trim() == "*" || part.trim().eq_ignore_ascii_case("Accept-Language")
                    })
                })
            });
            if !varies {
                response
                    .parts_mut()
                    .headers
                    .append("vary", http::HeaderValue::from_static("Accept-Language"));
            }
            Ok(response)
        })
    }
}

fn query_parameter() -> Result<Option<String>, HttpError> {
    match Config::get("app.locale_query_parameter").map_err(internal)? {
        None => Ok(Some("lang".into())),
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
