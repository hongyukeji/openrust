//! 有界 HTTP 测试响应与不泄漏响应内容的断言。

use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::core::{Error, Result};

/// 已有界读取的响应；断言失败不会把响应体或响应头值写入错误。
///
/// ```
/// # async fn example() -> openrust::Result<()> {
/// use std::time::Duration;
/// use openrust::http::axum::{body::Body, http::{Response, StatusCode}};
/// use openrust::testing::TestResponse;
/// let response = TestResponse::from_response(
///     Response::new(Body::from("{}")), 1024, Duration::from_secs(1),
/// ).await?;
/// response.assert_status(StatusCode::OK)?.assert_json(&serde_json::json!({}))?;
/// # Ok(())
/// # }
/// ```
pub struct TestResponse {
    response: Response<Bytes>,
}

impl TestResponse {
    /// 在指定时间与字节上限内读取响应体。
    ///
    /// 流式响应应保留原始 `Response<Body>`，避免等待一个没有结束的流。
    pub async fn from_response(
        response: Response<Body>,
        max_bytes: usize,
        timeout: Duration,
    ) -> Result<Self> {
        let (parts, body) = response.into_parts();
        let bytes = tokio::time::timeout(timeout, axum::body::to_bytes(body, max_bytes))
            .await
            .map_err(|_| Error::custom("test_response_timeout", "读取测试响应超过期限"))?
            .map_err(|_| {
                Error::custom(
                    "test_response_body_failed",
                    "测试响应体读取失败或超过字节上限",
                )
            })?;
        Ok(Self {
            response: Response::from_parts(parts, bytes),
        })
    }

    /// HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        self.response.status()
    }

    /// 响应头；调用方负责避免将敏感头值写入日志。
    pub fn headers(&self) -> &HeaderMap {
        self.response.headers()
    }

    /// 响应体字节；调用方负责避免将敏感内容写入日志。
    pub fn body(&self) -> &[u8] {
        self.response.body()
    }

    /// 按目标类型解析 JSON，解析失败不回显响应内容。
    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(self.body())
            .map_err(|_| Error::custom("test_response_json_invalid", "测试响应不是有效的目标 JSON"))
    }

    /// 检查状态码，成功时返回自身以便继续断言。
    pub fn assert_status(&self, expected: StatusCode) -> Result<&Self> {
        if self.status() != expected {
            return Err(Error::custom(
                "test_response_status_mismatch",
                "测试响应状态码不匹配",
            ));
        }
        Ok(self)
    }

    /// 检查响应头文本值；错误不包含实际值或预期值。
    pub fn assert_header(&self, name: &str, expected: &str) -> Result<&Self> {
        if self
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            != Some(expected)
        {
            return Err(Error::custom(
                "test_response_header_mismatch",
                "测试响应头不匹配",
            ));
        }
        Ok(self)
    }

    /// 检查完整 JSON 结构相等；错误不包含实际值或预期值。
    pub fn assert_json(&self, expected: &Value) -> Result<&Self> {
        if self.json::<Value>()? != *expected {
            return Err(Error::custom(
                "test_response_json_mismatch",
                "测试响应 JSON 不匹配",
            ));
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn response_assertions_return_safe_errors() -> Result<()> {
        let mut response = Response::new(Body::from(r#"{"token":"secret-sentinel"}"#));
        *response.status_mut() = StatusCode::CREATED;
        response.headers_mut().insert(
            "x-token",
            "secret-sentinel".parse().map_err(Error::internal)?,
        );
        let response = TestResponse::from_response(response, 1024, Duration::from_secs(1)).await?;
        response
            .assert_status(StatusCode::CREATED)?
            .assert_header("x-token", "secret-sentinel")?
            .assert_json(&serde_json::json!({"token":"secret-sentinel"}))?;
        for result in [
            response.assert_status(StatusCode::OK),
            response.assert_header("x-token", "wrong-secret-sentinel"),
            response.assert_json(&serde_json::json!({"token":"wrong-secret-sentinel"})),
        ] {
            let error = result
                .err()
                .ok_or_else(|| Error::custom("test_expected_failure", "预期响应断言失败"))?;
            assert!(!format!("{error:?} {error}").contains("secret-sentinel"));
        }
        Ok(())
    }

    #[tokio::test]
    async fn body_limits_and_timeout_are_explicit() -> Result<()> {
        let oversized = TestResponse::from_response(
            Response::new(Body::from("secret-sentinel")),
            2,
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(
            oversized
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("test_response_body_failed")
        );
        let pending = Body::from_stream(futures_util::stream::pending::<
            std::result::Result<Bytes, std::convert::Infallible>,
        >());
        let timed = TestResponse::from_response(Response::new(pending), 32, Duration::ZERO).await;
        assert_eq!(
            timed.err().map(|error| error.code().to_owned()).as_deref(),
            Some("test_response_timeout")
        );
        Ok(())
    }
}
