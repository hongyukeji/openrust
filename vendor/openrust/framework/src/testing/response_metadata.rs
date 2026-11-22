use super::TestResponse;
use crate::{
    contracts::routing::UrlGenerator as _,
    http::{Body, Request, ResponseKind},
    routing::{RouteParameter, SignatureMode, SignatureQueryFilter},
    view::ViewValue,
};
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[allow(non_snake_case)]
impl TestResponse {
    fn assertion_time(&self) -> SystemTime {
        self.clock.now().unwrap_or_else(jiff::Timestamp::now).into()
    }

    fn urls(&self) -> &crate::routing::UrlGenerator {
        self.urls.as_ref().expect("响应没有 URL 生成器快照")
    }

    #[track_caller]
    pub fn assertLocation(&self, uri: &str) -> &Self {
        let location = self
            .headers
            .get("location")
            .and_then(|header| header.to_str().ok())
            .unwrap_or("");
        assert_eq!(
            self.urls().to(uri, &[], None).expect("预期重定向地址无效"),
            self.urls()
                .to(location, &[], None)
                .expect("实际重定向地址无效"),
            "重定向地址不匹配"
        );
        self
    }

    #[track_caller]
    pub fn assertRedirectContains(&self, uri: &str) -> &Self {
        self.assertRedirect(None)
            .assertHeaderContains("location", uri)
    }

    #[track_caller]
    pub fn assertRedirectBack(&self) -> &Self {
        let previous = self.urls().previous(None).expect("无法生成上一个请求地址");
        self.assertRedirect(Some(&previous))
    }

    #[track_caller]
    pub fn assertRedirectBackWithErrors(
        &self,
        keys: &Value,
        format: Option<&str>,
        errorBag: &str,
    ) -> &Self {
        self.assertRedirectBack()
            .assertSessionHasErrors(keys, format, errorBag)
    }

    #[track_caller]
    pub fn assertRedirectBackWithoutErrors(&self) -> &Self {
        self.assertRedirectBack().assertSessionHasNoErrors()
    }

