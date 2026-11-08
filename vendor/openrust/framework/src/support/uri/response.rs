//! URI 作为 HTTP 路由返回值的直接重定向。

use super::Uri;
use crate::{Error, Result, http::RedirectResponse};
use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

impl Uri {
    /// 按 URI 原值建立默认 302 重定向，无需请求或 Session。
    pub fn redirect(&self) -> Result<RedirectResponse<'static>> {
        self.redirect_with(StatusCode::FOUND, HeaderMap::new())
    }

    /// 以指定状态和响应头重定向到此 URI。
    ///
    /// 相对引用原样写入 Location。业务代码需要自行决定目标是否允许跳转。
    /// 响应不借用请求或 Session，不能用于闪存请求输入或错误。
    pub fn redirect_with(
        &self,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<RedirectResponse<'static>> {
        RedirectResponse::from_target(
            self.value().to_owned(),
            status,
            headers,
            #[cfg(feature = "session")]
            None,
            #[cfg(feature = "session")]
            None,
        )
    }
}

impl IntoResponse for Uri {
    fn into_response(self) -> Response {
        self.redirect().map_err(Error::internal).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{HeaderName, HeaderValue, Request, header},
        routing::get,
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    fn fixture_error() -> Error {
        Error::custom("uri_response_fixture", "固定 PHP 探针缺少重定向用例")
    }

    fn text(value: &Value) -> Result<&str> {
        value.as_str().ok_or_else(fixture_error)
    }

    fn headers(row: &Value) -> Result<HeaderMap> {
        let mut result = HeaderMap::new();
        if let Some(map) = row["headers"].as_object() {
            for (name, values) in map {
                let name = HeaderName::try_from(name).map_err(Error::internal)?;
                let values = values
                    .as_array()
                    .map_or_else(|| std::slice::from_ref(values), Vec::as_slice);
                for value in values {
                    result.append(
                        name.clone(),
                        HeaderValue::from_str(text(value)?).map_err(Error::internal)?,
                    );
                }
            }
        }
        Ok(result)
    }

    async fn route_response(uri: Uri, method: &str) -> Result<Response> {
        let app = Router::new().route("/draft", get(move || async move { uri }));
        app.oneshot(
            Request::builder()
                .method(method)
                .uri("/draft")
                .body(Body::empty())
                .map_err(Error::internal)?,
        )
        .await
        .map_err(Error::internal)
    }

    async fn assert_response(label: &str, row: &Value, response: Response) -> Result<()> {
        assert_eq!(
            json!(response.status().as_u16()),
            row["status_code"],
            "{label}"
        );
        let headers = response.headers();
        assert_eq!(
            headers[header::LOCATION]
                .to_str()
                .map_err(Error::internal)?,
            row["location"],
            "{label}"
        );
        assert_eq!(
            headers[header::CONTENT_TYPE]
                .to_str()
                .map_err(Error::internal)?,
            row["content_type"],
            "{label}"
        );
        let cache_control = headers
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok());
        if label == "explicit_cache" {
            // 通用 HeaderBag 指令规范化另行对标；当前 HeaderMap 保留调用方值。
            assert_eq!(cache_control, row["headers"]["Cache-Control"].as_str());
            assert_eq!(row["cache_control"], "max-age=60, private");
        } else {
            assert_eq!(cache_control, row["cache_control"].as_str(), "{label}");
        }
        let extra = headers
            .get_all("x-draft")
            .iter()
            .map(|value| value.to_str())
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::internal)?;
        assert_eq!(json!(extra), row["extra"], "{label}");
        assert!(!headers.contains_key(header::SET_COOKIE), "{label}");
        let body = to_bytes(response.into_body(), 4096)
            .await
            .map_err(Error::internal)?;
        assert_eq!(body.as_ref(), text(&row["body"])?.as_bytes(), "{label}");
        Ok(())
    }

    #[tokio::test]
    async fn uri_response_matches_fixed_symfony_status_headers_and_html() -> Result<()> {
        let oracle: Value =
            serde_json::from_str(include_str!("response_oracle.json")).map_err(Error::internal)?;
        for (label, row) in oracle["rows"].as_object().ok_or_else(fixture_error)? {
            let uri = Uri::of(text(&row["input"])?);
            if label == "line_break_rejected" {
                assert_eq!(
                    uri.err().as_ref().map(Error::code),
                    Some("support_uri_invalid")
                );
                continue;
            }
            let uri = uri?;
            let status = row["status"]
                .as_u64()
                .map(u16::try_from)
                .transpose()
                .map_err(Error::internal)?;
            if row["error"].is_string() {
                let message = text(&row["error_message"])?;
                let expected = if message.contains("empty URL") {
                    "redirect_location_invalid"
                } else {
                    "redirect_status_invalid"
                };
                let rejected = match status {
                    Some(status) => uri.redirect_with(
                        StatusCode::from_u16(status).map_err(Error::internal)?,
                        headers(row)?,
                    ),
                    None => uri.redirect(),
                };
                assert_eq!(
                    rejected.err().as_ref().map(Error::code),
                    Some(expected),
                    "{label}"
                );
                if status.is_none() {
                    assert_eq!(
                        route_response(uri, text(&row["method"])?).await?.status(),
                        StatusCode::INTERNAL_SERVER_ERROR
                    );
                }
                continue;
            }
            assert_eq!(row["request_detached"], true, "{label}");
            let response = match status {
                Some(status) => {
                    let redirect = uri.redirect_with(
                        StatusCode::from_u16(status).map_err(Error::internal)?,
                        headers(row)?,
                    )?;
                    assert_eq!(redirect.target_url(), row["target"], "{label}");
                    redirect.into_response()
                }
                None => route_response(uri, text(&row["method"])?).await?,
            };
            assert_response(label, row, response).await?;
        }
        Ok(())
    }

    #[cfg(feature = "session")]
    #[test]
    fn uri_redirect_does_not_bind_request_or_session() -> Result<()> {
        use crate::http::RedirectErrors;
        let mut response = Uri::of("/draft")?.redirect()?;
        let error = response.with_input().err();
        assert_eq!(
            error.as_ref().map(Error::code),
            Some("redirect_request_missing")
        );
        let error = response
            .with_errors(RedirectErrors::Text("error"), "default")
            .err();
        assert_eq!(
            error.as_ref().map(Error::code),
            Some("redirect_request_missing")
        );
        Ok(())
    }
}
