//! 同一授权服务器可替换的原子工作单元；不另建Auth或Model体系。

use crate::GrantStorageResult;
use openrust::support::BoxFuture;

use crate::{ClientRecord, ClientSnapshot, CodeRecord, RefreshRedemption, TokenRecords};

/// 授权码与刷新操作的完整存储契约，默认实现复用官方表和框架主库事务。
///
/// 受信实现必须在自己真实的原子context中完成当前客户端校验、条件消费、
/// 实际scope和同一委托权威末检、全部新记录写入及失败回滚。最后一次时钟判断
/// 必须位于所有await和直接提交前事件之后、开始commit之前，不只是入口预检。
/// 使用其它后端时需要同等适用证据；本trait不会自动证明第三方存储的原子性。
///
/// 仅当可信后端明确确认本次尝试未提交或已完整回滚，才能返回
/// `GrantStorageError::IdentifierCollision`，准确指明本次新记录的kind。
/// 普通唯一约束错误和提交结果未知仍属基础设施失败，不自动重试。
///
/// 自定义context由存储持有，可以使用`GrantAuthority<C>`核验，不伪造默认
/// `DatabaseTransaction`。默认DB的单主库限制不是对所有后端的隐藏硬依赖。
/// 这不是Eloquent Model保存API；官方模型生命周期和创建/撤销事件仍须对应。
///
/// 取消发生在提交前应回滚消费及全部写入。开始提交后的取消或网络失败可能
/// 结果未知；实现和调用方不得自动重复兑换、返回未确认的bearer或声称已回滚。
pub trait GrantStorage: Send + Sync {
    /// 签发服务解析时检查本存储的授权前置。只读资源验证不会调用此方法。
    /// 自定义存储的权威由自身原子操作保证，默认没有额外装配检查。
    fn require_issuance_authority(&self) -> BoxFuture<'_, openrust::Result<()>> {
        Box::pin(async { Ok(()) })
    }

    /// 受信时钟；实现还须在锁后和最终commit边界重新取时。
    fn current_time(&self) -> GrantStorageResult<i64>;

    /// 默认客户端lookup复用官方Scope once；自定义Store沿自身读取契约。
    /// 缓存结果不是提交安全事实，正式操作仍须在原子单元内重验当前模型。
    fn client_by_protocol<'a>(
        &'a self,
        identifier: &'a str,
    ) -> BoxFuture<'a, GrantStorageResult<Option<ClientRecord>>>;

    /// 早期只读授权码状态；不存在或revoked=true为true，读取失败必须返回错误。
    /// 不用缓存未撤销或忙时false代替真实读取与失败。
    fn is_code_revoked<'a>(&'a self, id: &'a str) -> BoxFuture<'a, GrantStorageResult<bool>>;

    /// 早期只读refresh状态；false不是后续提交证明，不读取旧access来替代。
    fn is_refresh_revoked<'a>(&'a self, id: &'a str) -> BoxFuture<'a, GrantStorageResult<bool>>;

    /// 用户同意后发码；核实际subject/client/scopes及Authorize权威上下文。
    fn persist_code<'a>(
        &'a self,
        code: &'a CodeRecord,
        client: &'a ClientSnapshot,
    ) -> BoxFuture<'a, GrantStorageResult<()>>;

    /// 条件消费同一code且原子写入全部新记录；核最终签发scope的Redeem上下文。
    fn exchange_code<'a>(
        &'a self,
        code: &'a CodeRecord,
        client: &'a ClientSnapshot,
        tokens: &'a TokenRecords,
    ) -> BoxFuture<'a, GrantStorageResult<()>>;

    /// 同一委托刷新；默认撤销旧refresh，显式关闭时不能声称该模式单次消费。
    fn rotate_refresh<'a>(
        &'a self,
        refresh: &'a RefreshRedemption,
        client: &'a ClientSnapshot,
        tokens: &'a TokenRecords,
        revoke_old: bool,
    ) -> BoxFuture<'a, GrantStorageResult<()>>;

    /// 从权威存储核验access记录是否撤销；不存在必须视为已撤销。
    fn is_access_revoked<'a>(&'a self, id: &'a str) -> BoxFuture<'a, GrantStorageResult<bool>>;

    /// 撤销指定access；不将此方法冒称完整用户退出或全部refresh撤销。
    fn revoke_access<'a>(&'a self, id: &'a str) -> BoxFuture<'a, GrantStorageResult<()>>;
}
