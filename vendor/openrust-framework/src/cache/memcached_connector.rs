use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, Weak},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use aws_lc_rs::digest::{SHA256, digest};
use serde::Deserialize;
use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _},
    net::TcpStream,
    sync::Semaphore,
    time::timeout,
};

use super::{CacheError, CacheResult, backend};

const HEADER_LEN: usize = 24;
const MAX_KEY_LEN: usize = 250;
const GET_BATCH: usize = 16;
pub(super) const GET: u8 = 0x00;
pub(super) const SET: u8 = 0x01;
pub(super) const ADD: u8 = 0x02;
pub(super) const DELETE: u8 = 0x04;
pub(super) const INCREMENT: u8 = 0x05;
pub(super) const DECREMENT: u8 = 0x06;
pub(super) const FLUSH: u8 = 0x08;
pub(super) const TOUCH: u8 = 0x1c;
const SASL_AUTH: u8 = 0x21;

#[derive(Deserialize, PartialEq, Eq)]
pub(super) struct Server {
    host: String,
    port: u16,
    #[serde(default = "default_weight")]
    weight: u32,
}

fn default_weight() -> u32 {
    100
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Options {
    connect_timeout_ms: u64,
    timeout_ms: u64,
    max_connections: usize,
    max_item_bytes: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            connect_timeout_ms: 1000,
            timeout_ms: 1000,
            max_connections: 8,
            max_item_bytes: 1024 * 1024,
        }
    }
}

pub(super) type Credentials = Vec<Option<String>>;

pub(super) struct Settings {
    servers: Vec<Server>,
    credentials: Option<(String, String)>,
    options: Options,
}

impl Settings {
    pub(super) fn new(
        mut servers: Vec<Server>,
        credentials: Option<Credentials>,
        options: Options,
    ) -> CacheResult<Self> {
        if servers.is_empty()
            || options.connect_timeout_ms == 0
            || options.timeout_ms == 0
            || !(1..=4096).contains(&options.max_connections)
            || !(1..=128 * 1024 * 1024).contains(&options.max_item_bytes)
        {
            return Err(CacheError::InvalidConfiguration);
        }
        let mut endpoints = HashSet::with_capacity(servers.len());
        for server in &mut servers {
            let valid_endpoint = if server.port == 0 {
                cfg!(unix)
                    && std::path::Path::new(&server.host).is_absolute()
                    && !server.host.contains('\0')
            } else {
                server.host.make_ascii_lowercase();
                !server.host.is_empty()
                    && !server
                        .host
                        .bytes()
                        .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
            };
            if !valid_endpoint
                || server.weight == 0
                || !endpoints.insert((server.host.clone(), server.port))
            {
                return Err(CacheError::InvalidConfiguration);
            }
        }
        servers.sort_unstable_by(|left, right| {
            (&left.host, left.port).cmp(&(&right.host, right.port))
        });
        let credentials: Result<[Option<String>; 2], _> =
            credentials.unwrap_or_default().try_into();
        let credentials = match credentials {
            Ok([None, None]) => None,
            Ok([Some(username), Some(password)])
                if !username.is_empty()
                    && !password.is_empty()
                    && username.len().saturating_add(password.len()) <= 4096
                    && !username.contains('\0')
                    && !password.contains('\0') =>
            {
                Some((username, password))
            }
            Err(values) if values.is_empty() => None,
            _ => return Err(CacheError::InvalidConfiguration),
        };
        Ok(Self {
            servers,
            credentials,
            options,
        })
    }
}

/// Holds Memcached connections for one application.
#[derive(Default)]
pub(super) struct Connector(Mutex<HashMap<String, Weak<Client>>>);

