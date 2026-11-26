//! 浏览器设备索引；撤销标记与用户安全变更共用事务，不依赖删除会话行。
use openrust::{
    auth::{AuthError, AuthResult},
    contracts::{
        auth::Authenticatable, config::Repository, encryption::Encrypter, session::Session as _,
    },
    database::{Connection, Row, Transaction, Value, eloquent::Model, query::Record},
    http::HttpError,
    support::facades::Date,
};
use serde::Serialize;
use std::sync::Arc;

pub(crate) const TABLE: &str = "user_browser_sessions";

#[derive(Clone)]
pub(crate) struct Policy {
    pub guard: String,
    pub config: Arc<dyn Repository>,
    encrypter: Arc<dyn Encrypter>,
    lifetime: i64,
}

/// 可公开的本人设备摘要；不包含原始会话ID、CSRF值、指纹或加密载荷。
#[derive(Clone, Debug, Serialize)]
pub struct BrowserSession {
    pub id: String,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub last_active: i64,
    pub is_current_device: bool,
}

fn now() -> AuthResult<i64> {
    Ok(Date::now(None)
        .map_err(|error| AuthError::Backend(Box::new(error)))?
        .as_date_time()
        .timestamp()
        .as_second())
}
fn hash(bytes: &[u8]) -> String {
    aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn principal<M: Model>(user: &M, guard: &str) -> AuthResult<String> {
    let key = openrust::auth::AuthIdentifier::try_from(
        user.get_key().ok_or(AuthError::InvalidIdentity)?,
    )?;
    let connection = user.get_connection()?;
    Ok(hash(&serde_json::to_vec(&(
        "browser-device:v1",
        std::any::type_name::<M>(),
        connection.get_name(),
        connection.get_database_name(),
        connection.get_table_prefix(),
        connection.get_config("search_path"),
        user.get_table(),
        user.get_key_name(),
        key,
        guard,
    ))?))
}
fn device_id(principal: &str, raw: &str) -> AuthResult<String> {
    if raw.len() != 40 || !raw.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(AuthError::Configuration("设备会话ID不符合原生格式"));
    }
    Ok(hash(&serde_json::to_vec(&(
        "browser-device-id:v1",
        principal,
        raw,
    ))?))
}
fn integer(row: &Row, key: &str) -> AuthResult<i64> {
    match row.get(key) {
        Some(Value::Integer(value)) => Ok(*value),
        Some(Value::Unsigned(value)) => {
            i64::try_from(*value).map_err(|_| AuthError::Configuration("设备时间超出支持范围"))
        }
        _ => Err(AuthError::Configuration("设备记录时间字段无效")),
    }
}
fn text(row: &Row, key: &str) -> AuthResult<String> {
    match row.get(key) {
        Some(Value::Text(value)) => Ok(value.clone()),
        _ => Err(AuthError::Configuration("设备记录文本字段无效")),
    }
}
fn optional_text(row: &Row, key: &str) -> AuthResult<Option<String>> {
    match row.get(key) {
        Some(Value::Null) | None => Ok(None),
        _ => text(row, key).map(Some),
    }
}
fn revoked(row: &Row) -> AuthResult<bool> {
    match row.get("revoked_at") {
        Some(Value::Null) => Ok(false),
        Some(_) => integer(row, "revoked_at").map(|_| true),
        None => Err(AuthError::Configuration("设备记录缺少撤销状态")),
    }
}

