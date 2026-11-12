use std::{
    borrow::Cow,
    fs::{self, OpenOptions},
    io::{self, ErrorKind, Write as _},
    path::{Path, PathBuf},
};

use crate::foundation::Application;

use super::{Command, ConsoleError, Input};

/// Supplies the primary destination and stub for a file generator.
pub trait GeneratorCommand: Command {
    fn get_path(
        &self,
        app: &Application,
        name: &str,
        input: &Input,
    ) -> Result<PathBuf, ConsoleError>;

    fn get_stub(&self, app: &Application, input: &Input)
    -> Result<Cow<'static, str>, ConsoleError>;
}

pub(crate) fn write_generated(path: &Path, bytes: &[u8], force: bool) -> io::Result<()> {
    let directory = path.parent().expect("generator path has a parent");
    fs::create_dir_all(directory)?;
    if !force {
        return write_new(path, bytes);
    }
    let temporary = directory.join(format!(".generator-{:016x}.tmp", fastrand::u64(..)));
    write_new(&temporary, bytes)?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(temporary);
        return Err(error);
    }
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    if let Err(error) = file.write_all(bytes) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}

pub(crate) fn snake_case(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut snake = String::new();
    for (index, byte) in bytes.iter().copied().enumerate() {
        if index > 0
            && byte.is_ascii_uppercase()
            && (bytes[index - 1].is_ascii_lowercase()
                || bytes[index - 1].is_ascii_digit()
                || bytes.get(index + 1).is_some_and(u8::is_ascii_lowercase))
        {
            snake.push('_');
        }
        snake.push(char::from(byte.to_ascii_lowercase()));
    }
    snake
}

pub(crate) fn source_name_parts<'a>(
    name: &'a str,
    invalid_message: &'static str,
) -> Result<impl Iterator<Item = &'a str>, ConsoleError> {
    let parts = name.strip_suffix(".rs").unwrap_or(name).split(['/', '\\']);
    if parts.clone().any(|part| {
        !part.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
            || !part.bytes().all(|byte| byte.is_ascii_alphanumeric())
            || matches!(
                snake_case(part).as_str(),
                "self" | "super" | "crate" | "mod"
            )
    }) {
        return Err(ConsoleError::new(invalid_message));
    }
    Ok(parts)
}

pub(crate) fn load_stub(
    path: &Path,
    builtin: &'static str,
) -> Result<Cow<'static, str>, ConsoleError> {
    match fs::read_to_string(path) {
        Ok(template) => Ok(Cow::Owned(template)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Cow::Borrowed(builtin)),
        Err(error) => Err(ConsoleError::new(format!("{}: {error}", path.display()))),
    }
}
