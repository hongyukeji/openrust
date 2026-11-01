//! 应用作用域的内容 MIME 检测与固定 Symfony 有序扩展映射。

use std::{
    fmt,
    future::Future,
    path::Path,
    pin::Pin,
    process::Stdio,
    sync::{Arc, RwLock},
    time::Duration,
};

use tokio::{
    io::AsyncReadExt,
    process::{Child, Command},
    sync::oneshot,
    time::Instant,
};

use crate::{Error, Result};

use super::{mime_map, raw_body::TempFileReader};

/// 受管 MIME 检测输入；克隆会保留暂存所有权，供外部检测器安全打开。
#[derive(Clone)]
pub struct MimeFile(pub(crate) Arc<tempfile::TempPath>);

impl fmt::Debug for MimeFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("MimeFile").finish_non_exhaustive()
    }
}

impl MimeFile {
    /// 打开独立游标；取消时在途读取任务仍持有暂存文件所有权。
    pub async fn open(&self) -> Result<MimeFileReader> {
        let reader = TempFileReader::open(Arc::clone(&self.0))
            .await
            .map_err(|_| detection_error())?;
        Ok(MimeFileReader(reader))
    }
}

/// MIME 检测器专用独立异步游标。
pub struct MimeFileReader(TempFileReader);

impl tokio::io::AsyncRead for MimeFileReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(context, buffer)
    }
}

/// 一次检测器尝试的结果；不支持当前平台与支持但未知明确区分。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MimeGuess {
    /// 当前平台缺少此检测器。
    Unsupported,
    /// 检测器可用，但没有识别出内容类型。
    Unknown,
    /// 从文件内容得出的 MIME 类型。
    Detected(String),
}

/// 宿主可登记的文件内容检测器；回调持有暂存文件直到检测完成。
pub trait MimeTypeGuesser: Send + Sync + 'static {
    /// 检测受管暂存文件，不得依据客户端声明的文件名或 Content-Type 决定可信类型。
    fn guess(&self, file: MimeFile)
    -> Pin<Box<dyn Future<Output = Result<MimeGuess>> + Send + '_>>;
}

/// Symfony 默认的 `file -b --mime --` 检测器；Windows 无此默认后端。
#[derive(Default, Debug)]
pub struct FileBinaryMimeTypeGuesser;

impl MimeTypeGuesser for FileBinaryMimeTypeGuesser {
    fn guess(
        &self,
        file: MimeFile,
    ) -> Pin<Box<dyn Future<Output = Result<MimeGuess>> + Send + '_>> {
        Box::pin(async move {
            if cfg!(windows) {
                return Ok(MimeGuess::Unsupported);
            }
            let (sender, cancelled) = oneshot::channel();
            let _cancel_on_drop = CancelOnDrop(Some(sender));
            // 请求取消只通知 supervisor；它持有文件直到子进程结束并被回收。
            tokio::spawn(async move { file_binary(file, cancelled).await })
                .await
                .map_err(|_| detection_error())?
        })
    }
}

struct CancelOnDrop(Option<oneshot::Sender<()>>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

async fn file_binary(file: MimeFile, mut cancelled: oneshot::Receiver<()>) -> Result<MimeGuess> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let checked = tokio::select! {
        biased;
        _ = &mut cancelled => return Err(detection_cancelled()),
        result = tokio::time::timeout_at(deadline, check_readable(file.clone())) => {
            result.map_err(|_| detection_error())?
        }
    };
    checked?;
    let path: &Path = file.0.as_ref().as_ref();
    let child = match Command::new("file")
        .args(["-b", "--mime", "--"])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MimeGuess::Unsupported);
        }
        Err(_) => return Err(detection_error()),
    };
    run_child(child, file, deadline, cancelled).await
}

