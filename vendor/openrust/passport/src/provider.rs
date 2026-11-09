//! Passport 配置与服务绑定；此候选不自动注册 HTTP 路由或迁移。

use std::{
    fmt,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Arc,
};

use openrust::{
    Error, Result,
    config::repository::SectionSchema,
    container::Resolver,
    database::{DatabaseServiceProvider, Databases},
    encryption::{ContextualEncrypter, EncryptionServiceProvider},
    foundation::{BootContext, ProviderId, RegisterContext},
    hashing::{HashManager, HashServiceProvider},
    support::{BoxFuture, ServiceProvider},
};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt as _;
use zeroize::Zeroizing;

use crate::{
    AuthorizationServer, BridgeClientRepository, ClientFactory, ClientLookup, ClientModelFactory,
    ClientRepository, DatabaseGrantRepository, GrantConfig, GrantStorage, GrantUserKey,
    OAuthAccessSigner, OAuthAccessVerifier, ResourceServer, ScopeRepository,
    register_client_repository_scope,
};

/// 固定 Passport 的配置区段；部署值仍来自应用同一配置仓储。
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PassportConfig {
    /// 授权用户所用的命名 Guard，官方默认 web。
    pub guard: String,
    /// 官方路由中间件列表，默认空；本候选不自动安装路由。
    pub middleware: Vec<String>,
    /// 可选内联 RSA 私钥 PEM；未提供时读取可信目录的默认文件。
    pub private_key: Option<String>,
    /// 可选内联 RSA 公钥 PEM；未提供时读取可信目录的默认文件。
    pub public_key: Option<String>,
    /// Passport 模型连接名称；空值使用模型或应用默认连接。
    pub connection: Option<String>,
}

impl Default for PassportConfig {
    fn default() -> Self {
        Self {
            guard: "web".into(),
            middleware: Vec::new(),
            private_key: None,
            public_key: None,
            connection: None,
        }
    }
}

impl fmt::Debug for PassportConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PassportConfig")
            .field("guard", &self.guard)
            .field("middleware", &self.middleware)
            .field("private_key_configured", &self.private_key.is_some())
            .field("public_key_configured", &self.public_key.is_some())
            .field("connection", &self.connection)
            .finish()
    }
}

/// 使用应用唯一容器的 Passport Provider。
///
/// 应用必须先登记数据库、哈希、加密 Provider。默认 DB 授权仓储还必须绑定
/// 当前认证模式的 `DatabaseGrantAuthority`；显式替换的 `dyn GrantStorage` 自行承担其
/// 原子上下文与权威契约。配置 guard 不等于已经完成用户或二次认证。
/// 当前客户端模型保存生命周期、父凭据权威和 HTTP 适配仍须分别闭合。
#[derive(Clone)]
pub struct PassportServiceProvider {
    key_directory: PathBuf,
    grants: GrantConfig,
    scopes: ScopeRepository,
    user_key: GrantUserKey,
    scoped_client_models: bool,
    key_permissions_check: bool,
}

impl fmt::Debug for PassportServiceProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PassportServiceProvider")
            .finish_non_exhaustive()
    }
}

impl PassportServiceProvider {
    /// 使用可信应用显式提供的绝对密钥目录，例如 `application_root/storage`。
    ///
    /// 当前没有可复用的应用路径注入契约，因此不从 cwd 推断应用根。
    /// 默认读取 oauth-private.key 与 oauth-public.key，且仅用于 OAuth access token。
    #[must_use]
    pub fn new(key_directory: PathBuf) -> Self {
        Self {
            key_directory,
            grants: GrantConfig::default(),
            scopes: ScopeRepository::default(),
            user_key: GrantUserKey::Integer,
            scoped_client_models: false,
            key_permissions_check: true,
        }
    }

    /// 选择编译好的授权配置，不写入进程全局静态变量。
    #[must_use]
    pub fn with_grants(mut self, grants: GrantConfig) -> Self {
        self.grants = grants;
        self
    }

    /// 选择注册的 scope 集合与公开 scope 规则。
    #[must_use]
    pub fn with_scopes(mut self, scopes: ScopeRepository) -> Self {
        self.scopes = scopes;
        self
    }

