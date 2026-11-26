use std::{sync::Arc, time::Duration};

use ::redis::{
    Arg, AsyncConnectionConfig, Cmd, ConnectionAddr, ConnectionInfo, ErrorKind, FromRedisValue,
    IntoConnectionInfo, Pipeline, RedisFuture, RedisResult, ServerErrorKind, Value,
    aio::{ConnectionLike, MultiplexedConnection},
    cluster::ClusterClient,
    cluster_async::{ClusterConnection, Connect},
    cluster_routing::Slot,
    parse_redis_value,
};

pub(super) type ClusterTransport = ClusterConnection<ClusterNode>;

#[derive(Clone)]
pub(super) struct ClusterNode {
    transport: MultiplexedConnection,
    host: Arc<str>,
}

impl Connect for ClusterNode {
    fn connect_with_config<'a, T>(info: T, config: AsyncConnectionConfig) -> RedisFuture<'a, Self>
    where
        T: IntoConnectionInfo + Send + 'a,
    {
        Box::pin(async move {
            let info = info.into_connection_info()?;
            let host = match info.addr() {
                ConnectionAddr::Tcp(host, _) | ConnectionAddr::TcpTls { host, .. } => host,
                _ => {
                    return Err((
                        ErrorKind::InvalidClientConfig,
                        "A Redis Cluster node requires a TCP host.",
                    )
                        .into());
                }
            };
            let host = if host.contains(':') && !host.starts_with('[') {
                Arc::from(format!("[{host}]"))
            } else {
                Arc::from(host.as_str())
            };
            let transport = MultiplexedConnection::connect_with_config(info, config).await?;
            Ok(Self { transport, host })
        })
    }
}

impl ConnectionLike for ClusterNode {
    fn req_packed_command<'a>(&'a mut self, command: &'a Cmd) -> RedisFuture<'a, Value> {
        Box::pin(async move {
            let value = self.transport.req_packed_command(command).await?;
            Ok(complete_redirect(value, &self.host))
        })
    }

    fn req_packed_commands<'a>(
        &'a mut self,
        pipeline: &'a Pipeline,
        offset: usize,
        count: usize,
    ) -> RedisFuture<'a, Vec<Value>> {
        Box::pin(async move {
            let values = self
                .transport
                .req_packed_commands(pipeline, offset, count)
                .await?;
            Ok(values
                .into_iter()
                .map(|value| complete_redirect(value, &self.host))
                .collect())
        })
    }

    fn get_db(&self) -> i64 {
        self.transport.get_db()
    }
}

// Redis defines an empty endpoint relative to the connection that sent the reply.
fn complete_redirect(value: Value, host: &str) -> Value {
    let Value::ServerError(error) = &value else {
        return value;
    };
    if !matches!(
        error.kind(),
        Some(ServerErrorKind::Ask | ServerErrorKind::Moved)
    ) {
        return value;
    }
    let Some((slot, port)) = error.details().and_then(|details| details.split_once(" :")) else {
        return value;
    };
    if slot.parse::<u16>().ok().and_then(Slot::new).is_none()
        || port.parse::<u16>().ok().filter(|port| *port != 0).is_none()
        || host.is_empty()
        || host
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return value;
    }
    // Use the Redis parser to preserve the server-error kind and routing detail.
    parse_redis_value(format!("-{} {slot} {host}:{port}\r\n", error.code()).as_bytes())
        .unwrap_or(value)
}

pub(super) struct Cluster {
    pub(super) client: ClusterClient,
    seed: Box<ConnectionInfo>,
    redirects: u32,
}

impl Cluster {
    pub(super) fn new(client: ClusterClient, seed: ConnectionInfo, redirects: u32) -> Self {
        Self {
            client,
            seed: Box::new(seed),
            redirects,
        }
    }

