use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::console::ConsoleError;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub(super) struct StagedKeyFile<'a> {
    path: &'a Path,
    _lock: fs::File,
    temporary: std::path::PathBuf,
    original: zeroize::Zeroizing<String>,
    committed: bool,
}

impl StagedKeyFile<'_> {
    pub(super) fn commit(mut self) -> Result<(), ConsoleError> {
        require_regular_file(self.path)?;
        let current = zeroize::Zeroizing::new(
            fs::read_to_string(self.path)
                .map_err(|_| failure("Unable to reread the environment file."))?,
        );
        if *current != *self.original {
            return Err(failure(
                "The environment file changed before the key could be written.",
            ));
        }
        fs::rename(&self.temporary, self.path)
            .map_err(|_| failure("Unable to replace the environment file."))?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for StagedKeyFile<'_> {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.temporary);
        }
    }
}

pub(super) fn stage_key_file<'a>(
    path: &'a Path,
    variable: &str,
    expected: &str,
    new_key: &str,
    external_key_present: bool,
) -> Result<StagedKeyFile<'a>, ConsoleError> {
    require_regular_file(path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    let mut lock = options
        .open(path)
        .map_err(|_| failure("Unable to open the environment file."))?;
    lock.lock()
        .map_err(|_| failure("Unable to lock the environment file."))?;
    let mut original = zeroize::Zeroizing::new(String::new());
    lock.read_to_string(&mut original)
        .map_err(|_| failure("Unable to read the environment file."))?;
    if external_key_present {
        return Err(failure(
            "Unable to set application key. APP_KEY is already present in the environment.",
        ));
    }
    let updated = replace_assignment(&original, variable, expected, new_key)?;
    let parent = path
        .parent()
        .ok_or_else(|| failure("The environment file has no parent directory."))?;
    let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".key-generate-{}-{sequence}.tmp",
        std::process::id()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|_| failure("Unable to create the temporary environment file."))?;
    let staged = StagedKeyFile {
        path,
        _lock: lock,
        temporary,
        original,
        committed: false,
    };
    let written = file
        .write_all(updated.as_bytes())
        .and_then(|()| file.sync_all());
    drop(file);
    written.map_err(|_| failure("Unable to write the temporary environment file."))?;
    Ok(staged)
}

fn parsed_key(
    text: &str,
    variable: &str,
) -> Result<Option<zeroize::Zeroizing<String>>, ConsoleError> {
    let mut found = None;
    for entry in dotenvy::from_read_iter(text.as_bytes()) {
        let (name, value) = entry.map_err(|_| failure("The environment file is invalid."))?;
        let value = zeroize::Zeroizing::new(value);
        if name == variable {
            if found.is_some() {
                return Err(failure(
                    "APP_KEY must have a single unquoted assignment in the environment file.",
                ));
            }
            found = Some(value);
        }
    }
    Ok(found)
}

fn replace_assignment(
    original: &str,
    variable: &str,
    expected: &str,
    key: &str,
) -> Result<zeroize::Zeroizing<String>, ConsoleError> {
    let configured = parsed_key(original, variable)?.ok_or_else(|| {
        failure("Unable to set application key. No APP_KEY variable was found in the .env file.")
    })?;
    if configured.as_str() != expected {
        return Err(failure(
            "APP_KEY in the environment file differs from the active configuration.",
        ));
    }

    // Locate the logical assignment without changing matching text in multiline values.
    let raw_assignment = zeroize::Zeroizing::new(format!("{variable}={expected}"));
    let marker = "openrust-key-probe-";
    let mut probe = zeroize::Zeroizing::new(String::with_capacity(original.len()));
    for (index, line) in original.split_inclusive('\n').enumerate() {
        let body = line.trim_end_matches(['\r', '\n']);
        if body == raw_assignment.as_str() {
            write!(probe, "{variable}={marker}{index}{}", &line[body.len()..])
                .map_err(|_| failure("Unable to identify the APP_KEY assignment."))?;
        } else {
            probe.push_str(line);
        }
    }
    let line_index = parsed_key(&probe, variable)?
        .as_deref()
        .and_then(|value| value.strip_prefix(marker))
        .and_then(|index| index.parse::<usize>().ok())
        .ok_or_else(|| {
            failure("APP_KEY must have a single unquoted assignment in the environment file.")
        })?;

    let mut output = zeroize::Zeroizing::new(String::with_capacity(original.len()));
    for (index, line) in original.split_inclusive('\n').enumerate() {
        let body = line.trim_end_matches(['\r', '\n']);
        if index == line_index && body == raw_assignment.as_str() {
            write!(output, "{variable}={key}{}", &line[body.len()..])
                .map_err(|_| failure("Unable to replace the APP_KEY assignment."))?;
        } else {
            output.push_str(line);
        }
    }
    if parsed_key(&output, variable)?
        .as_deref()
        .map(String::as_str)
        != Some(key)
    {
        return Err(failure("Unable to verify the APP_KEY assignment."));
    }
    Ok(output)
}

fn require_regular_file(path: &Path) -> Result<(), ConsoleError> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file()) {
        Ok(())
    } else {
        Err(failure(
            "The environment file must be an existing regular file.",
        ))
    }
}

fn failure(message: &'static str) -> ConsoleError {
    ConsoleError::new(message)
}
