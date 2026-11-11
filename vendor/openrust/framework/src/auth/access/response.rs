/// An authorization decision, optionally carrying a denial reason.
#[derive(Debug)]
pub struct Response {
    message: String,
}

impl Response {
    pub fn deny(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn into_message(self) -> String {
        self.message
    }
}