    pub(super) async fn query<T: FromRedisValue>(
        &self,
        mut transport: ClusterTransport,
        command: &Cmd,
        known_single_target: bool,
    ) -> RedisResult<T> {
        let mut result = command.query_async(&mut transport).await;
        let mut verified_slot = None;
        for _ in 0..self.redirects {
            let Err(error) = &result else {
                break;
            };
            let Some((address, slot)) = error.redirect_node() else {
                break;
            };
            let Some((host, port)) = split_address(address) else {
                break;
            };
            if !known_single_target && !single_target(command) {
                let Some(slot) = Slot::new(slot) else { break };
                if verified_slot.is_none() {
                    verified_slot = command_slot(&mut transport, command).await;
                }
                if verified_slot != Some(slot) {
                    break;
                }
            }
            let ask = error.kind() == ErrorKind::Server(ServerErrorKind::Ask);
            result = self.query_redirect(command, host, port, ask).await;
        }
        result
    }

    async fn query_redirect<T: FromRedisValue>(
        &self,
        command: &Cmd,
        host: &str,
        port: u16,
        ask: bool,
    ) -> RedisResult<T> {
        let address = match self.seed.addr().clone() {
            ConnectionAddr::TcpTls {
                insecure,
                tls_params,
                ..
            } => ConnectionAddr::TcpTls {
                host: host.into(),
                port,
                insecure,
                tls_params,
            },
            _ => ConnectionAddr::Tcp(host.into(), port),
        };
        let info = self.seed.as_ref().clone().set_addr(address);
        let timeout = Some(Duration::from_secs(5));
        let config = AsyncConnectionConfig::new()
            .set_connection_timeout(timeout)
            .set_response_timeout(timeout);
        let mut connection = ClusterNode::connect_with_config(info, config).await?;
        if ask {
            // The importing node grants permission for exactly the following command.
            let (asking, result): (RedisResult<()>, RedisResult<T>) = ::redis::pipe()
                .ignore_errors()
                .cmd("ASKING")
                .add_command(command.clone())
                .query_async(&mut connection)
                .await?;
            asking?;
            result
        } else {
            command.query_async(&mut connection).await
        }
    }
}

fn split_address(address: &str) -> Option<(&str, u16)> {
    let (host, port) = address.rsplit_once(':')?;
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    let port = port.parse().ok()?;
    (!host.is_empty() && port != 0).then_some((host, port))
}

fn single_target(command: &Cmd) -> bool {
    let mut args = command.args_iter();
    let Some(Arg::Simple(name)) = args.next() else {
        return false;
    };
    if [
        b"GET".as_slice(),
        b"GETDEL",
        b"SET",
        b"EXPIRE",
        b"PEXPIRE",
        b"PTTL",
        b"TTL",
        b"INCR",
        b"INCRBY",
        b"DECR",
        b"DECRBY",
        b"PERSIST",
    ]
    .iter()
    .any(|key| name.eq_ignore_ascii_case(key))
    {
        return true;
    }
    if [b"DEL".as_slice(), b"EXISTS", b"UNLINK", b"TOUCH"]
        .iter()
        .any(|key| name.eq_ignore_ascii_case(key))
    {
        return args.len() == 1;
    }
    [
        b"EVAL".as_slice(),
        b"EVALSHA",
        b"EVAL_RO",
        b"EVALSHA_RO",
        b"FCALL",
        b"FCALL_RO",
    ]
    .iter()
    .any(|key| name.eq_ignore_ascii_case(key))
        && matches!(args.nth(1), Some(Arg::Simple(b"1")))
}

/// Verify uncommon commands only after a redirect. Fan-out writes may have already
/// succeeded on another shard, so the original command must target just this slot.
async fn command_slot(transport: &mut ClusterTransport, command: &Cmd) -> Option<Slot> {
    let mut metadata = ::redis::cmd("COMMAND");
    metadata.arg("GETKEYS");
    for argument in command.args_iter() {
        let Arg::Simple(bytes) = argument else {
            return None;
        };
        metadata.arg(bytes);
    }
    let keys: Vec<Vec<u8>> = metadata.query_async(transport).await.ok()?;
    let (first, remaining) = keys.split_first()?;
    let slot = Slot::for_key(first);
    remaining
        .iter()
        .all(|key| Slot::for_key(key) == slot)
        .then_some(slot)
}
