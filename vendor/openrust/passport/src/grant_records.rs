//! 授权码与令牌持久记录；与原始 bearer 分开，不实现敏感调试输出。

use openrust::{
    Result,
    database::{
        TransactionContext,
        sea_orm::{DbBackend, Value as SqlValue},
    },
    support::BoxFuture,
};
use serde_json::Value;

use crate::{BridgeClient, ClientModel, ClientRecord, PhpArray};

/// 尚待权威适配器验证的不透明上下文载体；构造成功不证明登录或二次认证。
///
/// 不提供Debug或公开序列化。只有实际GrantAuthority验证后才可据此签发；
/// HTTP请求自报的值不能替代唯一Guard/UserProvider/Fortify的证明。
#[derive(Clone)]
pub struct GrantAuthorityBinding(zeroize::Zeroizing<String>);

impl GrantAuthorityBinding {
    /// 保存完整不透明值，不解析或截断；64KiB为凭据封装的资源上限。
    ///
    /// 该上限为框架1MiB认证明文上限预留其余协议字段空间，不是官方证明格式。
    /// 未来权威格式若不适配须显式修改并验证，不接受前缀或部分证明。
    pub fn new(opaque: String) -> Result<Self> {
        if opaque.is_empty() || opaque.len() > 64 * 1024 {
            return Err(openrust::Error::custom_with_status(
                openrust::http::axum::http::StatusCode::BAD_REQUEST,
                "passport_authority_binding_invalid",
                "OAuth授权上下文缺失或超出载体上限",
            ));
        }
        Ok(Self(zeroize::Zeroizing::new(opaque)))
    }

    /// 仅供可信权威适配器完整核验；不能根据非空值推导认证已完成。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 默认OAuth表的用户键绑定；与模型实际Schema相符时才可选择。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrantUserKey {
    /// 对应固定Passport migration的foreignId；不能接收UUID或截断大数。
    Integer,
    /// 显式文本用户主键；不按字符串外形猜UUID。
    String,
    /// 显式UUID用户主键；PostgreSQL使用原生UUID绑定。
    Uuid,
}

impl GrantUserKey {
    pub(crate) fn binding(self, value: &str, backend: DbBackend) -> Result<SqlValue> {
        match self {
            Self::Integer => value
                .parse::<i64>()
                .map(Into::into)
                .map_err(|_| invalid_binding()),
            Self::String => Ok(value.to_owned().into()),
            Self::Uuid => {
                let id = uuid::Uuid::parse_str(value).map_err(|_| invalid_binding())?;
                if backend == DbBackend::Postgres {
                    Ok(SqlValue::Uuid(Some(id)))
                } else {
                    Ok(value.to_owned().into())
                }
            }
        }
    }
}

/// 已选用户模型的签发末检扩展；复用应用唯一Guard/UserProvider/Fortify。
///
/// 必须只在传入同一原子context内查当前安全事实，不在末检中做外部网络I/O。
/// 默认DB显式使用引擎签发的TransactionContext；其他后端显式选择自身context。
/// 所需账户或父凭据位于另一数据库时，适配器须拒绝或提供经验证的等价保证。
pub trait GrantAuthority<C: ?Sized>: Send + Sync {
    /// 在全部直接提交前监听器返回后检查当前安全状态；排队监听器仅保证已入队。
    fn validate<'a>(
        &'a self,
        transaction: &'a C,
        context: &'a GrantValidation<'a>,
        binding: &'a GrantAuthorityBinding,
    ) -> BoxFuture<'a, Result<()>>;
}

/// 默认DB仓储的单一权威对象；可接受每次由框架实际签发的事务context。
///
/// 实现须自行用 `TransactionContext::is_from` 核验其真实预期Db，再通过
/// `transaction()`读取主体、客户端与父凭据；来源不是认证或同意证明。
/// `'static`约束权威对象，不要求事务为static；自定义后端继续显式选择其context。
///
/// 裸事务权威不能作为默认DB权威传入：
/// ```compile_fail
/// use std::sync::Arc;
/// use openrust::database::DatabaseTransaction;
/// use openrust_passport::{DatabaseGrantAuthority, GrantAuthority};
/// fn reject(raw: Arc<dyn GrantAuthority<DatabaseTransaction>>) {
///     let _: Arc<DatabaseGrantAuthority> = raw;
/// }
/// ```
pub type DatabaseGrantAuthority = dyn for<'tx> GrantAuthority<TransactionContext<'tx>> + 'static;

