use std::{
    io,
    sync::Arc,
    time::{Duration, SystemTime},
};

use tokio::{
    io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader},
    net::TcpStream,
    time::timeout,
};

use crate::contracts::queue::{Job, Queue as Contract, QueueMetrics};

use super::{
    Delay, JobContext, Payload, Queue, QueueError, QueueFuture, QueueResult, backend,
    jobs::{Reservation, ReservedJob},
};

const PRIORITY: u32 = 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_FRAME: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct BeanstalkdQueue {
    pub(super) name: String,
    pub(super) queue: String,
    pub(super) host: String,
    pub(super) port: u16,
    pub(super) retry_after: u32,
    pub(super) block_for: u32,
    pub(super) registry: Arc<Queue>,
}

impl BeanstalkdQueue {
    async fn connect(&self) -> QueueResult<Session> {
        let stream = timeout(
            IO_TIMEOUT,
            TcpStream::connect((self.host.as_str(), self.port)),
        )
        .await
        .map_err(backend)?
        .map_err(backend)?;
        Ok(Session(BufReader::new(stream)))
    }

    async fn put(
        &self,
        payload: &Payload,
        queue: Option<&str>,
        delay: Delay,
    ) -> QueueResult<Option<String>> {
        let tube = tube(queue.unwrap_or(&self.queue))?;
        let delay = match delay {
            Delay::For(delay) => delay,
            Delay::Until(time) => time.duration_since(SystemTime::now()).unwrap_or_default(),
        };
        let delay = u32::try_from(delay.as_secs()).map_err(|_| QueueError::InvalidConfiguration)?;
        let body = serde_json::to_vec(payload).map_err(backend)?;
        if body.len() > MAX_FRAME {
            return Err(QueueError::InvalidPayload);
        }
        let mut session = self.connect().await?;
        let reply = session.command(&format!("use {tube}")).await?;
        if reply != format!("USING {tube}") {
            return Err(protocol(&reply));
        }
        let header = format!(
            "put {PRIORITY} {delay} {} {}\r\n",
            self.retry_after,
            body.len()
        );
        let reply = timeout(IO_TIMEOUT, async {
            session.0.write_all(header.as_bytes()).await?;
            session.0.write_all(&body).await?;
            session.0.write_all(b"\r\n").await?;
            session.line().await
        })
        .await
        .map_err(backend)?
        .map_err(backend)?;
        // BURIED also means the server accepted the payload. Retrying on another connection would duplicate it.
        let id = reply
            .strip_prefix("INSERTED ")
            .or_else(|| reply.strip_prefix("BURIED "))
            .ok_or_else(|| protocol(&reply))?;
        number(id)?;
        Ok(Some(id.to_owned()))
    }

    async fn reserve(&self, queue: Option<&str>, wait: u32) -> QueueResult<Option<Box<dyn Job>>> {
        let tube = tube(queue.unwrap_or(&self.queue))?;
        let mut session = self.connect().await?;
        if tube != "default"
            && (session.command(&format!("watch {tube}")).await? != "WATCHING 2"
                || session.command("ignore default").await? != "WATCHING 1")
        {
            return Err(protocol("could not select the watched tube"));
        }
        let reply = session
            .command_with_timeout(
                &format!("reserve-with-timeout {wait}"),
                IO_TIMEOUT + Duration::from_secs(u64::from(wait)),
            )
            .await?;
        if reply == "TIMED_OUT" {
            return Ok(None);
        }
        let (id, size) = reply
            .strip_prefix("RESERVED ")
            .and_then(|reply| reply.split_once(' '))
            .ok_or_else(|| protocol(&reply))?;
        number(id)?;
        let body = session.body(size).await?;
        let stats = session
            .stats(&format!("stats-job {id}"))
            .await?
            .ok_or(QueueError::LostReservation)?;
        let attempts = stats.number("reserves")?;
        let attempts = u32::try_from(attempts).map_err(|_| QueueError::InvalidPayload)?;
        let raw = std::str::from_utf8(&body).map_err(|_| QueueError::InvalidPayload)?;
        let payload = Payload::decode(raw)?;
        Ok(Some(Box::new(ReservedJob {
            registry: self.registry.clone(),
            storage: Box::new(BeanstalkdReservation {
                session,
                driver: self.clone(),
                id: id.to_owned(),
                tube: tube.into(),
            }),
            id: id.into(),
            context: JobContext {
                connection: self.name.clone(),
                queue: tube.into(),
                attempts,
                ..Default::default()
            },
            payload,
            deleted: false,
            released: false,
            failed: false,
        })))
    }
}

impl Contract for BeanstalkdQueue {
    fn get_connection_name(&self) -> &str {
        &self.name
    }
    fn default_queue(&self) -> &str {
        &self.queue
    }

    fn size<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<u64>> {
        Box::pin(async move { Ok(self.metrics(queue).await?.size) })
    }

    fn metrics<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<QueueMetrics>> {
        Box::pin(async move {
            let tube = tube(queue.unwrap_or(&self.queue))?;
            let mut session = self.connect().await?;
            let Some(stats) = session.stats(&format!("stats-tube {tube}")).await? else {
                return Ok(QueueMetrics {
                    pending: Some(0),
                    delayed: Some(0),
                    reserved: Some(0),
                    ..Default::default()
                });
            };
            let pending = stats.number("current-jobs-ready")?;
            let delayed = stats.number("current-jobs-delayed")?;
            let reserved = stats.number("current-jobs-reserved")?;
            Ok(QueueMetrics {
                size: pending.saturating_add(delayed).saturating_add(reserved),
                pending: Some(pending),
                delayed: Some(delayed),
                reserved: Some(reserved),
                oldest_pending: None,
            })
        })
    }

