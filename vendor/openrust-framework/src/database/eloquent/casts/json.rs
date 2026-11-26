use std::sync::Arc;

use serde_json::Value as Document;

use crate::{
    contracts::container::Container as _,
    database::{Connection, DatabaseError, DatabaseResult, Value},
};

type Encoder = dyn Fn(&Document) -> DatabaseResult<String> + Send + Sync;
type Decoder = dyn Fn(&str) -> DatabaseResult<Document> + Send + Sync;

/// JSON encoding used for stored Eloquent attributes.
#[derive(Clone, Default)]
pub struct Json {
    encoder: Option<Arc<Encoder>>,
    decoder: Option<Arc<Decoder>>,
}

impl Json {
    pub(super) const DEFAULT: Self = Self {
        encoder: None,
        decoder: None,
    };

    /// Sets the encoder on the codec installed in an application's container.
    #[must_use]
    pub fn encode_using(
        mut self,
        encoder: impl Fn(&Document) -> DatabaseResult<String> + Send + Sync + 'static,
    ) -> Self {
        self.encoder = Some(Arc::new(encoder));
        self
    }

    /// Sets the decoder on the codec installed in an application's container.
    #[must_use]
    pub fn decode_using(
        mut self,
        decoder: impl Fn(&str) -> DatabaseResult<Document> + Send + Sync + 'static,
    ) -> Self {
        self.decoder = Some(Arc::new(decoder));
        self
    }

    pub fn encode(&self, value: &Document) -> DatabaseResult<String> {
        self.encoder.as_ref().map_or_else(
            || serde_json::to_string(value).map_err(|_| invalid()),
            |encoder| encoder(value),
        )
    }

    pub fn decode(&self, value: &str) -> DatabaseResult<Document> {
        self.decoder.as_ref().map_or_else(
            || serde_json::from_str(value).map_err(|_| invalid()),
            |decoder| decoder(value),
        )
    }

    pub(super) fn for_model(connection: Option<&Connection>) -> DatabaseResult<Self> {
        let Some(container) = Connection::model_container(connection) else {
            return Ok(Self::DEFAULT);
        };
        if !container.bound::<Self>() {
            return Ok(Self::DEFAULT);
        }
        container
            .make::<Self>()
            .map(|codec| (*codec).clone())
            .map_err(|_| invalid())
    }

    pub(super) fn decode_lossy(&self, value: &str) -> DatabaseResult<Option<Document>> {
        self.decoder.as_ref().map_or_else(
            || Ok(serde_json::from_str(value).ok()),
            |decoder| decoder(value).map(Some),
        )
    }

    pub(super) fn decode_bytes_lossy(&self, value: &[u8]) -> DatabaseResult<Option<Document>> {
        match std::str::from_utf8(value) {
            Ok(value) => self.decode_lossy(value),
            Err(_) if self.decoder.is_some() => Err(invalid()),
            Err(_) => Ok(None),
        }
    }

    pub(super) fn decode_value(&self, value: &Value) -> DatabaseResult<Option<Document>> {
        match value {
            Value::Text(text) => self.decode_lossy(text),
            Value::Json(document) => self.decoder.as_ref().map_or_else(
                || Ok(Some(document.clone())),
                |decoder| decoder(&Self::DEFAULT.encode(document)?).map(Some),
            ),
            _ => Ok(None),
        }
    }

    pub(super) fn store(&self, value: Document) -> DatabaseResult<Value> {
        match &self.encoder {
            Some(encoder) => encoder(&value).map(Value::Text),
            None => Ok(Value::Json(value)),
        }
    }
}

fn invalid() -> DatabaseError {
    DatabaseError::InvalidQuery("Eloquent JSON codec")
}
