//! 应用与包共享的唯一 Provider 扩展契约。

use super::BoxFuture;
use crate::Result;
use crate::foundation::BootContext;
use crate::foundation::ProviderId;
use crate::foundation::RegisterContext;

/// 按清单顺序注册绑定，再按同一顺序启动贡献的扩展入口。
pub trait ServiceProvider: Send + Sync + 'static {
    /// 具体类型身份；不以盒装 trait 对象身份去重。
    fn id(&self) -> ProviderId {
        ProviderId::of::<Self>()
    }

    /// 声明必须先于当前 Provider 出现在清单中的依赖。
    fn dependencies(&self) -> Vec<ProviderId> {
        Vec::new()
    }

    /// 声明需参与静态依赖预检的子 Provider 身份，不自动执行它们的生命周期。
    ///
    /// 声明必须无副作用；register 返回前须实际登记或复用这些子项。
    /// Rust 无法在执行异步回调前反射其注册类型；没有外部类型依赖时可保留空声明。
    fn nested_providers(&self) -> Vec<ProviderId> {
        Vec::new()
    }

    /// 声明运行元数据可能选择的已编译子身份；不承诺实际登记，也不执行工厂。
    ///
    /// 候选只在当前项之后参与类型可达性预检；依赖方执行前仍检查真实 Registry。
    /// 用于无法在异步 register 前反射的条件加载，不能代替必登记的 `nested_providers`。
    fn possible_nested_providers(&self) -> Vec<ProviderId> {
        Vec::new()
    }

    /// 登记绑定、配置 schema 与默认值；显式解析沿同一容器，路由贡献留在 boot。
    fn register<'a>(&'a self, _app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    /// 所有 Provider 注册且基础配置验证后依序启动。
    fn boot<'a>(&'a self, _app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
