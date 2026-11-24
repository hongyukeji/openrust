use super::{Events, Queue};
use crate::{
    container::Container,
    contracts::{config::Repository, container::Container as _},
    queue::{
        QueueError, QueueFuture, QueueResult,
        failed::{
            CountableFailedJobProvider, FailedJob, FailedJobProviderInterface,
            FailedJobRetryStream, PrunableFailedJobProvider,
        },
    },
};
use futures_util::stream;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, PoisonError, RwLock, Weak},
    time::Duration,
};
#[derive(Default)]
pub(crate) struct Loaded(Mutex<HashMap<String, LoadedJob>>);
struct LoadedJob {
    id: Value,
    queue: String,
}
/// 普通失败存储保持原策略，托管任务使用事件和加密的分页重试载荷。
pub struct FailedJobProvider {
    parent: Arc<dyn FailedJobProviderInterface>,
    owner: Weak<Container>,
    events: Arc<Events>,
    queue: RwLock<Option<Arc<Queue>>>,
    loaded: Arc<Loaded>,
}
impl FailedJobProvider {
    pub fn new(
        parent: Arc<dyn FailedJobProviderInterface>,
        app: &crate::foundation::Application,
        events: Arc<Events>,
    ) -> Self {
        Self::from_parent(parent, Arc::downgrade(&app.container_handle()), events)
    }
    pub(crate) fn from_parent(
        parent: Arc<dyn FailedJobProviderInterface>,
        owner: Weak<Container>,
        events: Arc<Events>,
    ) -> Self {
        Self {
            parent,
            owner,
            events,
            queue: RwLock::new(None),
            loaded: Arc::default(),
        }
    }
    fn owner(&self) -> QueueResult<Arc<Container>> {
        self.owner.upgrade().ok_or(QueueError::InvalidConfiguration)
    }
    fn loaded(&self) -> QueueResult<Arc<Loaded>> {
        let owner = self.owner()?;
        if owner.has_active_scope() {
            Ok(owner.make::<Loaded>()?)
        } else {
            Ok(self.loaded.clone())
        }
    }
    /// 显式装配时指定托管队列；真实Worker优先使用自己的运行域。
    pub fn set_queue(&self, queue: Arc<Queue>) -> &Self {
        let old = self
            .queue
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(queue);
        drop(old);
        self
    }
    /// 保留父存储的可选计数能力，不把远端托管失败虚构为本地记录。
    pub async fn count(&self, connection: Option<&str>, queue: Option<&str>) -> QueueResult<u64> {
        match self.parent.countable() {
            Some(parent) => parent.count(connection, queue).await,
            None => Ok(0),
        }
    }
    /// 分页下载保持惰性；单条及批量URL均通过同一正式流契约消费。
    pub async fn find<'a>(&'a self, id: &str) -> QueueResult<Option<FailedJobRetryStream<'a>>> {
        if !id.starts_with("https://") {
            return Ok(self.parent.find(id).await?.map(|job| {
                Box::pin(stream::iter([Ok((id.to_owned(), job))])) as FailedJobRetryStream<'a>
            }));
        }
        let loaded = self.loaded()?;
        struct State<'a> {
            provider: &'a FailedJobProvider,
            url: Option<String>,
            self_url: String,
            rows: VecDeque<Value>,
            loaded: Arc<Loaded>,
        }
        let state = State {
            provider: self,
            url: Some(id.into()),
            self_url: String::new(),
            rows: VecDeque::new(),
            loaded,
        };
        let stream = stream::try_unfold(state, |mut state| async move {
            loop {
                if let Some(value) = state.rows.pop_front() {
                    if ends_iteration(&value) {
                        return Ok(None);
                    }
                    let raw_id = value
                        .get("id")
                        .filter(|id| id.is_string() || id.is_number())
                        .cloned()
                        .ok_or(QueueError::InvalidPayload)?;
                    let id = raw_id
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| raw_id.to_string());
                    let job = decode(value, id.clone())?;
                    let key = format!("{}:{id}", state.self_url);
                    state
                        .loaded
                        .0
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(
                            key.clone(),
                            LoadedJob {
                                id: raw_id,
                                queue: job.queue.clone(),
                            },
                        );
                    return Ok(Some(((key, job), state)));
                }
                let Some(url) = state.url.take() else {
                    return Ok(None);
                };
                let page = state.provider.resolve(&url).await?;
                state.rows = page.rows;
                state.self_url = page.self_url;
                state.url = page.next;
                // 固定迭代器不会跨过空页继续读取next。
                if state.rows.is_empty() {
                    return Ok(None);
                }
            }
        });
        Ok(Some(Box::pin(stream)))
    }
    async fn resolve(&self, url: &str) -> QueueResult<Page> {
        use crate::http::client::{Error, Factory};
        let owner = self.owner()?;
        let config = owner.get::<dyn Repository>("config")?;
        let cipher = config
            .get("app.cipher")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "AES-256-CBC".into());
        let mut headers = http::HeaderMap::new();
        headers.insert("cloud-payload-version", http::HeaderValue::from_static("1"));
        headers.insert(
            "cloud-encryption-cipher",
            http::HeaderValue::from_str(&cipher).map_err(crate::queue::backend)?,
        );
        let factory = owner.make::<Factory>()?;
        let response=factory.without_global_configuration(|http|http.pending_request()).connect_timeout(Duration::from_secs(10)).timeout(Duration::from_secs(10))
            .with_headers(headers).throw().retry_delays((0..9).map(|n|Duration::from_millis(500u64<<n)).collect::<Vec<_>>())
            .retry_when(|error|!matches!(error,Error::RequestFailed(response) if response.status().as_u16()==403)).get(url).await.map_err(crate::queue::backend)?;
        let bytes = response.body().map_err(crate::queue::backend)?;
        let encrypted = std::str::from_utf8(&bytes).map_err(crate::queue::backend)?;
        let decrypted = owner
            .make::<crate::encryption::Encrypter>()?
            .decrypt_string(encrypted)
            .map_err(crate::queue::backend)?;
        let mut value: Value = serde_json::from_slice(&decrypted).map_err(crate::queue::backend)?;
        let version = match response.headers().get("cloud-payload-version") {
            None => "",
            Some(header) => header.to_str().map_err(crate::queue::backend)?,
        };
        match version {
            "" => Ok(Page {
                rows: vec![value].into(),
                self_url: url.into(),
                next: None,
            }),
            "1" => {
                let rows = value
                    .get_mut("data")
                    .and_then(Value::as_array_mut)
                    .map(std::mem::take)
                    .ok_or(QueueError::InvalidPayload)?;
                let links = value.get("links").ok_or(QueueError::InvalidPayload)?;
                let self_url = links
                    .get("self")
                    .and_then(Value::as_str)
                    .ok_or(QueueError::InvalidPayload)?
                    .into();
                let next = match links.get("next") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(value)) => Some(value.clone()),
                    _ => return Err(QueueError::InvalidPayload),
                };
                Ok(Page {
                    rows: rows.into(),
                    self_url,
                    next,
                })
            }
            version => Err(crate::queue::backend(std::io::Error::other(format!(
                "不支持的托管失败载荷版本：{version}"
            )))),
        }
    }
}
struct Page {
    rows: VecDeque<Value>,
    self_url: String,
    next: Option<String>,
}
fn decode(value: Value, id: String) -> QueueResult<FailedJob> {
    let field = |name| {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(QueueError::InvalidPayload)
    };
    Ok(FailedJob {
        id,
        connection: field("connection")?,
        queue: field("queue")?,
        payload: crate::queue::Payload::decode(&field("payload")?)?,
        exception: field("exception")?,
        failed_at: field("failed_at")?,
    })
}
impl FailedJobProviderInterface for FailedJobProvider {
    fn countable(&self) -> Option<&dyn CountableFailedJobProvider> {
        Some(self)
    }
    fn prunable(&self) -> Option<&dyn PrunableFailedJobProvider> {
        Some(self)
    }
    fn log<'a>(&'a self, job: &'a FailedJob) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(async move {
            if job.connection != "cloud" {
                return self.parent.log(job).await;
            }
            let owner = self.owner()?;
            let queue = super::worker::managed_queue(&owner)
                .or_else(|| {
                    self.queue
                        .read()
                        .unwrap_or_else(PoisonError::into_inner)
                        .clone()
                })
                .ok_or_else(|| {
                    crate::queue::backend(std::io::Error::other("托管失败任务存储没有配置当前队列"))
                })?;
            let details = queue
                .processing_job_details()?
                .ok_or(QueueError::InvalidConfiguration)?;
            let at = jiff::Timestamp::now();
            let id = crate::support::Str::uuid7().to_string();
            self.events.emit(&json!({"_cloud_event":"failed_job","id":id,"queue":details.queue,"started_at":details.started_at.to_zoned(jiff::tz::TimeZone::UTC).strftime("%Y-%m-%d %H:%M:%S%.6f").to_string(),"attempts":details.attempts,"payload":job.payload.encode()?,"exception_preview":job.exception.chars().take(1001).collect::<String>(),"job_name":job.payload.display_name(),"exception":job.exception})).await;
            queue.finish_processing_job(None, Some(at)).await?;
            Ok(Some(id))
        })
    }
    fn all(&self) -> QueueFuture<'_, QueueResult<Vec<FailedJob>>> {
        self.parent.all()
    }
    fn ids<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<Vec<String>>> {
        self.parent.ids(queue)
    }
    fn find<'a>(&'a self, id: &'a str) -> QueueFuture<'a, QueueResult<Option<FailedJob>>> {
        if !id.starts_with("https://") {
            return self.parent.find(id);
        }
        Box::pin(async {
            Err(crate::queue::backend(std::io::Error::other(
                "分页失败任务需使用find_for_retry流，不能截断为单条记录",
            )))
        })
    }
    fn find_for_retry<'a>(
        &'a self,
        id: &'a str,
    ) -> QueueFuture<'a, QueueResult<Option<FailedJobRetryStream<'a>>>> {
        if id.starts_with("https://") {
            Box::pin(self.find(id))
        } else {
            self.parent.find_for_retry(id)
        }
    }
    fn forget<'a>(&'a self, id: &'a str) -> QueueFuture<'a, QueueResult<bool>> {
        Box::pin(async move {
            if !id.starts_with("https://") {
                return self.parent.forget(id).await;
            }
            let job = self
                .loaded()?
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(id);
            let Some(job) = job else { return Ok(false) };
            Ok(self.events.emit(&json!({"_cloud_event":"failed_job","id":job.id,"queue":job.queue,"retried_at":jiff::Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).strftime("%Y-%m-%d %H:%M:%S%.6f").to_string()})).await)
        })
    }
    fn forget_retry<'a>(&'a self, job: &'a FailedJob) -> QueueFuture<'a, QueueResult<bool>> {
        if job.id.starts_with("https://") {
            self.forget(&job.id)
        } else {
            self.parent.forget_retry(job)
        }
    }
    fn forget_retry_in_transaction<'a>(
        &'a self,
        job: &'a FailedJob,
        tx: &'a mut crate::database::Transaction,
    ) -> QueueFuture<'a, QueueResult<Option<bool>>> {
        if job.id.starts_with("https://") {
            Box::pin(async { Ok(None) })
        } else {
            self.parent.forget_retry_in_transaction(job, tx)
        }
    }
    fn flush(&self, older: Option<Duration>) -> QueueFuture<'_, QueueResult<()>> {
        self.parent.flush(older)
    }
    fn flush_before<'a>(
        &'a self,
        before: Option<jiff::Timestamp>,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        self.parent.flush_before(before, queue)
    }
    fn flush_queue<'a>(
        &'a self,
        older: Option<Duration>,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        self.parent.flush_queue(older, queue)
    }
}
impl CountableFailedJobProvider for FailedJobProvider {
    fn count<'a>(
        &'a self,
        connection: Option<&'a str>,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<u64>> {
        Box::pin(self.count(connection, queue))
    }
}
impl PrunableFailedJobProvider for FailedJobProvider {
    fn prune(&self, before: jiff::Timestamp) -> QueueFuture<'_, QueueResult<u64>> {
        Box::pin(async move {
            match self.parent.prunable() {
                Some(parent) => parent.prune(before).await,
                None => Ok(0),
            }
        })
    }
}

fn ends_iteration(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(value) => !*value,
        Value::Number(value) => value.as_f64() == Some(0.0),
        Value::String(value) => value.is_empty() || value == "0",
        Value::Array(value) => value.is_empty(),
        Value::Object(_) => false,
    }
}
