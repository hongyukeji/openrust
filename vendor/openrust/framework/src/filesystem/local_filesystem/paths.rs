use super::{FileResult, Filesystem, io_error};
use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};
impl Filesystem {
    pub fn basename(&self, path: impl AsRef<Path>) -> OsString {
        path.as_ref()
            .file_name()
            .unwrap_or_else(|| OsStr::new(""))
            .to_owned()
    }
    pub fn dirname(&self, path: impl AsRef<Path>) -> PathBuf {
        let path = path.as_ref();
        path.parent()
            .filter(|path| !path.as_os_str().is_empty())
            .map(Path::to_owned)
            .unwrap_or_else(|| {
                if path.has_root() {
                    path.to_owned()
                } else {
                    PathBuf::from(".")
                }
            })
    }
    pub fn name(&self, path: impl AsRef<Path>) -> OsString {
        split_name(&self.basename(path)).0
    }
    pub fn extension(&self, path: impl AsRef<Path>) -> OsString {
        split_name(&self.basename(path)).1
    }
    pub fn r#type(&self, path: impl AsRef<Path>) -> FileResult<&'static str> {
        let path = path.as_ref();
        let kind = fs::symlink_metadata(path)
            .map_err(|source| io_error(path, source))?
            .file_type();
        if kind.is_symlink() {
            return Ok("link");
        }
        if kind.is_file() {
            return Ok("file");
        }
        if kind.is_dir() {
            return Ok("dir");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt as _;
            if kind.is_fifo() {
                return Ok("fifo");
            }
            if kind.is_socket() {
                return Ok("socket");
            }
            if kind.is_char_device() {
                return Ok("char");
            }
            if kind.is_block_device() {
                return Ok("block");
            }
        }
        Ok("unknown")
    }
    pub fn is_readable(&self, path: impl AsRef<Path>) -> bool {
        #[cfg(unix)]
        {
            rustix::fs::access(path.as_ref(), rustix::fs::Access::READ_OK).is_ok()
        }
        #[cfg(not(unix))]
        {
            let path = path.as_ref();
            if path.is_dir() {
                fs::read_dir(path).is_ok()
            } else {
                fs::File::open(path).is_ok()
            }
        }
    }
    pub fn is_writable(&self, path: impl AsRef<Path>) -> bool {
        #[cfg(unix)]
        {
            rustix::fs::access(path.as_ref(), rustix::fs::Access::WRITE_OK).is_ok()
        }
        #[cfg(not(unix))]
        {
            fs::metadata(path).is_ok_and(|value| value.is_dir() || !value.permissions().readonly())
        }
    }
}
#[cfg(unix)]
fn split_name(name: &OsStr) -> (OsString, OsString) {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let bytes = name.as_bytes();
    match bytes.iter().rposition(|byte| *byte == b'.') {
        Some(index) => (
            OsString::from_vec(bytes[..index].to_vec()),
            OsString::from_vec(bytes[index + 1..].to_vec()),
        ),
        None => (name.to_owned(), OsString::new()),
    }
}
#[cfg(windows)]
fn split_name(name: &OsStr) -> (OsString, OsString) {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let wide: Vec<_> = name.encode_wide().collect();
    match wide.iter().rposition(|unit| *unit == u16::from(b'.')) {
        Some(index) => (
            OsString::from_wide(&wide[..index]),
            OsString::from_wide(&wide[index + 1..]),
        ),
        None => (name.to_owned(), OsString::new()),
    }
}
#[cfg(not(any(unix, windows)))]
fn split_name(name: &OsStr) -> (OsString, OsString) {
    let text = name.to_string_lossy();
    text.rsplit_once('.').map_or_else(
        || (name.to_owned(), OsString::new()),
        |(stem, extension)| (stem.into(), extension.into()),
    )
}
