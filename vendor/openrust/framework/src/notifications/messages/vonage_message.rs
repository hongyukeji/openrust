use serde::{Deserialize, Serialize};

/// The content and delivery options for a Vonage SMS notification.
#[derive(Default, Deserialize, Serialize)]
pub struct VonageMessage {
    pub(crate) content: String,
    pub(crate) from: Option<String>,
    pub(crate) unicode: bool,
    pub(crate) client_reference: Option<String>,
    pub(crate) status_callback: Option<String>,
    pub(crate) client: Option<String>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Vonage message builder")]
impl VonageMessage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    pub fn from(mut self, from: impl Into<String>) -> Self {
        self.from = Some(from.into());
        self
    }

    pub fn unicode(mut self) -> Self {
        self.unicode = true;
        self
    }

    pub fn clientReference(mut self, reference: impl Into<String>) -> Self {
        self.client_reference = Some(reference.into());
        self
    }

    pub fn statusCallback(mut self, callback: impl Into<String>) -> Self {
        self.status_callback = Some(callback.into());
        self
    }

    /// Selects an application-configured client by name.
    pub fn usingClient(mut self, name: impl Into<String>) -> Self {
        self.client = Some(name.into());
        self
    }
}
