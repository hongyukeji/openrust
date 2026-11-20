use std::{path::PathBuf, sync::Arc};

use serde::Deserialize;

#[cfg(any(unix, windows))]
use super::Level;
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

#[cfg(unix)]
fn default_socket() -> PathBuf {
    if cfg!(target_os = "macos") {
        "/var/run/syslog".into()
    } else {
        "/dev/log".into()
    }
}

#[cfg(unix)]
pub(super) fn writer(
    path: Option<PathBuf>,
    ident: String,
    facility: u8,
) -> LogResult<Arc<RecordHandler>> {
    use std::os::unix::net::UnixDatagram;

    let path = path.unwrap_or_else(default_socket);
    let socket = UnixDatagram::unbound().map_err(LogError::write)?;
    socket.set_nonblocking(true).map_err(LogError::write)?;
    let ident = ident.replace(
        |character: char| !character.is_ascii_graphic() || matches!(character, '[' | ']' | ':'),
        "-",
    );
    let tag = format!("{ident}[{}]", std::process::id());
    Ok(Arc::new(move |record, formatted| {
        let severity = priority(record.level);
        let message = formatted.trim_end_matches(['\n', '\r']);
        let time = record
            .at
            .to_zoned(jiff::tz::TimeZone::system())
            .strftime("%b %e %T");
        let packet = format!("<{}>{time} {tag}: {message}", facility + severity);
        socket
            .send_to(packet.as_bytes(), &path)
            .map_err(LogError::write)?;
        Ok(())
    }))
}

#[cfg(any(unix, windows))]
fn priority(level: Level) -> u8 {
    match level {
        Level::Emergency => 0,
        Level::Alert => 1,
        Level::Critical => 2,
        Level::Error => 3,
        Level::Warning => 4,
        Level::Notice => 5,
        Level::Info => 6,
        Level::Debug => 7,
    }
}

#[cfg(windows)]
pub(super) fn writer(path: Option<PathBuf>, ident: String, _: u8) -> LogResult<Arc<RecordHandler>> {
    use std::{io, ptr};
    use windows_sys::Win32::System::EventLog::{
        DeregisterEventSource, EVENTLOG_ERROR_TYPE, EVENTLOG_INFORMATION_TYPE,
        EVENTLOG_WARNING_TYPE, RegisterEventSourceW, ReportEventW,
    };

    if path.is_some() {
        return Err(LogError::InvalidConfiguration);
    }
    let ident = ident.replace(
        |character: char| character.is_control() || character == '\\',
        "-",
    );
    let source = wide(&ident);
    let tag = wide(&format!("{ident}[{}]", std::process::id()));
    Ok(Arc::new(move |record, formatted| {
        let message = wide(formatted.trim_end_matches(['\r', '\n']));
        if message.len() > 31_840 {
            return Err(LogError::write(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Windows event insertion string exceeds its length limit",
            )));
        }
        let (kind, id) = match record.level {
            Level::Alert => (EVENTLOG_ERROR_TYPE, 1),
            Level::Info => (EVENTLOG_INFORMATION_TYPE, 2),
            _ => (EVENTLOG_WARNING_TYPE, 3),
        };
        let strings = [tag.as_ptr(), message.as_ptr()];
        // SAFETY: the source is NUL terminated and both insertion strings
        // remain valid until ReportEventW returns. The handle is released once.
        unsafe {
            let handle = RegisterEventSourceW(ptr::null(), source.as_ptr());
            if handle.is_null() {
                return Err(LogError::write(io::Error::last_os_error()));
            }
            let written = ReportEventW(
                handle,
                kind,
                u16::from(priority(record.level)),
                id,
                ptr::null_mut(),
                2,
                0,
                strings.as_ptr(),
                ptr::null(),
            );
            let result = if written == 0 {
                Err(LogError::write(io::Error::last_os_error()))
            } else {
                Ok(())
            };
            DeregisterEventSource(handle);
            result
        }
    }))
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value
        .replace('\0', "\\0")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(not(any(unix, windows)))]
pub(super) fn writer(_: Option<PathBuf>, _: String, _: u8) -> LogResult<Arc<RecordHandler>> {
    Err(LogError::write(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "system logging is unavailable on this platform",
    )))
}
