use crate::http::client::{Error, Factory, PendingRequest};
use serde_json::{Value, json};
use std::{error::Error as StdError, fmt, path::PathBuf, sync::Arc, time::Duration};

/// Agent 连接及服务端故障保留来源，供 Worker 的丢失连接判定追溯。
#[derive(Debug)]
pub struct AgentUnreachable {
    message: String,
    source: Option<Error>,
}
impl fmt::Display for AgentUnreachable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl StdError for AgentUnreachable {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source.as_ref().map(|e| e as &dyn StdError)
    }
}
impl AgentUnreachable {
    fn new(message: impl Into<String>, source: Option<Error>) -> Self {
        Self {
            message: message.into(),
            source,
        }
    }
}
#[derive(Clone)]
pub(crate) struct Agent {
    factory: Arc<Factory>,
    socket: PathBuf,
}
pub(crate) struct Message {
    pub id: String,
    pub receipt: Option<String>,
    pub body: String,
    pub attempts: u32,
    pub queue_url: Option<String>,
}
impl Agent {
    pub fn new(factory: Arc<Factory>, socket: PathBuf) -> Self {
        Self { factory, socket }
    }
    fn request(&self) -> PendingRequest {
        self.factory.without_global_configuration(|http| {
            http.pending_request()
                .base_url("http://localhost")
                .unix_socket(self.socket.clone())
        })
    }
    pub async fn next(&self) -> crate::queue::QueueResult<Option<Message>> {
        let response = self
            .request()
            .timeout(Duration::from_secs(65))
            .retry_delays(vec![Duration::ZERO, Duration::from_millis(500)])
            .throw_on_retry_failure(false)
            .get("/next")
            .await
            .map_err(|error| {
                crate::queue::backend(AgentUnreachable::new(
                    "托管 Agent 运行时连接失败",
                    Some(error),
                ))
            })?;
        if response.status().as_u16() == 204 {
            return Ok(None);
        }
        if response.status().as_u16() != 200 {
            return Err(crate::queue::backend(AgentUnreachable::new(
                format!("托管 Agent GET /next 返回 HTTP {}", response.status()),
                None,
            )));
        }
        let data: Value = response.json().map_err(|error| {
            crate::queue::backend(AgentUnreachable::new(
                "托管 Agent GET /next 未返回数组或对象",
                Some(error),
            ))
        })?;
        if !data.is_object() && !data.is_array() {
            return Err(crate::queue::backend(AgentUnreachable::new(
                "托管 Agent GET /next 未返回数组或对象",
                None,
            )));
        }
        let Some(id) = data
            .get("messageId")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
        else {
            return Ok(None);
        };
        let attempts = data
            .get("attributes")
            .and_then(|v| v.get("ApproximateReceiveCount"))
            .and_then(|v| {
                v.as_str()
                    .and_then(|v| v.parse().ok())
                    .or_else(|| v.as_u64().and_then(|v| u32::try_from(v).ok()))
            })
            .unwrap_or(1);
        Ok(Some(Message {
            id: id.into(),
            receipt: data
                .get("receiptHandle")
                .and_then(Value::as_str)
                .map(str::to_owned),
            body: data
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            attempts,
            queue_url: data
                .get("queueUrl")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }))
    }
    pub async fn report(
        &self,
        id: &str,
        receipt: Option<&str>,
        status: &str,
        delay: Option<u64>,
    ) -> crate::queue::QueueResult<()> {
        let mut body = json!({"messageId":id,"status":status});
        if let Some(receipt) = receipt {
            body["receiptHandle"] = receipt.into()
        }
        if let Some(delay) = delay {
            body["delay"] = delay.into()
        }
        let result = self
            .request()
            .timeout(Duration::from_secs(10))
            .throw()
            .retry(3, Duration::from_millis(100))
            .retry_when(|e| matches!(e, Error::Connection | Error::Timeout))
            .post("/result", &body)
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(error)
                if matches!(&error, Error::Connection | Error::Timeout)
                    || matches!(&error,Error::RequestFailed(response) if response.status().is_server_error()) =>
            {
                Err(crate::queue::backend(AgentUnreachable::new(
                    "托管 Agent 未接受任务结果",
                    Some(error),
                )))
            }
            Err(error) => Err(crate::queue::backend(error)),
        }
    }
}
