use crate::{
    Passkey, PasskeyError, PasskeyModel, PasskeyRecord, PasskeyResult, WebAuthn,
    contracts::{PasskeyUser, scalar},
};
use openrust::{
    contracts::{auth::Authenticatable, config::Repository},
    database::{DatabaseError, DatabaseResult, Transaction, Value, eloquent::Model},
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
    /*
     * 调用方持有用户写锁并负责提交或回滚；仅清理已注册用户模型的真实主键。
     * 包括隐藏及软删除凭据，逐模型执行物理删除事件，不跨连接假装原子清理。
     */
    pub async fn purge_user_in_transaction<M: Model + Authenticatable>(
        &self,
        transaction: &mut Transaction,
        user: &M,
    ) -> DatabaseResult<u64> {
        let registered = self.user_model.read().expect("Passkeys model lock").clone();
        if registered.is_none_or(|model| model.user_type_id() != std::any::TypeId::of::<M>()) {
            return Err(DatabaseError::InvalidQuery(
                "通行密钥用户模型与注销模型不一致",
            ));
        }
        if !transaction.belongs_to(&user.get_connection()?) {
            return Err(DatabaseError::InvalidQuery(
                "通行密钥注销必须使用用户连接的事务",
            ));
        }
        let key = user
            .get_key()
            .filter(|key| scalar(key).is_some())
            .cloned()
            .ok_or(DatabaseError::InvalidQuery("通行密钥用户缺少真实主键"))?;
        if !user.state().exists() || user.get_raw_original(user.get_key_name()) != Some(&key) {
            return Err(DatabaseError::InvalidQuery(
                "通行密钥注销不能使用未持久化或改写主键的用户",
            ));
        }
        self.passkey_store()
            .purge_in_transaction(transaction, key)
            .await
    }
    pub async fn for_user(
        &self,
        user: &dyn Authenticatable,
    ) -> Result<Vec<PasskeyRecord>, HttpError> {
        self.passkey_store()
            .records("user_id", self.account(user)?.key)
            .await
            .map_err(crate::internal)
    }
    /// 调用方持有用户锁时，从同一事务读取所选模型的凭据，避免另取连接或绕过作用域。
    pub async fn for_user_in_transaction(
        &self,
        user: &dyn Authenticatable,
        transaction: &mut Transaction,
    ) -> Result<Vec<PasskeyRecord>, HttpError> {
        self.validate_user_transaction(user, transaction)?;
        self.passkey_store()
            .records_in_transaction(transaction, "user_id", self.account(user)?.key)
            .await
            .map_err(crate::internal)
    }
    pub async fn find_owned_in_transaction(
        &self,
        user: &dyn Authenticatable,
        transaction: &mut Transaction,
        key: Value,
    ) -> Result<Option<PasskeyRecord>, HttpError> {
        self.validate_user_transaction(user, transaction)?;
        self.passkey_store()
            .owned_in_transaction(transaction, key, self.account(user)?.key)
            .await
            .map_err(crate::internal)
    }
    /// 解析所选凭据模型的注册用户；返回值仍须经原生WebAuthn验证后才能用于登录。
    pub async fn user_for(
        &self,
        record: &PasskeyRecord,
    ) -> Result<Option<Box<dyn Authenticatable>>, HttpError> {
        let key = record
            .get_attribute_value("user_id")
            .await
            .map_err(crate::internal)?
            .ok_or(HttpError::NotFound)?;
        self.model()?.find(key).await
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
    pub(crate) fn validate_user_transaction(
        &self,
        user: &dyn Authenticatable,
        transaction: &Transaction,
    ) -> Result<(), HttpError> {
        self.model()?.validate_transaction(user, transaction)
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
    fn user_type_id(&self) -> std::any::TypeId;
    fn validate_transaction(
        &self,
        user: &dyn Authenticatable,
        transaction: &Transaction,
    ) -> Result<(), HttpError>;
    fn name(&self) -> &'static str;
    fn clone_user(&self, user: &dyn Authenticatable) -> Option<Box<dyn Authenticatable>>;
    fn account(&self, user: &dyn Authenticatable, secret: &str) -> Result<Account, HttpError>;
    fn find_in_transaction<'a>(
        &'a self,
        transaction: &'a mut Transaction,
        key: Value,
    ) -> HttpFuture<'a, Result<Option<Box<dyn Authenticatable>>, HttpError>>;
    fn find(
        &self,
        key: Value,
    ) -> HttpFuture<'_, Result<Option<Box<dyn Authenticatable>>, HttpError>>;
}
struct Adapter<T>(PhantomData<fn() -> T>);
impl<T: PasskeyUser + Clone> UserModel for Adapter<T> {
    fn user_type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<T>()
    }
    fn validate_transaction(
        &self,
        user: &dyn Authenticatable,
        transaction: &Transaction,
    ) -> Result<(), HttpError> {
        let user = (user as &dyn Any).downcast_ref::<T>().ok_or_else(|| {
            crate::internal(DatabaseError::InvalidQuery("通行密钥用户模型未注册"))
        })?;
        if !transaction.belongs_to(&user.get_connection().map_err(crate::internal)?)
            || !user.state().exists()
            || user.get_key().is_none()
            || user.get_key() != user.get_raw_original(user.get_key_name())
        {
            return Err(crate::internal(DatabaseError::InvalidQuery(
                "通行密钥事务必须使用已持久化用户的同一连接与真实主键",
            )));
        }
        Ok(())
    }
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
    fn find_in_transaction<'a>(
        &'a self,
        transaction: &'a mut Transaction,
        key: Value,
    ) -> HttpFuture<'a, Result<Option<Box<dyn Authenticatable>>, HttpError>> {
        Box::pin(async move {
            let connection = T::from_state(Default::default())
                .get_connection()
                .map_err(crate::internal)?;
            if !transaction.belongs_to(&connection) {
                return self.find(key).await;
            }
            let mut query = T::query_on(&connection).use_write_pdo();
            query.where_key(key);
            Ok(query
                .limit(1)
                .get_in_transaction(transaction)
                .await
                .map_err(crate::internal)?
                .into_iter()
                .next()
                .map(|user| Box::new(user) as Box<dyn Authenticatable>))
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
