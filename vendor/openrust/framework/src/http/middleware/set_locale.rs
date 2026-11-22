use crate::{
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response},
    support::facades::{App, Config},
    translation::{TranslationResult, canonical_locale},
};

/// 默认全局语言中间件；支持语言仅属于当前应用的装配实例。
#[derive(Clone)]
pub struct SetLocale {
    supported: Vec<String>,
}

impl Default for SetLocale {
    fn default() -> Self {
        Self {
            supported: ["en", "zh-Hans", "zh-Hant"].map(str::to_owned).to_vec(),
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
        Ok(Self { supported })
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
            let locale = self.preferred_locale(request, &default);
            App::setLocale(&locale).await.map_err(internal)?;
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
