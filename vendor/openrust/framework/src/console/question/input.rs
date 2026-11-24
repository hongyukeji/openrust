use super::super::{ConsoleError, Input};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

pub(crate) struct Interaction(Arc<AtomicBool>);
impl Default for Interaction {
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
}
impl Clone for Interaction {
    fn clone(&self) -> Self {
        Self(Arc::new(AtomicBool::new(self.0.load(Ordering::Relaxed))))
    }
}
impl Interaction {
    pub fn shared(&self) -> Self {
        Self(self.0.clone())
    }
    pub fn set(&self, value: bool) {
        self.0.store(value, Ordering::Relaxed);
    }
    pub fn get(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// 问答使用的异步缓冲读取器。
pub type ConsoleReader = Box<dyn AsyncBufRead + Unpin + Send>;

/// 可注入的异步回答流；克隆保留同一流位置，每次读取完整的一条回答。
#[derive(Clone)]
pub struct ConsoleInputStream(Arc<tokio::sync::Mutex<ConsoleReader>>);
impl ConsoleInputStream {
    pub fn new(reader: impl AsyncBufRead + Unpin + Send + 'static) -> Self {
        Self(Arc::new(tokio::sync::Mutex::new(Box::new(reader))))
    }
    /// 借用原读取器；读操作结束后释放锁，以便后续问答继续同一流。
    pub async fn lock(&self) -> tokio::sync::MutexGuard<'_, ConsoleReader> {
        self.0.lock().await
    }
    pub(crate) async fn read_answer(
        &self,
        multiline: bool,
    ) -> Result<Option<String>, ConsoleError> {
        let mut reader = self.lock().await;
        let mut bytes = Vec::new();
        loop {
            let buffer = reader.fill_buf().await.map_err(ConsoleError::from_source)?;
            if buffer.is_empty() {
                break;
            }
            let (consumed, complete) = append_answer_chunk(&mut bytes, buffer, multiline);
            reader.consume(consumed);
            if complete {
                break;
            }
            tokio::task::yield_now().await;
        }
        if bytes.is_empty() && !multiline {
            Ok(None)
        } else {
            String::from_utf8(bytes)
                .map(Some)
                .map_err(ConsoleError::from_source)
        }
    }
}
impl Input {
    pub fn is_interactive(&self) -> bool {
        self.interaction.get()
    }
    pub fn set_interactive(&mut self, value: bool) -> &mut Self {
        self.interaction.set(value);
        self
    }
    pub fn get_stream(&self) -> Option<&ConsoleInputStream> {
        self.stream.as_ref()
    }
    pub fn set_stream(&mut self, stream: Option<ConsoleInputStream>) -> &mut Self {
        self.stream = stream;
        self
    }
    pub(super) fn mark_input_exhausted(&self) {
        self.interaction.set(false);
    }
}

pub(super) fn append_answer_chunk(
    target: &mut Vec<u8>,
    buffer: &[u8],
    multiline: bool,
) -> (usize, bool) {
    let delimiter = buffer.iter().position(|byte| {
        if multiline {
            *byte == 4 || (target.is_empty() && buffer.first() == Some(&b'\n'))
        } else {
            *byte == b'\n'
        }
    });
    if let Some(index) = delimiter {
        let end = if multiline { index } else { index + 1 };
        target.extend_from_slice(&buffer[..end]);
        (index + 1, true)
    } else {
        target.extend_from_slice(buffer);
        (buffer.len(), false)
    }
}
