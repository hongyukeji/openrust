use super::super::{ConsoleError, OutputFormat, OutputStyle, OutputVerbosity};
use super::{ConsoleQuestionError, Question};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use std::{
    io::{self, IsTerminal as _},
    sync::LazyLock,
    time::Duration,
};

static STDIN: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));
pub(super) struct Lease {
    _lock: tokio::sync::MutexGuard<'static, ()>,
    restore_raw: bool,
    #[cfg(unix)]
    stdin_flags: Option<i32>,
}
impl Drop for Lease {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(flags) = self.stdin_flags {
            // SAFETY: 标准输入在租约期间仍由进程持有；只恢复原来的状态标志。
            let _ = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_SETFL, flags) };
        }
        if self.restore_raw {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
}
impl Lease {
    async fn acquire(raw: bool) -> Result<Self, ConsoleError> {
        let mutex: &'static tokio::sync::Mutex<()> = &STDIN;
        let lock = mutex.lock().await;
        let restore_raw = raw
            && !crossterm::terminal::is_raw_mode_enabled().map_err(ConsoleError::from_source)?;
        if restore_raw {
            crossterm::terminal::enable_raw_mode().map_err(ConsoleError::from_source)?;
        }
        #[cfg(unix)]
        let stdin_flags = if raw {
            None
        } else {
            // SAFETY: fcntl 不转移或关闭标准输入，仅读取／设置它的状态标志。
            let flags = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_GETFL) };
            if flags < 0 {
                return Err(ConsoleError::from_source(io::Error::last_os_error()));
            }
            if flags & libc::O_NONBLOCK == 0 {
                if unsafe {
                    libc::fcntl(libc::STDIN_FILENO, libc::F_SETFL, flags | libc::O_NONBLOCK)
                } < 0
                {
                    return Err(ConsoleError::from_source(io::Error::last_os_error()));
                }
                Some(flags)
            } else {
                None
            }
        };
        Ok(Self {
            _lock: lock,
            restore_raw,
            #[cfg(unix)]
            stdin_flags,
        })
    }
}
pub(super) enum Session {
    Injected(super::ConsoleInputStream),
    Captured,
    Terminal(Lease),
    Pipe(Lease),
}
pub(super) async fn prepare(
    style: &mut OutputStyle<'_>,
    question: &Question,
) -> Result<Session, ConsoleError> {
    if let Some(stream) = style.question_input().get_stream().cloned() {
        return Ok(Session::Injected(stream));
    }
    if !style.get_output().streams_to_terminal() {
        return Ok(Session::Captured);
    }
    if io::stdin().is_terminal() {
        match Lease::acquire(true).await {
            Ok(lease) => return Ok(Session::Terminal(lease)),
            Err(_) if question.hidden && !question.hidden_fallback => {
                return Err(ConsoleError::from_source(
                    ConsoleQuestionError::HiddenUnavailable,
                ));
            }
            Err(_) => {}
        }
    }
    Ok(Session::Pipe(Lease::acquire(false).await?))
}
pub(super) async fn read(
    style: &mut OutputStyle<'_>,
    question: &Question,
    session: &mut Session,
    prompt: &str,
) -> Result<Option<String>, ConsoleError> {
    match session {
        Session::Injected(stream) => {
            stream
                .read_answer(question.multiline && !question.hidden)
                .await
        }
        Session::Captured => Ok(None),
        Session::Terminal(_lease) => terminal(style, question, prompt).await,
        Session::Pipe(_lease) => pipe(question.multiline && !question.hidden).await,
    }
}
async fn terminal(
    style: &mut OutputStyle<'_>,
    question: &Question,
    prompt: &str,
) -> Result<Option<String>, ConsoleError> {
    let mut text = String::new();
    let mut suggestions = if let Some(callback) = &question.completion {
        callback.call(String::new()).await?
    } else {
        Vec::new()
    };
    let mut selected = None;
    loop {
        // 租约串行持有终端；poll 成功后 read 不阻塞，不跨问答遗留后台唤醒器。
        if !crossterm::event::poll(Duration::ZERO).map_err(ConsoleError::from_source)? {
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue;
        }
        let Event::Key(mut key) = crossterm::event::read().map_err(ConsoleError::from_source)?
        else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
            && matches!(key.code, KeyCode::Char('j') | KeyCode::Char('m'))
        {
            key.code = KeyCode::Enter;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Err(ConsoleError::from_source(ConsoleQuestionError::Interrupted));
        }
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('d') | KeyCode::Char('z'))
        {
            return Ok(
                (!text.is_empty() || (question.multiline && !question.hidden)).then_some(text),
            );
        }
        let mut changed = false;
        match key.code {
            KeyCode::Enter => {
                if let Some(index) = selected {
                    complete(&mut text, &suggestions, index);
                }
                if question.multiline && !question.hidden {
                    if text.is_empty() {
                        return Ok(Some(text));
                    }
                    text.push('\n');
                    if !question.hidden {
                        style
                            .get_output()
                            .emit_rendered("\r\n", OutputVerbosity::Normal)?;
                    }
                } else {
                    if !question.hidden {
                        if question.completion.is_some() {
                            style.write("\r\n", false, OutputFormat::Raw)?;
                        } else {
                            style.get_output().emit_input_echo("\r\n")?;
                        }
                    }
                    if question.completion.is_none() {
                        text.push('\n');
                    }
                    return Ok(Some(text));
                }
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    || key
                        .modifiers
                        .contains(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                text.push(character);
                changed = true;
            }
            KeyCode::Backspace => {
                text.pop();
                changed = true;
            }
            KeyCode::Tab if question.completion.is_none() => {
                text.push('\t');
                changed = true;
            }
            KeyCode::Tab if !suggestions.is_empty() => {
                if let Some(index) = selected {
                    complete(&mut text, &suggestions, index);
                }
                selected = None;
                suggestions.clear();
            }
            KeyCode::Up | KeyCode::Down if !suggestions.is_empty() => {
                selected = Some(match selected {
                    None if key.code == KeyCode::Down => 0,
                    None => suggestions.len() - 1,
                    Some(current) if key.code == KeyCode::Down => (current + 1) % suggestions.len(),
                    Some(current) => (current + suggestions.len() - 1) % suggestions.len(),
                });
            }
            _ => continue,
        }
        if changed && let Some(callback) = &question.completion {
            let prefix = text.rsplit(',').next().unwrap_or(&text).trim();
            suggestions = callback
                .call(text.clone())
                .await?
                .into_iter()
                .filter(|value| value.starts_with(prefix))
                .collect();
            selected = (!suggestions.is_empty()).then_some(0);
        }
        if !question.hidden {
            // 终端编辑只重画当前回答，不向日志或捕获缓冲保存隐藏输入。
            let ghost = selected
                .and_then(|index| suggestions.get(index))
                .and_then(|value| {
                    value.strip_prefix(text.rsplit(',').next().unwrap_or(&text).trim())
                })
                .unwrap_or("");
            let visible = text.rsplit('\n').next().unwrap_or(&text);
            let message = if style.get_output().is_decorated() {
                format!("\r\x1b[2K{prompt}{visible}\x1b[s\x1b[30;47m{ghost}\x1b[39;49m\x1b[u")
            } else {
                format!("\r{prompt}{visible}{ghost}")
            };
            if question.completion.is_none() {
                // 原生编辑器模拟终端回显，不把输入回显重复记入分区输出。
                style.get_output().emit_input_echo(&message)?;
            } else {
                style.write(message.as_str(), false, OutputFormat::Raw)?;
            }
        }
    }
}
fn complete(text: &mut String, suggestions: &[String], index: usize) {
    if let Some(value) = suggestions.get(index) {
        if let Some(comma) = text.rfind(',') {
            text.truncate(comma + 1);
            text.push_str(value);
        } else {
            *text = value.clone();
        }
    }
}
#[cfg(not(unix))]
#[derive(Clone, Copy)]
enum Byte {
    Pending,
    End,
    Value(u8),
}
#[cfg(unix)]
enum Chunk {
    Pending,
    End,
    Progress,
    Complete,
}
async fn pipe(multiline: bool) -> Result<Option<String>, ConsoleError> {
    let mut bytes = Vec::new();
    loop {
        #[cfg(unix)]
        match read_chunk(&mut bytes, multiline).map_err(ConsoleError::from_source)? {
            Chunk::Pending => tokio::time::sleep(Duration::from_millis(10)).await,
            Chunk::End | Chunk::Complete => break,
            Chunk::Progress => tokio::task::yield_now().await,
        }
        #[cfg(not(unix))]
        match read_byte().map_err(ConsoleError::from_source)? {
            Byte::Pending => tokio::time::sleep(Duration::from_millis(10)).await,
            Byte::End => break,
            Byte::Value(byte) => {
                let (_, complete) =
                    super::input::append_answer_chunk(&mut bytes, &[byte], multiline);
                if complete {
                    break;
                }
            }
        }
    }
    if bytes.is_empty() && !multiline {
        Ok(None)
    } else {
        String::from_utf8(bytes)
            .map(Some)
            .map_err(ConsoleError::from_source)
    }
}
#[cfg(unix)]
fn read_chunk(target: &mut Vec<u8>, multiline: bool) -> io::Result<Chunk> {
    use std::io::BufRead as _;
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let buffer = match reader.fill_buf() {
        Ok(buffer) => buffer,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            return Ok(Chunk::Pending);
        }
        Err(error) => return Err(error),
    };
    if buffer.is_empty() {
        return Ok(Chunk::End);
    }
    let (consumed, complete) = super::input::append_answer_chunk(target, buffer, multiline);
    reader.consume(consumed);
    Ok(if complete {
        Chunk::Complete
    } else {
        Chunk::Progress
    })
}
#[cfg(windows)]
fn read_byte() -> io::Result<Byte> {
    use windows_sys::Win32::{
        Foundation::ERROR_BROKEN_PIPE,
        Storage::FileSystem::{FILE_TYPE_PIPE, GetFileType, ReadFile},
        System::{
            Console::{GetStdHandle, STD_INPUT_HANDLE},
            Pipes::PeekNamedPipe,
        },
    };
    // SAFETY: 标准输入句柄归 Windows 所有，调用不关闭或转移它。
    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    if unsafe { GetFileType(handle) } == FILE_TYPE_PIPE {
        let mut available = 0;
        if unsafe {
            PeekNamedPipe(
                handle,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        } == 0
        {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
                Ok(Byte::End)
            } else {
                Err(error)
            };
        }
        if available == 0 {
            return Ok(Byte::Pending);
        }
    }
    let mut byte = 0u8;
    let mut read = 0;
    // SAFETY: 独占租约中的管道已有一字节可读，或标准输入是普通文件。
    if unsafe { ReadFile(handle, &mut byte, 1, &mut read, std::ptr::null_mut()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(if read == 0 {
        Byte::End
    } else {
        Byte::Value(byte)
    })
}
#[cfg(not(any(unix, windows)))]
fn read_byte() -> io::Result<Byte> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "当前平台不支持标准输入轮询",
    ))
}