    #[track_caller]
    pub fn assertRedirectToRoute(&self, name: &str, parameters: &[RouteParameter<'_>]) -> &Self {
        let target = self
            .urls()
            .route(name, parameters, true)
            .expect("无法生成预期路由");
        self.assertRedirect(Some(&target))
    }

    #[track_caller]
    pub fn assertRedirectToAction(&self, action: &str, parameters: &[RouteParameter<'_>]) -> &Self {
        let target = self
            .urls()
            .action(action, parameters, true)
            .expect("无法生成预期动作地址");
        self.assertRedirect(Some(&target))
    }

    #[track_caller]
    pub fn assertRedirectToSignedRoute(
        &self,
        name: Option<&str>,
        parameters: &[RouteParameter<'_>],
        absolute: bool,
    ) -> &Self {
        self.assertRedirect(None);
        let location = self
            .headers
            .get("location")
            .and_then(|header| header.to_str().ok())
            .expect("响应缺少重定向地址");
        let (parts, _) = ::http::Request::get(location)
            .body(())
            .expect("签名重定向地址无效")
            .into_parts();
        let request = Request::from_parts(parts, Body::empty());
        let mode = if absolute {
            SignatureMode::Absolute
        } else {
            SignatureMode::Relative
        };
        assert!(
            self.urls()
                .valid_signature_at(
                    &request,
                    mode,
                    SignatureQueryFilter::Keys(&[]),
                    self.assertion_time()
                )
                .expect("无法验证重定向签名"),
            "重定向签名无效或已过期"
        );
        if let Some(name) = name {
            let expected = self
                .urls()
                .route(name, parameters, true)
                .expect("无法生成预期签名路由");
            let (base, query) = location.split_once('?').unwrap_or((location, ""));
            let parameters: Vec<_> = url::form_urlencoded::parse(query.as_bytes())
                .filter(|(key, _)| key != "signature" && key != "expires")
                .map(|(key, value)| RouteParameter::named(key.into_owned(), value.into_owned()))
                .collect();
            let actual = self
                .urls()
                .query(base, &parameters, &[], None)
                .expect("无法规范化签名重定向地址");
            assert_eq!(
                self.urls().to(&expected, &[], None).unwrap(),
                actual,
                "签名重定向路由不匹配"
            );
        }
        self
    }

    fn raw_cookie(&self, name: &str) -> Option<&str> {
        self.headers
            .get_all("set-cookie")
            .iter()
            .filter_map(|header| header.to_str().ok())
            .find(|header| {
                header
                    .split(';')
                    .next()
                    .and_then(|pair| pair.split_once('='))
                    .is_some_and(|(key, _)| {
                        percent_encoding::percent_decode_str(key.trim())
                            .decode_utf8()
                            .is_ok_and(|key| key == name)
                    })
            })
    }

    fn cookie_expiration(&self, name: &str) -> Option<SystemTime> {
        let cookie = self
            .raw_cookie(name)
            .unwrap_or_else(|| panic!("响应缺少 Cookie {name}"));
        let mut expires = None;
        for attribute in cookie.split(';').skip(1) {
            if let Some((key, value)) = attribute.trim().split_once('=') {
                if key.eq_ignore_ascii_case("max-age")
                    && let Ok(seconds) = value.parse::<i64>()
                {
                    let now = self
                        .response_time
                        .duration_since(UNIX_EPOCH)
                        .expect("测试时钟早于 Unix 纪元")
                        .as_secs();
                    let base = UNIX_EPOCH + Duration::from_secs(now);
                    return if seconds >= 0 {
                        base.checked_add(Duration::from_secs(seconds.unsigned_abs()))
                    } else {
                        base.checked_sub(Duration::from_secs(seconds.unsigned_abs()))
                    };
                }
                if key.eq_ignore_ascii_case("expires") {
                    expires = httpdate::parse_http_date(value).ok();
                }
            }
        }
        expires.filter(|time| *time != UNIX_EPOCH)
    }

    #[track_caller]
    pub fn assertCookieExpired(&self, name: &str) -> &Self {
        assert!(
            self.cookie_expiration(name)
                .is_some_and(|expires| expires < self.assertion_time()),
            "Cookie {name} 尚未过期"
        );
        self
    }

    #[track_caller]
    pub fn assertCookieNotExpired(&self, name: &str) -> &Self {
        assert!(
            self.cookie_expiration(name)
                .is_none_or(|expires| expires > self.assertion_time()),
            "Cookie {name} 已过期"
        );
        self
    }

    #[track_caller]
    pub fn assertCookieMissing(&self, name: &str) -> &Self {
        assert!(self.raw_cookie(name).is_none(), "响应包含 Cookie {name}");
        self
    }

    #[track_caller]
    pub fn assertStreamed(&self) -> &Self {
        assert!(
            self.response_kind == Some(ResponseKind::Stream),
            "响应不是流响应"
        );
        self
    }

    #[track_caller]
    pub fn assertNotStreamed(&self) -> &Self {
        assert!(
            self.response_kind != Some(ResponseKind::Stream),
            "响应不应为流响应"
        );
        self
    }

    #[track_caller]
    pub fn streamedContent(&self) -> &[u8] {
        assert!(self.response_kind.is_some(), "响应不是流响应或文件响应");
        &self.body
    }

    #[track_caller]
    pub fn assertStreamedContent(&self, expected: impl AsRef<[u8]>) -> &Self {
        assert_eq!(
            self.streamedContent(),
            expected.as_ref(),
            "流响应内容不匹配"
        );
        self
    }

    #[track_caller]
    pub fn assertStreamedJsonContent(&self, expected: &Value) -> &Self {
        let encoded = super::json_assertions::encode(expected);
        let encoded: String = encoded
            .chars()
            .map(|character| {
                if character.is_ascii() {
                    character.to_string()
                } else {
                    character
                        .encode_utf16(&mut [0; 2])
                        .iter()
                        .map(|unit| format!("\\u{unit:04x}"))
                        .collect()
                }
            })
            .collect();
        self.assertStreamedContent(encoded)
    }

    fn view(&self) -> &crate::view::RenderedView {
        self.original_view.as_deref().expect("响应不是视图")
    }

    #[track_caller]
    pub fn assertViewIs(&self, name: &str) -> &Self {
        assert_eq!(self.view().name, name, "响应视图名称不匹配");
        self
    }

    #[track_caller]
    pub fn viewData(&self, key: Option<&str>) -> ViewValue {
        let data = &self.view().data;
        key.map_or_else(
            || ViewValue::Object(data.clone()),
            |key| {
                data.get(key)
                    .unwrap_or_else(|| panic!("视图缺少字段 {key}"))
                    .clone()
            },
        )
    }

    #[track_caller]
    pub fn assertViewHas(&self, key: &str, expected: Option<&ViewValue>) -> &Self {
        let actual = view_path(&self.view().data, key);
        if let Some(expected) = expected.filter(|value| !value.is_null()) {
            let actual = actual.unwrap_or(&ViewValue::Null);
            let equal = match (serde_json::to_value(actual), serde_json::to_value(expected)) {
                (Ok(actual), Ok(expected)) => {
                    super::json_assertions::phpunit_equal(&actual, &expected)
                }
                _ => actual == expected,
            };
            assert!(
                equal,
                "视图字段 {key} 不匹配，实际 {actual:?}，预期 {expected:?}"
            );
        } else {
            assert!(actual.is_some(), "视图缺少字段 {key}");
        }
        self
    }

    #[track_caller]
    pub fn assertViewHasAll(&self, bindings: &ViewValue) -> &Self {
        match bindings {
            ViewValue::Array(keys) => {
                for key in keys {
                    self.assertViewHas(key.as_str().expect("视图字段名必须为字符串"), None);
                }
            }
            ViewValue::Object(fields) => {
                for (key, value) in fields {
                    self.assertViewHas(key, Some(value));
                }
            }
            _ => panic!("视图预期必须为字段列表或对象"),
        }
        self
    }

    #[track_caller]
    pub fn assertViewMissing(&self, key: &str) -> &Self {
        assert!(
            view_path(&self.view().data, key).is_none(),
            "视图包含字段 {key}"
        );
        self
    }
}

fn view_path<'a>(data: &'a crate::view::ViewData, path: &str) -> Option<&'a ViewValue> {
    if let Some(value) = data.get(path) {
        return Some(value);
    }
    let mut parts = path.split('.');
    let mut value = data.get(parts.next()?)?;
    for part in parts {
        value = match value {
            ViewValue::Array(items) => part
                .parse::<usize>()
                .ok()
                .filter(|index| index.to_string() == part)
                .and_then(|index| items.get(index)),
            _ => value.get(part),
        }?;
    }
    Some(value)
}
