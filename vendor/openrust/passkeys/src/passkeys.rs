use crate::{
    Passkey, PasskeyError, PasskeyModel, PasskeyRecord, PasskeyResult, WebAuthn,
    contracts::{PasskeyUser, scalar},
};
use openrust::{
    contracts::{auth::Authenticatable, config::Repository},
    database::Value,
    http::{HttpError, HttpFuture, Request},
};
use std::{
    any::Any,
    marker::PhantomData,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

pub type AuthorizeLoginCallback = dyn for<'a> Fn(
        &'a mut Request,
        &'a dyn Authenticatable,
        &'a PasskeyRecord,
    ) -> HttpFuture<'a, Result<bool, HttpError>>
    + Send
    + Sync;
pub struct Passkeys {
    pub(crate) config: Arc<dyn Repository>,
    passkey_model: RwLock<Arc<dyn crate::store::PasskeyStore>>,
    user_model: RwLock<Option<Arc<dyn UserModel>>>,
    authorize: RwLock<Option<Arc<AuthorizeLoginCallback>>>,
    routes: AtomicBool,
}
impl Passkeys {
    pub fn new(config: Arc<dyn Repository>) -> Self {
        Self {
            config,
            passkey_model: RwLock::new(Arc::new(crate::store::Store::<Passkey>::default())),
            user_model: RwLock::new(None),
            authorize: RwLock::new(None),
            routes: AtomicBool::new(true),
        }
    }
    pub fn ignore_routes(&self) {
        self.routes.store(false, Ordering::Relaxed);
    }
    pub fn should_register_routes(&self) -> bool {
        self.routes.load(Ordering::Relaxed)
    }
    pub fn relying_party_id(&self) -> PasskeyResult<String> {
        self.config
            .get("passkeys.relying_party_id")
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or(PasskeyError::Configuration("missing RP ID"))
    }
    pub fn relying_party_name(&self) -> PasskeyResult<String> {
        self.relying_party_id()
    }
    pub fn allowed_origins(&self) -> PasskeyResult<Vec<String>> {
        let values = self
            .config
            .get("passkeys.allowed_origins")
            .ok_or(PasskeyError::Configuration("missing origins"))?;
        let values = values
            .as_array()
            .ok_or(PasskeyError::Configuration("origins must be an array"))?;
        let origins: Vec<_> = values
            .iter()
            .filter_map(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned))
            .collect();
        if origins.is_empty() {
            return Err(PasskeyError::Configuration(
                "at least one allowed origin is required",
            ));
        }
        Ok(origins)
    }
    pub fn timeout(&self) -> PasskeyResult<u64> {
        let timeout = self
            .config
            .get("passkeys.timeout")
            .unwrap_or(60000.into())
            .as_u64()
            .filter(|v| *v > 0 && *v <= u64::from(u32::MAX))
            .ok_or(PasskeyError::Configuration(
                "timeout must be a positive integer",
            ))?;
        Ok(timeout)
    }
    pub fn webauthn(&self) -> PasskeyResult<WebAuthn> {
        WebAuthn::new(
            &self.relying_party_id()?,
            &self.allowed_origins()?,
            self.timeout()?,
        )
    }
    pub fn use_passkey_model<M: PasskeyModel>(&self) {
        *self.passkey_model.write().expect("Passkeys model lock") =
            Arc::new(crate::store::Store::<M>::default());
    }
    pub fn passkey_model(&self) -> &'static str {
        self.passkey_store().name()
    }
    pub(crate) fn passkey_store(&self) -> Arc<dyn crate::store::PasskeyStore> {
        self.passkey_model
            .read()
            .expect("Passkeys model lock")
            .clone()
    }
    pub fn user_model(&self) -> Result<&'static str, HttpError> {
        Ok(self.model()?.name())
    }
    pub fn use_user_model<T: PasskeyUser + Clone>(&self) {
        *self.user_model.write().expect("Passkeys model lock") =
            Some(Arc::new(Adapter::<T>(PhantomData)));
    }
    pub fn authorize_login_using(&self, callback: Option<Arc<AuthorizeLoginCallback>>) {
        *self.authorize.write().expect("Passkeys callback lock") = callback;
    }
    pub async fn allows_login(
        &self,
        request: &mut Request,
        user: &dyn Authenticatable,
        passkey: &PasskeyRecord,
    ) -> Result<bool, HttpError> {
        let callback = self
            .authorize
            .read()
            .expect("Passkeys callback lock")
            .clone();
        match callback {
            Some(callback) => callback(request, user, passkey).await,
            None => Ok(true),
        }
    }
    pub(crate) fn account(&self, user: &dyn Authenticatable) -> Result<Account, HttpError> {
        let model = self.model()?;
        let secret = self
            .config
            .get("passkeys.user_handle_secret")
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or_else(|| {
                crate::internal(PasskeyError::Configuration("missing user handle secret"))
            })?;
        model.account(user, &secret)
    }
    pub(crate) fn model(&self) -> Result<Arc<dyn UserModel>, HttpError> {
        self.user_model
            .read()
            .expect("Passkeys model lock")
            .clone()
            .ok_or_else(|| {
                crate::internal(PasskeyError::Configuration(
                    "User model must implement the PasskeyUser contract and be registered",
                ))
            })
    }
    pub(crate) fn string(&self, key: &str, default: &str) -> String {
        self.config
            .get(key)
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| default.into())
    }
}
pub(crate) struct Account {
    pub key: Value,
    pub handle: String,
    pub username: String,
    pub display_name: String,
}
pub(crate) trait UserModel: Send + Sync {
    fn name(&self) -> &'static str;
    fn clone_user(&self, user: &dyn Authenticatable) -> Option<Box<dyn Authenticatable>>;
    fn account(&self, user: &dyn Authenticatable, secret: &str) -> Result<Account, HttpError>;
    fn find(
        &self,
        key: Value,
    ) -> HttpFuture<'_, Result<Option<Box<dyn Authenticatable>>, HttpError>>;
}
struct Adapter<T>(PhantomData<fn() -> T>);
impl<T: PasskeyUser + Clone> UserModel for Adapter<T> {
    fn name(&self) -> &'static str {
        std::any::type_name::<T>()
    }
    fn clone_user(&self, user: &dyn Authenticatable) -> Option<Box<dyn Authenticatable>> {
        (user as &dyn Any)
            .downcast_ref::<T>()
            .cloned()
            .map(|u| Box::new(u) as Box<dyn Authenticatable>)
    }
    fn account(&self, user: &dyn Authenticatable, secret: &str) -> Result<Account, HttpError> {
        let user = (user as &dyn Any).downcast_ref::<T>().ok_or_else(|| {
            crate::internal(PasskeyError::Configuration(
                "User model must implement the registered PasskeyUser contract",
            ))
        })?;
        let key = user
            .get_key()
            .filter(|key| scalar(key).is_some())
            .cloned()
            .ok_or_else(|| crate::internal(PasskeyError::Format))?;
        Ok(Account {
            key,
            handle: user
                .get_passkey_user_handle(secret)
                .map_err(crate::internal)?,
            username: user.get_passkey_username().map_err(crate::internal)?,
            display_name: user.get_passkey_display_name().map_err(crate::internal)?,
        })
    }
    fn find(
        &self,
        key: Value,
    ) -> HttpFuture<'_, Result<Option<Box<dyn Authenticatable>>, HttpError>> {
        Box::pin(async move {
            Ok(T::find(key)
                .await
                .map_err(crate::internal)?
                .map(|u| Box::new(u) as Box<dyn Authenticatable>))
        })
    }
}
