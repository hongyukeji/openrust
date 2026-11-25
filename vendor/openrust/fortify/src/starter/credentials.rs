//! 套件凭据生命周期。复用 Eloquent 事务、模型事件及 Sanctum 存储。

use aws_lc_rs::digest;
use openrust::{
    auth::{AuthError, AuthIdentifier, AuthResult},
    contracts::{auth::Authenticatable, config::Repository},
    database::{DatabaseError, Transaction, Value, eloquent::Model},
    hashing::{Hash, HashOptions},
    sanctum::{NewAccessToken, Sanctum, TokenOptions},
};

pub const VERSION_ATTRIBUTE: &str = "credential_version";
pub const TOKEN_STAMP_ATTRIBUTE: &str = "credential_stamp";

pub(super) fn version(user: &dyn Authenticatable) -> AuthResult<u64> {
    match user.get_auth_attribute(VERSION_ATTRIBUTE)? {
        Some(Value::Integer(value)) if value >= 0 => Ok(value as u64),
        Some(Value::Unsigned(value)) => Ok(value),
        _ => Err(AuthError::Configuration(
            "用户套件需要持久化 credential_version 非负整数列",
        )),
    }
}

/// 摘要绑定密码哈希与持久化版本；它不是访问令牌，也不应公开到用户资源。
pub fn stamp(user: &dyn Authenticatable) -> AuthResult<String> {
    let password = user.get_auth_password().unwrap_or_default();
    let identity = serde_json::to_string(&user.get_auth_identifier())?;
    let name = user.get_auth_identifier_name();
    let input = format!(
        "user-credentials:v1:{}:{}:{}:{}:{}:{}:{}",
        name.len(),
        name,
        identity.len(),
        identity,
        password.len(),
        password,
        version(user)?
    );
    let hash = digest::digest(&digest::SHA256, input.as_bytes());
    Ok(hash
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn token_stamp(user: &dyn Authenticatable, owner: &str) -> AuthResult<String> {
    let identity = serde_json::to_string(&user.get_auth_identifier())?;
    let name = user.get_auth_identifier_name();
    let input = format!(
        "token-credentials:v1:{}:{}:{}:{}:{}:{}:{}",
        owner.len(),
        owner,
        name.len(),
        name,
        identity.len(),
        identity,
        version(user)?
    );
    let hash = digest::digest(&digest::SHA256, input.as_bytes());
    Ok(hash
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// 在标准 Sanctum 校验之后核对最新用户版本；普通应用不安装此策略。
pub fn configure(sanctum: &Sanctum) {
    configure_matching(sanctum, |_| true);
}

/// 仅对选中的套件模型追加策略，保留其他多态归属及应用校验器。
pub fn configure_for<M: Authenticatable>(sanctum: &Sanctum) {
    configure_matching(sanctum, |user| (user as &dyn std::any::Any).is::<M>());
}
fn configure_matching(
    sanctum: &Sanctum,
    matches: impl Fn(&dyn Authenticatable) -> bool + Send + Sync + 'static,
) {
    use subtle::ConstantTimeEq as _;
    if sanctum.personal_access_token_model()
        == std::any::type_name::<openrust::sanctum::DefaultPersonalAccessToken>()
    {
        sanctum.use_personal_access_token_model::<CredentialToken>();
    }
    sanctum.add_token_owner_validator(move |token, user| {
        if !matches(user) {
            return Ok(true);
        }
        let Some(Value::Text(stored)) = token.get_attribute_value(TOKEN_STAMP_ATTRIBUTE)? else {
            return Ok(false);
        };
        Ok(bool::from(stored.as_bytes().ct_eq(
            token_stamp(user, &token.tokenable_type)?.as_bytes(),
        )))
    });
}

async fn lock<M: Model + Authenticatable + Clone>(
    user: &mut M,
    transaction: &mut Transaction,
) -> AuthResult<()> {
    if !user.state().exists() || user.get_raw_original(user.get_key_name()) != user.get_key() {
        return Err(openrust::http::HttpError::Unauthenticated.into());
    }
    let expected = stamp(user)?;
    let current_version = version(user)?;
    let key = user.get_key().cloned().ok_or(AuthError::InvalidIdentity)?;
    // 先取得写锁，SQLite 也不能依赖被忽略的 SELECT FOR UPDATE。
    // MySQL 的无变化 UPDATE 可能返回0，不据此判断用户是否存在。
    transaction
        .table(user.get_table())
        .r#where(user.get_key_name(), "=", key)
        .r#where(VERSION_ATTRIBUTE, "=", Value::Unsigned(current_version))
        .update([(VERSION_ATTRIBUTE.into(), Value::Unsigned(current_version))].into())
        .await?;
    if let Err(error) = user.refresh_for_update(transaction).await {
        return Err(match error {
            DatabaseError::ModelNotFound(_) => openrust::http::HttpError::Unauthenticated.into(),
            error => error.into(),
        });
    }
    if expected != stamp(user)? {
        return Err(openrust::http::HttpError::Unauthenticated.into());
    }
    Ok(())
}

async fn change<M: Model + Authenticatable + Clone>(
    user: &mut M,
    password: Option<String>,
) -> AuthResult<()> {
    let mut locked = user.clone();
    let connection = user.get_connection()?;
    let mut transaction = connection.begin_transaction().await?;
    lock(&mut locked, &mut transaction).await?;
    rotate_locked(&mut locked, &mut transaction, password).await?;
    transaction.commit().await?;
    *user = locked;
    Ok(())
}

/// 调用方已持有共同用户写锁；将撤销点与其敏感操作放在同一事务内。
pub(crate) async fn rotate_locked<M: Model + Authenticatable>(
    locked: &mut M,
    transaction: &mut Transaction,
    password: Option<String>,
) -> AuthResult<()> {
    let next = version(locked)?
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or(AuthError::Configuration("凭据版本已达到数据库整数上限"))?;
    locked.set_attribute_value(VERSION_ATTRIBUTE, Value::Unsigned(next))?;
    if let Some(password) = password {
        let column = locked.get_auth_password_name().to_owned();
        locked.set_attribute_value(&column, password)?;
    }
    locked.set_remember_token(None);
    if !locked.save_in_transaction(transaction).await? {
        return Err(DatabaseError::InvalidQuery("用户模型取消凭据变更").into());
    }
    Ok(())
}

/// 在与签发／凭据更新相同的用户写锁内执行账户操作，失败不会替换调用者模型。
pub(crate) async fn with_locked<M, F, T>(user: &mut M, action: F) -> AuthResult<T>
where
    M: Model + Authenticatable + Clone,
    F: for<'a> FnOnce(
            &'a mut M,
            &'a mut Transaction,
        ) -> openrust::auth::AuthFuture<'a, AuthResult<T>>
        + Send,
    T: Send,
{
    let mut locked = user.clone();
    let connection = user.get_connection()?;
    let mut transaction = connection.begin_transaction().await?;
    lock(&mut locked, &mut transaction).await?;
    let result = action(&mut locked, &mut transaction).await?;
    transaction.commit().await?;
    *user = locked;
    Ok(result)
}

/// 新注册模型读取数据库默认值时，不能把期间发生的安全变更当成原登录证明。
pub async fn refresh_created<M: Model + Authenticatable + Clone>(user: &mut M) -> AuthResult<()> {
    if !user.state().was_recently_created() {
        return Err(openrust::http::HttpError::Unauthenticated.into());
    }
    let expected_password = user.get_auth_password().map(str::to_owned);
    let expected_version = match user.get_auth_attribute(VERSION_ATTRIBUTE)? {
        None => 0,
        Some(Value::Integer(value)) if value >= 0 => value as u64,
        Some(Value::Unsigned(value)) => value,
        _ => return Err(AuthError::Configuration("新用户凭据版本无效")),
    };
    let mut fresh = user.clone();
    fresh.refresh().await?;
    if fresh.get_auth_password() != expected_password.as_deref()
        || version(&fresh)? != expected_version
    {
        return Err(openrust::http::HttpError::Unauthenticated.into());
    }
    *user = fresh;
    Ok(())
}

/// 验证及哈希在调用方规则通过后执行，锁内再次核对原身份，事件取消即回滚。
pub async fn change_password<M: Model + Authenticatable + Clone>(
    user: &mut M,
    password: &str,
) -> AuthResult<()> {
    let hash = Hash::make(password, &HashOptions::new()).await?;
    change(user, Some(hash)).await
}

/// 包括无本地密码账户的全设备退出。调用方自行决定是否重新登录当前会话。
pub async fn revoke_all<M: Model + Authenticatable + Clone>(user: &mut M) -> AuthResult<()> {
    change(user, None).await
}

/// 只从服务端配置授予能力，调用者必须先完成所需的近期认证／额外因素。
pub async fn issue<M: openrust::sanctum::HasApiTokens + Clone>(
    user: &mut M,
    sanctum: &Sanctum,
    database: &openrust::database::DatabaseManager,
    config: &dyn Repository,
    name: &str,
    abilities: &[&str],
) -> AuthResult<NewAccessToken> {
    issue_with_expiration(user, sanctum, database, config, name, abilities, None).await
}

pub async fn issue_with_expiration<M: openrust::sanctum::HasApiTokens + Clone>(
    user: &mut M,
    sanctum: &Sanctum,
    database: &openrust::database::DatabaseManager,
    config: &dyn Repository,
    name: &str,
    abilities: &[&str],
    expires_at: Option<jiff::Timestamp>,
) -> AuthResult<NewAccessToken> {
    issue_checked(
        user,
        sanctum,
        database,
        config,
        Grant {
            name,
            abilities,
            expires_at,
        },
        |_, _| Box::pin(async { Ok(()) }),
    )
    .await
}

pub(crate) struct Grant<'a> {
    pub name: &'a str,
    pub abilities: &'a [&'a str],
    pub expires_at: Option<jiff::Timestamp>,
}

/// 客户端证明和可验证状态须在用户行锁内再次核对，不能在校验后留出变更窗口。
pub(crate) async fn issue_checked<M, F>(
    user: &mut M,
    sanctum: &Sanctum,
    database: &openrust::database::DatabaseManager,
    config: &dyn Repository,
    grant: Grant<'_>,
    mut check: F,
) -> AuthResult<NewAccessToken>
where
    M: openrust::sanctum::HasApiTokens + Clone,
    F: for<'a> FnMut(&'a M, &'a mut Transaction) -> openrust::auth::AuthFuture<'a, AuthResult<()>>
        + Send,
{
    let Grant {
        name,
        abilities,
        expires_at,
    } = grant;
    if name.trim().is_empty() || name.chars().count() > 255 {
        return Err(AuthError::Configuration("令牌设备名称必须为1至255个字符"));
    }
    let allowed = config
        .get("user.token_abilities")
        .unwrap_or_else(|| serde_json::json!(["account:read", "account:update", "tokens:manage"]));
    let allowed = allowed
        .as_array()
        .filter(|values| values.iter().all(serde_json::Value::is_string))
        .ok_or(AuthError::Configuration("user.token_abilities"))?;
    if abilities.is_empty()
        || abilities
            .iter()
            .any(|ability| !allowed.iter().any(|value| value.as_str() == Some(ability)))
    {
        return Err(AuthError::Configuration("请求的令牌能力不在服务端白名单内"));
    }
    let days = config
        .get("user.token_expiration_days")
        .unwrap_or_else(|| serde_json::json!(30));
    let seconds = days
        .as_u64()
        .filter(|days| *days > 0)
        .and_then(|days| days.checked_mul(86400))
        .and_then(|seconds| i64::try_from(seconds).ok())
        .ok_or(AuthError::Configuration("user.token_expiration_days"))?;
    let now = openrust::support::facades::Date::now(None)
        .map_err(|error| AuthError::Backend(Box::new(error)))?
        .as_date_time()
        .timestamp();
    let expires = now.checked_add(jiff::SignedDuration::from_secs(seconds));
    let expires = expires.map_err(|_| AuthError::Configuration("令牌过期时间超出支持范围"))?;
    let expires = expires_at.map_or(expires, |requested| requested.min(expires));
    let secret = user.generate_token_string(sanctum)?;
    let tokens = sanctum.personal_access_tokens(database, "")?;
    let connection = user.get_connection()?;
    if !tokens.uses_connection(&connection) {
        return Err(AuthError::Configuration("用户套件不支持跨连接原子签发令牌"));
    }
    let mut locked = user.clone();
    let mut transaction = connection.begin_transaction().await?;
    lock(&mut locked, &mut transaction).await?;
    check(&locked, &mut transaction).await?;
    let owner_type = locked.get_morph_class()?;
    let owner_id = AuthIdentifier::try_from(locked.get_key().ok_or(AuthError::InvalidIdentity)?)?;
    let token = tokens
        .create_token_in_transaction(
            &mut transaction,
            owner_type,
            owner_id,
            TokenOptions::new(name)
                .abilities(abilities)
                .expires_at(Some(expires))
                .secret(secret)
                .attributes(
                    [(
                        TOKEN_STAMP_ATTRIBUTE.into(),
                        token_stamp(&locked, owner_type)?.into(),
                    )]
                    .into(),
                ),
        )
        .await?;
    check(&locked, &mut transaction).await?;
    transaction.commit().await?;
    *user = locked;
    Ok(token)
}

/// 套件默认模型仅追加隐藏版本摘要；自定义令牌模型仍由应用选择。
pub struct CredentialToken(openrust::database::eloquent::ModelState);
impl Model for CredentialToken {
    fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
        static META: openrust::database::eloquent::ModelMetadata =
            openrust::database::eloquent::ModelMetadata::new("personal_access_tokens")
                .hidden(&["token", TOKEN_STAMP_ATTRIBUTE]);
        &META
    }
    fn casts() -> &'static [(
        &'static str,
        openrust::database::eloquent::casts::AttributeCast,
    )] {
        openrust::sanctum::DefaultPersonalAccessToken::casts()
    }
    fn from_state(state: openrust::database::eloquent::ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &openrust::database::eloquent::ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut openrust::database::eloquent::ModelState {
        &mut self.0
    }
}
impl openrust::sanctum::PersonalAccessTokenModel for CredentialToken {}

