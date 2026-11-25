//! 可选扩展观测执行作用域的接缝。

use std::{
    error::Error,
    future::Future,
    sync::{Arc, Weak},
    task::Poll,
};

use futures_util::Stream;

use crate::{
    container::Container,
    contracts::container::{BindingResolutionError, Container as _},
};

/// 框架开始执行的操作。只传递埋点所需的元数据。
#[non_exhaustive]
pub enum Operation<'a> {
    Http {
        method: &'a http::Method,
        headers: &'a http::HeaderMap,
    },
    HttpClient {
        method: &'a http::Method,
        origin: &'a url::Origin,
        headers: &'a mut http::HeaderMap,
    },
    Command {
        name: &'a str,
    },
    Sql {
        system: &'static str,
        operation: Option<&'static str>,
    },
    JobDispatch {
        destination: &'a str,
        trace: &'a mut Option<TraceContext>,
    },
    JobCreate {
        destination: &'a str,
        trace: &'a mut Option<TraceContext>,
    },
    Job {
        name: &'a str,
        attempt: u32,
        trace: Option<&'a TraceContext>,
    },
    JobExecution {
        name: &'a str,
        parent: Option<&'a TraceContext>,
    },
}

/// 跨进程携带的 W3C 消息创建上下文。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TraceContext {
    traceparent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tracestate: Option<String>,
}

impl TraceContext {
    pub fn from_headers(headers: &http::HeaderMap) -> Option<Self> {
        let traceparent = headers.get("traceparent")?.to_str().ok()?;
        if traceparent.len() > 512 {
            return None;
        }
        let tracestate = headers
            .get("tracestate")
            .and_then(|value| value.to_str().ok())
            .filter(|value| value.len() <= 512)
            .map(str::to_owned);
        Some(Self {
            traceparent: traceparent.to_owned(),
            tracestate,
        })
    }

    pub fn headers(&self) -> http::HeaderMap {
        let mut headers = http::HeaderMap::new();
        if self.traceparent.len() <= 512
            && let Ok(value) = http::HeaderValue::from_str(&self.traceparent)
        {
            headers.insert("traceparent", value);
        }
        if let Some(state) = &self.tracestate
            && state.len() <= 512
            && let Ok(value) = http::HeaderValue::from_str(state)
        {
            headers.insert("tracestate", value);
        }
        headers
    }
}

pub(crate) fn deserialize_trace<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<TraceContext>, D::Error> {
    let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
    // 旧版本或第三方生产者携带不认识的字段时，任务仍然可执行。
    let trace = serde_json::from_value::<TraceContext>(value).ok();
    Ok(trace.filter(|trace| trace.headers().contains_key("traceparent")))
}

/// 操作的终态。错误文案由单独的异常报告管线处理。
#[non_exhaustive]
#[derive(Clone, Copy)]
pub enum Outcome {
    Http(http::StatusCode),
    Command(i32),
    HttpClient(Result<http::StatusCode, &'static str>),
    Job(JobOutcome),
    JobDispatch { failed: bool },
    Sql { failed: bool },
    Failed,
    Cancelled,
}

/// 队列执行的终态。
#[non_exhaustive]
#[derive(Clone, Copy)]
pub enum JobOutcome {
    Processed,
    Released,
    TimedOut,
    Failed,
    Idle,
}

/// 共享异常的稳定身份。持有它不会保留异常内容，也不会复用已经报告的地址。
pub struct ExceptionIdentity(Weak<dyn Error + Send + Sync>);

impl ExceptionIdentity {
    pub(crate) fn new(identity: Weak<dyn Error + Send + Sync>) -> Self {
        Self(identity)
    }
}

impl PartialEq for ExceptionIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }
}

impl Eq for ExceptionIdentity {}

/// 一个应用的观测扩展。
pub trait Observer: Send + Sync + 'static {
    fn begin(&self, operation: Operation<'_>) -> Option<Box<dyn Observation>>;

    /// 接收通过框架报告策略的异常。
    fn report(&self, _identity: Option<ExceptionIdentity>, _snapshot: &serde_json::Value) {}

    fn current_trace(&self) -> Option<TraceContext> {
        None
    }
}

