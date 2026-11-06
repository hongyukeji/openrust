//! 请求生命周期受管的上传文件；客户端元数据不参与存储授权。

use std::{
    any::Any,
    fmt,
    pin::Pin,
    sync::{Arc, OnceLock},
    task::{Context, Poll},
};

use axum::body::Bytes;
#[cfg(test)]
use std::io::Write;
use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};

use crate::Error;
use crate::Result;
use crate::filesystem::FilesystemManager;
use crate::filesystem::StoragePath;
use crate::filesystem::UploadOptions;
use crate::filesystem::Visibility;
use crate::http::request::mime::MimeFile;
use crate::http::request::mime::MimeTypes;
use crate::http::request::raw_body::TempFileReader;
use crate::support::DataObject;

struct Inner {
    temporary: Arc<tempfile::TempPath>,
    client_name: String,
    client_path: String,
    client_mime: String,
    size: u64,
    mime_types: Arc<MimeTypes>,
    hash: OnceLock<String>,
}

/// 已接收的上传文件。克隆共享暂存所有权，读取时各自打开独立游标。
#[derive(Clone)]
pub struct UploadedFile(Arc<Inner>);

/// 保有暂存文件所有权的独立异步读取游标。
pub struct UploadedFileReader {
    reader: TempFileReader,
}

impl AsyncRead for UploadedFileReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.reader).poll_read(context, buffer)
    }
}

impl fmt::Debug for UploadedFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UploadedFile")
            .field("size", &self.0.size)
            .finish_non_exhaustive()
    }
}

impl DataObject for UploadedFile {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl UploadedFile {
    pub(crate) fn from_temp(
        temporary: tempfile::TempPath,
        size: usize,
        client_name: &str,
        client_mime: Option<String>,
        mime_types: Arc<MimeTypes>,
    ) -> Result<Self> {
        let size = u64::try_from(size).map_err(|_| file_limit())?;
        let client_path = client_name.replace('\\', "/");
        let client_name = client_path
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned();
        Ok(Self(Arc::new(Inner {
            temporary: Arc::new(temporary),
            client_name,
            client_path,
            client_mime: client_mime
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "application/octet-stream".to_owned()),
            size,
            mime_types,
            hash: OnceLock::new(),
        })))
    }

    #[cfg(test)]
    async fn from_part(
        bytes: Bytes,
        client_name: &str,
        client_mime: Option<String>,
    ) -> Result<Self> {
        let size = bytes.len();
        let temporary = tokio::task::spawn_blocking(move || -> Result<tempfile::TempPath> {
            let mut file = tempfile::NamedTempFile::new().map_err(|_| file_io())?;
            file.write_all(&bytes).map_err(|_| file_io())?;
            file.flush().map_err(|_| file_io())?;
            Ok(file.into_temp_path())
        })
        .await
        .map_err(|_| file_io())??;
        Self::from_temp(
            temporary,
            size,
            client_name,
            client_mime,
            Arc::new(MimeTypes::new()),
        )
    }

    /// 客户端声明的原始文件名，仅可用于展示，不能用作存储路径。
    pub fn client_original_name(&self) -> &str {
        &self.0.client_name
    }

    /// 客户端声明的相对路径，反斜线规范化为斜线，仍不可信。
    pub fn client_original_path(&self) -> &str {
        &self.0.client_path
    }

    /// 客户端文件名后缀，仅供展示，不参与服务端 MIME 判断。
    pub fn client_original_extension(&self) -> &str {
        self.0
            .client_name
            .rsplit_once('.')
            .map_or("", |(_, extension)| extension)
    }

    /// 客户端声明的媒体类型，不代表服务端内容检测结果。
    pub fn client_mime_type(&self) -> &str {
        &self.0.client_mime
    }

    /// 上传文件的实际接收字节数。
    pub fn size(&self) -> u64 {
        self.0.size
    }

    /// 暂存文件仍存在且由本次请求成功接收。
    pub fn is_valid(&self) -> bool {
        self.0.temporary.exists()
    }

    /// 仅由受管暂存文件内容检测MIME；客户端声明不参与。
    pub async fn mime_type(&self) -> Result<Option<String>> {
        if !self.is_valid() {
            return Err(file_io());
        }
        self.0
            .mime_types
            .guess_mime(MimeFile(Arc::clone(&self.0.temporary)))
            .await
    }

    /// 由真实内容MIME查固定有序表的首扩展；识别类型但无映射返回None。
    pub async fn extension(&self) -> Result<Option<String>> {
        let mime = self
            .mime_type()
            .await?
            .ok_or_else(|| Error::custom("uploaded_file_mime_unknown", "内容类型检测未返回MIME"))?;
        Ok(self.0.mime_types.extensions(&mime).into_iter().next())
    }

    /// 由不可信客户端MIME查首扩展，仅供展示。
    pub fn client_extension(&self) -> Option<String> {
        self.0
            .mime_types
            .extensions(&self.0.client_mime)
            .into_iter()
            .next()
    }

    /// 每次创建独立异步读取游标；最后一个句柄丢弃后暂存自动删除。
    pub async fn open(&self) -> Result<UploadedFileReader> {
        let reader = TempFileReader::open(Arc::clone(&self.0.temporary))
            .await
            .map_err(|_| file_io())?;
        Ok(UploadedFileReader { reader })
    }

