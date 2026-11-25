//! 将现有分块契约接回异步读写，保持有界背压并避免整文件缓冲。

use super::{FilesystemFuture, FilesystemReader, FilesystemResult, WriteOptions};
use crate::contracts::filesystem::{Cloud, FileStream};
use tokio::io::{AsyncRead, AsyncReadExt as _};

pub(super) fn reader(stream: Box<dyn FileStream>) -> FilesystemReader {
    let chunks = futures_util::stream::try_unfold(stream, |mut stream| async move {
        stream
            .next_chunk()
            .await
            .map_err(std::io::Error::other)
            .map(|chunk| chunk.map(|bytes| (axum::body::Bytes::from(bytes), stream)))
    });
    Box::new(tokio_util::io::StreamReader::new(Box::pin(chunks)))
}

struct Chunks(tokio::sync::mpsc::Receiver<FilesystemResult<Vec<u8>>>);
impl FileStream for Chunks {
    fn next_chunk(&mut self) -> FilesystemFuture<'_, FilesystemResult<Option<Vec<u8>>>> {
        Box::pin(async { self.0.recv().await.transpose() })
    }
}

pub(super) async fn write(
    disk: &dyn Cloud,
    path: &str,
    reader: &mut (impl AsyncRead + Unpin),
    options: WriteOptions,
) -> FilesystemResult<bool> {
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    let mut chunks = Chunks(receiver);
    // 允许原有非 Send reader；仅契约端跨任务安全，不启动后台任务。
    let pump = async move {
        loop {
            let mut bytes = vec![0; 64 * 1024];
            match reader.read(&mut bytes).await {
                Ok(0) => break,
                Ok(count) => {
                    bytes.truncate(count);
                    if sender.send(Ok(bytes)).await.is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(error.into())).await;
                    break;
                }
            }
        }
    };
    let consume = async {
        // 后端提早返回时立即释放接收端，防止写入泵等待无人接收的块。
        let result = disk.write_stream(path, &mut chunks, &options).await;
        drop(chunks);
        result
    };
    tokio::pin!(pump, consume);
    tokio::select! {
        // 后端先结束时取消尚在等待源数据的读取，不留下后台任务。
        result = &mut consume => result,
        () = &mut pump => consume.await,
    }
}
