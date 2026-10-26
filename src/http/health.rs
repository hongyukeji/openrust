//! 存活探针端点。

use crate::core::ApiResponse;
use serde::Serialize;

use crate::http::routes::Routes;

/// `GET /health` 的响应正文。
#[derive(Debug, Serialize)]
pub struct Health {
    /// 进程能够响应时始终为 `"ok"`。
    pub status: &'static str,
}

/// 存活探针：只要进程能够处理请求就返回成功。依赖项检查应由能力 crate 提供的就绪探针端点负责。
pub async fn live() -> ApiResponse<Health> {
    ApiResponse::success(Health { status: "ok" })
}

/// 注册 `GET /health`。
pub fn routes(routes: Routes) -> Routes {
    routes.get("/health", live)
}