impl Connector {
    pub(super) fn connect(
        &self,
        id: Option<String>,
        settings: Settings,
    ) -> CacheResult<Arc<Client>> {
        let Some(id) = id.filter(|id| !id.is_empty()) else {
            return Ok(Arc::new(Client::new(settings)));
        };
        let mut clients = self.0.lock().expect("Memcached pool registry poisoned");
        clients.retain(|_, client| client.strong_count() > 0);
        if let Some(client) = clients.get(&id).and_then(Weak::upgrade) {
            if !client.matches(&settings) {
                return Err(CacheError::InvalidConfiguration);
            }
            return Ok(client);
        }
        let client = Arc::new(Client::new(settings));
        clients.insert(id, Arc::downgrade(&client));
        Ok(client)
    }
}

struct Node {
    server: Server,
    identity: String,
    permits: Semaphore,
    idle: Mutex<Vec<Connection>>,
}

trait Wire: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Wire for T {}

enum Connection {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
}

impl Connection {
    async fn connect(server: &Server) -> std::io::Result<Self> {
        #[cfg(unix)]
        if server.port == 0 {
            return tokio::net::UnixStream::connect(&server.host)
                .await
                .map(Self::Unix);
        }
        let stream = TcpStream::connect((server.host.as_str(), server.port)).await?;
        stream.set_nodelay(true)?;
        Ok(Self::Tcp(stream))
    }

    fn wire(&mut self) -> &mut (dyn Wire + 'static) {
        match self {
            Self::Tcp(stream) => stream,
            #[cfg(unix)]
            Self::Unix(stream) => stream,
        }
    }

    fn is_idle(&self) -> bool {
        let mut byte = [0];
        let result = match self {
            Self::Tcp(stream) => stream.try_read(&mut byte),
            #[cfg(unix)]
            Self::Unix(stream) => stream.try_read(&mut byte),
        };
        matches!(result, Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    }
}

/// A bounded, asynchronous pool of native Memcached connections.
pub struct Client {
    nodes: Vec<Node>,
    credentials: Option<(String, String)>,
    options: Options,
}

impl Client {
    fn new(settings: Settings) -> Self {
        let Settings {
            servers,
            credentials,
            options,
        } = settings;
        let nodes = servers
            .into_iter()
            .map(|server| Node {
                identity: format!("{}:{}", server.host, server.port),
                permits: Semaphore::new(options.max_connections),
                idle: Mutex::default(),
                server,
            })
            .collect();
        Self {
            nodes,
            credentials,
            options,
        }
    }

    fn matches(&self, settings: &Settings) -> bool {
        self.credentials == settings.credentials
            && self.options == settings.options
            && self
                .nodes
                .iter()
                .map(|node| &node.server)
                .eq(&settings.servers)
    }

    fn index(&self, key: &str) -> usize {
        if self.nodes.len() == 1 {
            return 0;
        }
        self.nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                let hash = digest(&SHA256, format!("{}\0{key}", node.identity).as_bytes());
                let bits =
                    u64::from_be_bytes(hash.as_ref()[..8].try_into().expect("eight bytes")) >> 11;
                let sample = (bits as f64 + 1.0) / ((1_u64 << 53) as f64);
                (index, -sample.ln() / f64::from(node.server.weight))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .expect("at least one Memcached node")
            .0
    }

    async fn connect(&self, node: &Node) -> CacheResult<Connection> {
        let mut stream = timeout(
            Duration::from_millis(self.options.connect_timeout_ms),
            Connection::connect(&node.server),
        )
        .await
        .map_err(backend)?
        .map_err(backend)?;
        if let Some((username, password)) = &self.credentials {
            let mut credentials = Vec::with_capacity(username.len() + password.len() + 2);
            credentials.push(0);
            credentials.extend_from_slice(username.as_bytes());
            credentials.push(0);
            credentials.extend_from_slice(password.as_bytes());
            send(stream.wire(), SASL_AUTH, "PLAIN", &[], &credentials, 0, 0).await?;
            read(stream.wire(), SASL_AUTH, 0, 4096).await?.success()?;
        }
        Ok(stream)
    }

