use axum::body::Bytes;

use crate::http::{HeaderMap, StatusCode};

/// 测试请求的状态、响应头、正文及会话快照。
pub struct TestResponse {
    pub(super) status: StatusCode,
    pub(super) headers: HeaderMap,
    pub(super) body: Bytes,
    pub(super) session: serde_json::Value,
    pub(super) response_kind: Option<crate::http::ResponseKind>,
    pub(super) original_view: Option<std::sync::Arc<crate::view::RenderedView>>,
    pub(super) urls: Option<crate::routing::UrlGenerator>,
    pub(super) response_time: std::time::SystemTime,
    pub(super) clock: std::sync::Arc<crate::support::TestClock>,
    encrypter: Option<std::sync::Arc<dyn crate::contracts::encryption::Encrypter>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's HTTP testing assertions")]
impl TestResponse {
    pub(super) fn new(status: StatusCode, headers: HeaderMap, body: Bytes) -> Self {
        Self {
            status,
            headers,
            body,
            session: serde_json::Value::Null,
            response_kind: None,
            original_view: None,
            urls: None,
            response_time: std::time::SystemTime::now(),
            clock: std::sync::Arc::new(crate::support::TestClock::default()),
            encrypter: None,
        }
    }

    pub(super) fn with_original(
        mut self,
        kind: Option<crate::http::ResponseKind>,
        view: Option<std::sync::Arc<crate::view::RenderedView>>,
        urls: crate::routing::UrlGenerator,
        clock: std::sync::Arc<crate::support::TestClock>,
        response_time: std::time::SystemTime,
    ) -> Self {
        self.response_kind = kind;
        self.original_view = view;
        self.urls = Some(urls);
        self.clock = clock;
        self.response_time = response_time;
        self
    }

    pub(super) fn with_session(mut self, session: serde_json::Value) -> Self {
        self.session = session;
        self
    }

    pub(super) fn with_encrypter(
        mut self,
        encrypter: Option<std::sync::Arc<dyn crate::contracts::encryption::Encrypter>>,
    ) -> Self {
        self.encrypter = encrypter;
        self
    }

    #[track_caller]
    pub fn assertJsonPath(&self, path: &str, expected: &serde_json::Value) -> &Self {
        let actual: serde_json::Value = self.json().expect("响应不是有效 JSON");
        let actual =
            crate::support::data_get(&actual, Some(path)).unwrap_or(serde_json::Value::Null);
        assert!(
            super::json_assertions::strict_equal(&actual, expected),
            "JSON 路径 {path} 不匹配，实际 {actual}，预期 {expected}"
        );
        self
    }

