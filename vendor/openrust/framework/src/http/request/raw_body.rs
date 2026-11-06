//! 单次消费网络正文，按阈值保留内存或异步写入受管暂存。

use std::{
    future::Future,
    io::{Read, Write},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::body::{Body, Bytes};
use futures_util::StreamExt;
use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};

use crate::{Error, Result};

/// 受管理 Request 的额外资源预算；Axum `DefaultBodyLimit` 仍先行生效。
#[derive(Clone, Copy, Debug)]
pub struct RequestBodyLimits {
    /// 最大原始正文大小。默认32 MiB；禁用 Axum 默认限制也不能绕过它。
    pub max_bytes: usize,
    /// 超过此大小将原文从内存移入受管暂存；默认256 KiB。
    pub memory_threshold: usize,
    /// 动态 JSON/form 与显式 JSON bag 解码时允许的最大字节数；默认8 MiB。
    /// 这是独立于原文暂存阈值的 Rust 内存预算。
    pub max_decoded_bytes: usize,
    /// 单个上传文件最大大小；默认16 MiB。
    pub max_file_bytes: usize,
    /// 单个文本字段最大大小；默认1 MiB。
    pub max_field_bytes: usize,
    /// multipart 总字段数；默认1024。
    pub max_fields: usize,
    /// 单个 multipart part 的头部最大字节数；默认16 KiB。
    pub max_part_header_bytes: usize,
}

impl Default for RequestBodyLimits {
    fn default() -> Self {
        Self {
            max_bytes: 32 * 1024 * 1024,
            memory_threshold: 256 * 1024,
            max_decoded_bytes: 8 * 1024 * 1024,
            max_file_bytes: 16 * 1024 * 1024,
            max_field_bytes: 1024 * 1024,
            max_fields: 1024,
            max_part_header_bytes: 16 * 1024,
        }
    }
}