    async fn use_node<T>(
        &self,
        index: usize,
        operation: impl for<'a> FnOnce(
            &'a mut (dyn Wire + 'static),
        ) -> super::CacheFuture<'a, CacheResult<T>>,
    ) -> CacheResult<T> {
        let node = &self.nodes[index];
        timeout(Duration::from_millis(self.options.timeout_ms), async {
            let _permit = node.permits.acquire().await.map_err(backend)?;
            let idle = node
                .idle
                .lock()
                .expect("Memcached pool poisoned")
                .pop()
                .filter(Connection::is_idle);
            let mut stream = match idle {
                Some(stream) => stream,
                None => self.connect(node).await?,
            };
            // Returns only a fully consumed stream. Cancellation and framing errors drop it.
            let result = operation(stream.wire()).await?;
            node.idle
                .lock()
                .expect("Memcached pool poisoned")
                .push(stream);
            Ok(result)
        })
        .await
        .map_err(backend)?
    }

    pub(super) async fn command(
        &self,
        opcode: u8,
        key: String,
        extras: Vec<u8>,
        value: Vec<u8>,
        cas: u64,
    ) -> CacheResult<Response> {
        validate_key(&key)?;
        if value.len() > self.options.max_item_bytes {
            return Err(CacheError::InvalidValue);
        }
        self.on(self.index(&key), opcode, key, extras, value, cas)
            .await
    }

    async fn on(
        &self,
        index: usize,
        opcode: u8,
        key: String,
        extras: Vec<u8>,
        value: Vec<u8>,
        cas: u64,
    ) -> CacheResult<Response> {
        let max = self.options.max_item_bytes.max(4096) + MAX_KEY_LEN + 32;
        self.use_node(index, move |stream| {
            Box::pin(async move {
                send(stream, opcode, &key, &extras, &value, cas, 0).await?;
                read(stream, opcode, 0, max).await
            })
        })
        .await
    }

    pub(super) async fn flush(&self) -> CacheResult<()> {
        let result =
            futures_util::future::join_all((0..self.nodes.len()).map(|index| async move {
                self.on(index, FLUSH, String::new(), Vec::new(), Vec::new(), 0)
                    .await?
                    .success()
            }))
            .await;
        result.into_iter().collect()
    }

    pub(super) async fn many(&self, keys: Vec<String>) -> CacheResult<Vec<Option<Vec<u8>>>> {
        let mut batches = vec![Vec::new(); self.nodes.len()];
        let mut values = vec![None; keys.len()];
        for (position, key) in keys.into_iter().enumerate() {
            validate_key(&key)?;
            batches[self.index(&key)].push((position, key));
        }
        let max = self.options.max_item_bytes.max(4096) + MAX_KEY_LEN + 32;
        let groups = futures_util::future::join_all(
            batches
                .into_iter()
                .enumerate()
                .filter(|(_, keys)| !keys.is_empty())
                .map(|(index, keys)| {
                    self.use_node(index, move |stream| {
                        Box::pin(async move {
                            let mut results = Vec::with_capacity(keys.len());
                            for batch in keys.chunks(GET_BATCH) {
                                for (offset, (_, key)) in batch.iter().enumerate() {
                                    send(stream, GET, key, &[], &[], 0, offset as u32).await?;
                                }
                                for (offset, (position, _)) in batch.iter().enumerate() {
                                    let response = read(stream, GET, offset as u32, max).await?;
                                    results
                                        .push((*position, response.item()?.map(|item| item.value)));
                                }
                            }
                            Ok(results)
                        })
                    })
                }),
        )
        .await;
        for group in groups {
            for (position, value) in group? {
                values[position] = value;
            }
        }
        Ok(values)
    }
}

pub(super) fn validate_key(key: &str) -> CacheResult<()> {
    if key.is_empty()
        || key.len() > MAX_KEY_LEN
        || key
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return Err(CacheError::InvalidValue);
    }
    Ok(())
}

pub(super) fn expiration(seconds: u64) -> CacheResult<u32> {
    deadline(seconds, 30)
}

pub(super) fn lock_expiration(seconds: u64) -> CacheResult<u32> {
    deadline(seconds, 10)
}

