use std::{error::Error, pin::Pin};

use async_stream::try_stream;
use axum::body::Bytes;
use futures_util::{Stream, StreamExt as _, stream};
use http::{
    HeaderName, HeaderValue,
    header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE},
};
use serde::Serialize;

use super::{HeaderMap, HttpError, Response, StatusCode, filename_header};

type StreamError = Box<dyn Error + Send + Sync>;
type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, StreamError>> + Send>>;

/// A JSON document assembled incrementally from values, containers and item streams.
pub struct StreamedJson(ByteStream);

impl StreamedJson {
    /// Serializes a value before response headers are sent.
    pub fn value(value: impl Serialize) -> Result<Self, HttpError> {
        let bytes = serde_json::to_vec(&value).map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        Ok(Self(Box::pin(stream::once(async {
            Ok(Bytes::from(bytes))
        }))))
    }

    /// Streams a source into a JSON array without collecting it.
    pub fn items<T, E>(source: impl Stream<Item = Result<T, E>> + Send + 'static) -> Self
    where
        T: Serialize,
        E: Into<StreamError>,
    {
        Self::sequence(b"[", b"]", source, |item, bytes| {
            serde_json::to_writer(bytes, &item)
        })
    }

    /// Streams key-value pairs into a JSON object without collecting them.
    pub fn entries<K, T, E>(source: impl Stream<Item = Result<(K, T), E>> + Send + 'static) -> Self
    where
        K: AsRef<str>,
        T: Serialize,
        E: Into<StreamError>,
    {
        Self::sequence(b"{", b"}", source, |(key, value): (K, T), bytes| {
            serde_json::to_writer(&mut *bytes, key.as_ref())?;
            bytes.push(b':');
            serde_json::to_writer(bytes, &value)
        })
    }

    fn sequence<T, E>(
        open: &'static [u8],
        close: &'static [u8],
        source: impl Stream<Item = Result<T, E>> + Send + 'static,
        encode: impl Fn(T, &mut Vec<u8>) -> serde_json::Result<()> + Send + 'static,
    ) -> Self
    where
        E: Into<StreamError>,
    {
        Self(Box::pin(try_stream! {
            yield Bytes::from_static(open);
            let mut comma = false;
            let mut source = std::pin::pin!(source.map(move |item| {
                let item = item.map_err(Into::<StreamError>::into)?;
                let mut bytes = Vec::new();
                if comma {
                    bytes.push(b',');
                }
                encode(item, &mut bytes)?;
                comma = true;
                Ok::<_, StreamError>(Bytes::from(bytes))
            }));
            while let Some(item) = source.next().await {
                yield item?;
            }
            yield Bytes::from_static(close);
        }))
    }

    pub fn array(items: impl IntoIterator<Item = Self>) -> Self {
        Self::container(
            b"[",
            b"]",
            items.into_iter().map(|item| (None, item)).collect(),
        )
    }

    pub fn object<K: Into<String>>(fields: impl IntoIterator<Item = (K, Self)>) -> Self {
        Self::container(
            b"{",
            b"}",
            fields
                .into_iter()
                .map(|(key, item)| (Some(key.into()), item))
                .collect(),
        )
    }

    fn container(
        open: &'static [u8],
        close: &'static [u8],
        fields: Vec<(Option<String>, Self)>,
    ) -> Self {
        Self(Box::pin(try_stream! {
            yield Bytes::from_static(open);
            for (index, (key, Self(mut value))) in fields.into_iter().enumerate() {
                let mut prefix = Vec::new();
                if index != 0 {
                    prefix.push(b',');
                }
                if let Some(key) = key {
                    serde_json::to_writer(&mut prefix, &key)?;
                    prefix.push(b':');
                }
                if !prefix.is_empty() {
                    yield Bytes::from(prefix);
                }
                while let Some(bytes) = value.next().await {
                    yield bytes?;
                }
            }
            yield Bytes::from_static(close);
        }))
    }
}

/// An event and its payload in a server-sent event response.
pub struct StreamedEvent {
    event: String,
    data: String,
}

impl StreamedEvent {
    pub fn new(event: impl Into<String>, data: impl Into<String>) -> Result<Self, HttpError> {
        let event = event.into();
        if event.contains(['\r', '\n', '\0']) {
            return Err(HttpError::InvalidRequest);
        }
        Ok(Self {
            event,
            data: data.into(),
        })
    }

    pub fn message(data: impl Into<String>) -> Self {
        Self {
            event: "update".to_owned(),
            data: data.into(),
        }
    }

    pub fn json(event: impl Into<String>, data: impl Serialize) -> Result<Self, HttpError> {
        let data = serde_json::to_string(&data).map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        Self::new(event, data)
    }

    fn encode(self) -> Bytes {
        use std::fmt::Write as _;

        let mut frame = format!("event: {}\n", self.event);
        for line in self
            .data
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .split('\n')
        {
            writeln!(frame, "data: {line}").expect("writing to String");
        }
        frame.push('\n');
        Bytes::from(frame)
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's streamed response API")]
impl Response {
    pub fn streamJson(status: StatusCode, data: StreamedJson) -> Self {
        if status.is_informational()
            || matches!(status, StatusCode::NO_CONTENT | StatusCode::NOT_MODIFIED)
        {
            return Self::empty(status);
        }
        Self::stream(status, data.0)
            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
    }

    pub fn eventStream<E>(
        source: impl Stream<Item = Result<StreamedEvent, E>> + Send + 'static,
    ) -> Self
    where
        E: Into<StreamError> + 'static,
    {
        Self::eventStreamWithEnd(source, Some(StreamedEvent::message("</stream>")))
    }

    /// Supplies a custom final event, or disables the final event with `None`.
    pub fn eventStreamWithEnd<E>(
        source: impl Stream<Item = Result<StreamedEvent, E>> + Send + 'static,
        end: Option<StreamedEvent>,
    ) -> Self
    where
        E: Into<StreamError> + 'static,
    {
        let events = try_stream! {
            let mut source = std::pin::pin!(source.map(|event| {
                event.map_err(Into::<StreamError>::into).map(StreamedEvent::encode)
            }));
            while let Some(event) = source.next().await {
                yield event?;
            }
            if let Some(event) = end {
                yield event.encode();
            }
        };
        Self::stream::<Bytes, StreamError>(StatusCode::OK, events)
            .header(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))
            .header(CACHE_CONTROL, HeaderValue::from_static("no-cache"))
            .header(
                HeaderName::from_static("x-accel-buffering"),
                HeaderValue::from_static("no"),
            )
    }

    pub fn streamDownload<T, E>(
        source: impl Stream<Item = Result<T, E>> + Send + 'static,
        name: Option<&str>,
        mut headers: HeaderMap,
        disposition: &str,
    ) -> Result<Self, HttpError>
    where
        T: Into<Bytes> + 'static,
        E: Into<StreamError> + 'static,
    {
        if let Some(name) = name {
            headers.insert(CONTENT_DISPOSITION, filename_header(disposition, name)?);
        }
        Ok(Self::stream(StatusCode::OK, source).withHeaders(headers))
    }
}