async fn run_child(
    mut child: Child,
    file: MimeFile,
    deadline: Instant,
    mut cancelled: oneshot::Receiver<()>,
) -> Result<MimeGuess> {
    let result = tokio::select! {
        biased;
        _ = &mut cancelled => None,
        result = tokio::time::timeout_at(deadline, async {
        let mut output = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            stdout
                .take(513)
                .read_to_end(&mut output)
                .await
                .map_err(|_| detection_error())?;
        }
        if output.len() > 512 {
            return Ok(None);
        }
        let status = child.wait().await.map_err(|_| detection_error())?;
        Ok::<_, Error>(status.success().then_some(output))
        }) => Some(result.map_err(|_| detection_error()).and_then(|value| value)),
    };
    let bytes = match result {
        Some(Ok(Some(bytes))) => bytes,
        Some(Ok(None)) => {
            stop_child(child, file).await?;
            return Ok(MimeGuess::Unknown);
        }
        Some(Err(error)) => {
            stop_child(child, file).await?;
            return Err(error);
        }
        None => {
            stop_child(child, file).await?;
            return Err(detection_cancelled());
        }
    };
    let value = String::from_utf8_lossy(&bytes);
    Ok(parse_file_mime(&value).map_or(MimeGuess::Unknown, MimeGuess::Detected))
}

async fn check_readable(file: MimeFile) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let opened = std::fs::File::open(file.0.as_ref()).map_err(|_| detection_error())?;
        if !opened.metadata().map_err(|_| detection_error())?.is_file() {
            return Err(detection_error());
        }
        Ok(())
    })
    .await
    .map_err(|_| detection_error())?
}

async fn stop_child(child: Child, file: MimeFile) -> Result<()> {
    stop_child_with_limit(
        child,
        file,
        Duration::from_secs(2),
        std::future::ready(()),
        std::future::ready(()),
    )
    .await
}

async fn stop_child_with_limit<F, G>(
    mut child: Child,
    file: MimeFile,
    limit: Duration,
    before_wait: F,
    before_reap: G,
) -> Result<()>
where
    F: Future<Output = ()>,
    G: Future<Output = ()> + Send + 'static,
{
    let _ = child.start_kill();
    if let Ok(Ok(_)) = tokio::time::timeout(limit, async {
        before_wait.await;
        child.wait().await
    })
    .await
    {
        return Ok(());
    }
    // 前台有界返回；回收任务共同持有子进程和暂存文件直到 wait 结束。
    tokio::spawn(reap_child_after(child, file, before_reap));
    Err(Error::custom(
        "uploaded_file_mime_cleanup_pending",
        "上传文件内容检测进程的回收尚未确认",
    ))
}

async fn reap_child_after<F>(mut child: Child, file: MimeFile, before_reap: F)
where
    F: Future<Output = ()>,
{
    before_reap.await;
    let _ = child.start_kill();
    if child.wait().await.is_err() {
        // 无法确认回收时保留暂存文件，避免在子进程可能仍读取时提前删除。
        std::mem::forget(file);
        tracing::error!("上传文件 MIME 检测进程回收失败，暂存文件已保留");
    }
}

fn detection_cancelled() -> Error {
    Error::custom("uploaded_file_mime_cancelled", "上传文件内容类型检测已取消")
}

fn parse_file_mime(output: &str) -> Option<String> {
    let token = output.split_once('/')?;
    let major = token.0;
    if major.is_empty()
        || !major
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return None;
    }
    let minor: String = token
        .1
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '.'))
        .collect();
    (!minor.is_empty()).then(|| format!("{major}/{minor}"))
}

fn detection_error() -> Error {
    Error::custom("uploaded_file_mime_unavailable", "上传文件内容类型检测失败")
}

/// 同一应用共享的MIME检测与扩展表；克隆共享注册状态，不跨应用共享。
#[derive(Clone)]
pub struct MimeTypes {
    inner: Arc<RwLock<MimeTypesInner>>,
}

struct MimeTypesInner {
    guessers: Vec<Arc<dyn MimeTypeGuesser>>,
    custom: Vec<(String, Vec<String>)>,
}

impl fmt::Debug for MimeTypes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("MimeTypes").finish_non_exhaustive()
    }
}

impl Default for MimeTypes {
    fn default() -> Self {
        Self::new()
    }
}

