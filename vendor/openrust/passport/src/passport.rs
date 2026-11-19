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

pub type TokenEncryptionKey = fn(&openrust::encryption::Encrypter) -> Vec<u8>;

/// Application-local counterpart of Laravel Passport's configuration state.
#[allow(non_snake_case)]
pub struct Passport {
    /// Mirrors Passport::$validateKeyPermissions without process-global state.
    pub validateKeyPermissions: AtomicBool,
    /// Whether a parent scope grants a colon-delimited descendant scope.
    pub withInheritedScopes: AtomicBool,
    pub deviceCodeGrantEnabled: AtomicBool,
    pub clientUuids: AtomicBool,
    pub registersRoutes: AtomicBool,
    pub ignoreCsrfToken: AtomicBool,
    pub decryptsCookies: AtomicBool,
    pub unserializesCookies: AtomicBool,
    cookie_name: RwLock<String>,
    token_key: RwLock<Option<TokenEncryptionKey>>,
    authorization_view: RwLock<Option<crate::http::AuthorizationView>>,
    device_authorization_view: RwLock<Option<crate::http::AuthorizationView>>,
    device_user_code_view: RwLock<Option<crate::http::DeviceUserCodeView>>,
    config: Arc<dyn Repository>,
    paths: Arc<Paths>,
    key_path: RwLock<Option<PathBuf>>,
    database: Arc<DatabaseManager>,
    client_model: RwLock<ClientModelRegistration>,
    pub(crate) token_models: Arc<crate::token_store::TokenModels>,
    scopes: RwLock<std::collections::BTreeMap<String, String>>,
    default_scopes: RwLock<String>,
    access_lifetime: RwLock<crate::GrantLifetime>,
    refresh_lifetime: RwLock<crate::GrantLifetime>,
    personal_lifetime: RwLock<crate::GrantLifetime>,
    client_credentials_lifetime: RwLock<Option<crate::GrantLifetime>>,
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
            scopes: RwLock::new(Default::default()),
            access_lifetime: RwLock::new(crate::GrantLifetime::CalendarYears(1)),
            refresh_lifetime: RwLock::new(crate::GrantLifetime::CalendarYears(1)),
            personal_lifetime: RwLock::new(crate::GrantLifetime::CalendarYears(1)),
            client_credentials_lifetime: RwLock::new(None),
            default_scopes: RwLock::new(String::new()),
            validateKeyPermissions: AtomicBool::new(true),
            withInheritedScopes: AtomicBool::new(false),
            deviceCodeGrantEnabled: AtomicBool::new(true),
            clientUuids: AtomicBool::new(true),
            registersRoutes: AtomicBool::new(true),
            ignoreCsrfToken: AtomicBool::new(false),
            decryptsCookies: AtomicBool::new(true),
            unserializesCookies: AtomicBool::new(false),
            cookie_name: RwLock::new("openrust_token".into()),
            token_key: RwLock::new(None),
            authorization_view: RwLock::new(None),
            device_authorization_view: RwLock::new(None),
            device_user_code_view: RwLock::new(None),
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
    /// Explicit Option retains PHP's combined getter/setter contract.
    pub fn tokensExpireIn(&self, lifetime: Option<crate::GrantLifetime>) -> crate::GrantLifetime {
        let mut value = self
            .access_lifetime
            .write()
            .expect("Passport lifetime lock");
        if let Some(lifetime) = lifetime {
            *value = lifetime;
        }
        *value
    }
    pub fn refreshTokensExpireIn(
        &self,
        lifetime: Option<crate::GrantLifetime>,
    ) -> crate::GrantLifetime {
        let mut value = self
            .refresh_lifetime
            .write()
            .expect("Passport lifetime lock");
        if let Some(lifetime) = lifetime {
            *value = lifetime;
        }
        *value
    }
    pub fn personalAccessTokensExpireIn(
        &self,
        lifetime: Option<crate::GrantLifetime>,
    ) -> crate::GrantLifetime {
        let mut current = self
            .personal_lifetime
            .write()
            .expect("Passport lifetime lock");
        if let Some(lifetime) = lifetime {
            *current = lifetime;
        }
        *current
    }
    pub fn clientCredentialsTokensExpireIn(
        &self,
        lifetime: Option<crate::GrantLifetime>,
    ) -> crate::GrantLifetime {
        let mut current = self
            .client_credentials_lifetime
            .write()
            .expect("Passport lifetime lock");
        if let Some(lifetime) = lifetime {
            *current = Some(lifetime);
        }
        current.unwrap_or_else(|| self.tokensExpireIn(None))
    }
    pub fn deviceAuthorizationView(&self, view: crate::http::AuthorizationView) {
        *self
            .device_authorization_view
            .write()
            .expect("Passport device view lock") = Some(view);
    }
    pub fn deviceUserCodeView(&self, view: crate::http::DeviceUserCodeView) {
        *self
            .device_user_code_view
            .write()
            .expect("Passport device view lock") = Some(view);
    }
    pub(crate) fn device_authorization_view(&self) -> Option<crate::http::AuthorizationView> {
        *self
            .device_authorization_view
            .read()
            .expect("Passport device view lock")
    }
    pub(crate) fn device_user_code_view(&self) -> Option<crate::http::DeviceUserCodeView> {
        *self
            .device_user_code_view
            .read()
            .expect("Passport device view lock")
    }
    pub fn cookie(&self, name: Option<&str>) -> String {
        let mut current = self.cookie_name.write().expect("Passport cookie lock");
        if let Some(name) = name {
            *current = name.into();
        }
        current.clone()
    }
    pub fn ignoreCsrfToken(&self, ignore: bool) {
        self.ignoreCsrfToken
            .store(ignore, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn withCookieEncryption(&self) {
        self.decryptsCookies
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn withoutCookieEncryption(&self) {
        self.decryptsCookies
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn withCookieSerialization(&self) {
        self.unserializesCookies
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn withoutCookieSerialization(&self) {
        self.unserializesCookies
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn encryptTokensUsing(&self, callback: Option<TokenEncryptionKey>) {
        *self.token_key.write().expect("Passport token key lock") = callback;
    }
    pub fn tokenEncryptionKey(
        &self,
        encrypter: &openrust::encryption::Encrypter,
    ) -> zeroize::Zeroizing<Vec<u8>> {
        zeroize::Zeroizing::new(
            self.token_key
                .read()
                .expect("Passport token key lock")
                .map(|f| f(encrypter))
                .unwrap_or_else(|| encrypter.getKey().to_vec()),
        )
    }
    pub fn ignoreRoutes(&self) {
        self.registersRoutes
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn authorizationView(&self, view: crate::http::AuthorizationView) {
        *self.authorization_view.write().expect("Passport view lock") = Some(view);
    }
    pub(crate) fn authorization_view(&self) -> Option<crate::http::AuthorizationView> {
        *self.authorization_view.read().expect("Passport view lock")
    }
    pub fn tokensCan(&self, scopes: std::collections::BTreeMap<String, String>) {
        *self.scopes.write().expect("Passport scopes lock") = scopes;
    }
    pub fn setDefaultScope(&self, scopes: Vec<String>) {
        self.defaultScopes(Some(scopes));
    }
    pub fn scopes(&self) -> Vec<crate::Scope> {
        self.scopes
            .read()
            .expect("Passport scopes lock")
            .iter()
            .map(|(id, description)| crate::Scope {
                id: id.clone(),
                description: description.clone(),
            })
            .collect()
    }
    pub fn defaultScopes(&self, scopes: Option<Vec<String>>) -> Vec<String> {
        let mut value = self.default_scopes.write().expect("Passport scopes lock");
        if let Some(scopes) = scopes {
            *value = scopes.join(" ");
        }
        // PHP treats both an empty string and "0" as false.
        if value.is_empty() || *value == "0" {
            Vec::new()
        } else {
            value.split(' ').map(str::to_owned).collect()
        }
    }
    pub fn validScopes(&self, scopes: &[String]) -> Vec<String> {
        let configured = self.scopes.read().expect("Passport scopes lock");
        let mut seen = std::collections::HashSet::new();
        scopes
            .iter()
            .filter(|id| configured.contains_key(*id) && seen.insert(*id))
            .cloned()
            .collect()
    }
    pub fn scopeIds(&self) -> Vec<String> {
        self.scopes
            .read()
            .expect("Passport scopes lock")
            .keys()
            .cloned()
            .collect()
    }
    pub fn hasScope(&self, id: &str) -> bool {
        id == "*"
            || self
                .scopes
                .read()
                .expect("Passport scopes lock")
                .contains_key(id)
    }
    pub fn scopesFor(&self, ids: &[String]) -> Vec<crate::Scope> {
        let configured = self.scopes.read().expect("Passport scopes lock");
        ids.iter()
            .filter_map(|id| {
                configured
                    .get(id)
                    .map(|description| crate::Scope::new(id, description))
            })
            .collect()
    }
    pub(crate) fn default_scopes(&self) -> Vec<String> {
        self.defaultScopes(None)
            .join(" ")
            .trim_matches([' ', '\t', '\n', '\r', '\0', '\u{b}'])
            .split(' ')
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .collect()
    }
    pub(crate) fn has_scope(&self, id: &str) -> bool {
        self.hasScope(id)
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
