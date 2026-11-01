//! 已编译模块文件初始化；运行期声明只选择此回调，不执行磁盘上的 Rust 源码。

use openrust::Result;
use openrust::core::{ApiResponse, BoxFuture, ModuleFile, RegisterContext};
use openrust::http::Inject;
use std::sync::Arc;

pub(crate) struct ExampleModuleInit;

/// HTTP 可观察的初始化结果；同时证明文件回调位于业务 Provider 注册之后。
#[derive(Clone, serde::Serialize)]
pub(crate) struct ModuleInitializationMarker {
    initialized: bool,
    business_registered: bool,
}

impl ModuleFile for ExampleModuleInit {
    fn initialize<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let modules = app.resolve::<openrust::core::ModuleRepository>().await?;
            let marker = ModuleInitializationMarker {
                initialized: modules.find("example").is_some(),
                business_registered: app.is_bound::<super::services::ExampleMessage>(),
            };
            app.instance(Arc::new(marker)).await
        })
    }
}

/// 只读展示显式编译初始化的结果，不暴露文件路径或执行外部代码。
pub(crate) async fn show(
    Inject(marker): Inject<ModuleInitializationMarker>,
) -> ApiResponse<ModuleInitializationMarker> {
    ApiResponse::success((*marker).clone())
}
