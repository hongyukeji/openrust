use openrust::{
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response},
    support::facades::App,
    translation::canonical_locale,
};

/// 在整个请求处理期间选择语言，供模板、校验及控制器共用。
pub(crate) struct SetLocale;

impl Middleware for SetLocale {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            // 浏览器识别是应用策略，不向 Laravel 默认配置增加配置键。
            let supported = ["en", "zh-Hans", "zh-Hant"].map(str::to_owned);
            let locale = preferred_locale(
                request,
                &App::currentLocale().map_err(internal)?,
                &supported,
            );
            App::setLocale(&locale).await.map_err(internal)?;
            let mut response = next.run(request).await;
            // 使用下游最终语言；控制器可以在当前请求内主动切换语言。
            let language = App::currentLocale().map_err(internal)?;
            if !response.headers().contains_key("content-language") {
                response
                    .parts_mut()
                    .headers
                    .insert("content-language", language.parse().map_err(internal)?);
            }
            let varies = response.headers().get_all("vary").iter().any(|value| {
                value.to_str().is_ok_and(|value| {
                    value.split(',').any(|part| {
                        matches!(part.trim(), "*")
                            || part.trim().eq_ignore_ascii_case("Accept-Language")
                    })
                })
            });
            if !varies {
                response
                    .parts_mut()
                    .headers
                    .append("vary", "Accept-Language".parse().expect("固定响应头有效"));
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

fn preferred_locale(request: &Request, default: &str, supported: &[String]) -> String {
    request
        .getLanguages()
        .iter()
        .find_map(|language| {
            if language == "*" {
                supported_locale(default, supported)
                    .or_else(|| supported.first().map(String::as_str))
            } else {
                supported_locale(language, supported)
            }
        })
        .or_else(|| supported_locale(default, supported))
        .unwrap_or_else(|| supported.first().map_or("en", String::as_str))
        .to_owned()
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
