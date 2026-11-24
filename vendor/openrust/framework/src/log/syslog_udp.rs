use super::{LogError, LogResult, RecordHandler, syslog};
use serde::Deserialize;
use serde_json::Value;
use std::{
    io,
    net::{ToSocketAddrs, UdpSocket},
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Deserialize)]
#[serde(untagged)]
enum Port {
    Number(u16),
    String(String),
}
impl Default for Port {
    fn default() -> Self {
        Self::Number(514)
    }
}
impl Port {
    fn number(self) -> LogResult<u16> {
        match self {
            Self::Number(port) => Ok(port),
            Self::String(port) => port.parse().map_err(|_| LogError::InvalidConfiguration),
        }
    }
}
#[derive(Deserialize)]
struct Options {
    host: String,
    #[serde(default)]
    port: Port,
    facility: Option<syslog::Facility>,
    ident: Option<String>,
    rfc: Option<u8>,
}
enum Socket {
    Network(UdpSocket),
    #[cfg(unix)]
    Local(std::os::unix::net::UnixDatagram),
}

pub(super) fn writer(
    config: &Value,
    root: &Path,
    default_ident: &str,
) -> LogResult<Arc<RecordHandler>> {
    let options: Options =
        serde_json::from_value(config.clone()).map_err(|_| LogError::InvalidConfiguration)?;
    let port = options.port.number()?;
    let rfc = options.rfc.unwrap_or(1);
    if options.host.trim().is_empty() || rfc > 2 || (port == 0 && !cfg!(unix)) {
        return Err(LogError::InvalidConfiguration);
    }
    let facility = options
        .facility
        .unwrap_or(syslog::Facility::Number(8))
        .priority()?;
    let ident = options.ident.unwrap_or_else(|| default_ident.to_owned());
    let hostname = hostname::get()
        .ok()
        .and_then(|name| name.into_string().ok())
        .unwrap_or_else(|| "-".into());
    #[cfg(unix)]
    let local_path = root.join(&options.host);
    #[cfg(not(unix))]
    let _ = root;
    let cached = Mutex::new(None);
    Ok(Arc::new(move |record, formatted| {
        let mut cached = cached
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let socket = match cached.take() {
            Some(socket) => socket,
            None => connect(&options.host, port)?,
        };
        let priority = facility + syslog::priority(record.level);
        let timestamp = record.at.strftime(match rfc {
            0 => "%b %d %T",
            2 => "%Y-%m-%dT%H:%M:%S%.3f%:z",
            _ => "%Y-%m-%dT%H:%M:%S%:z",
        });
        let header = if rfc == 0 {
            format!(
                "<{priority}>{timestamp} {hostname} {ident}[{}]: ",
                std::process::id()
            )
        } else {
            format!(
                "<{priority}>1 {timestamp} {hostname} {ident} {} - - ",
                std::process::id()
            )
        };
        let limit = 65023usize.saturating_sub(header.len());
        for line in formatted
            .split([
                '\n', '\r', '\u{b}', '\u{c}', '\u{85}', '\u{2028}', '\u{2029}',
            ])
            .filter(|line| !line.is_empty())
        {
            let mut end = line.len().min(limit);
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            let packet = format!("{header}{}", &line[..end]);
            match &socket {
                Socket::Network(socket) => {
                    socket.send(packet.as_bytes())?;
                }
                #[cfg(unix)]
                Socket::Local(socket) => {
                    socket.send_to(packet.as_bytes(), &local_path)?;
                }
            }
        }
        *cached = Some(socket);
        Ok(())
    }))
}
fn connect(host: &str, port: u16) -> io::Result<Socket> {
    #[cfg(unix)]
    if port == 0 {
        let socket = std::os::unix::net::UnixDatagram::unbound()?;
        socket.set_nonblocking(true)?;
        return Ok(Socket::Local(socket));
    }
    let mut last = io::Error::other("未找到远程日志接收地址。");
    for address in (host, port).to_socket_addrs()? {
        let result = UdpSocket::bind(if address.is_ipv6() {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        })
        .and_then(|socket| {
            socket.connect(address)?;
            socket.set_write_timeout(Some(std::time::Duration::from_secs(10)))?;
            Ok(Socket::Network(socket))
        });
        match result {
            Ok(socket) => return Ok(socket),
            Err(error) => last = error,
        }
    }
    Err(last)
}