    /// 在注册前选择请求级客户端模型链，供租户或请求级 Factory 使用。
    ///
    /// 将默认 Factory trait alias、`ClientRepository`、`ClientLookup` 和 Bridge 仓储
    /// 一并登记为 scoped；无请求状态的默认 Factory 实例仍可安全共享。
    /// 预绑定对象保持原寿命，不修改已解析实例，也不支持运行时切换。
    /// 未选择此项却预绑定 scoped Factory 时保留容器的 captive 依赖拒绝。
    #[must_use]
    pub fn with_scoped_client_models(mut self) -> Self {
        self.scoped_client_models = true;
        self
    }

    /// 对应官方静态权限开关，默认启用；仅关闭文件mode检查。
    ///
    /// 应用将 League 权限 notice 提升为脱敏错误。
    /// 合法inline密钥及Windows不检查文件mode，读取/PEM校验始终保留；配对仅由显式预检执行。
    #[must_use]
    pub fn with_key_permissions_check(mut self, enabled: bool) -> Self {
        self.key_permissions_check = enabled;
        self
    }

    /// 显式选择用户表真实键类型，不根据字符串形状推测。
    #[must_use]
    pub fn with_user_key(mut self, user_key: GrantUserKey) -> Self {
        self.user_key = user_key;
        self
    }
}

struct SectionRegistered;
struct RepositoryScopeRegistered;

impl ServiceProvider for PassportServiceProvider {
    fn dependencies(&self) -> Vec<ProviderId> {
        vec![
            ProviderId::of::<DatabaseServiceProvider>(),
            ProviderId::of::<HashServiceProvider>(),
            ProviderId::of::<EncryptionServiceProvider>(),
        ]
    }

    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if !app.is_bound::<SectionRegistered>() {
                app.config_section("passport", PassportConfig::default(), schema(), |_| Ok(()))?;
                app.instance(Arc::new(SectionRegistered)).await?;
            }
            register_clients(app, self.scoped_client_models).await?;
            if !app.is_bound::<GrantConfig>() {
                app.instance(Arc::new(self.grants.clone())).await?;
            }
            if !app.is_bound::<ScopeRepository>() {
                app.instance(Arc::new(self.scopes.clone())).await?;
            }
            if !app.is_bound::<GrantUserKey>() {
                app.instance(Arc::new(self.user_key)).await?;
            }
            if !app.is_bound::<OAuthAccessSigner>() {
                let directory = self.key_directory.clone();
                let permissions_check = self.key_permissions_check;
                app.singleton::<OAuthAccessSigner, _>(move |resolver| {
                    let directory = directory.clone();
                    Box::pin(async move {
                        let config = resolver.section::<PassportConfig>("passport")?;
                        Ok(Arc::new(
                            load_signer(&directory, &config, permissions_check).await?,
                        ))
                    })
                })
                .await?;
            }
            if !app.is_bound::<OAuthAccessVerifier>() {
                let directory = self.key_directory.clone();
                let permissions_check = self.key_permissions_check;
                app.singleton::<OAuthAccessVerifier, _>(move |resolver| {
                    let directory = directory.clone();
                    Box::pin(async move {
                        let config = resolver.section::<PassportConfig>("passport")?;
                        Ok(Arc::new(
                            load_verifier(&directory, &config, permissions_check).await?,
                        ))
                    })
                })
                .await?;
            }
            register_grants(app).await?;
            // 保持官方惰性：安装扩展和无关命令不主动读取 OAuth 密钥。
            Ok(())
        })
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let _ = app;
            // 权威在真实授权服务解析处检查，未使用 OAuth 时不阻断应用启动。
            Ok(())
        })
    }
}

