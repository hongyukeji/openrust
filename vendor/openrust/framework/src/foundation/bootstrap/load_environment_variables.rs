use std::{env, io, path::Path};

use crate::config::{ConfigError, Environment};

/// Captures environment variables and the selected environment file.
pub struct LoadEnvironmentVariables;

impl LoadEnvironmentVariables {
    pub fn bootstrap(
        base_path: &Path,
        selected_env: Option<&str>,
    ) -> Result<Environment, ConfigError> {
        let mut environment = Environment {
            values: env::vars_os()
                .filter_map(|(key, value)| key.into_string().ok().map(|key| (key, value)))
                .collect(),
        };
        let process_env = environment
            .get_utf8("APP_ENV")?
            .filter(|name| !name.is_empty() && *name != "0");
        let mut path = base_path.join(".env");
        for name in [selected_env, process_env].into_iter().flatten() {
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            {
                return Err(ConfigError::InvalidEnvironmentName);
            }
            let candidate = base_path.join(format!(".env.{name}"));
            match candidate.metadata() {
                Ok(metadata) if metadata.is_file() => {
                    path = candidate;
                    break;
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(ConfigError::ReadFailed {
                        path: candidate,
                        source,
                    });
                }
            }
        }

        let entries = match dotenvy::from_path_iter(&path) {
            Ok(entries) => entries,
            Err(error) if error.not_found() => return Ok(environment),
            Err(error) => return Err(file_error(&path, error)),
        };
        let file_values = entries
            .collect::<Result<std::collections::HashMap<_, _>, _>>()
            .map_err(|error| file_error(&path, error))?;
        for (key, value) in file_values {
            environment
                .values
                .entry(key)
                .or_insert_with(|| value.into());
        }
        Ok(environment)
    }
}

fn file_error(path: &Path, error: dotenvy::Error) -> ConfigError {
    match error {
        dotenvy::Error::Io(source) => ConfigError::ReadFailed {
            path: path.to_owned(),
            source,
        },
        _ => ConfigError::InvalidEnvironmentFile {
            path: path.to_owned(),
        },
    }
}
