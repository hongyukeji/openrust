use std::io::{self, IsTerminal as _, Write as _};

#[cfg(unix)]
use {
    std::{
        ffi::{CStr, OsStr},
        fs::{File, OpenOptions},
        io::Read as _,
        mem::MaybeUninit,
        os::{
            fd::AsRawFd as _,
            unix::{ffi::OsStrExt as _, fs::OpenOptionsExt as _},
        },
    },
    tokio::io::{Interest, unix::AsyncFd},
};

#[derive(Clone, Copy)]
#[cfg_attr(not(unix), allow(dead_code, reason = "Interactive input is Unix-only"))]
pub(super) enum Key {
    Character(char),
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Tab,
    Enter,
    Backspace,
    Escape,
    Quit,
}

pub(super) struct Device {
    #[cfg(unix)]
    input: AsyncFd<File>,
    #[cfg(unix)]
    original: libc::termios,
    #[cfg(unix)]
    keys: Vec<u8>,
    #[cfg(unix)]
    escape_deadline: Option<tokio::time::Instant>,
}

impl Device {
    pub(super) fn open() -> io::Result<Self> {
        if !io::stdin().is_terminal()
            || !io::stdout().is_terminal()
            || std::env::var_os("TERM").is_none_or(|term| term.is_empty() || term == "dumb")
        {
            return Err(io::Error::other("interactive terminal unavailable"));
        }
        #[cfg(unix)]
        {
            // 在 Darwin 上不能把 /dev/tty 别名注册到 kqueue。独立打开实际终端，
            // 使非阻塞标志局限于本次会话所拥有的文件描述。
            let mut name = [0; libc::PATH_MAX as usize];
            // SAFETY: 输入描述符仍由标准输入持有；name 是可写且长度已传入的缓冲区。
            let error =
                unsafe { libc::ttyname_r(io::stdin().as_raw_fd(), name.as_mut_ptr(), name.len()) };
            if error != 0 {
                return Err(io::Error::from_raw_os_error(error));
            }
            // SAFETY: ttyname_r 成功时保证返回以 NUL 结尾的终端设备名。
            let name = unsafe { CStr::from_ptr(name.as_ptr()) };
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
                .open(OsStr::from_bytes(name.to_bytes()))?;
            // SAFETY: AsyncFd owns this File, whose descriptor remains open and stable
            // through deregistration. It is opened nonblocking before registration.
            let input = unsafe { AsyncFd::register_with_interest(file, Interest::READABLE) }?;
            let fd = input.get_ref().as_raw_fd();
            let mut original = MaybeUninit::uninit();
            // SAFETY: The device owns a live descriptor; tcgetattr writes the pointed-to termios.
            if unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } != 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: tcgetattr returned success.
            let original = unsafe { original.assume_init() };
            let mut raw = original;
            // SAFETY: raw is an initialized copy of the terminal attributes.
            unsafe { libc::cfmakeraw(&mut raw) };
            // SAFETY: fd is live and raw was initialized above.
            if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
                return Err(io::Error::last_os_error());
            }
            let device = Self {
                input,
                original,
                keys: Vec::new(),
                escape_deadline: None,
            };
            device.write("\x1b[?1049h\x1b[?25l")?;
            Ok(device)
        }
        #[cfg(not(unix))]
        Err(io::Error::other("interactive terminal unavailable"))
    }

    pub(super) fn size() -> Option<(u16, u16)> {
        console::Term::stdout().size_checked()
    }

    pub(super) fn write(&self, frame: &str) -> io::Result<()> {
        let mut output = io::stdout().lock();
        output.write_all(frame.as_bytes())?;
        output.flush()
    }

    pub(super) async fn key(&mut self) -> Key {
        #[cfg(unix)]
        loop {
            if let Some((key, length)) = decode(&self.keys) {
                self.keys.drain(..length);
                self.escape_deadline = None;
                return key;
            }
            if self.keys.first() == Some(&27) {
                let deadline = *self.escape_deadline.get_or_insert_with(|| {
                    tokio::time::Instant::now() + std::time::Duration::from_millis(35)
                });
                match tokio::time::timeout_at(deadline, self.read()).await {
                    Ok(true) => continue,
                    Ok(false) => return Key::Quit,
                    Err(_) => {
                        self.keys.remove(0);
                        self.escape_deadline = None;
                        return Key::Escape;
                    }
                }
            }
            if !self.read().await {
                return Key::Quit;
            }
        }
        #[cfg(not(unix))]
        std::future::pending().await
    }

    #[cfg(unix)]
    async fn read(&mut self) -> bool {
        let mut bytes = [0; 128];
        loop {
            let Ok(mut ready) = self.input.readable().await else {
                return false;
            };
            match ready.try_io(|source| source.get_ref().read(&mut bytes)) {
                Ok(Ok(count)) if count > 0 => {
                    self.keys.extend_from_slice(&bytes[..count]);
                    return true;
                }
                Ok(Err(error)) if error.kind() == io::ErrorKind::Interrupted => {}
                Ok(_) => return false,
                Err(_) => {}
            }
        }
    }
}

#[cfg(unix)]
impl Drop for Device {
    fn drop(&mut self) {
        let fd = self.input.get_ref().as_raw_fd();
        // SAFETY: The device still owns the terminal and saved these attributes in open.
        let _ = unsafe { libc::tcsetattr(fd, libc::TCSANOW, &self.original) };
        let _ = self.write("\x1b[0m\x1b[?25h\x1b[?1049l");
    }
}

#[cfg(unix)]
fn decode(bytes: &[u8]) -> Option<(Key, usize)> {
    let first = *bytes.first()?;
    if first == 27 {
        let sequence = [
            (&b"\x1b[A"[..], Key::Up),
            (&b"\x1b[B"[..], Key::Down),
            (&b"\x1b[C"[..], Key::Right),
            (&b"\x1b[D"[..], Key::Left),
            (&b"\x1b[H"[..], Key::Home),
            (&b"\x1b[F"[..], Key::End),
            (&b"\x1b[5~"[..], Key::PageUp),
            (&b"\x1b[6~"[..], Key::PageDown),
            (&b"\x1bOH"[..], Key::Home),
            (&b"\x1bOF"[..], Key::End),
            (&b"\x1b[1~"[..], Key::Home),
            (&b"\x1b[4~"[..], Key::End),
        ];
        for (pattern, key) in sequence {
            if bytes.starts_with(pattern) {
                return Some((key, pattern.len()));
            }
            if pattern.starts_with(bytes) {
                return None;
            }
        }
        // Consume other CSI keys in full so their final letter cannot trigger a shortcut.
        if bytes.starts_with(b"\x1b[") {
            let length = bytes[2..]
                .iter()
                .position(|byte| (0x40..=0x7e).contains(byte))?
                + 3;
            return Some((Key::Character('\0'), length));
        }
        return Some((Key::Escape, 1));
    }
    let key = match first {
        3 | 4 => Key::Quit,
        8 | 127 => Key::Backspace,
        9 => Key::Tab,
        10 | 13 => Key::Enter,
        _ => {
            let length = match first {
                0..=127 => 1,
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => return Some((Key::Character('�'), 1)),
            };
            let bytes = bytes.get(..length)?;
            let character = std::str::from_utf8(bytes)
                .ok()
                .and_then(|value| value.chars().next())
                .unwrap_or('�');
            return Some((Key::Character(character), length));
        }
    };
    Some((key, 1))
}