/// 套件版本策略只作用于对应模型的会话Guard及实际数据表，不能波及独立后台身份。
pub(crate) fn scope_for<M: Model + Authenticatable>(
    app: &openrust::foundation::Application,
) -> openrust::support::ProviderResult {
    use openrust::contracts::{container::Container as _, foundation::Application as _};
    use serde_json::json;
    let config = app.container().get::<dyn Repository>("config")?;
    let model = M::from_state(Default::default());
    let guard = config
        .get("fortify.guard")
        .and_then(|value| {
            value
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
        .or_else(|| {
            config
                .get("auth.defaults.guard")
                .and_then(|value| value.as_str().map(str::to_owned))
        })
        .unwrap_or_else(|| "web".into());
    let guards = config
        .get("auth.guards")
        .ok_or(AuthError::Configuration("auth.guards"))?;
    let provider = guards
        .get(&guard)
        .and_then(|value| value.get("provider"))
        .and_then(serde_json::Value::as_str)
        .ok_or(AuthError::Configuration("用户套件会话Guard的Provider"))?;
    let providers = config
        .get("auth.providers")
        .ok_or(AuthError::Configuration("auth.providers"))?;
    let chosen = providers
        .get(provider)
        .ok_or(AuthError::Configuration("用户套件Provider"))?;
    let database = app
        .container()
        .make::<openrust::database::DatabaseManager>()?;
    let connection = database.connection(
        chosen
            .get("connection")
            .and_then(serde_json::Value::as_str)
            .or(model.get_connection_name()),
    )?;
    let mut eligible = vec![provider.to_owned()];
    let mut tables = vec![json!({"connection":connection.get_name(),"table":model.get_table()})];
    if chosen.get("driver").and_then(serde_json::Value::as_str) == Some("eloquent")
        && let Some(entries) = providers.as_object()
    {
        for (name, value) in entries {
            if value.get("driver").and_then(serde_json::Value::as_str) == Some("eloquent")
                && value.get("model") == chosen.get("model")
            {
                let db = database.connection(
                    value
                        .get("connection")
                        .and_then(serde_json::Value::as_str)
                        .or(model.get_connection_name()),
                )?;
                eligible.push(name.clone());
                let row = json!({"connection":db.get_name(),"table":model.get_table()});
                if !tables.contains(&row) {
                    tables.push(row);
                }
            }
        }
    }
    let selected: Vec<_> = guards
        .as_object()
        .ok_or(AuthError::Configuration("auth.guards"))?
        .iter()
        .filter_map(|(name, value)| {
            let selected = value.get("driver").and_then(serde_json::Value::as_str)
                == Some("session")
                && value
                    .get("provider")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|provider| eligible.iter().any(|name| name == provider));
            selected.then_some(name.clone())
        })
        .collect();
    if !selected.contains(&guard) {
        return Err(AuthError::Configuration("用户套件需要会话Guard").into());
    }
    config.set(
        "auth.session_credential_version_scope",
        json!({"guards":selected,"tables":tables}),
    )?;
    configure_for::<M>(app.container().make::<Sanctum>()?.as_ref());
    Ok(())
}
