use openrust::{
    http::{HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Config, Lang},
    translation::canonical_locale,
    view::view,
};

pub(crate) struct WelcomeController;

impl WelcomeController {
    pub(crate) fn index(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let locale = preferred_locale(request, &Config::stringOr("app.locale", "en"));
            let language = match locale {
                "zh_CN" => "zh-CN",
                "zh_TW" => "zh-TW",
                _ => "en",
            };
            let mut page = view("welcome")
                .with("name", Config::stringOr("app.name", "App"))
                .with("version", Config::stringOr("app.version", "1.0.0"))
                .with("language", language)
                .with("docs_locale", locale);
            for key in [
                "heading",
                "description",
                "read",
                "documentation",
                "explore",
                "source",
                "deployment",
                "changelog",
            ] {
                // 显式传入本次语言，不修改应用或其他请求的默认语言。
                let text = Lang::getWith(&format!("welcome.{key}"), &[], Some(locale)).map_err(
                    |source| HttpError::Internal {
                        source: Box::new(source),
                    },
                )?;
                page = page.with(key, text);
            }
            let mut headers = HeaderMap::new();
            headers.insert(
                "content-language",
                language.parse().expect("固定语言标识有效"),
            );
            headers.insert("vary", "Accept-Language".parse().expect("固定响应头有效"));
            Response::view(StatusCode::OK, page, headers, request).await
        })
    }
}

fn preferred_locale(request: &Request, default: &str) -> &'static str {
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
                supported_locale(default).or(Some("en"))
            } else {
                supported_locale(language)
            };
            if let Some(locale) = locale {
                selected = Some(locale);
                highest = weight;
            }
        }
    }
    selected.unwrap_or_else(|| supported_locale(default).unwrap_or("en"))
}

fn supported_locale(locale: &str) -> Option<&'static str> {
    let locale = canonical_locale(locale).ok()?;
    let mut segments = locale.split('_');
    match segments.next()? {
        "en" => Some("en"),
        "zh" => {
            let parts: Vec<_> = segments.collect();
            // 文字体系优先于地区，例如 zh-Hant-CN 仍使用繁体。
            if parts.contains(&"Hant") {
                Some("zh_TW")
            } else if parts.contains(&"Hans") {
                Some("zh_CN")
            } else if parts.iter().any(|part| matches!(*part, "TW" | "HK" | "MO")) {
                Some("zh_TW")
            } else if parts.iter().any(|part| part.len() == 4) {
                None
            } else {
                Some("zh_CN")
            }
        }
        _ => None,
    }
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
