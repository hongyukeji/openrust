use super::{Error, Result};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncSeekExt as _, AsyncWriteExt as _};

/// 响应写入目标：路径或由调用方移交的文件资源。
#[derive(Clone)]
pub struct Sink(Target);
#[derive(Clone)]
enum Target {
    Path(PathBuf),
    File(Arc<Mutex<std::fs::File>>),
}
impl From<PathBuf> for Sink {
    fn from(path: PathBuf) -> Self {
        Self(Target::Path(path))
    }
}
impl From<&Path> for Sink {
    fn from(path: &Path) -> Self {
        path.to_path_buf().into()
    }
}
impl From<&PathBuf> for Sink {
    fn from(path: &PathBuf) -> Self {
        path.as_path().into()
    }
}
impl From<String> for Sink {
    fn from(path: String) -> Self {
        PathBuf::from(path).into()
    }
}
impl From<&str> for Sink {
    fn from(path: &str) -> Self {
        Path::new(path).into()
    }
}
impl From<std::fs::File> for Sink {
    fn from(file: std::fs::File) -> Self {
        Self(Target::File(Arc::new(Mutex::new(file))))
    }
}

impl Sink {
    pub(super) fn writer(&self) -> Writer {
        Writer {
            target: self.clone(),
            file: None,
        }
    }
}
pub(super) struct Writer {
    target: Sink,
    file: Option<tokio::fs::File>,
}
impl Writer {
    pub async fn validate(&self) -> Result<()> {
        if let Target::Path(path) = &self.target.0 {
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let metadata = tokio::fs::metadata(parent).await.map_err(Error::Sink)?;
            if !metadata.is_dir() {
                return Err(Error::Sink(std::io::Error::new(
                    std::io::ErrorKind::NotADirectory,
                    "响应写入目标的父路径不是目录",
                )));
            }
        }
        Ok(())
    }
    async fn open(&mut self) -> Result<&mut tokio::fs::File> {
        if self.file.is_none() {
            let file = match &self.target.0 {
                Target::Path(path) => tokio::fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .read(true)
                    .write(true)
                    .open(path)
                    .await
                    .map_err(Error::Sink)?,
                Target::File(file) => {
                    let file = file
                        .lock()
                        .map_err(|_| Error::Sink(std::io::Error::other("响应写入文件锁中毒")))?
                        .try_clone()
                        .map_err(Error::Sink)?;
                    tokio::fs::File::from_std(file)
                }
            };
            self.file = Some(file);
        }
        Ok(self.file.as_mut().expect("响应写入文件已打开"))
    }
    pub async fn write(&mut self, chunk: &[u8]) -> Result<()> {
        let file = self.open().await?;
        file.write_all(chunk).await.map_err(Error::Sink)?;
        // 完成当前块的文件写入后再等下一块，避免等待网络时仍只停留在线程缓冲中。
        file.flush().await.map_err(Error::Sink)
    }
    pub async fn finish(mut self) -> Result<std::fs::File> {
        let file = self.open().await?;
        file.flush().await.map_err(Error::Sink)?;
        file.rewind().await.map_err(Error::Sink)?;
        Ok(self
            .file
            .take()
            .expect("响应写入文件已打开")
            .into_std()
            .await)
    }
}
