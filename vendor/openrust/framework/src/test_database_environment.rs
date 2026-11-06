//! 库内并发测试的原始数据库输入；只由cfg(test)接入，不改变外部test-util环境语义。

use std::{
    env::VarError,
    ffi::OsString,
    path::{Component, PathBuf},
    sync::{Arc, OnceLock},
};

struct Inputs {
    state_dir: Option<OsString>,
    cwd: std::result::Result<PathBuf, Arc<std::io::Error>>,
    database_url: Option<OsString>,
}

static INPUTS: OnceLock<Inputs> = OnceLock::new();

fn inputs() -> &'static Inputs {
    INPUTS.get_or_init(|| Inputs {
        state_dir: std::env::var_os("OPENRUST_TEST_DATABASE_STATE_DIR"),
        cwd: std::env::current_dir().map_err(Arc::new),
        database_url: std::env::var_os("OPENRUST_TEST_DATABASE_URL"),
    })
}

pub(crate) fn seed() {
    let _ = inputs();
}

pub(crate) fn state_root() -> crate::Result<PathBuf> {
    inputs().state_root()
}

pub(crate) fn database_url() -> std::result::Result<String, VarError> {
    inputs().database_url()
}

impl Inputs {
    fn state_root(&self) -> crate::Result<PathBuf> {
        if let Some(raw) = &self.state_dir {
            let path = PathBuf::from(raw);
            // 保留原始非法路径交既有OwnershipJournal验证；不能join后抹掉前导CurDir。
            if path.as_os_str().is_empty()
                || path
                    .components()
                    .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
                || path.is_absolute()
            {
                return Ok(path);
            }
            return Ok(self.original_cwd()?.join(path));
        }
        Ok(self
            .original_cwd()?
            .join("storage/framework/testing/databases"))
    }

    fn original_cwd(&self) -> crate::Result<&PathBuf> {
        self.cwd
            .as_ref()
            .map_err(|error| crate::Error::internal(Arc::clone(error)))
    }

    fn database_url(&self) -> std::result::Result<String, VarError> {
        self.database_url
            .clone()
            .ok_or(VarError::NotPresent)?
            .into_string()
            .map_err(VarError::NotUnicode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_state(state: Option<&str>) -> Inputs {
        Inputs {
            state_dir: state.map(OsString::from),
            cwd: Ok(PathBuf::from("/original-test-cwd")),
            database_url: None,
        }
    }

    #[test]
    fn default_and_relative_roots_use_original_cwd_but_invalid_components_stay_raw()
    -> crate::Result<()> {
        assert_eq!(
            with_state(None).state_root()?,
            PathBuf::from("/original-test-cwd/storage/framework/testing/databases")
        );
        assert_eq!(
            with_state(Some("private/journal")).state_root()?,
            PathBuf::from("/original-test-cwd/private/journal")
        );
        for raw in ["", ".", "./journal", "../journal"] {
            assert_eq!(with_state(Some(raw)).state_root()?, PathBuf::from(raw));
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn absolute_override_ignores_cwd_error_but_relative_and_default_propagate_it()
    -> crate::Result<()> {
        let mut inputs = with_state(Some("/explicit-journal"));
        inputs.cwd = Err(Arc::new(std::io::Error::from_raw_os_error(2)));
        assert_eq!(inputs.state_root()?, PathBuf::from("/explicit-journal"));
        inputs.state_dir = Some(OsString::from("relative"));
        assert!(inputs.state_root().is_err());
        inputs.state_dir = None;
        assert!(inputs.state_root().is_err());
        assert_eq!(
            inputs
                .cwd
                .as_ref()
                .err()
                .and_then(|error| error.raw_os_error()),
            Some(2)
        );
        inputs.state_dir = Some(OsString::new());
        assert_eq!(inputs.state_root()?, PathBuf::new());
        Ok(())
    }

    #[test]
    fn database_url_keeps_absence_and_empty_string_distinct() {
        let mut inputs = with_state(None);
        assert!(matches!(inputs.database_url(), Err(VarError::NotPresent)));
        inputs.database_url = Some(OsString::new());
        assert_eq!(inputs.database_url(), Ok(String::new()));
    }

    #[cfg(unix)]
    #[test]
    fn database_url_preserves_non_unicode_error_bytes() {
        use std::os::unix::ffi::OsStringExt;
        let raw = OsString::from_vec(vec![0xff]);
        let mut inputs = with_state(None);
        inputs.database_url = Some(raw.clone());
        assert!(matches!(inputs.database_url(), Err(VarError::NotUnicode(value)) if value == raw));
    }
}
