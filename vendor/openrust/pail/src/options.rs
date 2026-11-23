use openrust::console::{ConsoleError, Input};

use crate::value_objects::MessageLogged;

/// Filters applied together to each captured message, following Pail's options.
#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
pub struct Options {
    timeout: i64,
    authId: Option<String>,
    level: Option<String>,
    filter: Option<String>,
    message: Option<String>,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Options {
    pub fn new(
        timeout: i64,
        authId: Option<String>,
        level: Option<String>,
        filter: Option<String>,
        message: Option<String>,
    ) -> Self {
        Self {
            timeout,
            authId,
            level,
            filter,
            message,
        }
    }

    /// Rust separates the parsed input from the command implementation.
    pub fn fromCommand(input: &Input) -> Result<Self, ConsoleError> {
        let option = |name| {
            input
                .matches()
                .try_get_one::<String>(name)
                .map(|value| value.cloned())
                .map_err(|error| ConsoleError::new(error.to_string()))
        };
        let authId = match option("auth")? {
            Some(value) => Some(value),
            None => option("user")?,
        };
        Ok(Self::new(
            integer(option("timeout")?.as_deref().unwrap_or("3600")),
            authId,
            option("level")?,
            option("filter")?,
            option("message")?,
        ))
    }

    pub fn accepts(&self, messageLogged: &MessageLogged) -> bool {
        if self
            .authId
            .as_ref()
            .is_some_and(|id| messageLogged.authId().as_ref() != Some(id))
        {
            return false;
        }
        if self
            .level
            .as_ref()
            .is_some_and(|level| !level.eq_ignore_ascii_case(messageLogged.level()))
        {
            return false;
        }
        if self
            .filter
            .as_ref()
            .is_some_and(|filter| !contains(&messageLogged.to_string(), filter))
        {
            return false;
        }
        self.message
            .as_ref()
            .is_none_or(|filter| contains(messageLogged.message(), filter))
    }

    pub fn timeout(&self) -> i64 {
        self.timeout
    }
}

fn contains(value: &str, filter: &str) -> bool {
    value
        .to_ascii_lowercase()
        .contains(&filter.to_ascii_lowercase())
}

// PHP's integer cast accepts a numeric prefix, including a decimal/exponent.
fn integer(value: &str) -> i64 {
    let value = value.trim_start_matches([' ', '\t', '\r', '\n', '\x0b', '\x0c']);
    let bytes = value.as_bytes();
    let mut end = usize::from(bytes.first().is_some_and(|c| matches!(c, b'+' | b'-')));
    let start = end;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    let mut digits = end > start;
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        let fraction = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        digits |= end > fraction;
    }
    if !digits {
        return 0;
    }
    if bytes.get(end).is_some_and(|c| matches!(c, b'e' | b'E')) {
        let mut exponent = end + 1;
        if bytes
            .get(exponent)
            .is_some_and(|c| matches!(c, b'+' | b'-'))
        {
            exponent += 1;
        }
        let start = exponent;
        while bytes.get(exponent).is_some_and(u8::is_ascii_digit) {
            exponent += 1;
        }
        if exponent > start {
            end = exponent;
        }
    }
    let prefix = &value[..end];
    match prefix.parse::<i64>() {
        Ok(value) => value,
        Err(_) if !prefix.contains(['.', 'e', 'E']) => {
            if prefix.starts_with('-') {
                i64::MIN
            } else {
                i64::MAX
            }
        }
        Err(_) => prefix
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .unwrap_or(0.0) as i64,
    }
}