    #[track_caller]
    pub fn assertJsonStructure(&self, structure: &serde_json::Value) -> &Self {
        super::json_assertions::check_structure(
            &self.json::<serde_json::Value>().expect("响应不是有效 JSON"),
            structure,
            false,
        );
        self
    }
    #[track_caller]
    pub fn assertJsonValidationErrors(&self, keys: &[&str]) -> &Self {
        self.assertJsonValidationErrorsAt(&serde_json::json!(keys), "errors")
    }
    #[track_caller]
    pub fn assertRedirect(&self, target: Option<&str>) -> &Self {
        assert!(
            matches!(self.status.as_u16(), 201 | 301 | 302 | 303 | 307 | 308),
            "响应不是重定向"
        );
        if let Some(target) = target {
            self.assertLocation(target);
        }
        self
    }
    #[track_caller]
    pub fn assertSessionHas(&self, key: &str, expected: Option<&serde_json::Value>) -> &Self {
        let actual =
            crate::support::Arr::get(&self.session, key).unwrap_or(&serde_json::Value::Null);
        if let Some(expected) = expected.filter(|value| !value.is_null()) {
            assert!(
                super::json_assertions::phpunit_equal(actual, expected),
                "Session 字段 {key} 不匹配，实际 {actual}，预期 {expected}"
            );
        } else {
            assert!(!actual.is_null(), "Session 缺少 {key}");
        }
        self
    }
    fn cookie_value(&self, name: &str) -> String {
        self.headers
            .get_all("set-cookie")
            .iter()
            .filter_map(|header| header.to_str().ok())
            .filter_map(|header| header.split(';').next())
            .find_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                let key = percent_encoding::percent_decode_str(key.trim())
                    .decode_utf8()
                    .ok()?;
                (key == name).then(|| {
                    percent_encoding::percent_decode_str(value)
                        .decode_utf8()
                        .expect("Cookie 编码无效")
                        .into_owned()
                })
            })
            .unwrap_or_else(|| panic!("响应缺少 Cookie {name}"))
    }
    #[track_caller]
    pub fn assertCookie(&self, name: &str, expected: Option<&str>) -> &Self {
        let cookie = self.cookie_value(name);
        if let Some(expected) = expected {
            let encrypter = self.encrypter.as_ref().expect("应用没有绑定 Cookie 解密器");
            let clear = encrypter.decryptString(&cookie).expect("Cookie 解密失败");
            let clear =
                crate::cookie::CookieValuePrefix::validate(name, &clear, encrypter.getAllKeys())
                    .expect("Cookie 名称前缀无效");
            assert_eq!(
                std::str::from_utf8(clear).expect("Cookie 不是 UTF-8"),
                expected,
                "Cookie {name} 不匹配"
            );
        }
        self
    }
    #[track_caller]
    pub fn assertPlainCookie(&self, name: &str, expected: Option<&str>) -> &Self {
        let cookie = self.cookie_value(name);
        if let Some(expected) = expected {
            assert_eq!(cookie, expected, "Cookie {name} 不匹配");
        }
        self
    }
    #[track_caller]
    pub fn assertDownload(&self, filename: Option<&str>) -> &Self {
        let disposition = self
            .headers
            .get("content-disposition")
            .and_then(|value| value.to_str().ok())
            .expect("响应缺少下载头");
        assert_eq!(
            disposition.split(';').next().map(str::trim),
            Some("attachment"),
            "响应不是附件下载"
        );
        if let Some(filename) = filename {
            let actual = disposition
                .split(';')
                .find_map(|part| part.trim().strip_prefix("filename="))
                .map(|value| value.trim_matches('"'));
            assert_eq!(actual, Some(filename), "下载文件名不匹配");
        }
        self
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }

    #[track_caller]
    pub fn assertStatus(&self, expected: StatusCode) -> &Self {
        assert_eq!(self.status, expected);
        self
    }

    #[track_caller]
    pub fn assertOk(&self) -> &Self {
        self.assertStatus(StatusCode::OK)
    }

    #[track_caller]
    pub fn assertHeader<'a>(
        &self,
        name: &str,
        expected: impl Into<HeaderExpectation<'a>>,
    ) -> &Self {
        let actual = self
            .headers
            .get(name)
            .unwrap_or_else(|| panic!("响应缺少头 {name}"));
        if let Some(expected) = expected.into().0 {
            assert!(
                actual
                    .to_str()
                    .is_ok_and(|actual| actual.eq_ignore_ascii_case(expected)),
                "响应头 {name} 不匹配"
            );
        }
        self
    }

    #[track_caller]
    pub fn assertHeaderContains(&self, name: &str, value: &str) -> &Self {
        self.assertHeader(name, None);
        assert!(
            !value.is_empty()
                && self
                    .headers
                    .get(name)
                    .and_then(|header| header.to_str().ok())
                    .is_some_and(|actual| actual.contains(value)),
            "响应头 {name} 不包含 {value}"
        );
        self
    }

    #[track_caller]
    pub fn assertHeaderMissing(&self, name: &str) -> &Self {
        assert!(
            !self.headers.contains_key(name),
            "响应包含不应出现的头 {name}"
        );
        self
    }

    #[track_caller]
    pub fn assertSuccessful(&self) -> &Self {
        assert!(
            self.status.is_success(),
            "响应状态 {} 不在 200..300 范围",
            self.status
        );
        self
    }

    #[track_caller]
    pub fn assertClientError(&self) -> &Self {
        assert!(
            self.status.is_client_error(),
            "响应状态 {} 不在 400..500 范围",
            self.status
        );
        self
    }

    #[track_caller]
    pub fn assertServerError(&self) -> &Self {
        assert!(
            self.status.is_server_error(),
            "响应状态 {} 不在 500..600 范围",
            self.status
        );
        self
    }

    #[track_caller]
    pub fn assertNoContent(&self, status: Option<StatusCode>) -> &Self {
        self.assertStatus(status.unwrap_or(StatusCode::NO_CONTENT));
        // PHPUnit 对字符串采用 PHP empty 语义，字符串 0 同样为空。
        assert!(
            self.body.is_empty() || self.body.as_ref() == b"0",
            "响应正文不为空"
        );
        self
    }

    #[track_caller]
    pub fn assertSuccessfulPrecognition(&self) -> &Self {
        self.assertNoContent(None)
            .assertHeader("precognition-success", "true")
    }

    #[track_caller]
    pub fn assertContent(&self, expected: impl AsRef<[u8]>) -> &Self {
        assert_eq!(self.body(), expected.as_ref());
        self
    }

    #[track_caller]
    pub fn assertExactJson(&self, expected: &serde_json::Value) -> &Self {
        let actual: serde_json::Value = self.json().expect("响应不是有效 JSON");
        assert!(
            super::json_assertions::exact_json(&actual, expected),
            "JSON 内容不完全匹配，实际 {actual}，预期 {expected}"
        );
        self
    }
}

