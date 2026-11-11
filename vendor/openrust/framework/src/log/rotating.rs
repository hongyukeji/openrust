use std::{
    ffi::OsString,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};

use chrono::{Datelike as _, NaiveDate};

use super::sink::FileOptions;

#[derive(Clone, Copy)]
pub(super) enum Period {
    Daily,
    Monthly,
}

impl Period {
    fn format(self) -> &'static str {
        match self {
            Self::Daily => "%Y-%m-%d",
            Self::Monthly => "%Y-%m",
        }
    }

    fn start(self, date: NaiveDate) -> NaiveDate {
        match self {
            Self::Daily => date,
            Self::Monthly => date.with_day(1).expect("every month starts on day one"),
        }
    }

    fn matches(self, text: &str) -> bool {
        let date = match self {
            Self::Daily => NaiveDate::parse_from_str(text, self.format()),
            Self::Monthly => NaiveDate::parse_from_str(&format!("{text}-01"), "%Y-%m-%d"),
        };
        date.is_ok_and(|date| date.format(self.format()).to_string() == text)
    }
}

#[derive(Default)]
struct State {
    active: Option<(NaiveDate, PathBuf)>,
    file: Option<File>,
}

pub(super) struct RotatingFile {
    options: FileOptions,
    period: Period,
    max_files: usize,
    state: Mutex<State>,
}

impl RotatingFile {
    pub fn new(options: FileOptions, period: Period, max_files: usize) -> Option<Self> {
        options.path.file_stem()?.to_str()?;
        if let Some(extension) = options.path.extension() {
            extension.to_str()?;
        }
        Some(Self {
            options,
            period,
            max_files,
            state: Mutex::default(),
        })
    }

    pub fn write(&self, date: NaiveDate, bytes: &[u8]) -> io::Result<()> {
        let date = self.period.start(date);
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let State { active, file } = &mut *guard;
        if let Some((current, path)) = active {
            if *current == date {
                return self.options.write(path, file, bytes);
            }
            if date < *current {
                // A record can reach the sink late when writes overlap midnight.
                let result = self.options.write(&self.path(date), &mut None, bytes);
                if result.is_ok() {
                    self.prune(path);
                }
                return result;
            }
        }
        let path = self.path(date);
        let mut next = None;
        self.options.write(&path, &mut next, bytes)?;
        *file = next;
        self.prune(&path);
        *active = Some((date, path));
        Ok(())
    }

    fn path(&self, date: NaiveDate) -> PathBuf {
        let mut name = self
            .options
            .path
            .file_stem()
            .expect("validated file name")
            .to_os_string();
        name.push(
            date.format(&format!("-{}", self.period.format()))
                .to_string(),
        );
        name.push(self.extension());
        self.options.path.with_file_name(name)
    }

    fn extension(&self) -> OsString {
        self.options
            .path
            .extension()
            .map_or_else(OsString::new, |ext| {
                let mut suffix = OsString::from(".");
                suffix.push(ext);
                suffix
            })
    }

    fn prune(&self, active: &Path) {
        if self.max_files == 0 {
            return;
        }
        let parent = active
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let stem = self
            .options
            .path
            .file_stem()
            .and_then(|name| name.to_str())
            .expect("configuration is UTF-8");
        let suffix = self.extension();
        let suffix = suffix.to_str().expect("configuration is UTF-8");
        let prefix = format!("{stem}-");
        let Ok(entries) = fs::read_dir(parent) else {
            return;
        };
        let mut old: Vec<_> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name();
                let date = name.to_str()?.strip_prefix(&prefix)?.strip_suffix(suffix)?;
                (self.period.matches(date)
                    && entry.file_type().is_ok_and(|kind| kind.is_file())
                    && Some(name.as_os_str()) != active.file_name())
                .then_some(name)
            })
            .collect();
        old.sort_unstable_by(|left, right| right.cmp(left));
        for name in old.into_iter().skip(self.max_files - 1) {
            // Other application processes can prune the same file concurrently.
            let _ = fs::remove_file(parent.join(name));
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/log_rotation.rs"]
mod tests;