    fn push<'a>(
        &'a self,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.put(payload, queue, Delay::For(Duration::ZERO)))
    }

    fn later<'a>(
        &'a self,
        delay: Delay,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.put(payload, queue, delay))
    }

    fn pop<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(self.reserve(queue, self.block_for))
    }

    fn pop_now<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(self.reserve(queue, 0))
    }

    fn pop_within<'a>(
        &'a self,
        queue: Option<&'a str>,
        wait: Duration,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(
            self.reserve(
                queue,
                self.block_for
                    .min(u32::try_from(wait.as_secs()).unwrap_or(u32::MAX)),
            ),
        )
    }
}

struct BeanstalkdReservation {
    session: Session,
    driver: BeanstalkdQueue,
    id: String,
    tube: String,
}

impl Reservation for BeanstalkdReservation {
    fn advance_chain<'a>(
        &'a mut self,
        next: &'a Payload,
        queue: Option<&'a str>,
        delay: Option<Delay>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            self.touch().await?;
            self.driver
                .put(
                    next,
                    queue.or(Some(&self.tube)),
                    delay.unwrap_or(Delay::For(Duration::ZERO)),
                )
                .await?;
            self.delete().await
        })
    }

    fn delete(&mut self) -> QueueFuture<'_, QueueResult<()>> {
        Box::pin(async move {
            // Beanstalkd also permits deleting unreserved jobs. Verify this session owns the lease first.
            self.touch().await?;
            let reply = self.session.command(&format!("delete {}", self.id)).await?;
            settlement(&reply, "DELETED")
        })
    }

    fn release(&mut self, delay: Duration) -> QueueFuture<'_, QueueResult<()>> {
        Box::pin(async move {
            let delay =
                u32::try_from(delay.as_secs()).map_err(|_| QueueError::InvalidConfiguration)?;
            let reply = self
                .session
                .command(&format!("release {} {PRIORITY} {delay}", self.id))
                .await?;
            if reply == "BURIED" {
                return Ok(());
            }
            settlement(&reply, "RELEASED")
        })
    }
}

impl BeanstalkdReservation {
    async fn touch(&mut self) -> QueueResult<()> {
        let reply = self.session.command(&format!("touch {}", self.id)).await?;
        settlement(&reply, "TOUCHED")
    }
}

struct Session(BufReader<TcpStream>);

impl Session {
    async fn line(&mut self) -> io::Result<String> {
        let mut line = String::new();
        (&mut self.0).take(1024).read_line(&mut line).await?;
        line.strip_suffix("\r\n").map(str::to_owned).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "invalid Beanstalkd response line",
            )
        })
    }

    async fn command(&mut self, command: &str) -> QueueResult<String> {
        self.command_with_timeout(command, IO_TIMEOUT).await
    }

    async fn command_with_timeout(
        &mut self,
        command: &str,
        limit: Duration,
    ) -> QueueResult<String> {
        timeout(limit, async {
            self.0
                .write_all(format!("{command}\r\n").as_bytes())
                .await?;
            self.line().await
        })
        .await
        .map_err(backend)?
        .map_err(backend)
    }

    async fn body(&mut self, size: &str) -> QueueResult<Vec<u8>> {
        let size = usize::try_from(number(size)?).map_err(backend)?;
        if size > MAX_FRAME {
            return Err(protocol("frame too large"));
        }
        let mut body = vec![0; size + 2];
        timeout(IO_TIMEOUT, self.0.read_exact(&mut body))
            .await
            .map_err(backend)?
            .map_err(backend)?;
        if body.get(size..) != Some(b"\r\n") {
            return Err(protocol("invalid frame delimiter"));
        }
        body.truncate(size);
        Ok(body)
    }

    async fn stats(&mut self, command: &str) -> QueueResult<Option<Stats>> {
        let reply = self.command(command).await?;
        if reply == "NOT_FOUND" {
            return Ok(None);
        }
        let size = reply.strip_prefix("OK ").ok_or_else(|| protocol(&reply))?;
        let data = String::from_utf8(self.body(size).await?).map_err(backend)?;
        Ok(Some(Stats(data)))
    }
}

struct Stats(String);

impl Stats {
    fn number(&self, name: &str) -> QueueResult<u64> {
        let value = self
            .0
            .lines()
            .filter_map(|line| line.split_once(": "))
            .find_map(|(key, value)| (key == name).then_some(value))
            .ok_or_else(|| protocol("missing statistics field"))?;
        number(value)
    }
}

pub(super) fn tube(value: &str) -> QueueResult<&str> {
    if value.is_empty()
        || value.len() > 200
        || value.starts_with('-')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"+/;.$_()-".contains(&byte))
    {
        return Err(QueueError::InvalidConfiguration);
    }
    Ok(value)
}

fn number(value: &str) -> QueueResult<u64> {
    value.parse().map_err(backend)
}

fn settlement(reply: &str, expected: &str) -> QueueResult<()> {
    match reply {
        reply if reply == expected => Ok(()),
        "NOT_FOUND" => Err(QueueError::LostReservation),
        _ => Err(protocol(reply)),
    }
}

fn protocol(reply: &str) -> QueueError {
    backend(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("Beanstalkd: {reply}"),
    ))
}