impl RequestBodyLimits {
    pub(crate) fn validate(self) -> Result<Self> {
        if self.max_bytes == 0
            || self.memory_threshold == 0
            || self.max_decoded_bytes == 0
            || self.max_file_bytes == 0
            || self.max_field_bytes == 0
            || self.max_fields == 0
            || self.max_part_header_bytes == 0
            || self.memory_threshold > self.max_bytes
        {
            return Err(Error::custom(
                "request_body_limits_invalid",
                "请求正文资源限制无效",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone)]
pub(crate) enum RawBody {
    Memory(Bytes),
    Temporary {
        path: Arc<tempfile::TempPath>,
        len: usize,
    },
}

/// 写任务独占文件与路径；任务即使被取消也先关闭文件再删除暂存。
pub(crate) struct TempWriter {
    file: std::fs::File,
    path: tempfile::TempPath,
}

impl TempWriter {
    pub(crate) async fn create() -> std::io::Result<Self> {
        tokio::task::spawn_blocking(|| {
            let (file, path) = tempfile::NamedTempFile::new()?.into_parts();
            Ok(Self { file, path })
        })
        .await
        .map_err(std::io::Error::other)?
    }

    pub(crate) async fn write(self, bytes: Vec<u8>) -> std::io::Result<Self> {
        self.write_with_hook(bytes, || Ok(())).await
    }

    async fn write_with_hook(
        self,
        bytes: Vec<u8>,
        before: impl FnOnce() -> std::io::Result<()> + Send + 'static,
    ) -> std::io::Result<Self> {
        // 取消等待只丢 JoinHandle；闭包仍拥有 file/path，写完才按字段顺序销毁。
        let (writer, result) = tokio::task::spawn_blocking(move || {
            let mut writer = self;
            let result = before().and_then(|()| writer.file.write_all(&bytes));
            (writer, result)
        })
        .await
        .map_err(std::io::Error::other)?;
        result?;
        Ok(writer)
    }

    pub(crate) fn finish(self) -> tempfile::TempPath {
        let Self { file, path } = self;
        drop(file);
        path
    }
}

/// 保持原始正文暂存所有权的独立异步读取游标。
pub struct RawBodyReader {
    inner: Reader,
}

enum Reader {
    Memory { bytes: Bytes, position: usize },
    Temporary(TempFileReader),
}

pub(crate) struct TempFileReader {
    state: TempReadState,
    #[cfg(test)]
    before_read: Option<Box<dyn FnOnce() + Send>>,
}

enum TempReadState {
    Ready {
        file: std::fs::File,
        path: Arc<tempfile::TempPath>,
    },
    Buffered {
        file: std::fs::File,
        path: Arc<tempfile::TempPath>,
        bytes: Vec<u8>,
        position: usize,
    },
    Reading(
        tokio::task::JoinHandle<(
            std::fs::File,
            Arc<tempfile::TempPath>,
            std::io::Result<Vec<u8>>,
        )>,
    ),
    Closed,
}

impl TempFileReader {
    pub(crate) async fn open(path: Arc<tempfile::TempPath>) -> std::io::Result<Self> {
        Self::open_with_hook(path, || {}).await
    }

    async fn open_with_hook(
        path: Arc<tempfile::TempPath>,
        before: impl FnOnce() + Send + 'static,
    ) -> std::io::Result<Self> {
        // 打开任务也持有 path；等待者取消时不得先删掉后台尚在打开的文件。
        let (file, path) = Self::open_task(path, before)
            .await
            .map_err(std::io::Error::other)?;
        Ok(Self {
            state: TempReadState::Ready { file: file?, path },
            #[cfg(test)]
            before_read: None,
        })
    }

    fn open_task(
        path: Arc<tempfile::TempPath>,
        before: impl FnOnce() + Send + 'static,
    ) -> tokio::task::JoinHandle<(std::io::Result<std::fs::File>, Arc<tempfile::TempPath>)> {
        tokio::task::spawn_blocking(move || {
            before();
            let file = std::fs::File::open(path.as_ref());
            (file, path)
        })
    }
}

impl AsyncRead for TempFileReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if buffer.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        loop {
            match std::mem::replace(&mut self.state, TempReadState::Closed) {
                TempReadState::Ready { file, path } => {
                    let size = buffer.remaining().min(64 * 1024);
                    #[cfg(test)]
                    let before = self.before_read.take();
                    self.state = TempReadState::Reading(tokio::task::spawn_blocking(move || {
                        #[cfg(test)]
                        if let Some(before) = before {
                            before();
                        }
                        let mut file = file;
                        let mut bytes = vec![0_u8; size];
                        let result = file.read(&mut bytes).map(|count| {
                            bytes.truncate(count);
                            bytes
                        });
                        (file, path, result)
                    }));
                }
                TempReadState::Buffered {
                    file,
                    path,
                    bytes,
                    position,
                } => {
                    if bytes.is_empty() {
                        self.state = TempReadState::Buffered {
                            file,
                            path,
                            bytes,
                            position,
                        };
                        return Poll::Ready(Ok(()));
                    }
                    if position == bytes.len() {
                        self.state = TempReadState::Ready { file, path };
                        continue;
                    }
                    // 等待方取消后可能以更小的 ReadBuf 重试，剩余字节须分次交付。
                    let count = (bytes.len() - position).min(buffer.remaining());
                    buffer.put_slice(&bytes[position..position + count]);
                    self.state = TempReadState::Buffered {
                        file,
                        path,
                        bytes,
                        position: position + count,
                    };
                    return Poll::Ready(Ok(()));
                }
                TempReadState::Reading(mut task) => match Pin::new(&mut task).poll(context) {
                    Poll::Pending => {
                        self.state = TempReadState::Reading(task);
                        return Poll::Pending;
                    }
                    Poll::Ready(Ok((file, path, Ok(bytes)))) => {
                        self.state = TempReadState::Buffered {
                            file,
                            path,
                            bytes,
                            position: 0,
                        };
                    }
                    Poll::Ready(Ok((file, path, Err(error)))) => {
                        drop(file);
                        drop(path);
                        return Poll::Ready(Err(error));
                    }
                    Poll::Ready(Err(error)) => {
                        return Poll::Ready(Err(std::io::Error::other(error)));
                    }
                },
                TempReadState::Closed => {
                    return Poll::Ready(Err(std::io::Error::other("暂存读取状态无效")));
                }
            }
        }
    }
}

impl AsyncRead for RawBodyReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match &mut self.inner {
            Reader::Memory { bytes, position } => {
                let remaining = &bytes[*position..];
                let count = remaining.len().min(buffer.remaining());
                buffer.put_slice(&remaining[..count]);
                *position += count;
                Poll::Ready(Ok(()))
            }
            Reader::Temporary(reader) => Pin::new(reader).poll_read(context, buffer),
        }
    }
}

