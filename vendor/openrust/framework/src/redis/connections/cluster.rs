use std::time::Duration;

use ::redis::{
    Arg, AsyncConnectionConfig, Client, Cmd, ConnectionAddr, ConnectionInfo, ErrorKind,
    FromRedisValue, RedisResult, ServerErrorKind, cluster::ClusterClient,
    cluster_async::ClusterConnection,
};

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
        mut transport: ClusterConnection,
        command: &Cmd,
        known_single_target: bool,
    ) -> RedisResult<T> {
        let mut result = command.query_async(&mut transport).await;
        for _ in 0..self.redirects {
            let Err(error) = &result else {
                break;
            };
            let Some((address, _)) = error.redirect_node() else {
                break;
            };
            if !known_single_target && !single_target(command) {
                break;
            }
            let Some((host, port)) = split_address(address) else {
                break;
            };
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
        let client = Client::open(self.seed.as_ref().clone().set_addr(address))?;
        let timeout = Some(Duration::from_secs(5));
        let config = AsyncConnectionConfig::new()
            .set_connection_timeout(timeout)
            .set_response_timeout(timeout);
        let mut connection = client
            .get_multiplexed_async_connection_with_config(&config)
            .await?;
        if ask {
            // The importing node grants permission for exactly the following command.
            ::redis::pipe()
                .cmd("ASKING")
                .ignore()
                .add_command(command.clone())
                .query_async::<(T,)>(&mut connection)
                .await
                .map(|(result,)| result)
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
