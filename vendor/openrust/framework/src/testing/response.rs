use axum::body::Bytes;

use crate::http::{HeaderMap, StatusCode};

/// The status, headers, and captured body of a test request.
#[derive(Debug)]
pub struct TestResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

#[allow(non_snake_case, reason = "Matches Laravel's HTTP testing assertions")]
impl TestResponse {
    pub(super) fn new(status: StatusCode, headers: HeaderMap, body: Bytes) -> Self {
        Self {
            status,
            headers,
            body,
        }
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