impl RawBody {
    pub(crate) async fn capture(body: Body, limits: RequestBodyLimits) -> Result<Self> {
        let mut stream = body.into_data_stream();
        let mut memory = Vec::new();
        let mut temporary: Option<TempWriter> = None;
        let mut total = 0usize;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| body_read_error(&error))?;
            total = total.checked_add(chunk.len()).ok_or_else(limit)?;
            if total > limits.max_bytes {
                return Err(limit());
            }
            if temporary.is_none() && total > limits.memory_threshold {
                let writer = TempWriter::create().await.map_err(|_| io_error())?;
                let mut bytes = std::mem::take(&mut memory);
                bytes.extend_from_slice(&chunk);
                temporary = Some(writer.write(bytes).await.map_err(|_| io_error())?);
                continue;
            }
            if let Some(writer) = temporary.take() {
                temporary = Some(writer.write(chunk.to_vec()).await.map_err(|_| io_error())?);
            } else {
                memory.extend_from_slice(&chunk);
            }
        }
        if let Some(writer) = temporary {
            let path = writer.finish();
            Ok(Self::Temporary {
                path: Arc::new(path),
                len: total,
            })
        } else {
            Ok(Self::Memory(Bytes::from(memory)))
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Memory(bytes) => bytes.len(),
            Self::Temporary { len, .. } => *len,
        }
    }

    pub(crate) async fn open(&self) -> Result<RawBodyReader> {
        let inner = match self {
            Self::Memory(bytes) => Reader::Memory {
                bytes: bytes.clone(),
                position: 0,
            },
            Self::Temporary { path, .. } => Reader::Temporary(
                TempFileReader::open(Arc::clone(path))
                    .await
                    .map_err(|_| io_error())?,
            ),
        };
        Ok(RawBodyReader { inner })
    }

    pub(crate) async fn bytes(&self, max: usize) -> Result<Bytes> {
        if self.len() > max {
            return Err(limit());
        }
        match self {
            Self::Memory(bytes) => Ok(bytes.clone()),
            Self::Temporary { .. } => {
                let mut reader = self.open().await?;
                let mut bytes = Vec::with_capacity(self.len());
                reader
                    .read_to_end(&mut bytes)
                    .await
                    .map_err(|_| io_error())?;
                Ok(Bytes::from(bytes))
            }
        }
    }
}

