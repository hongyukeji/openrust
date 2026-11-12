use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    path::Path,
};

use crate::console::ConsoleError;

pub(super) fn lock_manifest(root: &Path) -> Result<File, ConsoleError> {
    let manifest_path = root.join("Cargo.toml");
    let manifest = File::open(&manifest_path).map_err(|error| file_error(&manifest_path, error))?;
    manifest
        .lock()
        .map_err(|error| file_error(&manifest_path, error))?;
    Ok(manifest)
}

pub(super) fn insert_before(
    source: &str,
    path: &Path,
    statement: &str,
    is_anchor: impl Fn(&str) -> bool,
) -> Result<String, ConsoleError> {
    let mut offset = 0;
    let mut found = None;
    for line in source.split_inclusive('\n') {
        if is_anchor(line) {
            let indent = &line[..line.len() - line.trim_start().len()];
            if found.replace((offset, indent)).is_some() {
                return Err(manual_registration(path, statement));
            }
        }
        offset += line.len();
    }
    let (offset, indent) = found.ok_or_else(|| manual_registration(path, statement))?;
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut updated = source.to_owned();
    updated.insert_str(offset, &format!("{indent}{statement}{newline}"));
    Ok(updated)
}

fn manual_registration(path: &Path, statement: &str) -> ConsoleError {
    ConsoleError::new(format!(
        "Cannot locate the registration point in {}. Add `{statement}` manually.",
        path.display()
    ))
}

pub(super) fn replace(path: &Path, previous: &str, updated: &str) -> Result<(), ConsoleError> {
    let current = read(path)?;
    if current == updated {
        return Ok(());
    }
    if current != previous {
        return Err(ConsoleError::new(format!(
            "{} changed during installation. Retry with the updated file.",
            path.display()
        )));
    }
    let temporary = path.with_extension(format!("openrust-{:016x}.tmp", fastrand::u64(..)));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| file_error(path, error))?;
    let result = (|| {
        file.set_permissions(fs::metadata(path)?.permissions())?;
        file.write_all(updated.as_bytes())?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(temporary);
        return Err(file_error(path, error));
    }
    Ok(())
}

pub(super) fn read(path: &Path) -> Result<String, ConsoleError> {
    fs::read_to_string(path).map_err(|error| file_error(path, error))
}

pub(super) fn file_error(path: &Path, error: io::Error) -> ConsoleError {
    ConsoleError::new(format!("{}: {error}", path.display()))
}
