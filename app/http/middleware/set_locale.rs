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
    let mut selected = None;
    let mut highest = 0;
    for header in request.headers().get_all("accept-language") {
        let Ok(header) = header.to_str() else {
            continue;
        };
        for item in header.split(',') {
            let mut segments = item.split(';');
            let language = segments.next().unwrap_or_default().trim();
            let mut weight = Some(1000);
            let mut has_weight = false;
            for parameter in segments {
                let Some((key, value)) = parameter.trim().split_once('=') else {
                    weight = None;
                    break;
                };
                if !key.trim().eq_ignore_ascii_case("q") || has_weight {
                    weight = None;
                    break;
                }
                has_weight = true;
                weight = quality(value.trim());
            }
            let Some(weight) = weight.filter(|weight| *weight > highest) else {
                continue;
            };
            let locale = if language == "*" {
                supported_locale(default, supported)
                    .or_else(|| supported.first().map(String::as_str))
            } else {
                supported_locale(language, supported)
            };
            if let Some(locale) = locale {
                selected = Some(locale);
                highest = weight;
            }
        }
    }
    selected
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
    let script = base.iter().skip(1).find(|part| part.len() == 4);
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

// HTTP qvalue 最多三位小数，使用整数比较，避免浮点误差和 NaN。
fn quality(value: &str) -> Option<u16> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if !matches!(whole, "0" | "1")
        || fraction.len() > 3
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || (whole == "1" && fraction.bytes().any(|byte| byte != b'0'))
    {
        return None;
    }
    let mut weight = whole.parse::<u16>().ok()? * 1000;
    for (digit, scale) in fraction.bytes().zip([100, 10, 1]) {
        weight += u16::from(digit - b'0') * scale;
    }
    Some(weight)
}
