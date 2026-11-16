use std::{path::PathBuf, sync::Arc};

use serde::Deserialize;

use super::{LogError, LogResult, RecordHandler};

const FACILITIES: &[(&str, u8)] = &[
    ("kern", 0),
    ("user", 8),
    ("mail", 16),
    ("daemon", 24),
    ("auth", 32),
    ("syslog", 40),
    ("lpr", 48),
    ("news", 56),
    ("uucp", 64),
    ("cron", 72),
    ("authpriv", 80),
    ("local0", 128),
    ("local1", 136),
    ("local2", 144),
    ("local3", 152),
    ("local4", 160),
    ("local5", 168),
    ("local6", 176),
    ("local7", 184),
];

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Facility {
    Name(String),
    Number(u8),
}

impl Facility {
    pub(super) fn priority(self) -> LogResult<u8> {
        FACILITIES
            .iter()
            .find_map(|(name, value)| {
                let matches = match &self {
                    Self::Name(requested) => name.eq_ignore_ascii_case(requested),
                    Self::Number(requested) => requested == value,
                };
                matches.then_some(*value)
            })
            .ok_or(LogError::InvalidConfiguration)
    }
}

pub(super) fn default_socket() -> PathBuf {
    if cfg!(target_os = "macos") {
        "/var/run/syslog".into()
    } else {
        "/dev/log".into()
    }
}

#[cfg(unix)]
pub(super) fn writer(path: PathBuf, ident: String, facility: u8) -> LogResult<Arc<RecordHandler>> {
    use super::Level;
    use std::os::unix::net::UnixDatagram;

    let socket = UnixDatagram::unbound().map_err(LogError::write)?;
    socket.set_nonblocking(true).map_err(LogError::write)?;
    let ident = ident.replace(
        |character: char| !character.is_ascii_graphic() || matches!(character, '[' | ']' | ':'),
        "-",
    );
    let tag = format!("{ident}[{}]", std::process::id());
    Ok(Arc::new(move |record, formatted| {
        let severity = match record.level {
            Level::Emergency => 0,
            Level::Alert => 1,
            Level::Critical => 2,
            Level::Error => 3,
            Level::Warning => 4,
            Level::Notice => 5,
            Level::Info => 6,
            Level::Debug => 7,
        };
        let message = formatted.trim_end_matches(['\n', '\r']);
        let time = record.at.with_timezone(&chrono::Local).format("%b %e %T");
        let packet = format!("<{}>{time} {tag}: {message}", facility + severity);
        socket
            .send_to(packet.as_bytes(), &path)
            .map_err(LogError::write)?;
        Ok(())
    }))
}

#[cfg(not(unix))]
pub(super) fn writer(_: PathBuf, _: String, _: u8) -> LogResult<Arc<RecordHandler>> {
    Err(LogError::write(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "local Unix syslog is unavailable on this platform",
    )))
}
