use axum::body::Bytes;
use std::{
    io::{self, Read, Seek, SeekFrom, Write},
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub(super) struct Body(Arc<Mutex<State>>);
enum State {
    Memory { bytes: Bytes, position: u64 },
    File(std::fs::File),
    Stream(super::ResponseStream),
    Closed,
}

impl Body {
    pub fn new(bytes: Bytes) -> Self {
        Self(Arc::new(Mutex::new(State::Memory { bytes, position: 0 })))
    }
    pub fn file(file: std::fs::File) -> Self {
        Self(Arc::new(Mutex::new(State::File(file))))
    }
    pub fn stream(stream: super::ResponseStream) -> Self {
        Self(Arc::new(Mutex::new(State::Stream(stream))))
    }

    pub fn take_stream(&self) -> super::Result<super::ResponseStream> {
        let mut state = self.lock().map_err(super::Error::ResponseBody)?;
        if matches!(*state, State::Stream(_))
            && let State::Stream(stream) = std::mem::replace(&mut *state, State::Closed)
        {
            return Ok(stream);
        }
        drop(state);
        // Fake 和中间件可以直接返回已有缓冲的响应。
        let bytes = self.read().map_err(super::Error::ResponseBody)?;
        Ok(Box::pin(futures_util::stream::once(async { Ok(bytes) })))
    }
    fn lock(&self) -> io::Result<std::sync::MutexGuard<'_, State>> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("HTTP 响应正文锁中毒"))
    }
    pub fn read(&self) -> io::Result<Bytes> {
        let mut state = self.lock()?;
        match &mut *state {
            State::Memory { bytes, position } => {
                *position = bytes.len() as u64;
                Ok(bytes.clone())
            }
            State::File(file) => {
                file.rewind()?;
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)?;
                Ok(bytes.into())
            }
            State::Stream(_) => Err(stream_error()),
            State::Closed => Err(io::Error::other("HTTP 响应正文已关闭")),
        }
    }
    pub fn len(&self) -> io::Result<u64> {
        match &*self.lock()? {
            State::Memory { bytes, .. } => Ok(bytes.len() as u64),
            State::File(file) => Ok(file.metadata()?.len()),
            State::Stream(_) => Err(stream_error()),
            State::Closed => Ok(0),
        }
    }
    pub fn resource(&self) -> io::Result<ResponseResource> {
        match &*self.lock()? {
            State::Stream(_) => return Err(stream_error()),
            State::Closed => return Err(io::Error::other("HTTP 响应正文已关闭")),
            _ => {}
        }
        Ok(ResponseResource(self.clone()))
    }
    pub fn close(&self) -> io::Result<()> {
        *self.lock()? = State::Closed;
        Ok(())
    }
}

fn stream_error() -> io::Error {
    io::Error::other("HTTP 流式响应正文须通过异步流读取")
}

/// 共享正文游标的可读、可定位资源；文件正文不会为此转换为内存缓冲。
pub struct ResponseResource(Body);
impl Read for ResponseResource {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match &mut *self.0.lock()? {
            State::Memory { bytes, position } => {
                let offset = usize::try_from(*position)
                    .unwrap_or(usize::MAX)
                    .min(bytes.len());
                let length = buffer.len().min(bytes.len() - offset);
                buffer[..length].copy_from_slice(&bytes[offset..offset + length]);
                *position = position.saturating_add(length as u64);
                Ok(length)
            }
            State::File(file) => file.read(buffer),
            State::Stream(_) => Err(stream_error()),
            State::Closed => Err(io::Error::other("HTTP 响应正文已关闭")),
        }
    }
}
impl Seek for ResponseResource {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        match &mut *self.0.lock()? {
            State::Memory { bytes, position } => {
                let value = match from {
                    SeekFrom::Start(value) => i128::from(value),
                    SeekFrom::Current(offset) => i128::from(*position) + i128::from(offset),
                    SeekFrom::End(offset) => bytes.len() as i128 + i128::from(offset),
                };
                *position = u64::try_from(value).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "HTTP 响应正文定位超出范围")
                })?;
                Ok(*position)
            }
            State::File(file) => file.seek(from),
            State::Stream(_) => Err(stream_error()),
            State::Closed => Err(io::Error::other("HTTP 响应正文已关闭")),
        }
    }
}

impl Write for ResponseResource {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        match &mut *self.0.lock()? {
            State::Memory { bytes, position } => {
                if input.is_empty() {
                    return Ok(0);
                }
                let offset = usize::try_from(*position)
                    .map_err(|_| io::Error::other("HTTP 响应正文写入位置超出范围"))?;
                let end = offset
                    .checked_add(input.len())
                    .ok_or_else(|| io::Error::other("HTTP 响应正文长度超出范围"))?;
                let mut buffer = bytes.to_vec();
                buffer
                    .try_reserve(end.saturating_sub(buffer.len()))
                    .map_err(io::Error::other)?;
                if end > buffer.len() {
                    buffer.resize(end, 0);
                }
                buffer[offset..end].copy_from_slice(input);
                *bytes = buffer.into();
                *position = end as u64;
                Ok(input.len())
            }
            State::File(file) => file.write(input),
            State::Stream(_) => Err(stream_error()),
            State::Closed => Err(io::Error::other("HTTP 响应正文已关闭")),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match &mut *self.0.lock()? {
            State::Memory { .. } => Ok(()),
            State::File(file) => file.flush(),
            State::Stream(_) => Err(stream_error()),
            State::Closed => Err(io::Error::other("HTTP 响应正文已关闭")),
        }
    }
}
