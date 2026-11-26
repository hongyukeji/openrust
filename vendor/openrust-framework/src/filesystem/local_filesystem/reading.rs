use super::{FileError, FileResult, Filesystem, io_error};
use crate::support::{CollectionError, LazyCollection};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

impl Filesystem {
    /// 默认按 JSON 解码规则返回 null；严格入口单独传播格式错误。
    pub fn json(&self, path: impl AsRef<Path>) -> FileResult<serde_json::Value> {
        self.json_with_options(path, false, false)
    }
    pub fn json_strict(&self, path: impl AsRef<Path>) -> FileResult<serde_json::Value> {
        self.json_with_options(path, false, true)
    }
    pub fn json_with_options(
        &self,
        path: impl AsRef<Path>,
        lock: bool,
        strict: bool,
    ) -> FileResult<serde_json::Value> {
        let path = path.as_ref();
        let content = self.get_with_lock(path, lock)?;
        match serde_json::from_slice(&content) {
            Ok(value) => Ok(value),
            Err(source) if strict => Err(FileError::Json {
                path: path.to_owned(),
                source,
            }),
            Err(_) => Ok(serde_json::Value::Null),
        }
    }
    /// 每次遍历重新打开文件，逐行读取；末尾换行产生最后一个空项。
    pub fn lines(&self, path: impl AsRef<Path>) -> FileResult<LazyCollection<'static, Vec<u8>>> {
        let path = path.as_ref().to_owned();
        if !path.is_file() {
            return Err(io_error(
                &path,
                std::io::Error::new(std::io::ErrorKind::NotFound, "路径不是普通文件"),
            ));
        }
        Ok(LazyCollection::try_make(move || {
            LineIterator::open(path.clone())
        }))
    }
    pub fn mime_type(&self, path: impl AsRef<Path>) -> FileResult<String> {
        let path = path.as_ref();
        let mut bytes = Vec::new();
        File::open(path)
            .and_then(|file| file.take(8192).read_to_end(&mut bytes))
            .map_err(|error| io_error(path, error))?;
        Ok(crate::http::UploadedFile::detect_type(&bytes)
            .map(|(_, mime)| mime)
            .unwrap_or("application/octet-stream")
            .into())
    }
    pub fn guess_extension(&self, path: impl AsRef<Path>) -> FileResult<Option<String>> {
        let mime = self.mime_type(path)?;
        Ok(mime_guess::get_mime_extensions_str(&mime)
            .and_then(|values| values.first())
            .map(|value| (*value).to_owned()))
    }
}
struct LineIterator {
    path: PathBuf,
    reader: Result<BufReader<File>, Option<std::io::Error>>,
    finished: bool,
}
impl LineIterator {
    fn open(path: PathBuf) -> Self {
        let reader = File::open(&path).map(BufReader::new).map_err(Some);
        Self {
            path,
            reader,
            finished: false,
        }
    }
}
impl Iterator for LineIterator {
    type Item = Result<Vec<u8>, CollectionError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        let reader = match &mut self.reader {
            Ok(reader) => reader,
            Err(error) => {
                self.finished = true;
                return error.take().map(|error| {
                    Err(CollectionError::Source(Box::new(io_error(
                        &self.path, error,
                    ))))
                });
            }
        };
        let mut bytes = Vec::new();
        match reader.read_until(b'\n', &mut bytes) {
            Ok(size) => {
                if size == 0 {
                    self.finished = true;
                } else if bytes.last() == Some(&b'\n') {
                    bytes.pop();
                    if bytes.last() == Some(&b'\r') {
                        bytes.pop();
                    }
                } else {
                    self.finished = true;
                }
                Some(Ok(bytes))
            }
            Err(error) => {
                self.finished = true;
                Some(Err(CollectionError::Source(Box::new(io_error(
                    &self.path, error,
                )))))
            }
        }
    }
}
