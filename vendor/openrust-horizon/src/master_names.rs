use crate::{Error, Result, supervisor::hostname};
use openrust::{
    container::ServiceKey,
    support::{Str, facades::Facade},
};
use serde_json::Value;
use std::sync::{Arc, LazyLock, RwLock};

type Resolver = Arc<dyn Fn() -> String + Send + Sync>;

#[derive(Default)]
pub(crate) struct MasterNames(RwLock<Option<Resolver>>);

impl MasterNames {
    pub(crate) fn basename(&self, host: &str) -> Result<String> {
        let (basename, _) = self.resolve(host)?;
        Ok(basename)
    }

    fn resolve(&self, host: &str) -> Result<(String, bool)> {
        let resolver = self
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let legacy = resolver.is_none();
        let basename = resolver.map_or_else(|| Str::slug(host), |resolver| resolver());
        if basename.chars().any(char::is_control) {
            return Err(Error::InvalidConfiguration);
        }
        Ok((basename, legacy))
    }

    pub(crate) fn local(&self) -> Result<LocalMasters> {
        let host = hostname()?;
        let (basename, legacy) = self.resolve(&host)?;
        Ok(LocalMasters {
            host,
            basename,
            legacy,
        })
    }

    pub(crate) fn determine_using(
        resolve: impl Fn() -> String + Send + Sync + 'static,
    ) -> Result<()> {
        let names = NameStore::get_facade_root().map_err(|_| Error::Unavailable)?;
        *names
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(resolve));
        Ok(())
    }

    pub(crate) fn current_basename() -> Result<String> {
        let host = hostname()?;
        match NameStore::get_facade_root() {
            Ok(names) => names.basename(&host),
            Err(_) => Self::default().basename(&host),
        }
    }
}

pub(crate) fn process_name(basename: &str) -> String {
    static TOKEN: LazyLock<String> = LazyLock::new(|| Str::random(4));
    format!("{basename}-{}", TOKEN.as_str())
}

pub(crate) fn valid_process_name(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(char::is_control)
}

pub(crate) fn supervisor_suffix<'a>(master: &str, supervisor: &'a str) -> Option<&'a str> {
    supervisor
        .strip_prefix(master)?
        .strip_prefix(':')
        .filter(|suffix| !suffix.is_empty())
}

pub(crate) fn supervisor_parent(name: &str) -> Option<&str> {
    let (master, suffix) = name.rsplit_once(':')?;
    (!suffix.is_empty() && valid_process_name(master)).then_some(master)
}

pub(crate) fn basename_from_name(name: &str) -> Option<&str> {
    let (basename, token) = name.rsplit_once('-')?;
    (token.len() == 4 && token.bytes().all(|byte| byte.is_ascii_alphanumeric())).then_some(basename)
}

pub(crate) struct LocalMasters {
    host: String,
    basename: String,
    legacy: bool,
}

impl LocalMasters {
    pub(crate) fn matches(&self, heartbeat: &Value) -> bool {
        heartbeat["host"].as_str() == Some(self.host.as_str())
            && heartbeat["basename"]
                .as_str()
                .map_or(self.legacy, |basename| basename == self.basename)
    }
}

struct NameStore;
impl Facade for NameStore {
    type Service = MasterNames;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<MasterNames>()
    }
}