impl Policy {
    pub fn new(
        config: Arc<dyn Repository>,
        encrypter: Arc<dyn Encrypter>,
        guard: String,
    ) -> AuthResult<Self> {
        let lifetime = config
            .get("session.lifetime")
            .unwrap_or(serde_json::json!(120))
            .as_i64()
            .and_then(|minutes| minutes.checked_mul(60))
            .filter(|seconds| *seconds > 0)
            .ok_or(AuthError::Configuration("设备管理需要正整数会话分钟数"))?;
        Ok(Self {
            guard,
            config,
            encrypter,
            lifetime,
        })
    }
    fn expiry(&self, at: i64) -> AuthResult<i64> {
        at.checked_add(self.lifetime)
            .ok_or(AuthError::Configuration("设备期限超出支持范围"))
    }
    fn query<'a>(
        &self,
        user: &impl Model,
        connection: &'a Connection,
    ) -> AuthResult<openrust::database::query::Builder<'a>> {
        Ok(connection.table(TABLE).use_write_pdo().r#where(
            "owner_hash",
            "=",
            principal(user, &self.guard)?,
        ))
    }
    /// Guard调用时核对持久化撤销点。无会话登录值时只允许原生remember继续验证。
    pub async fn accepts<M: Model>(
        &self,
        user: &M,
        session: &openrust::session::Store,
    ) -> AuthResult<bool> {
        let owner = principal(user, &self.guard)?;
        let id = device_id(&owner, session.get_id())?;
        let connection = user.get_connection()?;
        let row = self
            .query(user, &connection)?
            .r#where("id", "=", id)
            .first()
            .await?;
        let remembered = session.get(&format!("login_{}", self.guard)).is_none();
        match row {
            None => Ok(remembered),
            Some(row) => {
                Ok(!revoked(&row)? && (remembered || integer(&row, "expires_at")? > now()?))
            }
        }
    }
    fn fields(&self, context: &Snapshot, at: i64) -> AuthResult<Record> {
        Ok([
            ("last_seen".into(), at.into()),
            ("expires_at".into(), self.expiry(at)?.into()),
            (
                "ip_address".into(),
                context
                    .ip_address
                    .clone()
                    .map(Value::from)
                    .unwrap_or(Value::Null),
            ),
            (
                "user_agent".into(),
                context
                    .user_agent
                    .as_ref()
                    .map(|s| s.chars().take(500).collect::<String>().into())
                    .unwrap_or(Value::Null),
            ),
            (
                "session_id".into(),
                self.encrypter
                    .encrypt_string(context.session_id.as_bytes())
                    .map_err(|error| AuthError::Backend(Box::new(error)))?
                    .into(),
            ),
        ]
        .into())
    }

    /// 普通同ID请求只条件更新已有索引，不取得用户写锁，也不能插入或恢复撤销记录。
    async fn touch_existing<M: Model>(&self, user: &M, context: &Snapshot) -> AuthResult<()> {
        let connection = user.get_connection()?;
        let owner = principal(user, &self.guard)?;
        let id = device_id(&owner, &context.session_id)?;
        let at = now()?;
        let query = || {
            connection
                .table(TABLE)
                .use_write_pdo()
                .r#where("owner_hash", "=", owner.clone())
                .r#where("id", "=", id.clone())
                .where_null("revoked_at")
                .r#where("expires_at", ">", at)
        };
        if query().update(self.fields(context, at)?).await? == 0 && !query().exists().await? {
            return Err(HttpError::Unauthenticated.into());
        }
        Ok(())
    }

    /// 调用者持有原生用户写锁；不得用upsert清除已存在的撤销标记。
    async fn track<M: Model + Authenticatable>(
        &self,
        user: &M,
        tx: &mut Transaction,
        context: &Snapshot,
    ) -> AuthResult<()> {
        let owner = principal(user, &self.guard)?;
        let id = device_id(&owner, &context.session_id)?;
        let at = now()?;
        let existing = tx
            .table(TABLE)
            .r#where("owner_hash", "=", owner.clone())
            .r#where("id", "=", id.clone())
            .first()
            .await?;
        let mut fields = self.fields(context, at)?;
        if let Some(row) = existing {
            if revoked(&row)? {
                return Err(HttpError::Unauthenticated.into());
            }
            tx.table(TABLE)
                .r#where("owner_hash", "=", owner)
                .r#where("id", "=", id)
                .where_null("revoked_at")
                .update(fields)
                .await?;
        } else {
            fields.insert("id".into(), id.into());
            fields.insert("owner_hash".into(), owner.into());

            fields.insert("revoked_at".into(), Value::Null);
            tx.table(TABLE).insert(&[fields]).await?;
        }
        Ok(())
    }
    /// 在敏感操作的共同事务内再次检查Cookie设备，防止已取得的确认状态越过撤销点。
    pub async fn check_in_transaction<M: Model>(
        &self,
        user: &M,
        tx: &mut Transaction,
        raw: &str,
    ) -> AuthResult<()> {
        let owner = principal(user, &self.guard)?;
        let id = device_id(&owner, raw)?;
        let row = tx
            .table(TABLE)
            .r#where("owner_hash", "=", owner)
            .r#where("id", "=", id)
            .first()
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        if revoked(&row)? || integer(&row, "expires_at")? <= now()? {
            return Err(HttpError::Unauthenticated.into());
        }
        Ok(())
    }
    pub async fn list<M: Model + Authenticatable>(
        &self,
        user: &M,
        manager: &openrust::session::SessionManager,
        current: Option<&str>,
    ) -> AuthResult<Vec<BrowserSession>> {
        let owner = principal(user, &self.guard)?;
        let connection = user.get_connection()?;
        let rows = self
            .query(user, &connection)?
            .where_null("revoked_at")
            .r#where("expires_at", ">", now()?)
            .order_by_desc("last_seen")
            .order_by("id", "asc")
            .get()
            .await?;
        let mut devices = Vec::new();
        for row in rows {
            let raw = String::from_utf8(
                self.encrypter
                    .decrypt_string(&text(&row, "session_id")?)
                    .map_err(|error| AuthError::Backend(Box::new(error)))?,
            )
            .map_err(|_| AuthError::Configuration("设备会话ID编码无效"))?;
            let id = text(&row, "id")?;
            if device_id(&owner, &raw)? != id {
                return Err(AuthError::Configuration("设备记录身份不一致"));
            }
            let attributes = manager
                .read_device_session(&raw)
                .await
                .map_err(|error| AuthError::Backend(Box::new(error)))?;
            if !openrust::auth::SessionGuard::session_belongs_to_user(
                self.config.as_ref(),
                &self.guard,
                user,
                &attributes,
            )? {
                continue;
            }
            devices.push(BrowserSession {
                id,
                ip_address: optional_text(&row, "ip_address")?,
                user_agent: optional_text(&row, "user_agent")?,
                last_active: integer(&row, "last_seen")?,
                is_current_device: current == Some(raw.as_str()),
            });
        }
        Ok(devices)
    }
    pub fn is_current<M: Model>(&self, user: &M, raw: &str, id: &str) -> AuthResult<bool> {
        Ok(device_id(&principal(user, &self.guard)?, raw)? == id)
    }
    /// 只标记本人设备；物理会话写回不能撤销这个拒绝点。共享remember token须由调用方同时清除。
    pub async fn revoke<M: Model>(
        &self,
        user: &M,
        tx: &mut Transaction,
        id: &str,
    ) -> AuthResult<bool> {
        if id.len() != 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Ok(false);
        }
        let owner = principal(user, &self.guard)?;
        let Some(row) = tx
            .table(TABLE)
            .r#where("owner_hash", "=", owner.clone())
            .r#where("id", "=", id)
            .first()
            .await?
        else {
            return Ok(false);
        };
        if !revoked(&row)? {
            tx.table(TABLE)
                .r#where("owner_hash", "=", owner)
                .r#where("id", "=", id)
                .where_null("revoked_at")
                .update([("revoked_at".into(), now()?.into())].into())
                .await?;
        }
        Ok(true)
    }
    pub async fn revoke_others<M: Model>(
        &self,
        user: &M,
        tx: &mut Transaction,
        current: Option<&str>,
    ) -> AuthResult<()> {
        let owner = principal(user, &self.guard)?;
        let mut query = tx
            .table(TABLE)
            .r#where("owner_hash", "=", owner.clone())
            .where_null("revoked_at");
        if let Some(raw) = current {
            query = query.r#where("id", "!=", device_id(&owner, raw)?);
        }
        query
            .update([("revoked_at".into(), now()?.into())].into())
            .await?;
        Ok(())
    }
}

