mod generated_function;
pub use generated_function::{
    add_generated_trait_methods, replace_generated_function, replace_generated_method,
};
mod middleware;
pub use middleware::enable_stateful_api;
mod user_model;
pub use user_model::UserModelSource;

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    path::Path,
};

use crate::console::ConsoleError;

pub fn lock_manifest(root: &Path) -> Result<File, ConsoleError> {
    let manifest_path = root.join("Cargo.toml");
    File::open(&manifest_path).map_err(|error| file_error(&manifest_path, error))?;
    lock_source(root)
}

pub fn lock_source(root: &Path) -> Result<File, ConsoleError> {
    // A separate inode keeps the lock valid when an installer replaces Cargo.toml.
    let directory = root.join("tmp");
    fs::create_dir_all(&directory).map_err(|error| file_error(&directory, error))?;
    let path = directory.join("artisan-source.lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|error| file_error(&path, error))?;
    lock.try_lock().map_err(|error| {
        ConsoleError::new(format!(
            "Cannot acquire installation lock {}: {error}. Another source operation may be running; retry after it finishes.",
            path.display()
        ))
    })?;
    Ok(lock)
}

pub fn insert_before(
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

pub fn replace(path: &Path, previous: &str, updated: &str) -> Result<(), ConsoleError> {
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

pub fn read(path: &Path) -> Result<String, ConsoleError> {
    fs::read_to_string(path).map_err(|error| file_error(path, error))
}

pub fn file_error(path: &Path, error: io::Error) -> ConsoleError {
    ConsoleError::new(format!("{}: {error}", path.display()))
}

/// 追加独立文件模块；遇到内联模块或条件不匹配时要求手动接线，避免覆盖已有代码。
pub fn declare_module(
    source: &str,
    name: &str,
    feature: Option<&str>,
) -> Result<String, ConsoleError> {
    let ident =
        syn::parse_str::<syn::Ident>(name).map_err(|error| ConsoleError::new(error.to_string()))?;
    let file = syn::parse_file(source).map_err(|error| ConsoleError::new(error.to_string()))?;
    for item in file.items {
        if let syn::Item::Mod(module) = item
            && module.ident == ident
        {
            let attrs_ok = if let Some(feature) = feature {
                module.attrs.len() == 1 && module.attrs[0].path().is_ident("cfg") &&
                module.attrs[0].parse_args::<syn::Meta>().ok().is_some_and(|meta| {
                    matches!(meta, syn::Meta::NameValue(value) if value.path.is_ident("feature") &&
                        matches!(&value.value, syn::Expr::Lit(literal) if matches!(&literal.lit, syn::Lit::Str(text) if text.value() == feature)))
                })
            } else {
                module.attrs.is_empty()
            };
            if module.content.is_some() || !attrs_ok {
                return Err(ConsoleError::new(format!(
                    "模块{name}使用内联或自定义条件；请先手动装配文件模块后重试"
                )));
            }
            return Ok(source.to_owned());
        }
    }
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let condition = feature
        .map(|value| format!("#[cfg(feature = {value:?})]{newline}"))
        .unwrap_or_default();
    Ok(format!(
        "{source}{newline}{condition}pub(crate) mod {name};{newline}"
    ))
}
