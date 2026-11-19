use crate::client_repository::{ClientStore, EloquentClientStore};
use crate::{Client, ClientModel, ClientRepository, CryptKey, KeyError};
use openrust::database::DatabaseManager;

type ClientStoreFactory = fn(Arc<DatabaseManager>, Arc<dyn Repository>) -> Arc<dyn ClientStore>;
#[derive(Clone, Copy)]
struct ClientModelRegistration {
    name: &'static str,
    factory: ClientStoreFactory,
}
fn client_store<M: ClientModel>(
    database: Arc<DatabaseManager>,
    config: Arc<dyn Repository>,
) -> Arc<dyn ClientStore> {
    Arc::new(EloquentClientStore::<M>::new(database, config))
}
use openrust::{contracts::config::Repository, foundation::Paths};
use std::io::Read as _;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, RwLock, atomic::AtomicBool},
};
use zeroize::Zeroizing;

/// Application-local counterpart of Laravel Passport's configuration state.
#[allow(non_snake_case)]
pub struct Passport {
    /// Mirrors Passport::$validateKeyPermissions without process-global state.
    pub validateKeyPermissions: AtomicBool,
    /// Whether a parent scope grants a colon-delimited descendant scope.
    pub withInheritedScopes: AtomicBool,
    pub deviceCodeGrantEnabled: AtomicBool,
    pub clientUuids: AtomicBool,
    config: Arc<dyn Repository>,
    paths: Arc<Paths>,
    key_path: RwLock<Option<PathBuf>>,
    database: Arc<DatabaseManager>,
    client_model: RwLock<ClientModelRegistration>,
    token_models: Arc<crate::token_store::TokenModels>,
}
#[allow(non_snake_case)]
impl Passport {
    pub(crate) fn new(
        config: Arc<dyn Repository>,
        paths: Arc<Paths>,
        database: Arc<DatabaseManager>,
    ) -> Self {
        Self {
            token_models: Arc::new(crate::token_store::TokenModels::new(
                database.clone(),
                config.clone(),
            )),
            validateKeyPermissions: AtomicBool::new(true),
            withInheritedScopes: AtomicBool::new(false),
            deviceCodeGrantEnabled: AtomicBool::new(true),
            clientUuids: AtomicBool::new(true),
            config,
            paths,
            database,
            client_model: RwLock::new(ClientModelRegistration {
                name: "openrust_passport::Client",
                factory: client_store::<Client>,
            }),
            key_path: RwLock::new(None),
        }
    }
    /// Selects a compiled client type before resolving the lifecycle's repository.
    pub fn useClientModel<M: ClientModel>(&self) {
        *self
            .client_model
            .write()
            .expect("Passport client model lock poisoned") = ClientModelRegistration {
            name: std::any::type_name::<M>(),
            factory: client_store::<M>,
        };
    }
    pub fn useTokenModel<M: crate::TokenModel>(&self) {
        self.token_models.use_token::<M>();
    }
    pub fn useRefreshTokenModel<M: crate::RefreshTokenModel>(&self) {
        self.token_models.use_refresh::<M>();
    }
    pub fn tokenModel(&self) -> &'static str {
        self.token_models.token_name()
    }
    pub fn refreshTokenModel(&self) -> &'static str {
        self.token_models.refresh_name()
    }
    pub fn clientModel(&self) -> &'static str {
        self.client_model
            .read()
            .expect("Passport client model lock poisoned")
            .name
    }
    /// Creates an explicitly owned repository; DI scopes one instance per lifecycle.
    pub fn clientRepository(&self) -> ClientRepository {
        let factory = self
            .client_model
            .read()
            .expect("Passport client model lock poisoned")
            .factory;
        ClientRepository::new(
            factory(self.database.clone(), self.config.clone()),
            self.token_models.clone(),
        )
    }
    /// Sets a directory before lazy key services are first resolved.
    pub fn loadKeysFrom(&self, path: impl AsRef<Path>) {
        *self
            .key_path
            .write()
            .expect("Passport key path lock poisoned") = Some(path.as_ref().to_owned());
    }
    /// Resolves keys under the configured application storage directory by default.
    pub fn keyPath(&self, file: &str) -> PathBuf {
        let file = file.trim_start_matches(['/', '\\']);
        match self
            .key_path
            .read()
            .expect("Passport key path lock poisoned")
            .as_ref()
        {
            Some(path) => path.join(file),
            None => self.paths.storagePath(file),
        }
    }
    pub(crate) fn make_crypt_key(&self, kind: &str) -> Result<CryptKey, KeyError> {
        let option = match self.config.get(&format!("passport.{kind}_key")) {
            None | Some(serde_json::Value::Null) | Some(serde_json::Value::Bool(false)) => {
                String::new()
            }
            Some(serde_json::Value::Bool(true)) => "1".into(),
            Some(serde_json::Value::Number(value)) => value.to_string(),
            Some(serde_json::Value::String(value)) => value.replace("\\n", "\n"),
            _ => return Err(KeyError::InvalidConfiguration),
        };
        let option = Zeroizing::new(option);
        if !option.starts_with("file://")
            && let Ok(key) = CryptKey::new(Zeroizing::new(option.to_string()), None, None)
        {
            return Ok(key);
        }
        let path = if option.is_empty() || option.as_str() == "0" {
            self.keyPath(&format!("oauth-{kind}.key"))
        } else {
            configured_file_path(&option)?
        };
        if !path.is_absolute() {
            return Err(KeyError::Unreadable);
        }
        let mut file = std::fs::File::open(&path).map_err(|_| KeyError::Unreadable)?;
        let metadata = file.metadata().map_err(|_| KeyError::Unreadable)?;
        if !metadata.is_file() {
            return Err(KeyError::Unreadable);
        }
        let mut content = Zeroizing::new(String::new());
        file.read_to_string(&mut content)
            .map_err(|_| KeyError::Unreadable)?;
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            self.validateKeyPermissions
                .load(std::sync::atomic::Ordering::Relaxed)
                .then(|| metadata.permissions().mode() & 0o777)
        };
        #[cfg(not(unix))]
        let mode = None;
        CryptKey::new(content, Some(path), mode)
    }
}

// Reuses the old provider's verified local-wrapper rules. PHP prepends file://
// to raw paths, so a relative file is not silently anchored to the application.
fn configured_file_path(value: &str) -> Result<PathBuf, KeyError> {
    if value.contains('\0') {
        return Err(KeyError::Invalid);
    }
    let value = if let Some(suffix) = value.strip_prefix("file://") {
        suffix
            .strip_prefix("localhost")
            .filter(|path| path.starts_with('/'))
            .unwrap_or(suffix)
    } else {
        if value.contains("://") {
            return Err(KeyError::Invalid);
        }
        value
    };
    #[cfg(windows)]
    let value = if value.starts_with('/') && value.as_bytes().get(2) == Some(&b':') {
        &value[1..]
    } else {
        value
    };
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(KeyError::Unreadable);
    }
    Ok(path)
}