impl std::fmt::Debug for TestResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TestResponse")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body", &self.body)
            .finish_non_exhaustive()
    }
}

macro_rules! status_assertions {
    ($($method:ident => $status:ident),+ $(,)?) => {
        #[allow(non_snake_case)]
        impl TestResponse {
            $(#[track_caller]
            pub fn $method(&self) -> &Self {
                self.assertStatus(StatusCode::$status)
            })+
        }
    };
}

status_assertions! {
    assertCreated => CREATED,
    assertAccepted => ACCEPTED,
    assertMovedPermanently => MOVED_PERMANENTLY,
    assertFound => FOUND,
    assertNotModified => NOT_MODIFIED,
    assertTemporaryRedirect => TEMPORARY_REDIRECT,
    assertPermanentRedirect => PERMANENT_REDIRECT,
    assertBadRequest => BAD_REQUEST,
    assertUnauthorized => UNAUTHORIZED,
    assertPaymentRequired => PAYMENT_REQUIRED,
    assertForbidden => FORBIDDEN,
    assertNotFound => NOT_FOUND,
    assertMethodNotAllowed => METHOD_NOT_ALLOWED,
    assertNotAcceptable => NOT_ACCEPTABLE,
    assertRequestTimeout => REQUEST_TIMEOUT,
    assertConflict => CONFLICT,
    assertGone => GONE,
    assertUnsupportedMediaType => UNSUPPORTED_MEDIA_TYPE,
    assertUnprocessable => UNPROCESSABLE_ENTITY,
    assertFailedDependency => FAILED_DEPENDENCY,
    assertTooManyRequests => TOO_MANY_REQUESTS,
    assertInternalServerError => INTERNAL_SERVER_ERROR,
    assertServiceUnavailable => SERVICE_UNAVAILABLE,
}

/// 单个字符串或字符串列表，用于正文断言的类型化输入。
pub struct ContentValues(Vec<String>);

impl From<&str> for ContentValues {
    fn from(value: &str) -> Self {
        Self(vec![value.to_owned()])
    }
}
impl From<String> for ContentValues {
    fn from(value: String) -> Self {
        Self(vec![value])
    }
}
impl From<&String> for ContentValues {
    fn from(value: &String) -> Self {
        Self::from(value.as_str())
    }
}
impl From<Vec<String>> for ContentValues {
    fn from(values: Vec<String>) -> Self {
        Self(values)
    }
}
impl From<&[&str]> for ContentValues {
    fn from(values: &[&str]) -> Self {
        Self(values.iter().map(|value| (*value).to_owned()).collect())
    }
}
impl<const N: usize> From<[&str; N]> for ContentValues {
    fn from(values: [&str; N]) -> Self {
        Self::from(values.as_slice())
    }
}
impl<const N: usize> From<&[&str; N]> for ContentValues {
    fn from(values: &[&str; N]) -> Self {
        Self::from(values.as_slice())
    }
}

fn escaped(value: &str, escape: bool) -> String {
    if escape {
        let mut result = String::new();
        crate::support::escape_html(&mut result, value);
        result
    } else {
        value.to_owned()
    }
}

fn normalized_text(value: &str) -> String {
    use std::sync::LazyLock;
    static WHITESPACE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\s+").expect("空白匹配表达式"));
    let stripped = crate::support::Stringable::from(value)
        .stripTags(&[])
        .into_string();
    let decoded = crate::support::decode_html_entities(&stripped);
    WHITESPACE
        .replace_all(
            decoded.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b']),
            " ",
        )
        .into_owned()
}