/// 当前OAuth委托所处的正常阶段，不是要求binding字面标签与阶段相等。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrantOperation {
    /// 已认证用户确认实际客户端与scope，建立OAuth委托。
    Authorize,
    /// 已绑定授权码兑换；保留同一合法委托。
    Redeem,
    /// 同一合法委托刷新，可以缩小scope，不能扩大。
    Refresh,
}

/// 必须与不透明委托证明关联的实际上下文；构造本身不证明授权。
///
/// 来自已验证请求、认证密文与同事务记录，不从未验证的表单拼装。
/// 权威还须核对自身 Guard / Provider / 应用与存储域，不能仅比较字符串 `user_id`。
pub struct GrantValidation<'a> {
    /// 实际用户原始字符串标识。
    pub user_id: &'a str,
    /// 当前客户端的协议标识。
    pub client_id: &'a str,
    /// 此操作实际批准或将签发的scope；委托权威必须检查允许集合。
    pub scopes: &'a [String],
    /// 当前阶段；正常Authorize→Redeem→Refresh链允许继续。
    pub operation: GrantOperation,
}

/// 签发前已读取的客户端投影；自定义原子存储必须与同单元的当前模型重比，不是展示缓存。
#[derive(Clone)]
pub struct ClientSnapshot {
    pub(crate) identifier: Value,
    pub(crate) entity: BridgeClient,
    pub(crate) encoded_secret: Option<String>,
    pub(crate) restrictions: Option<PhpArray>,
}

impl ClientSnapshot {
    /// 从所选模型采集同一投影；不证明身份或授权。
    pub fn capture(client: &dyn ClientModel) -> Result<Self> {
        let attributes = client.attributes()?;
        let restrictions = match attributes.get("scopes") {
            None | Some(Value::Null) => None,
            Some(_) => Some(PhpArray::try_from(
                client.get_attribute("scopes")?.unwrap_or(Value::Null),
            )?),
        };
        let encoded_secret = match openrust::database::eloquent::AttributeCast::String
            .read(client.get_attribute("secret")?)?
        {
            None | Some(Value::Null) => None,
            Some(Value::String(secret)) => Some(secret),
            Some(_) => return Err(invalid_binding()),
        };
        Ok(Self {
            identifier: client.model_key()?,
            entity: BridgeClient::from_client_model(client)?,
            encoded_secret,
            restrictions,
        })
    }

    /// 当前模型必须仍活动，且身份、secret、grant、redirect与scope投影均相同。
    pub fn matches(&self, current: &ClientRecord) -> Result<bool> {
        if current.revoked()? {
            return Ok(false);
        }
        let fresh = Self::capture(current.as_ref())?;
        Ok(self.identifier == fresh.identifier
            && self.entity == fresh.entity
            && self.encoded_secret == fresh.encoded_secret
            && self.restrictions == fresh.restrictions)
    }
}

/// 授权码协议持久快照；不是Eloquent模型，不包含交付给客户端的原始凭据。
#[derive(Clone)]
pub struct CodeRecord {
    pub(crate) id: String,
    pub(crate) client_id: String,
    pub(crate) user_id: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) expires_at: i64,
    pub(crate) authority: GrantAuthorityBinding,
}

/// OAuth access持久快照；签名与完整bearer不进入存储接口。
#[derive(Clone)]
pub struct AccessRecord {
    pub(crate) id: String,
    pub(crate) client_id: String,
    pub(crate) user_id: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) issued_at: i64,
    pub(crate) expires_at: i64,
    pub(crate) authority: GrantAuthorityBinding,
}

