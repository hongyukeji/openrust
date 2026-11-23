use crate::{
    config::{EnvironmentEditor, EnvironmentFile},
    console::ConsoleError,
};
use std::fmt::Write as _;
use std::path::Path;

pub(super) struct StagedKeyFile {
    editor: EnvironmentEditor,
}
impl StagedKeyFile {
    pub(super) fn commit(mut self) -> Result<(), ConsoleError> {
        self.editor
            .save()
            .map_err(|e| ConsoleError::new(e.to_string()))?;
        Ok(())
    }
}
pub(super) fn stage_key_file(
    path: &Path,
    variable: &str,
    expected: &str,
    new_key: &str,
    external_key_present: bool,
) -> Result<StagedKeyFile, ConsoleError> {
    if external_key_present {
        return Err(failure(
            "Unable to set application key. APP_KEY is already present in the environment.",
        ));
    }
    let mut editor = EnvironmentFile::open(path)
        .and_then(|file| file.edit())
        .map_err(|e| ConsoleError::new(e.to_string()))?;
    let updated = replace_assignment(editor.original(), variable, expected, new_key)?;
    editor
        .replace_content(updated.to_string())
        .map_err(|e| ConsoleError::new(e.to_string()))?;
    editor.restrict_permissions();
    Ok(StagedKeyFile { editor })
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

fn failure(message: &'static str) -> ConsoleError {
    ConsoleError::new(message)
}
