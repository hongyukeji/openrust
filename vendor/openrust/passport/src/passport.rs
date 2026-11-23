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

/// Application-local counterpart of the framework Passport's configuration state.
pub struct Passport {
    /// Mirrors Passport::$validateKeyPermissions without process-global state.
    pub validate_key_permissions: AtomicBool,
    /// Whether a parent scope grants a colon-delimited descendant scope.
    pub with_inherited_scopes: AtomicBool,
    pub device_code_grant_enabled: AtomicBool,
    pub password_grant_enabled: AtomicBool,
    pub implicit_grant_enabled: AtomicBool,
    password_resolvers:
        RwLock<std::collections::HashMap<String, crate::bridge::PasswordGrantResolver>>,
    pub client_uuids: AtomicBool,
    pub registers_routes: AtomicBool,
    pub ignore_csrf_token: AtomicBool,
    pub decrypts_cookies: AtomicBool,
    pub unserializes_cookies: AtomicBool,
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
    pub(crate) grant_models: crate::grant_store::GrantModels,
    pub(crate) token_models: Arc<crate::token_store::TokenModels>,
    scopes: RwLock<std::collections::BTreeMap<String, String>>,
    default_scopes: RwLock<String>,
    access_lifetime: RwLock<crate::GrantLifetime>,
    refresh_lifetime: RwLock<crate::GrantLifetime>,
    personal_lifetime: RwLock<crate::GrantLifetime>,
    client_credentials_lifetime: RwLock<Option<crate::GrantLifetime>>,
}
impl Passport {
    pub(crate) fn new(
        config: Arc<dyn Repository>,
        paths: Arc<Paths>,
        database: Arc<DatabaseManager>,
    ) -> Self {
        Self {
            grant_models: crate::grant_store::GrantModels::new(database.clone(), config.clone()),
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
            validate_key_permissions: AtomicBool::new(true),
            with_inherited_scopes: AtomicBool::new(false),
            device_code_grant_enabled: AtomicBool::new(true),
            password_grant_enabled: AtomicBool::new(false),
            implicit_grant_enabled: AtomicBool::new(false),
            password_resolvers: RwLock::new(Default::default()),
            client_uuids: AtomicBool::new(true),
            registers_routes: AtomicBool::new(true),
            ignore_csrf_token: AtomicBool::new(false),
            decrypts_cookies: AtomicBool::new(true),
            unserializes_cookies: AtomicBool::new(false),
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
    pub fn tokens_expire_in(&self, lifetime: Option<crate::GrantLifetime>) -> crate::GrantLifetime {
        let mut value = self
            .access_lifetime
            .write()
            .expect("Passport lifetime lock");
        if let Some(lifetime) = lifetime {
            *value = lifetime;
        }
        *value
    }
    pub fn refresh_tokens_expire_in(
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
    pub fn personal_access_tokens_expire_in(
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
    pub fn client_credentials_tokens_expire_in(
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
        current.unwrap_or_else(|| self.tokens_expire_in(None))
    }
    pub fn device_authorization_view(&self, view: crate::http::AuthorizationView) {
        *self
            .device_authorization_view
            .write()
            .expect("Passport device view lock") = Some(view);
    }
    pub fn device_user_code_view(&self, view: crate::http::DeviceUserCodeView) {
        *self
            .device_user_code_view
            .write()
            .expect("Passport device view lock") = Some(view);
    }
    pub(crate) fn device_authorization_view_response(
        &self,
    ) -> Option<crate::http::AuthorizationView> {
        *self
            .device_authorization_view
            .read()
            .expect("Passport device view lock")
    }
    pub(crate) fn device_user_code_view_response(&self) -> Option<crate::http::DeviceUserCodeView> {
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
    pub fn ignore_csrf_token(&self, ignore: bool) {
        self.ignore_csrf_token
            .store(ignore, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn with_cookie_encryption(&self) {
        self.decrypts_cookies
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn without_cookie_encryption(&self) {
        self.decrypts_cookies
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn with_cookie_serialization(&self) {
        self.unserializes_cookies
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn without_cookie_serialization(&self) {
        self.unserializes_cookies
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn encrypt_tokens_using(&self, callback: Option<TokenEncryptionKey>) {
        *self.token_key.write().expect("Passport token key lock") = callback;
    }
    pub fn token_encryption_key(
        &self,
        encrypter: &openrust::encryption::Encrypter,
    ) -> zeroize::Zeroizing<Vec<u8>> {
        zeroize::Zeroizing::new(
            self.token_key
                .read()
                .expect("Passport token key lock")
                .map(|f| f(encrypter))
                .unwrap_or_else(|| encrypter.get_key().to_vec()),
        )
    }
    pub fn enable_password_grant(&self) {
        self.password_grant_enabled
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn enable_implicit_grant(&self) {
        self.implicit_grant_enabled
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    /// Native adapter for PHP's model-level findAndValidateForPassport hook.
    pub fn find_and_validate_for_passport_using(
        &self,
        provider: &str,
        resolver: crate::bridge::PasswordGrantResolver,
    ) {
        self.password_resolvers
            .write()
            .expect("Passport password resolver lock")
            .insert(provider.into(), resolver);
    }
    pub(crate) fn password_resolver(
        &self,
        provider: &str,
    ) -> Option<crate::bridge::PasswordGrantResolver> {
        self.password_resolvers
            .read()
            .expect("Passport password resolver lock")
            .get(provider)
            .copied()
    }
    pub fn ignore_routes(&self) {
        self.registers_routes
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn authorization_view(&self, view: crate::http::AuthorizationView) {
        *self.authorization_view.write().expect("Passport view lock") = Some(view);
    }
    pub(crate) fn authorization_view_response(&self) -> Option<crate::http::AuthorizationView> {
        *self.authorization_view.read().expect("Passport view lock")
    }
    pub fn tokens_can(&self, scopes: std::collections::BTreeMap<String, String>) {
        *self.scopes.write().expect("Passport scopes lock") = scopes;
    }
    pub fn set_default_scope(&self, scopes: Vec<String>) {
        self.default_scopes(Some(scopes));
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
    pub fn default_scopes(&self, scopes: Option<Vec<String>>) -> Vec<String> {
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
    pub fn valid_scopes(&self, scopes: &[String]) -> Vec<String> {
        let configured = self.scopes.read().expect("Passport scopes lock");
        let mut seen = std::collections::HashSet::new();
        scopes
            .iter()
            .filter(|id| configured.contains_key(*id) && seen.insert(*id))
            .cloned()
            .collect()
    }
    pub fn scope_ids(&self) -> Vec<String> {
        self.scopes
            .read()
            .expect("Passport scopes lock")
            .keys()
            .cloned()
            .collect()
    }
    pub fn has_scope(&self, id: &str) -> bool {
        id == "*"
            || self
                .scopes
                .read()
                .expect("Passport scopes lock")
                .contains_key(id)
    }
    pub fn scopes_for(&self, ids: &[String]) -> Vec<crate::Scope> {
        let configured = self.scopes.read().expect("Passport scopes lock");
        ids.iter()
            .filter_map(|id| {
                configured
                    .get(id)
                    .map(|description| crate::Scope::new(id, description))
            })
            .collect()
    }
    pub(crate) fn configured_default_scopes(&self) -> Vec<String> {
        self.default_scopes(None)
            .join(" ")
            .trim_matches([' ', '\t', '\n', '\r', '\0', '\u{b}'])
            .split(' ')
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// Selects a compiled client type before resolving the lifecycle's repository.
    pub fn use_client_model<M: ClientModel>(&self) {
        *self
            .client_model
            .write()
            .expect("Passport client model lock poisoned") = ClientModelRegistration {
            name: std::any::type_name::<M>(),
            factory: client_store::<M>,
        };
    }
    pub fn use_auth_code_model<M: openrust::database::eloquent::Model>(&self) {
        self.grant_models.use_auth::<M>();
    }
    pub fn use_device_code_model<M: openrust::database::eloquent::Model>(&self) {
        self.grant_models.use_device::<M>();
    }
    pub fn auth_code_model(&self) -> &'static str {
        self.grant_models.auth_name()
    }
    pub fn device_code_model(&self) -> &'static str {
        self.grant_models.device_name()
    }
    pub fn use_token_model<M: crate::TokenModel>(&self) {
        self.token_models.use_token::<M>();
    }
    pub fn use_refresh_token_model<M: crate::RefreshTokenModel>(&self) {
        self.token_models.use_refresh::<M>();
    }
    pub fn token_model(&self) -> &'static str {
        self.token_models.token_name()
    }
    pub fn refresh_token_model(&self) -> &'static str {
        self.token_models.refresh_name()
    }
    pub fn client_model(&self) -> &'static str {
        self.client_model
            .read()
            .expect("Passport client model lock poisoned")
            .name
    }
    /// Creates an explicitly owned repository; DI scopes one instance per lifecycle.
    pub fn client_repository(&self) -> ClientRepository {
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
    pub fn load_keys_from(&self, path: impl AsRef<Path>) {
        *self
            .key_path
            .write()
            .expect("Passport key path lock poisoned") = Some(path.as_ref().to_owned());
    }
    /// Resolves keys under the configured application storage directory by default.
    pub fn key_path(&self, file: &str) -> PathBuf {
        let file = file.trim_start_matches(['/', '\\']);
        match self
            .key_path
            .read()
            .expect("Passport key path lock poisoned")
            .as_ref()
        {
            Some(path) => path.join(file),
            None => self.paths.storage_path(file),
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
            self.key_path(&format!("oauth-{kind}.key"))
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
            self.validate_key_permissions
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