fn deadline(seconds: u64, days: u64) -> CacheResult<u32> {
    let deadline = if seconds > days * 24 * 60 * 60 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(backend)?
            .as_secs()
            .checked_add(seconds)
            .ok_or(CacheError::InvalidValue)?
    } else {
        seconds
    };
    u32::try_from(deadline).map_err(|_| CacheError::InvalidValue)
}

async fn send(
    stream: &mut dyn Wire,
    opcode: u8,
    key: &str,
    extras: &[u8],
    value: &[u8],
    cas: u64,
    opaque: u32,
) -> CacheResult<()> {
    let length = extras.len() + key.len() + value.len();
    let mut frame = Vec::with_capacity(HEADER_LEN + length);
    frame.extend_from_slice(&[0x80, opcode]);
    frame.extend_from_slice(&(key.len() as u16).to_be_bytes());
    frame.extend_from_slice(&[extras.len() as u8, 0, 0, 0]);
    frame.extend_from_slice(&(length as u32).to_be_bytes());
    frame.extend_from_slice(&opaque.to_be_bytes());
    frame.extend_from_slice(&cas.to_be_bytes());
    frame.extend_from_slice(extras);
    frame.extend_from_slice(key.as_bytes());
    frame.extend_from_slice(value);
    stream.write_all(&frame).await.map_err(backend)
}

pub(super) struct Item {
    pub(super) value: Vec<u8>,
    pub(super) cas: u64,
}

pub(super) struct Response {
    status: u16,
    cas: u64,
    extras: usize,
    key: usize,
    data: Vec<u8>,
}

#[derive(Debug)]
struct Status(u16);

impl std::fmt::Display for Status {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "Memcached returned status {:#06x}", self.0)
    }
}

impl std::error::Error for Status {}

impl Response {
    pub(super) fn success(&self) -> CacheResult<()> {
        if self.status == 0 {
            Ok(())
        } else {
            Err(backend(Status(self.status)))
        }
    }

    pub(super) fn mutation(&self, negative: &[u16]) -> CacheResult<bool> {
        if negative.contains(&self.status) {
            return Ok(false);
        }
        self.success().map(|()| true)
    }

    pub(super) fn item(self) -> CacheResult<Option<Item>> {
        if self.status == 1 {
            return Ok(None);
        }
        self.success()?;
        if self.extras != 4 || self.key != 0 || self.cas == 0 {
            return Err(protocol_error());
        }
        let mut value = self.data;
        value.drain(..self.extras);
        Ok(Some(Item {
            value,
            cas: self.cas,
        }))
    }

    pub(super) fn counter(self) -> CacheResult<i64> {
        match self.status {
            1 => return Err(CacheError::MissingCounter),
            6 => return Err(CacheError::InvalidValue),
            _ => self.success()?,
        }
        let value = self.data.try_into().map_err(|_| protocol_error())?;
        i64::try_from(u64::from_be_bytes(value)).map_err(|_| CacheError::InvalidValue)
    }
}

fn protocol_error() -> CacheError {
    backend(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "invalid Memcached response",
    ))
}

async fn read(stream: &mut dyn Wire, opcode: u8, opaque: u32, max: usize) -> CacheResult<Response> {
    let mut header = [0; HEADER_LEN];
    stream.read_exact(&mut header).await.map_err(backend)?;
    let key = usize::from(u16::from_be_bytes([header[2], header[3]]));
    let extras = usize::from(header[4]);
    let length = u32::from_be_bytes(header[8..12].try_into().expect("four bytes")) as usize;
    if header[..2] != [0x81, opcode]
        || header[12..16] != opaque.to_be_bytes()
        || length < key + extras
        || length > max
    {
        return Err(protocol_error());
    }
    let mut data = vec![0; length];
    stream.read_exact(&mut data).await.map_err(backend)?;
    Ok(Response {
        status: u16::from_be_bytes([header[6], header[7]]),
        cas: u64::from_be_bytes(header[16..24].try_into().expect("eight bytes")),
        extras,
        key,
        data,
    })
}