/// 新refresh记录与关联access快照；不是另一套长期用户会话。
#[derive(Clone)]
pub struct RefreshRecord {
    pub(crate) id: String,
    pub(crate) access: AccessRecord,
    pub(crate) expires_at: i64,
}

/// 同一原子操作内必须共同提交或回滚的access与可选refresh记录。
#[derive(Clone)]
pub struct TokenRecords {
    pub(crate) access: AccessRecord,
    pub(crate) refresh: Option<RefreshRecord>,
}

fn invalid_binding() -> openrust::Error {
    openrust::Error::custom_with_status(
        openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "passport_model_binding_invalid",
        "OAuth模型主键或属性与持久Schema不匹配",
    )
}

impl ClientSnapshot {
    /// 所选模型的原始主键，不按协议字符串猜列类型。
    pub fn identifier(&self) -> &Value {
        &self.identifier
    }
    /// 不携带secret的客户端协议实体。
    pub fn client(&self) -> &BridgeClient {
        &self.entity
    }
}
impl CodeRecord {
    /// 唯一授权码记录标识，不是原始code密文。
    pub fn id(&self) -> &str {
        &self.id
    }
    /// 已绑定客户端标识。
    pub fn client_id(&self) -> &str {
        &self.client_id
    }
    /// 已绑定用户标识。
    pub fn user_id(&self) -> &str {
        &self.user_id
    }
    /// 用户实际同意的原始scope；兑换前不提前finalize。
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }
    /// Unix秒期限。
    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }
    /// 必须由同原子单元权威验证的未验证载体。
    pub fn authority(&self) -> &GrantAuthorityBinding {
        &self.authority
    }
}
impl AccessRecord {
    /// 唯一access jti。
    pub fn id(&self) -> &str {
        &self.id
    }
    /// 已绑定客户端标识。
    pub fn client_id(&self) -> &str {
        &self.client_id
    }
    /// 已绑定用户标识。
    pub fn user_id(&self) -> &str {
        &self.user_id
    }
    /// 本次真正签发的scope。
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }
    /// Unix秒签发时刻。
    pub fn issued_at(&self) -> i64 {
        self.issued_at
    }
    /// Unix秒期限。
    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }
    /// 不得当作已完成认证的未验证载体。
    pub fn authority(&self) -> &GrantAuthorityBinding {
        &self.authority
    }
}
impl RefreshRecord {
    /// 唯一refresh记录标识，不是原始bearer。
    pub fn id(&self) -> &str {
        &self.id
    }
    /// 同次新access记录。
    pub fn access(&self) -> &AccessRecord {
        &self.access
    }
    /// Unix秒期限。
    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }
}
impl TokenRecords {
    /// 本次真正签发的access。
    pub fn access(&self) -> &AccessRecord {
        &self.access
    }
    /// 客户端支持refresh时的关联记录。
    pub fn refresh(&self) -> Option<&RefreshRecord> {
        self.refresh.as_ref()
    }
}

/// 从已认证refresh密文取得的原始绑定；不伪造负载中不存在的旧access时间。
#[derive(Clone)]
pub struct RefreshRedemption {
    pub(crate) id: String,
    pub(crate) client_id: String,
    pub(crate) access_token_id: String,
    pub(crate) user_id: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) expires_at: i64,
    pub(crate) authority: GrantAuthorityBinding,
}
impl RefreshRedemption {
    /// 唯一旧refresh记录标识。
    pub fn id(&self) -> &str {
        &self.id
    }
    /// 关联旧access标识。
    pub fn access_token_id(&self) -> &str {
        &self.access_token_id
    }
    /// 已绑定客户端标识。
    pub fn client_id(&self) -> &str {
        &self.client_id
    }
    /// 已绑定用户标识。
    pub fn user_id(&self) -> &str {
        &self.user_id
    }
    /// 旧授权的scope上界，不是本次任意请求值。
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }
    /// Unix秒期限。
    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }
    /// 必须按真实操作上下文核验的未验证载体。
    pub fn authority(&self) -> &GrantAuthorityBinding {
        &self.authority
    }
}