#[allow(non_snake_case)]
impl TestResponse {
    #[track_caller]
    pub fn assertSee(&self, values: impl Into<ContentValues>, escape: bool) -> &Self {
        self.check_raw(values.into(), escape, false)
    }
    #[track_caller]
    pub fn assertDontSee(&self, values: impl Into<ContentValues>, escape: bool) -> &Self {
        self.check_raw(values.into(), escape, true)
    }
    #[track_caller]
    pub fn assertSeeHtml(&self, values: impl Into<ContentValues>) -> &Self {
        self.assertSee(values, false)
    }
    #[track_caller]
    pub fn assertDontSeeHtml(&self, values: impl Into<ContentValues>) -> &Self {
        self.assertDontSee(values, false)
    }

    #[track_caller]
    fn check_raw(&self, values: ContentValues, escape: bool, negate: bool) -> &Self {
        for value in values.0 {
            let needle = escaped(&value, escape);
            let found = needle.is_empty()
                || self
                    .body
                    .windows(needle.len())
                    .any(|window| window == needle.as_bytes());
            assert_ne!(
                found,
                negate,
                "响应正文{}包含 {value:?}",
                if negate { "不应" } else { "应" }
            );
        }
        self
    }

    #[track_caller]
    pub fn assertSeeInOrder(&self, values: &[&str], escape: bool) -> &Self {
        let content = std::str::from_utf8(&self.body).expect("顺序断言要求 UTF-8 响应正文");
        let decoded = crate::support::decode_html_entities(content);
        let mut remaining = decoded.as_ref();
        for value in values {
            let value = escaped(value, escape);
            if value.is_empty() || value == "0" {
                continue;
            }
            let needle = crate::support::decode_html_entities(&value);
            let offset = remaining
                .find(needle.as_ref())
                .unwrap_or_else(|| panic!("响应正文未按顺序包含 {value:?}"));
            remaining = &remaining[offset + needle.len()..];
        }
        self
    }
    #[track_caller]
    pub fn assertSeeHtmlInOrder(&self, values: &[&str]) -> &Self {
        self.assertSeeInOrder(values, false)
    }

    #[track_caller]
    pub fn assertSeeText(&self, values: impl Into<ContentValues>, escape: bool) -> &Self {
        self.check_text(values.into(), escape, false, false)
    }
    #[track_caller]
    pub fn assertDontSeeText(&self, values: impl Into<ContentValues>, escape: bool) -> &Self {
        self.check_text(values.into(), escape, false, true)
    }
    #[track_caller]
    pub fn assertSeeTextInOrder(&self, values: &[&str], escape: bool) -> &Self {
        self.check_text(values.into(), escape, true, false)
    }

    #[track_caller]
    fn check_text(
        &self,
        values: ContentValues,
        escape: bool,
        ordered: bool,
        negate: bool,
    ) -> &Self {
        let content = std::str::from_utf8(&self.body).expect("文本断言要求 UTF-8 响应正文");
        let content = normalized_text(content);
        let mut remaining = content.as_str();
        for value in values.0 {
            let value = escaped(&value, escape);
            if value
                .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                .is_empty()
            {
                continue;
            }
            let needle = normalized_text(&value);
            let offset = remaining.find(&needle);
            assert_ne!(
                offset.is_some(),
                negate,
                "响应文本{}包含 {value:?}",
                if negate { "不应" } else { "应" }
            );
            if ordered {
                remaining = &remaining[offset.expect("已检查文本存在") + needle.len()..];
            }
        }
        self
    }
}

/// 响应头的预期文本；None 仅检查响应头存在。
pub struct HeaderExpectation<'a>(Option<&'a str>);

impl<'a> From<&'a str> for HeaderExpectation<'a> {
    fn from(value: &'a str) -> Self {
        Self(Some(value))
    }
}
impl<'a> From<&'a String> for HeaderExpectation<'a> {
    fn from(value: &'a String) -> Self {
        Self(Some(value.as_str()))
    }
}
impl<'a> From<Option<&'a str>> for HeaderExpectation<'a> {
    fn from(value: Option<&'a str>) -> Self {
        Self(value)
    }
}