impl MimeTypes {
    /// 创建独立注册表，默认仅登记当前可用的 Symfony `FileBinary` 对应后端。
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(MimeTypesInner {
                guessers: vec![Arc::new(FileBinaryMimeTypeGuesser)],
                custom: Vec::new(),
            })),
        }
    }

    /// 后注册检测器优先于前注册检测器和默认后端。
    pub fn register_guesser(&self, guesser: Arc<dyn MimeTypeGuesser>) {
        self.inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .guessers
            .insert(0, guesser);
    }

    /// 覆盖指定MIME的有序扩展列表；空列表明确表示无扩展。
    pub fn register_extensions(&self, mime: impl Into<String>, extensions: Vec<String>) {
        let mime = mime.into();
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(item) = inner.custom.iter_mut().find(|(name, _)| name == &mime) {
            *item = (mime, extensions);
        } else {
            inner.custom.push((mime, extensions));
        }
    }

    /// 按当前注册顺序从受管暂存文件检测MIME。
    pub async fn guess_mime(&self, file: MimeFile) -> Result<Option<String>> {
        let guessers = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .guessers
            .clone();
        let mut supported = false;
        for guesser in guessers {
            match guesser.guess(file.clone()).await? {
                MimeGuess::Unsupported => {}
                MimeGuess::Unknown => supported = true,
                MimeGuess::Detected(mime) => return Ok(Some(mime)),
            }
        }
        if supported {
            Ok(None)
        } else {
            Err(Error::custom(
                "uploaded_file_mime_no_guesser",
                "没有可用的内容类型检测器",
            ))
        }
    }

    /// 读取固定表或应用覆盖的有序扩展名。
    pub fn extensions(&self, mime: &str) -> Vec<String> {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, values)) = inner.custom.iter().find(|(name, _)| name == mime) {
            return values.clone();
        }
        let lowered = mime.to_ascii_lowercase();
        if let Some((_, values)) = inner.custom.iter().find(|(name, _)| name == &lowered) {
            return values.clone();
        }
        table(mime_map::MAP, mime)
            .iter()
            .map(|value| (*value).to_owned())
            .collect()
    }

    /// 按固定反向表及应用覆盖读取客户端后缀可能对应的MIME列表。
    pub fn mime_types(&self, extension: &str) -> Vec<String> {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut custom = custom_reverse(&inner.custom, extension);
        if !custom.is_empty() {
            return custom;
        }
        let lowered = extension.to_ascii_lowercase();
        if lowered != extension {
            custom = custom_reverse(&inner.custom, &lowered);
            if !custom.is_empty() {
                return custom;
            }
        }
        table(mime_map::REVERSE_MAP, extension)
            .iter()
            .map(|value| (*value).to_owned())
            .collect()
    }
}

fn custom_reverse(custom: &[(String, Vec<String>)], extension: &str) -> Vec<String> {
    let mut result = Vec::new();
    for (mime, extensions) in custom {
        for value in extensions {
            if value == extension {
                result.push(mime.clone());
            }
        }
    }
    result
}