async fn register_clients(app: &mut RegisterContext<'_>, scoped: bool) -> Result<()> {
    if !app.is_bound::<RepositoryScopeRegistered>() {
        register_client_repository_scope(app).await?;
        app.instance(Arc::new(RepositoryScopeRegistered)).await?;
    }
    if !app.is_bound::<ClientFactory>() {
        app.instance(Arc::new(ClientFactory::default())).await?;
    }
    if !app.is_bound::<dyn ClientModelFactory>() {
        register_client_binding::<dyn ClientModelFactory, _>(app, scoped, |resolver| {
            Box::pin(async move {
                let factory: Arc<dyn ClientModelFactory> =
                    resolver.resolve::<ClientFactory>().await?;
                Ok(factory)
            })
        })
        .await?;
    }
    if !app.is_bound::<ClientRepository>() {
        register_client_binding::<ClientRepository, _>(app, scoped, |resolver| {
            Box::pin(async move {
                let config = resolver.section::<PassportConfig>("passport")?;
                let databases = resolver.resolve::<Databases>().await?;
                let factory = resolver.resolve::<dyn ClientModelFactory>().await?;
                Ok(Arc::new(ClientRepository::new(
                    &databases,
                    config.connection.as_deref(),
                    factory,
                )?))
            })
        })
        .await?;
    }
    if !app.is_bound::<dyn ClientLookup>() {
        register_client_binding::<dyn ClientLookup, _>(app, scoped, |resolver| {
            Box::pin(async move {
                let clients: Arc<dyn ClientLookup> = resolver.resolve::<ClientRepository>().await?;
                Ok(clients)
            })
        })
        .await?;
    }
    if !app.is_bound::<BridgeClientRepository>() {
        register_client_binding::<BridgeClientRepository, _>(app, scoped, |resolver| {
            Box::pin(async move {
                let clients = resolver.resolve::<dyn ClientLookup>().await?;
                let hashing = resolver.resolve::<HashManager>().await?;
                Ok(Arc::new(BridgeClientRepository::new(clients, hashing)))
            })
        })
        .await?;
    }
    Ok(())
}

// 四个默认绑定使用同一注册期选项，避免只缩短下游而留下 singleton 捕获 scoped。
async fn register_client_binding<T, F>(
    app: &mut RegisterContext<'_>,
    scoped: bool,
    factory: F,
) -> Result<()>
where
    T: ?Sized + Send + Sync + 'static,
    F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
{
    if scoped {
        app.scoped::<T, F>(factory).await
    } else {
        app.singleton::<T, F>(factory).await
    }
}

async fn register_grants(app: &mut RegisterContext<'_>) -> Result<()> {
    if !app.is_bound::<DatabaseGrantRepository>() {
        app.scoped::<DatabaseGrantRepository, _>(|resolver| {
            Box::pin(async move {
                let config = resolver.section::<PassportConfig>("passport")?;
                let databases = resolver.resolve::<Databases>().await?;
                let factory = resolver.resolve::<dyn ClientModelFactory>().await?;
                let user_key = resolver.resolve::<GrantUserKey>().await?;
                Ok(Arc::new(DatabaseGrantRepository::from_scope(
                    &databases,
                    config.connection.as_deref(),
                    factory,
                    *user_key,
                    resolver.resolve::<dyn ClientLookup>().await?,
                    resolver.clone(),
                )?))
            })
        })
        .await?;
    }
    if !app.is_bound::<dyn GrantStorage>() {
        app.scoped::<dyn GrantStorage, _>(|resolver| {
            Box::pin(async move {
                let storage: Arc<dyn GrantStorage> =
                    resolver.resolve::<DatabaseGrantRepository>().await?;
                Ok(storage)
            })
        })
        .await?;
    }
    if !app.is_bound::<AuthorizationServer>() {
        app.scoped::<AuthorizationServer, _>(|resolver| {
            Box::pin(async move {
                let repository = resolver.resolve::<dyn GrantStorage>().await?;
                repository.require_issuance_authority().await?;
                let hashing = resolver.resolve::<HashManager>().await?;
                let signer = resolver.resolve::<OAuthAccessSigner>().await?;
                let encryption = resolver.resolve::<ContextualEncrypter>().await?;
                let scopes = resolver.resolve::<ScopeRepository>().await?;
                let config = resolver.resolve::<GrantConfig>().await?;
                Ok(Arc::new(AuthorizationServer::new(
                    repository,
                    hashing,
                    signer,
                    encryption,
                    (*scopes).clone(),
                    (*config).clone(),
                )))
            })
        })
        .await?;
    }
    if !app.is_bound::<ResourceServer>() {
        app.scoped::<ResourceServer, _>(|resolver| {
            Box::pin(async move {
                let verifier = resolver.resolve::<OAuthAccessVerifier>().await?;
                let storage = resolver.resolve::<dyn GrantStorage>().await?;
                Ok(Arc::new(ResourceServer::new(verifier, storage)))
            })
        })
        .await?;
    }
    Ok(())
}

fn schema() -> SectionSchema {
    use SectionSchema as S;
    S::object([
        ("guard", S::String),
        ("middleware", S::array(S::String)),
        ("private_key", S::String.nullable().sensitive()),
        ("public_key", S::String.nullable()),
        ("connection", S::String.nullable()),
    ])
}