struct Registration {
    model: std::any::TypeId,
    policy: Arc<Policy>,
}

pub(super) fn enabled(config: &dyn Repository) -> Result<bool, openrust::config::ConfigError> {
    super::configuration::boolean(config, "user.devices.enabled", true)
}

pub(super) fn policy() -> AuthResult<Option<Arc<Policy>>> {
    use openrust::{
        contracts::{container::Container as _, foundation::Application as _},
        support::facades::{App, Facade as _},
    };
    let app = App::get_facade_application()?.ok_or(AuthError::Configuration("设备管理所属应用"))?;
    if !app
        .container()
        .has(openrust::container::ServiceKey::typed::<Registration>())
    {
        return Ok(None);
    }
    Ok(Some(app.container().make::<Registration>()?.policy.clone()))
}

/// 显式安装的套件才注册设备策略；启动阶段验证驱动能力，不访问尚未迁移的数据表。
pub(super) fn configure<M>(
    app: &openrust::foundation::Application,
) -> openrust::support::ProviderResult
where
    M: openrust::sanctum::HasApiTokens + Clone,
{
    use openrust::{
        auth::AuthManager,
        contracts::{container::Container as _, foundation::Application as _},
        session::SessionManager,
    };
    let config = app.container().make::<dyn Repository>()?;
    if !enabled(config.as_ref())? {
        return Ok(());
    }
    if app
        .container()
        .has(openrust::container::ServiceKey::typed::<Registration>())
    {
        let registration = app.container().make::<Registration>()?;
        if registration.model != std::any::TypeId::of::<M>() {
            return Err(AuthError::Configuration("同一用户套件不能重复注册不同的设备模型").into());
        }
        return Ok(());
    }
    app.container().make::<SessionManager>()?.validate_device_session_driver().map_err(|error| match error {
        openrust::session::SessionError::Configuration=>AuthError::Configuration("浏览器设备管理需要有效的原生 database 或 Redis 会话；其他处理器须显式关闭 user.devices.enabled"),
        error=>AuthError::Backend(Box::new(error)),
    })?;
    let guard = config
        .get("fortify.guard")
        .and_then(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned))
        .or_else(|| {
            config
                .get("auth.defaults.guard")
                .and_then(|v| v.as_str().map(str::to_owned))
        })
        .unwrap_or_else(|| "web".into());
    let policy = Arc::new(Policy::new(
        config,
        app.container().make::<dyn Encrypter>()?,
        guard,
    )?);
    let validation = policy.clone();
    app.container()
        .make::<AuthManager>()?
        .add_session_validator(
            format!("user.devices:{}", std::any::type_name::<M>()),
            move |guard, user, session| {
                let mut policy = (*validation).clone();
                policy.guard = guard.to_owned();
                Box::pin(async move {
                    let Some(user) = (user as &dyn std::any::Any).downcast_ref::<M>() else {
                        return Ok(true);
                    };
                    policy.accepts(user, session).await
                })
            },
        )?;
    app.container().instance(Arc::new(Registration {
        model: std::any::TypeId::of::<M>(),
        policy,
    }))?;
    use openrust::support::ServiceProvider as _;
    super::StarterKitServiceProvider.load_routes_from(app, register_tracking::<M>)?;
    super::StarterKitServiceProvider
        .commands(app, crate::console::prune_browser_sessions::register::<M>);
    Ok(())
}