fn table<'a>(table: &'a [(&str, &[&str])], key: &str) -> &'a [&'a str] {
    let index = table
        .binary_search_by(|(name, _)| name.cmp(&key))
        .or_else(|_| {
            let lowered = key.to_ascii_lowercase();
            table.binary_search_by(|(name, _)| name.cmp(&lowered.as_str()))
        });
    index.map_or(&[], |index| table[index].1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    struct FixedGuesser(MimeGuess);

    impl MimeTypeGuesser for FixedGuesser {
        fn guess(
            &self,
            _file: MimeFile,
        ) -> Pin<Box<dyn Future<Output = Result<MimeGuess>> + Send + '_>> {
            Box::pin(async { Ok(self.0.clone()) })
        }
    }

    fn sample() -> Result<MimeFile> {
        let mut file = tempfile::NamedTempFile::new().map_err(|_| detection_error())?;
        file.write_all(b"hello").map_err(|_| detection_error())?;
        Ok(MimeFile(Arc::new(file.into_temp_path())))
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn missing_and_non_file_inputs_fail_before_default_detection() -> Result<()> {
        let missing = sample()?;
        let path = missing.0.to_path_buf();
        std::fs::remove_file(&path).map_err(|_| detection_error())?;
        let error = FileBinaryMimeTypeGuesser
            .guess(missing)
            .await
            .err()
            .ok_or_else(detection_error)?;
        assert_eq!(error.code(), "uploaded_file_mime_unavailable");
        let directory = sample()?;
        let path = directory.0.to_path_buf();
        std::fs::remove_file(&path).map_err(|_| detection_error())?;
        std::fs::create_dir(&path).map_err(|_| detection_error())?;
        let error = FileBinaryMimeTypeGuesser
            .guess(directory.clone())
            .await
            .err()
            .ok_or_else(detection_error)?;
        assert_eq!(error.code(), "uploaded_file_mime_unavailable");
        std::fs::remove_dir(&path).map_err(|_| detection_error())?;
        drop(directory);
        let unreadable = sample()?;
        let path = unreadable.0.to_path_buf();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
            .map_err(|_| detection_error())?;
        if std::fs::File::open(&path).is_err() {
            let error = FileBinaryMimeTypeGuesser
                .guess(unreadable.clone())
                .await
                .err()
                .ok_or_else(detection_error)?;
            assert_eq!(error.code(), "uploaded_file_mime_unavailable");
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|_| detection_error())?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancelled_detector_kills_and_reaps_before_tempfile_release() -> Result<()> {
        let file = sample()?;
        let path = file.0.to_path_buf();
        let child = Command::new("sleep")
            .arg("30")
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| detection_error())?;
        let pid = child.id().ok_or_else(detection_error)?;
        let (sender, cancelled) = oneshot::channel();
        let guard = CancelOnDrop(Some(sender));
        let task = tokio::spawn(run_child(
            child,
            file,
            Instant::now() + Duration::from_secs(5),
            cancelled,
        ));
        drop(guard);
        let outcome = tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .map_err(|_| detection_error())?
            .map_err(|_| detection_error())?;
        let error = outcome.err().ok_or_else(detection_error)?;
        assert_eq!(error.code(), "uploaded_file_mime_cancelled");
        assert!(!path.exists());
        let still_running = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map_err(|_| detection_error())?;
        assert!(!still_running.success());
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn delayed_reap_keeps_tempfile_owner_after_bounded_cleanup_error() -> Result<()> {
        let file = sample()?;
        let path = file.0.to_path_buf();
        let child = Command::new("sleep")
            .arg("30")
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| detection_error())?;
        let pid = child.id().ok_or_else(detection_error)?;
        let (resume, gate) = oneshot::channel();
        let error = stop_child_with_limit(
            child,
            file,
            Duration::ZERO,
            async move {
                std::future::pending::<()>().await;
            },
            async move {
                let _ = gate.await;
            },
        )
        .await
        .err()
        .ok_or_else(detection_error)?;
        assert_eq!(error.code(), "uploaded_file_mime_cleanup_pending");
        assert!(path.exists());
        resume.send(()).map_err(|()| detection_error())?;
        tokio::time::timeout(Duration::from_secs(3), async {
            while path.exists() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| detection_error())?;
        let still_running = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map_err(|_| detection_error())?;
        assert!(!still_running.success());
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn detector_output_overflow_and_deadline_are_bounded() -> Result<()> {
        let file = sample()?;
        let overflow = Command::new("yes")
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| detection_error())?;
        let (keep_sender, cancel) = oneshot::channel();
        assert_eq!(
            run_child(
                overflow,
                file,
                Instant::now() + Duration::from_secs(5),
                cancel
            )
            .await?,
            MimeGuess::Unknown
        );
        drop(keep_sender);
        let file = sample()?;
        let sleeping = Command::new("sleep")
            .arg("30")
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| detection_error())?;
        let (keep_sender, cancel) = oneshot::channel();
        let result = run_child(
            sleeping,
            file,
            Instant::now() + Duration::from_millis(1),
            cancel,
        )
        .await;
        drop(keep_sender);
        let error = result.err().ok_or_else(detection_error)?;
        assert_eq!(error.code(), "uploaded_file_mime_unavailable");
        Ok(())
    }

    #[tokio::test]
    async fn late_guesser_priority_unknown_fallback_and_app_isolation() -> Result<()> {
        let first = MimeTypes::new();
        let second = MimeTypes::new();
        first.register_guesser(Arc::new(FixedGuesser(MimeGuess::Detected(
            "application/x-late".to_owned(),
        ))));
        first.register_guesser(Arc::new(FixedGuesser(MimeGuess::Unknown)));
        assert_eq!(
            first.guess_mime(sample()?).await?,
            Some("application/x-late".to_owned())
        );
        assert_ne!(
            second.guess_mime(sample()?).await?,
            Some("application/x-late".to_owned())
        );
        first.register_extensions("application/x-late", vec!["later".to_owned()]);
        assert_eq!(first.extensions("application/x-late"), ["later"]);
        assert_eq!(
            second.extensions("application/x-late"),
            Vec::<String>::new()
        );
        let unsupported = MimeTypes::new();
        unsupported
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .guessers = vec![Arc::new(FixedGuesser(MimeGuess::Unsupported))];
        let error = unsupported
            .guess_mime(sample()?)
            .await
            .err()
            .ok_or_else(detection_error)?;
        assert_eq!(error.code(), "uploaded_file_mime_no_guesser");
        unsupported
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .guessers = vec![Arc::new(FixedGuesser(MimeGuess::Unknown))];
        assert_eq!(unsupported.guess_mime(sample()?).await?, None);
        Ok(())
    }

    fn ordered_digest(table: &[(&str, &[&str])]) -> (usize, u64) {
        let mut count = 0;
        let mut digest = 14_695_981_039_346_656_037_u64;
        let mut push = |byte: u8| {
            digest = (digest ^ u64::from(byte)).wrapping_mul(1_099_511_628_211);
        };
        for (name, values) in table {
            for byte in name.bytes().chain(std::iter::once(0)) {
                push(byte);
            }
            for value in *values {
                count += 1;
                for byte in value.bytes().chain(std::iter::once(0)) {
                    push(byte);
                }
            }
            push(255);
        }
        (count, digest)
    }

    #[test]
    fn public_tables_match_full_php_oracle_including_order() {
        // 真实 PHP8.5 公开 getExtensions/getMimeTypes 全表导出见
        // /tmp/openrust-request-mime-oracle/full-map.json；固定 Symfony v8.0.0。
        assert_eq!(mime_map::MAP.len(), 1838);
        assert!(mime_map::MAP.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert_eq!(ordered_digest(mime_map::MAP), (2786, 0x82dc_75fa_2b25_84b6));
        assert_eq!(mime_map::REVERSE_MAP.len(), 1871);
        assert!(
            mime_map::REVERSE_MAP
                .windows(2)
                .all(|pair| pair[0].0 < pair[1].0)
        );
        assert_eq!(
            ordered_digest(mime_map::REVERSE_MAP),
            (2765, 0xfbb3_352e_c5fb_0a5a)
        );
        let mime = MimeTypes::new();
        for (key, values) in mime_map::MAP {
            let actual = mime.extensions(key);
            assert_eq!(
                actual.iter().map(String::as_str).collect::<Vec<_>>(),
                *values
            );
        }
        for (key, values) in mime_map::REVERSE_MAP {
            let actual = mime.mime_types(key);
            assert_eq!(
                actual.iter().map(String::as_str).collect::<Vec<_>>(),
                *values
            );
        }
    }

    #[test]
    fn fixed_map_preserves_first_extension_and_reverse_order() {
        let mime = MimeTypes::new();
        assert_eq!(
            mime.extensions("image/jpeg").first().map(String::as_str),
            Some("jpg")
        );
        assert_eq!(
            mime.extensions("application/octet-stream")
                .first()
                .map(String::as_str),
            Some("bin")
        );
        assert!(mime.mime_types("jpg").contains(&"image/jpeg".to_owned()));
        assert_eq!(
            mime.extensions("application/whatever-symfony"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn custom_mapping_uses_exact_then_lowercase_and_keeps_reverse_duplicates() {
        let mime = MimeTypes::new();
        mime.register_extensions("application/x-case", vec!["lower".to_owned()]);
        mime.register_extensions("APPLICATION/X-CASE", vec!["UPPER".to_owned()]);
        mime.register_extensions("application/x-repeat", vec!["x".to_owned(), "x".to_owned()]);
        assert_eq!(mime.extensions("application/x-case"), ["lower"]);
        assert_eq!(mime.extensions("APPLICATION/X-CASE"), ["UPPER"]);
        assert_eq!(mime.extensions("Application/X-Case"), ["lower"]);
        assert_eq!(mime.mime_types("UPPER"), ["APPLICATION/X-CASE"]);
        assert_eq!(mime.mime_types("upper"), Vec::<String>::new());
        assert_eq!(
            mime.mime_types("X"),
            ["application/x-repeat", "application/x-repeat"]
        );
        assert_eq!(
            mime.mime_types("x"),
            ["application/x-repeat", "application/x-repeat"]
        );
    }

    #[test]
    fn file_output_requires_mime_token() {
        assert_eq!(
            parse_file_mime("application/pdf; charset=binary\n"),
            Some("application/pdf".to_owned())
        );
        assert_eq!(parse_file_mime("not a mime"), None);
    }
}
