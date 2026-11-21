use axum::body::Bytes;

use crate::http::{HeaderMap, StatusCode};

/// 测试请求的状态、响应头、正文及会话快照。
pub struct TestResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
    session: serde_json::Value,
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
            encrypter: None,
        }
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
        assert_eq!(
            crate::support::Arr::get(&actual, path),
            Some(expected),
            "JSON 路径 {path} 不匹配"
        );
        self
    }
    #[track_caller]
    pub fn assertJsonStructure(&self, structure: &serde_json::Value) -> &Self {
        fn check(data: &serde_json::Value, shape: &serde_json::Value) {
            match shape {
                serde_json::Value::Array(keys) => {
                    for key in keys {
                        let key = key.as_str().expect("JSON 结构字段名必须为字符串");
                        assert!(data.get(key).is_some(), "JSON 缺少字段 {key}");
                    }
                }
                serde_json::Value::Object(fields) => {
                    for (key, shape) in fields {
                        if key == "*" {
                            match data {
                                serde_json::Value::Array(values) => {
                                    for value in values {
                                        check(value, shape);
                                    }
                                }
                                serde_json::Value::Object(values) => {
                                    for value in values.values() {
                                        check(value, shape);
                                    }
                                }
                                _ => panic!("JSON 通配结构要求数组或对象"),
                            }
                        } else {
                            check(
                                data.get(key)
                                    .unwrap_or_else(|| panic!("JSON 缺少字段 {key}")),
                                shape,
                            );
                        }
                    }
                }
                _ => panic!("JSON 结构必须为字段列表或嵌套对象"),
            }
        }
        check(
            &self.json::<serde_json::Value>().expect("响应不是有效 JSON"),
            structure,
        );
        self
    }
    #[track_caller]
    pub fn assertJsonValidationErrors(&self, keys: &[&str]) -> &Self {
        let actual: serde_json::Value = self.json().expect("响应不是有效 JSON");
        let errors = actual
            .get("errors")
            .and_then(serde_json::Value::as_object)
            .expect("响应缺少 errors 对象");
        for key in keys {
            assert!(errors.contains_key(*key), "响应缺少验证错误 {key}");
        }
        self
    }
    #[track_caller]
    pub fn assertRedirect(&self, target: Option<&str>) -> &Self {
        assert!(
            matches!(self.status.as_u16(), 201 | 301 | 302 | 303 | 307 | 308),
            "响应不是重定向"
        );
        if let Some(target) = target {
            self.assertHeader("location", target);
        }
        self
    }
    #[track_caller]
    pub fn assertSessionHas(&self, key: &str, expected: Option<&serde_json::Value>) -> &Self {
        let actual = crate::support::Arr::get(&self.session, key)
            .unwrap_or_else(|| panic!("Session 缺少 {key}"));
        if let Some(expected) = expected {
            assert_eq!(actual, expected, "Session 字段 {key} 不匹配");
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
    pub fn assertHeader(&self, name: &str, expected: &str) -> &Self {
        let actual = self.headers.get(name).and_then(|value| value.to_str().ok());
        assert_eq!(actual, Some(expected), "response header {name}");
        self
    }

    #[track_caller]
    pub fn assertContent(&self, expected: impl AsRef<[u8]>) -> &Self {
        assert_eq!(self.body(), expected.as_ref());
        self
    }

    #[track_caller]
    pub fn assertExactJson(&self, expected: &serde_json::Value) -> &Self {
        let actual: serde_json::Value = self.json().expect("response body is valid JSON");
        assert_eq!(&actual, expected);
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