struct Snapshot {
    session_id: String,
    ip_address: Option<String>,
    user_agent: Option<String>,
}

fn register_tracking<M: openrust::sanctum::HasApiTokens + Clone>()
-> Result<(), openrust::routing::RoutingError> {
    use openrust::{routing::MiddlewareEntry, support::facades::Route};
    let tracker = Arc::new(TrackBrowserSessions::<M>::new());
    Route::push_middleware_to_group("web", MiddlewareEntry::Instance(tracker.clone()))?;
    Route::push_middleware_to_group("api", MiddlewareEntry::Instance(tracker))
}

/// 在web／api以外的自定义会话路由组中登记最终会话ID与活动信息。
/// 默认套件已为web和api组装配；自定义组应放在StartSession之后。
pub struct TrackBrowserSessions<M>(std::marker::PhantomData<fn() -> M>);
impl<M> TrackBrowserSessions<M> {
    pub const fn new() -> Self {
        Self(std::marker::PhantomData)
    }
}
impl<M> Default for TrackBrowserSessions<M> {
    fn default() -> Self {
        Self::new()
    }
}
impl<M: openrust::sanctum::HasApiTokens + Clone> openrust::contracts::http::Middleware
    for TrackBrowserSessions<M>
{
    fn handle<'a>(
        &'a self,
        request: &'a mut openrust::http::Request,
        next: openrust::http::Next<'a>,
    ) -> openrust::http::HttpFuture<'a, Result<openrust::http::Response, HttpError>> {
        use openrust::{
            auth::Auth,
            contracts::auth::{Guard as _, StatefulGuard as _},
        };
        Box::pin(async move {
            if !request.has_session() {
                return Ok(next.run(request).await);
            }
            let policy = policy()?.ok_or(AuthError::Configuration("设备策略未注册"))?;
            let guards = policy
                .config
                .get("auth.guards")
                .and_then(|v| v.as_object().cloned())
                .ok_or(AuthError::Configuration("auth.guards"))?;
            let names: Vec<_> = guards
                .into_iter()
                .filter(|(_, v)| {
                    v.get("driver").and_then(serde_json::Value::as_str) == Some("session")
                })
                .map(|(name, _)| name)
                .collect();
            let mut origins = std::collections::HashMap::new();
            let mut remembered = std::collections::HashMap::new();
            for name in &names {
                let raw = request
                    .session()
                    .map_err(crate::resolver::internal)?
                    .get_id()
                    .to_owned();
                let had_login = request
                    .session()
                    .map_err(crate::resolver::internal)?
                    .get(&format!("login_{name}"))
                    .is_some();
                let mut guard = Auth::session_guard(request, Some(name))?;
                let user = match guard.user().await {
                    Ok(user) => user
                        .and_then(|user| (user as &dyn std::any::Any).downcast_ref::<M>())
                        .cloned(),
                    Err(AuthError::Http(error))
                        if matches!(error.as_ref(), HttpError::Unauthenticated) =>
                    {
                        None
                    }
                    Err(error) => return Err(error.into()),
                };
                if let Some(user) = user {
                    if had_login {
                        origins.insert(name.clone(), (principal(&user, name)?, raw));
                    }
                    if guard.via_remember() {
                        remembered
                            .insert(name.clone(), user.get_remember_token().map(str::to_owned));
                    }
                }
            }
            let response = next.run(request).await;
            for name in names {
                let user = {
                    let mut guard = Auth::session_guard(request, Some(&name))?;
                    guard
                        .user()
                        .await?
                        .and_then(|user| (user as &dyn std::any::Any).downcast_ref::<M>())
                        .cloned()
                };
                let Some(mut user) = user else { continue };
                let attributes = request.session().map_err(crate::resolver::internal)?.all();
                if !openrust::auth::SessionGuard::session_belongs_to_user(
                    policy.config.as_ref(),
                    &name,
                    &user,
                    attributes,
                )? {
                    continue;
                }
                let snapshot = Snapshot {
                    session_id: request
                        .session()
                        .map_err(crate::resolver::internal)?
                        .get_id()
                        .to_owned(),
                    ip_address: request.ip().map(|ip| ip.to_string()),
                    user_agent: request.user_agent().map(str::to_owned),
                };
                let origin = origins.remove(&name);
                let remember = remembered.remove(&name);
                let mut selected = (*policy).clone();
                selected.guard = name;
                let current_principal = principal(&user, &selected.guard)?;
                if remember.is_none()
                    && origin.as_ref().is_some_and(|(owner, raw)| {
                        raw == &snapshot.session_id && *owner == current_principal
                    })
                {
                    selected.touch_existing(&user, &snapshot).await?;
                    continue;
                }
                super::credentials::with_locked(&mut user, move |locked, tx| {
                    Box::pin(async move {
                        if let Some((owner, raw)) = origin
                            && owner == principal(locked, &selected.guard)?
                        {
                            selected.check_in_transaction(locked, tx, &raw).await?;
                        }
                        if let Some(remember) = remember
                            && locked.get_remember_token() != remember.as_deref()
                        {
                            return Err(HttpError::Unauthenticated.into());
                        }
                        selected.track(locked, tx, &snapshot).await
                    })
                })
                .await?;
            }
            Ok(response)
        })
    }
}

pub(crate) async fn prune_for<M: Model>(retention: i64) -> AuthResult<u64> {
    let cutoff = now()?
        .checked_sub(retention)
        .filter(|_| retention >= 0)
        .ok_or(AuthError::Configuration("设备记录保留期限必须为非负秒数"))?;
    let model = M::from_state(Default::default());
    Ok(model
        .get_connection()?
        .table(TABLE)
        .r#where("expires_at", "<", cutoff)
        .delete()
        .await?)
}
