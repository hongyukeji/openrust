use crate::http::StatusCode;

/// An authorization decision and its optional public reason and HTTP status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    allowed: bool,
    message: String,
    status: Option<StatusCode>,
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel's authorization response API"
)]
impl Response {
    pub fn allow() -> Self {
        Self {
            allowed: true,
            message: String::new(),
            status: None,
        }
    }

    pub fn deny(message: impl Into<String>) -> Self {
        Self {
            allowed: false,
            message: message.into(),
            status: None,
        }
    }

    pub fn denyWithStatus(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status: Some(status),
            ..Self::deny(message)
        }
    }

    pub fn denyAsNotFound(message: impl Into<String>) -> Self {
        Self::denyWithStatus(StatusCode::NOT_FOUND, message)
    }

    pub fn allowed(&self) -> bool {
        self.allowed
    }
    pub fn denied(&self) -> bool {
        !self.allowed
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    pub fn status(&self) -> Option<StatusCode> {
        self.status
    }
    pub(crate) fn into_message(self) -> String {
        self.message
    }
}

impl From<bool> for Response {
    fn from(allowed: bool) -> Self {
        if allowed {
            Self::allow()
        } else {
            Self::deny("This action is unauthorized.")
        }
    }
}
