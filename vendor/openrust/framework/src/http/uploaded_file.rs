use axum::body::Bytes;

/// A received upload with untrusted client metadata and bounded file content.
#[derive(Clone, Debug)]
pub struct UploadedFile {
    path: String,
    media_type: Option<String>,
    content: Bytes,
}

#[allow(non_snake_case, reason = "Matches Laravel's uploaded-file API")]
impl UploadedFile {
    pub(super) fn new(path: String, media_type: Option<String>, content: Bytes) -> Self {
        Self {
            path,
            media_type,
            content,
        }
    }

    /// The filename supplied by the client. Treat it as display data.
    pub fn getClientOriginalName(&self) -> &str {
        self.path.rsplit(['/', '\\']).next().unwrap_or_default()
    }

    /// The original client path, which may include directories.
    pub fn getClientOriginalPath(&self) -> &str {
        &self.path
    }

    /// The media type supplied by the client.
    pub fn getClientMimeType(&self) -> Option<&str> {
        self.media_type.as_deref()
    }

    pub fn getSize(&self) -> usize {
        self.content.len()
    }

    pub fn get(&self) -> Bytes {
        self.content.clone()
    }
}
