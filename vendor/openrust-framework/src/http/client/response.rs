use http::{HeaderMap, StatusCode, header};
use serde::de::DeserializeOwned;

use super::{Error, Result, transport_error};

/// Transport timing for the final HTTP hop. Fake responses have no network handler stats.
#[derive(Clone, Debug)]
pub struct TransferStats {
    pub(super) duration: std::time::Duration,
    pub(super) network: bool,
}
impl TransferStats {
    /// 实际网络跳的耗时。模拟响应没有网络测量值。
    pub fn network_duration(&self) -> Option<std::time::Duration> {
        self.network.then_some(self.duration)
    }

    pub fn get_transfer_time(&self) -> f64 {
        self.duration.as_secs_f64()
    }
    pub fn get_handler_stats(&self) -> std::collections::HashMap<&'static str, f64> {
        if self.network {
            [("total_time", self.get_transfer_time())].into()
        } else {
            Default::default()
        }
    }
}

/// 出站响应正文的异步字节流。
pub type ResponseStream =
    std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<axum::body::Bytes>> + Send>>;

/// 出站 HTTP 响应。普通正文可重复读取，流式正文可消费一次。
#[derive(Clone)]
pub struct Response {
    pub transfer_stats: Option<TransferStats>,
    status: StatusCode,
    headers: HeaderMap,
    body: super::body::Body,
}

impl std::fmt::Debug for Response {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Response")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl Response {
    pub fn new(status: StatusCode, headers: HeaderMap, body: impl Into<Vec<u8>>) -> Self {
        Self {
            transfer_stats: None,
            status,
            headers,
            body: super::body::Body::new(axum::body::Bytes::from(body.into())),
        }
    }

    /// Returns measured transport statistics; durations are seconds, never fabricated phase timings.
    pub fn handler_stats(&self) -> std::collections::HashMap<&'static str, f64> {
        self.transfer_stats
            .as_ref()
            .map(TransferStats::get_handler_stats)
            .unwrap_or_default()
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn header(&self, name: &str) -> Option<&http::HeaderValue> {
        self.headers.get(name)
    }

    /// 按需读取完整正文；文件响应在调用此方法时才分配完整内存。
    pub fn body(&self) -> Result<axum::body::Bytes> {
        self.body.read().map_err(Error::ResponseBody)
    }

    /// 读取流式正文；缓冲响应（包括 HTTP Fake）作为单块读取。
    pub fn into_stream(self) -> Result<ResponseStream> {
        self.body.take_stream()
    }

    pub fn resource(&self) -> Result<super::ResponseResource> {
        self.body.resource().map_err(Error::ResponseBody)
    }

    pub fn close(&self) -> Result<&Self> {
        self.body.close().map_err(Error::ResponseBody)?;
        Ok(self)
    }

    pub(super) fn body_len(&self) -> Result<u64> {
        self.body.len().map_err(Error::ResponseBody)
    }

    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.body()?).map_err(Error::InvalidJson)
    }

    pub fn successful(&self) -> bool {
        self.status.is_success()
    }

    pub fn redirect(&self) -> bool {
        self.status.is_redirection()
    }

    pub fn client_error(&self) -> bool {
        self.status.is_client_error()
    }

    pub fn server_error(&self) -> bool {
        self.status.is_server_error()
    }

    pub fn failed(&self) -> bool {
        self.client_error() || self.server_error()
    }

    pub fn ok(&self) -> bool {
        self.status == StatusCode::OK
    }

    pub fn created(&self) -> bool {
        self.status == StatusCode::CREATED
    }

    pub fn accepted(&self) -> bool {
        self.status == StatusCode::ACCEPTED
    }

    pub fn no_content(&self) -> bool {
        self.status == StatusCode::NO_CONTENT
    }

    pub fn moved_permanently(&self) -> bool {
        self.status == StatusCode::MOVED_PERMANENTLY
    }

    pub fn found(&self) -> bool {
        self.status == StatusCode::FOUND
    }

    pub fn bad_request(&self) -> bool {
        self.status == StatusCode::BAD_REQUEST
    }

    pub fn unauthorized(&self) -> bool {
        self.status == StatusCode::UNAUTHORIZED
    }

    pub fn payment_required(&self) -> bool {
        self.status == StatusCode::PAYMENT_REQUIRED
    }

    pub fn forbidden(&self) -> bool {
        self.status == StatusCode::FORBIDDEN
    }

    pub fn not_found(&self) -> bool {
        self.status == StatusCode::NOT_FOUND
    }

    pub fn request_timeout(&self) -> bool {
        self.status == StatusCode::REQUEST_TIMEOUT
    }

    pub fn conflict(&self) -> bool {
        self.status == StatusCode::CONFLICT
    }

    pub fn unprocessable_entity(&self) -> bool {
        self.status == StatusCode::UNPROCESSABLE_ENTITY
    }

    pub fn too_many_requests(&self) -> bool {
        self.status == StatusCode::TOO_MANY_REQUESTS
    }

    pub fn throw(self) -> Result<Self> {
        if self.failed() {
            return Err(Error::RequestFailed(Box::new(self)));
        }
        Ok(self)
    }

    pub(super) async fn from_transport(
        mut response: reqwest::Response,
        limit: usize,
        mut sink: Option<super::sink::Writer>,
    ) -> Result<Self> {
        let mut body = Vec::new();
        let mut received = 0usize;
        // HEAD and 304 may advertise the size of a representation they do not send.
        if response
            .content_length()
            .is_some_and(|size| size > limit as u64)
        {
            return Err(Error::BodyTooLarge);
        }
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if chunk.len() > limit.saturating_sub(received) {
                return Err(Error::BodyTooLarge);
            }
            received += chunk.len();
            if let Some(sink) = &mut sink {
                sink.write(&chunk).await?;
            } else {
                body.extend_from_slice(&chunk);
            }
        }
        let headers = std::mem::take(response.headers_mut());
        if let Some(sink) = sink {
            Ok(Self {
                transfer_stats: None,
                status: response.status(),
                headers,
                body: super::body::Body::file(sink.finish().await?),
            })
        } else {
            Ok(Self::new(response.status(), headers, body))
        }
    }

    pub(super) fn from_transport_stream(
        mut response: reqwest::Response,
        limit: usize,
        deadline: tokio::time::Instant,
    ) -> Result<Self> {
        if response
            .content_length()
            .is_some_and(|size| size > limit as u64)
        {
            return Err(Error::BodyTooLarge);
        }
        let status = response.status();
        let headers = std::mem::take(response.headers_mut());
        let stream = async_stream::try_stream! {
            let mut remaining = limit;
            while let Some(chunk) = tokio::time::timeout_at(deadline, response.chunk())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(transport_error)?
            {
                remaining = remaining.checked_sub(chunk.len()).ok_or(Error::BodyTooLarge)?;
                yield chunk;
            }
        };
        Ok(Self {
            transfer_stats: None,
            status,
            headers,
            body: super::body::Body::stream(Box::pin(stream)),
        })
    }

    pub fn from_json(status: StatusCode, value: &impl serde::Serialize) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );
        Ok(Self::new(
            status,
            headers,
            serde_json::to_vec(value).map_err(Error::InvalidJson)?,
        ))
    }
}
