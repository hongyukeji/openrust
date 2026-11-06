//! 模块测试选择通过通用 Provider 蓝图回放，不向框架测试器加入模块特判。

use crate::ModuleProviderBuilderExt;
use openrust::foundation::ApplicationDefinition;
use openrust::testing::TestAppBuilder;

/// 测试应用的模块选择扩展；重建与 reset 使用同一 Provider 配置回调。
pub trait ModuleTestAppBuilderExt {
    /// 仅在当前测试蓝图禁用模块，不改激活器持久状态。
    fn remove_module(&mut self, name: impl Into<String>) -> &mut Self;
}
impl<A: ApplicationDefinition> ModuleTestAppBuilderExt for TestAppBuilder<A> {
    fn remove_module(&mut self, name: impl Into<String>) -> &mut Self {
        let name = name.into();
        self.configure_providers(move |providers| {
            providers.remove_module(name.clone());
        });
        self
    }
}