    /// 在当前正文限制内取得文件字节；无效暂存明确失败。
    pub async fn get(&self) -> Result<Vec<u8>> {
        let mut reader = self.open().await?;
        let capacity = usize::try_from(self.0.size).map_err(|_| file_limit())?;
        let mut bytes = Vec::with_capacity(capacity);
        reader
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| file_io())?;
        Ok(bytes)
    }

    /// 为同一文件对象缓存40字符随机名，并附服务端已识别扩展名。
    pub async fn hash_name(&self) -> Result<String> {
        let hash = if let Some(hash) = self.0.hash.get() {
            hash.clone()
        } else {
            let generated = random_name()?;
            let _ = self.0.hash.set(generated);
            self.0.hash.get().ok_or_else(file_io)?.clone()
        };
        Ok(self
            .extension()
            .await?
            .map_or_else(|| hash.clone(), |extension| format!("{hash}.{extension}")))
    }

    /// 在默认disk的目录中以服务端随机名存储。
    pub async fn store(&self, storage: &FilesystemManager, directory: &str) -> Result<String> {
        self.store_as(storage, directory, &self.hash_name().await?, None, None)
            .await
    }

    /// 以显式名称和可选具名disk存储；对象路径由StoragePath校验。
    pub async fn store_as(
        &self,
        storage: &FilesystemManager,
        directory: &str,
        name: &str,
        disk: Option<&str>,
        visibility: Option<Visibility>,
    ) -> Result<String> {
        let key = if directory.is_empty() {
            name.to_owned()
        } else {
            format!("{directory}/{name}")
        };
        let path = StoragePath::new(key)?;
        let disk = match disk {
            Some(name) => storage.disk(name).await?,
            None => storage.default_disk().await?,
        };
        let reader = self.open().await?;
        let body = futures_util::stream::unfold(Some(reader), |reader| async move {
            let mut reader = reader?;
            let mut buffer = vec![0; 64 * 1024];
            match reader.read(&mut buffer).await {
                Ok(0) => None,
                Ok(size) => {
                    buffer.truncate(size);
                    Some((Ok(Bytes::from(buffer)), Some(reader)))
                }
                Err(_) => Some((Err(file_io()), None)),
            }
        });
        let options = UploadOptions {
            visibility,
            ..UploadOptions::default()
        };
        let stored = disk
            .put_stream(path, None, Box::pin(body), options, None)
            .await?;
        Ok(stored.path.as_str().to_owned())
    }

    /// 以默认disk的公开可见性存储；public不等于公网可达。
    pub async fn store_publicly(
        &self,
        storage: &FilesystemManager,
        directory: &str,
    ) -> Result<String> {
        self.store_as(
            storage,
            directory,
            &self.hash_name().await?,
            None,
            Some(Visibility::Public),
        )
        .await
    }

    /// 在具名disk以显式名称公开存储。
    pub async fn store_publicly_as(
        &self,
        storage: &FilesystemManager,
        directory: &str,
        name: &str,
        disk: Option<&str>,
    ) -> Result<String> {
        self.store_as(storage, directory, name, disk, Some(Visibility::Public))
            .await
    }
}

fn random_name() -> Result<String> {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut output = String::with_capacity(40);
    while output.len() < 40 {
        let mut bytes = [0u8; 40];
        getrandom::fill(&mut bytes).map_err(|_| file_io())?;
        for byte in bytes {
            if byte < 248 && output.len() < 40 {
                output.push(char::from(ALPHABET[usize::from(byte % 62)]));
            }
        }
    }
    Ok(output)
}

fn file_limit() -> Error {
    Error::custom("uploaded_file_limit", "上传文件超过资源限制")
}
fn file_io() -> Error {
    Error::custom("uploaded_file_unavailable", "上传文件不可读取")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn recognized_mime_without_extension_keeps_unextended_random_name() -> Result<()> {
        let file = UploadedFile::from_part(
            Bytes::new(),
            "claimed.pdf",
            Some("application/pdf".to_owned()),
        )
        .await?;
        assert_eq!(file.mime_type().await?, Some("inode/x-empty".to_owned()));
        assert_eq!(file.extension().await?, None);
        assert_eq!(file.hash_name().await?.len(), 40);
        assert_eq!(file.client_extension(), Some("pdf".to_owned()));
        Ok(())
    }

    #[tokio::test]
    async fn reader_retains_tempfile_and_hash_uses_content_not_client_suffix() -> Result<()> {
        let file = UploadedFile::from_part(
            Bytes::from_static(b"%PDF-demo"),
            "folder\\client.png",
            Some("image/png".to_owned()),
        )
        .await?;
        let path = file.0.temporary.to_path_buf();
        assert_eq!(file.client_original_name(), "client.png");
        assert_eq!(file.client_original_path(), "folder/client.png");
        assert_eq!(file.client_mime_type(), "image/png");
        assert_eq!(file.extension().await?, Some("pdf".to_owned()));
        let name = file.hash_name().await?;
        assert_eq!(name.len(), 44);
        assert_eq!(
            std::path::Path::new(&name).extension(),
            Some(std::ffi::OsStr::new("pdf"))
        );
        assert_eq!(file.hash_name().await?, name);
        file.0
            .mime_types
            .register_extensions("application/pdf", vec!["pdx".to_owned(), "pdf".to_owned()]);
        let changed = file.hash_name().await?;
        assert_eq!(&changed[..40], &name[..40]);
        assert_eq!(&changed[40..], ".pdx");
        assert_eq!(file.extension().await?, Some("pdx".to_owned()));
        let mut reader = file.open().await?;
        drop(file);
        assert!(path.exists());
        let mut content = Vec::new();
        reader
            .read_to_end(&mut content)
            .await
            .map_err(|_| file_io())?;
        assert_eq!(content, b"%PDF-demo");
        drop(reader);
        assert!(!path.exists());
        Ok(())
    }
}
