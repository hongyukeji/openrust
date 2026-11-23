use super::{FileResult, Filesystem, io_error};
#[cfg(not(unix))]
use std::fs;
use std::path::Path;
impl Filesystem {
    pub fn link(&self, target: impl AsRef<Path>, link: impl AsRef<Path>) -> FileResult<()> {
        let target = target.as_ref();
        let link = link.as_ref();
        #[cfg(unix)]
        let result = std::os::unix::fs::symlink(target, link);
        #[cfg(windows)]
        let result = if target.is_dir() {
            std::os::windows::fs::symlink_dir(target, link)
        } else {
            fs::hard_link(target, link)
        };
        #[cfg(not(any(unix, windows)))]
        let result = fs::hard_link(target, link);
        result.map_err(|error| io_error(link, error))
    }
    pub fn relative_link(
        &self,
        target: impl AsRef<Path>,
        link: impl AsRef<Path>,
    ) -> FileResult<()> {
        let target = target.as_ref();
        let link = link.as_ref();
        let absolute = std::path::absolute(target).map_err(|error| io_error(target, error))?;
        let parent = link
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = std::path::absolute(parent).map_err(|error| io_error(parent, error))?;
        let relative = pathdiff::diff_paths(&absolute, &parent).unwrap_or(absolute);
        #[cfg(windows)]
        if !target.is_dir() {
            return fs::hard_link(target, link).map_err(|error| io_error(link, error));
        }
        self.link(relative, link)
    }
    pub fn glob(&self, pattern: &str) -> FileResult<Vec<std::path::PathBuf>> {
        let options = glob::MatchOptions {
            case_sensitive: cfg!(unix),
            require_literal_separator: true,
            require_literal_leading_dot: true,
        };
        let matches = glob::glob_with(pattern, options)
            .map_err(|error| super::FileError::Pattern(error.to_string()))?;
        matches
            .map(|value| {
                value.map_err(|error| {
                    let path = error.path().to_owned();
                    io_error(&path, error.into())
                })
            })
            .collect()
    }
}