fn limit() -> Error {
    Error::custom_with_status(
        axum::http::StatusCode::PAYLOAD_TOO_LARGE,
        "payload_too_large",
        "请求正文超过资源限制",
    )
}
fn body_read_error(error: &axum::Error) -> Error {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        if current.is::<http_body_util::LengthLimitError>() {
            return limit();
        }
        source = current.source();
    }
    Error::BadRequest("请求正文无法读取".into())
}
fn io_error() -> Error {
    Error::custom("request_body_unavailable", "请求正文暂存不可读取")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::Path, time::Duration};

    async fn removed(path: &Path) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(3), async {
            while path.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| io_error())
    }

    #[tokio::test]
    async fn spill_reader_keeps_tempfile_after_request_clone_is_dropped() -> Result<()> {
        let limits = RequestBodyLimits {
            memory_threshold: 2,
            ..RequestBodyLimits::default()
        };
        let raw = RawBody::capture(Body::from("streamed content"), limits).await?;
        let RawBody::Temporary { path, len } = &raw else {
            return Err(io_error());
        };
        let path = path.to_path_buf();
        assert_eq!(*len, 16);
        let mut first = raw.open().await?;
        let mut second = raw.open().await?;
        drop(raw);
        assert!(path.exists());
        let mut prefix = [0_u8; 6];
        first
            .read_exact(&mut prefix)
            .await
            .map_err(|_| io_error())?;
        assert_eq!(&prefix, b"stream");
        let mut full = Vec::new();
        second
            .read_to_end(&mut full)
            .await
            .map_err(|_| io_error())?;
        assert_eq!(full, b"streamed content");
        drop(first);
        drop(second);
        assert!(!path.exists());
        Ok(())
    }

    #[tokio::test]
    async fn configured_hard_limit_rejects_even_when_axum_limit_is_disabled() -> Result<()> {
        let limits = RequestBodyLimits {
            max_bytes: 4,
            memory_threshold: 2,
            ..RequestBodyLimits::default()
        };
        let Err(error) = RawBody::capture(Body::from("oversize"), limits).await else {
            return Err(io_error());
        };
        assert_eq!(error.status(), axum::http::StatusCode::PAYLOAD_TOO_LARGE);
        Ok(())
    }

    #[tokio::test]
    async fn axum_length_limit_is_413_but_other_stream_failures_are_400() -> Result<()> {
        let body = Body::new(http_body_util::Limited::new(Body::from("oversize"), 4));
        let Err(error) = RawBody::capture(body, RequestBodyLimits::default()).await else {
            return Err(io_error());
        };
        assert_eq!(error.status(), axum::http::StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(error.code(), "payload_too_large");

        let body = Body::from_stream(futures_util::stream::once(async {
            Err::<Bytes, std::io::Error>(std::io::Error::other("private stream failure"))
        }));
        let Err(error) = RawBody::capture(body, RequestBodyLimits::default()).await else {
            return Err(io_error());
        };
        assert_eq!(error.status(), axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(error.code(), "bad_request");
        assert!(!error.to_string().contains("private stream failure"));
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_or_failed_writer_closes_before_temp_path_cleanup() -> Result<()> {
        let writer = TempWriter::create().await.map_err(|_| io_error())?;
        let path = writer.path.to_path_buf();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(writer.write_with_hook(vec![1, 2, 3], move || {
            let _ = started_tx.send(());
            release_rx.recv().map_err(std::io::Error::other)
        }));
        started_rx.await.map_err(|_| io_error())?;
        task.abort();
        let _ = task.await;
        assert!(path.exists());
        release_tx.send(()).map_err(|_| io_error())?;
        removed(&path).await?;

        let writer = TempWriter::create().await.map_err(|_| io_error())?;
        let path = writer.path.to_path_buf();
        assert!(
            writer
                .write_with_hook(vec![1], || Err(std::io::Error::other(
                    "private write failure"
                )))
                .await
                .is_err()
        );
        removed(&path).await
    }

    #[tokio::test]
    async fn cancelled_open_and_pending_read_retain_last_temp_owner_until_io_finishes() -> Result<()>
    {
        let raw = RawBody::capture(
            Body::from("streamed content"),
            RequestBodyLimits {
                memory_threshold: 2,
                ..RequestBodyLimits::default()
            },
        )
        .await?;
        let RawBody::Temporary { path, .. } = &raw else {
            return Err(io_error());
        };
        let pathname = path.to_path_buf();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let path_for_open = Arc::clone(path);
        let task = tokio::spawn(TempFileReader::open_with_hook(path_for_open, move || {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
        }));
        started_rx.await.map_err(|_| io_error())?;
        task.abort();
        let _ = task.await;
        drop(raw);
        assert!(pathname.exists());
        release_tx.send(()).map_err(|_| io_error())?;
        removed(&pathname).await?;

        let raw = RawBody::capture(
            Body::from("streamed content"),
            RequestBodyLimits {
                memory_threshold: 2,
                ..RequestBodyLimits::default()
            },
        )
        .await?;
        let RawBody::Temporary { path, .. } = &raw else {
            return Err(io_error());
        };
        let pathname = path.to_path_buf();
        let mut reader = TempFileReader::open(Arc::clone(path))
            .await
            .map_err(|_| io_error())?;
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        reader.before_read = Some(Box::new(move || {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
        }));
        let task = tokio::spawn(async move {
            let mut byte = [0_u8; 1];
            reader.read_exact(&mut byte).await
        });
        started_rx.await.map_err(|_| io_error())?;
        task.abort();
        let _ = task.await;
        drop(raw);
        assert!(pathname.exists());
        release_tx.send(()).map_err(|_| io_error())?;
        removed(&pathname).await
    }

    #[tokio::test]
    async fn cancelled_large_read_resumes_with_small_buffers_without_losing_bytes() -> Result<()> {
        let raw = RawBody::capture(
            Body::from("streamed content"),
            RequestBodyLimits {
                memory_threshold: 2,
                ..RequestBodyLimits::default()
            },
        )
        .await?;
        let RawBody::Temporary { path, .. } = &raw else {
            return Err(io_error());
        };
        let pathname = path.to_path_buf();
        let mut reader = raw.open().await?;
        let Reader::Temporary(temporary) = &mut reader.inner else {
            return Err(io_error());
        };
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        temporary.before_read = Some(Box::new(move || {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
        }));

        let mut large = [0_u8; 64];
        let mut pending = Box::pin(reader.read(&mut large));
        tokio::select! {
            result = &mut pending => {
                let _ = result;
                return Err(io_error());
            }
            started = started_rx => started.map_err(|_| io_error())?,
        }
        drop(pending);
        release_tx.send(()).map_err(|_| io_error())?;

        let mut empty = [];
        let mut zero = ReadBuf::new(&mut empty);
        std::future::poll_fn(|context| Pin::new(&mut reader).poll_read(context, &mut zero)).await?;
        assert_eq!(zero.filled().len(), 0);

        let mut first = [0_u8; 1];
        reader.read_exact(&mut first).await?;
        assert_eq!(&first, b"s");
        let mut remaining = Vec::new();
        reader.read_to_end(&mut remaining).await?;
        assert_eq!(remaining, b"treamed content");
        let mut eof = [0_u8; 1];
        assert_eq!(reader.read(&mut eof).await?, 0);
        drop(reader);
        drop(raw);
        assert!(!pathname.exists());
        Ok(())
    }

    #[test]
    fn queued_open_keeps_path_until_the_single_blocking_worker_releases_it() -> Result<()> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .map_err(|_| io_error())?;
        runtime.block_on(async {
            let raw = RawBody::capture(
                Body::from("queued content"),
                RequestBodyLimits {
                    memory_threshold: 2,
                    ..RequestBodyLimits::default()
                },
            )
            .await?;
            let RawBody::Temporary { path, .. } = &raw else {
                return Err(io_error());
            };
            let pathname = path.to_path_buf();
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let holder = tokio::task::spawn_blocking(move || {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
            });
            started_rx.await.map_err(|_| io_error())?;
            let queued = TempFileReader::open_task(Arc::clone(path), || {});
            drop(queued);
            drop(raw);
            assert!(pathname.exists());
            release_tx.send(()).map_err(|_| io_error())?;
            holder.await.map_err(|_| io_error())?;
            removed(&pathname).await
        })
    }
}