async fn load_signer(
    directory: &Path,
    config: &PassportConfig,
    permissions_check: bool,
) -> Result<OAuthAccessSigner> {
    validate_directory(directory)?;
    let private = load_key(
        directory,
        config.private_key.as_deref(),
        KeyKind::Private,
        permissions_check,
    )
    .await?;
    OAuthAccessSigner::from_pem(&private).map_err(|_| invalid_key_material())
}

async fn load_verifier(
    directory: &Path,
    config: &PassportConfig,
    permissions_check: bool,
) -> Result<OAuthAccessVerifier> {
    validate_directory(directory)?;
    let public = load_key(
        directory,
        config.public_key.as_deref(),
        KeyKind::Public,
        permissions_check,
    )
    .await?;
    OAuthAccessVerifier::from_pem(&public).map_err(|_| invalid_key_material())
}

/// 显式增强：按先私钥、后公钥的原错误顺序加载并验证 OAuth RSA 配对。
///
/// 默认注册及两种能力解析不调用本函数；每份文件只 open/read 一次，
/// 并由同一个打开文件执行材料/权限检查。调用方提供可信配置及绝对默认目录。
///
/// # Errors
/// 目录、读取、材料、权限或最终配对无效时返回原稳定脱敏错误。
pub async fn validate_pair(
    directory: &Path,
    config: &PassportConfig,
    permissions_check: bool,
) -> Result<()> {
    validate_directory(directory)?;
    let private = load_key(
        directory,
        config.private_key.as_deref(),
        KeyKind::Private,
        permissions_check,
    )
    .await?;
    let public = load_key(
        directory,
        config.public_key.as_deref(),
        KeyKind::Public,
        permissions_check,
    )
    .await?;
    crate::validate_key_pair(&private, &public).map_err(|_| invalid_key_material())
}