/// 对一次执行建立上下文，且在结束或取消时收尾。
pub trait Observation: Send + Sync {
    /// 同步调用一次 poll。上下文守卫应在返回前退出。
    fn in_scope(&self, poll: &mut dyn FnMut());
    fn finish(&mut self, outcome: Outcome);
}

struct Registered(Arc<dyn Observer>);

/// 为当前应用登记观察者。
pub fn register(
    container: &Container,
    observer: Arc<dyn Observer>,
) -> Result<(), BindingResolutionError> {
    container.instance(Arc::new(Registered(observer)))?;
    Ok(())
}

pub(crate) fn begin_sql(
    owner: Option<Arc<Container>>,
    system: &'static str,
    sql: &str,
) -> Execution {
    let Some(owner) = owner else {
        return Execution::default();
    };
    // 只映射固定操作词。不会向观察者传递 SQL、绑定值或标识符。
    const OPERATIONS: [&str; 12] = [
        "SELECT", "INSERT", "UPDATE", "DELETE", "REPLACE", "MERGE", "CREATE", "ALTER", "DROP",
        "TRUNCATE", "PRAGMA", "CALL",
    ];
    let operation = sql.split_ascii_whitespace().next().and_then(|first| {
        OPERATIONS
            .into_iter()
            .find(|operation| first.eq_ignore_ascii_case(operation))
    });
    begin(&owner, Operation::Sql { system, operation })
}

fn registered(container: &Container) -> Option<Arc<Registered>> {
    if !container.has(crate::container::ServiceKey::typed::<Registered>()) {
        return None;
    }
    container.make::<Registered>().ok()
}

pub(crate) fn begin(container: &Container, operation: Operation<'_>) -> Execution {
    Execution(registered(container).and_then(|registration| registration.0.begin(operation)))
}

pub(crate) fn report(
    container: &Container,
    identity: Option<ExceptionIdentity>,
    snapshot: &serde_json::Value,
) {
    if let Some(registration) = registered(container) {
        registration.0.report(identity, snapshot);
    }
}

pub(crate) fn current_trace(container: &Container) -> Option<TraceContext> {
    registered(container).and_then(|registration| registration.0.current_trace())
}

#[derive(Default)]
pub(crate) struct Execution(Option<Box<dyn Observation>>);

impl Execution {
    pub(crate) async fn run<T>(
        mut self,
        future: impl Future<Output = T>,
        outcome: impl FnOnce(&T) -> Outcome,
    ) -> T {
        let Some(observation) = self.0.as_ref() else {
            return future.await;
        };
        let mut future = std::pin::pin!(future);
        let result = std::future::poll_fn(|cx| {
            let mut result = Poll::Pending;
            observation.in_scope(&mut || result = future.as_mut().poll(cx));
            result
        })
        .await;
        if let Some(mut observation) = self.0.take() {
            observation.finish(outcome(&result));
        }
        result
    }
}

pub(crate) fn sql_stream<T, E>(
    start: impl FnOnce() -> Execution,
    stream: impl Stream<Item = Result<T, E>>,
) -> impl Stream<Item = Result<T, E>> {
    let mut start = Some(start);
    let mut execution = Execution::default();
    let mut stream = Box::pin(stream);
    futures_util::stream::poll_fn(move |cx| {
        if let Some(start) = start.take() {
            execution = start();
        }
        let mut result = Poll::Pending;
        if let Some(observation) = execution.0.as_ref() {
            observation.in_scope(&mut || result = stream.as_mut().poll_next(cx));
        } else {
            result = stream.as_mut().poll_next(cx);
        }
        let outcome = match &result {
            Poll::Ready(None) => Some(Outcome::Sql { failed: false }),
            Poll::Ready(Some(Err(_))) => Some(Outcome::Sql { failed: true }),
            _ => None,
        };
        if let Some(outcome) = outcome
            && let Some(mut observation) = execution.0.take()
        {
            observation.finish(outcome);
        }
        result
    })
}

impl Drop for Execution {
    fn drop(&mut self) {
        if let Some(observation) = self.0.as_mut() {
            observation.finish(Outcome::Cancelled);
        }
    }
}