fn validate_directory(directory: &Path) -> Result<()> {
    if !directory.is_absolute() {
        return Err(failure(
            "passport_key_directory_invalid",
            "OAuth密钥目录必须由可信应用提供绝对路径",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum KeyKind {
    Private,
    Public,
}
impl KeyKind {
    fn filename(self) -> &'static str {
        match self {
            Self::Private => "oauth-private.key",
            Self::Public => "oauth-public.key",
        }
    }
    fn read_error(self, kind: ErrorKind) -> Error {
        match (self, kind) {
            (Self::Private, ErrorKind::NotFound) => {
                failure("passport_private_key_missing", "OAuth私钥文件缺失")
            }
            (Self::Public, ErrorKind::NotFound) => {
                failure("passport_public_key_missing", "OAuth公钥文件缺失")
            }
            (Self::Private, _) => {
                failure("passport_private_key_unreadable", "OAuth私钥文件不可读取")
            }
            (Self::Public, _) => failure("passport_public_key_unreadable", "OAuth公钥文件不可读取"),
        }
    }
}

async fn load_key(
    directory: &Path,
    inline: Option<&str>,
    kind: KeyKind,
    permissions_check: bool,
) -> Result<Zeroizing<Vec<u8>>> {
    let configured = inline.map(|value| Zeroizing::new(value.replace("\\n", "\n")));
    if let Some(value) = configured
        .as_ref()
        .map(|value| value.as_str())
        .filter(|value| !value.is_empty() && *value != "0")
    {
        // CryptKey先识别有效材料，再决定文件fallback；不把合法但role不符的key当路径。
        if !value.starts_with("file://") && inline_material(value.as_bytes(), kind)? {
            return Ok(Zeroizing::new(value.as_bytes().to_vec()));
        }
        #[cfg(unix)]
        {
            let path = configured_file_path(value, kind).await?;
            return read_key_file(&path, kind, permissions_check).await;
        }
        #[cfg(not(unix))]
        return Err(failure(
            "passport_key_path_platform_unverified",
            "当前平台的显式密钥路径行为尚未核验",
        ));
    }
    read_key_file(&directory.join(kind.filename()), kind, permissions_check).await
}

// 返回true才是已识别且role可用的inline；识别为相反role时直接返回原role错误。
fn inline_material(contents: &[u8], kind: KeyKind) -> Result<bool> {
    match validate_key_material(contents, kind) {
        Ok(()) => Ok(true),
        Err(error) => {
            let other = match kind {
                KeyKind::Private => KeyKind::Public,
                KeyKind::Public => KeyKind::Private,
            };
            if validate_key_material(contents, other).is_ok() {
                Err(error)
            } else {
                Ok(false)
            }
        }
    }
}

#[cfg(unix)]
async fn configured_file_path(value: &str, kind: KeyKind) -> Result<PathBuf> {
    if value.contains('\0') {
        return Err(invalid_key_material());
    }
    let initial = if value
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
    {
        // PHP is_file识别文件wrapper大小写；CryptKey后面的前缀判断依然严格区分大小写。
        literal_file_uri_path(&value[7..]).ok_or_else(invalid_key_material)?
    } else {
        if value.contains("://") {
            return Err(failure(
                "passport_key_wrapper_unsupported",
                "此密钥读取尚未实现其它stream wrapper",
            ));
        }
        PathBuf::from(value)
    };
    // 按is_file语义跟随symlink；不提前对开发者relative配置施加absolute-only策略。
    if !tokio::fs::metadata(&initial)
        .await
        .is_ok_and(|metadata| metadata.is_file())
    {
        return Err(invalid_key_material());
    }
    if value.starts_with("file://") {
        return Ok(initial);
    }
    // 实际Linux oracle：现存relative加file://后不可读；不能猜它自动绑定应用目录。
    literal_file_uri_path(value).ok_or_else(|| kind.read_error(ErrorKind::InvalidInput))
}

#[cfg(unix)]
fn literal_file_uri_path(suffix: &str) -> Option<PathBuf> {
    if suffix.starts_with('/') {
        return Some(PathBuf::from(suffix));
    }
    suffix
        .strip_prefix("localhost")
        .filter(|path| path.starts_with('/'))
        .map(PathBuf::from)
}

async fn read_key_file(
    path: &Path,
    kind: KeyKind,
    permissions_check: bool,
) -> Result<Zeroizing<Vec<u8>>> {
    // open跟随symlink；同一打开文件读取和metadata，不增加ACL/Source FileLoader策略。
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| kind.read_error(error.kind()))?;
    let mut contents = Zeroizing::new(Vec::new());
    file.read_to_end(&mut contents)
        .await
        .map_err(|error| kind.read_error(error.kind()))?;
    validate_key_material(&contents, kind)?;
    // 固定CryptKey先验证密钥，随后才检查mode；关闭mode仍不能跳过PEM错误。
    #[cfg(unix)]
    if permissions_check {
        check_file_permissions(&file, kind).await?;
    }
    #[cfg(not(unix))]
    let _ = permissions_check;
    Ok(contents)
}

fn validate_key_material(contents: &[u8], kind: KeyKind) -> Result<()> {
    use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey};
    let valid = match kind {
        KeyKind::Private => EncodingKey::from_rsa_pem(contents)
            .and_then(|key| {
                jsonwebtoken::crypto::sign(
                    b"openrust:oauth-key-material-check",
                    &key,
                    Algorithm::RS256,
                )
            })
            .map(|_| ()),
        // DecodingKey只保存DER；公开JWK投影经当前成熟backend真正解析RSA公钥，
        // 不拿签名false猜测key是否合法，也不在这里检查私公配对。
        KeyKind::Public => DecodingKey::from_rsa_pem(contents)
            .and_then(|key| jsonwebtoken::jwk::Jwk::from_decoding_key(&key, Some(Algorithm::RS256)))
            .map(|_| ()),
    };
    valid.map_err(|_| invalid_key_material())
}

fn invalid_key_material() -> Error {
    failure(
        "passport_signing_key_pair_invalid",
        "OAuth RSA密钥无效或不配对",
    )
}

#[cfg(unix)]
async fn check_file_permissions(file: &tokio::fs::File, kind: KeyKind) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = file
        .metadata()
        .await
        .map_err(|error| kind.read_error(error.kind()))?
        .permissions()
        .mode();
    if key_mode_allowed(mode) {
        Ok(())
    } else {
        // 将 E_USER_NOTICE 提升为错误，不向外回显上游包含的路径。
        Err(failure(
            "passport_key_permissions_invalid",
            "OAuth密钥文件权限不符合默认宿主要求",
        ))
    }
}

#[cfg(unix)]
fn key_mode_allowed(mode: u32) -> bool {
    matches!(mode & 0o777, 0o400 | 0o440 | 0o600 | 0o640 | 0o660)
}

fn failure(code: &'static str, message: &'static str) -> Error {
    Error::custom_with_status(
        openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        code,
        message,
    )
}

#[cfg(test)]
#[path = "../tests/unit/provider.rs"]
mod tests;
