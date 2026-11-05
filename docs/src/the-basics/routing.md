# 路由

## 基本路由

在路由文件中声明方法、相对 URI 和处理器。例如根应用的 API 入口：

```rust
pub(crate) fn register(routes: openrust::http::Routes) -> openrust::http::Routes {
    routes.get("/", crate::app::http::controllers::application_info::index)
}
```

### 默认路由文件

`routes/web.rs` 声明 Web 路由，`routes/api.rs` 声明 API 路由；`routes/mod.rs` 是 Rust 显式模块声明。`bootstrap/app.rs` 统一装配路由文件、路由组和前缀。普通应用直接在根 `app/` 编写控制器；业务模块也使用同一公开路由机制，启用模块不是根 API 存在的前提。

### API 路由

固定 Laravel 的 `withRouting(apiPrefix: 'api/admin')` 示例将 API 前缀改为 `api/admin`；公开默认值为 `api`，实际 URL 分别为 `/api` 和 `/api/admin`。路由文件不重复写此前缀，`api` 中间件组也只在启动装配时挂载一次。

本项目按已批准的配置扩展，在可编辑的 `config/api.rs` 声明同一个前缀，由 `bootstrap/app.rs` 读取并挂载；Laravel 原生配置位置是应用的 `bootstrap/app.php`，没有对应的内置 `config/api.php`。下例保留官方案例的值，仅适配 Rust 配置语法：

```rust
use openrust::config::{ApiConfig, ConfigContext, ConfigError};

pub fn config(ctx: &ConfigContext) -> Result<ApiConfig, ConfigError> {
    Ok(ApiConfig {
        prefix: ctx.env_or("API_PREFIX", "api/admin"),
        ..ApiConfig::default()
    })
}
```

部署也可通过 `.env` 覆盖同一值：

```dotenv
API_PREFIX=api/admin
```

以上两种方式都进入唯一配置仓储。根信息端点相对路径为 `/`，因此实际访问 `GET /api/admin`。根异常分类从同一 `ApiConfig` 判断 API 范围，无需另外改 Controller 或异常 Handler；`/api/admin2` 不属于 `/api/admin` 的子路径。

前后斜线按路由组语义归一化，`api/admin`、`/api/admin/` 挂载结果一致。显式空值或纯斜线挂载到根，此时错误范围覆盖应用根路径。普通 `{tenant}` 参数段可用于前缀；末尾 catch-all 无法继续挂子路由，启动时明确拒绝。当前底层匹配器最多支持 25 个普通参数（前缀与子路由合计），此为待解除的后端差距，不能视为 Laravel 或 Rust 的固有限制。

API 组不自动变成 Web Session 路由，需要会话的路由显式使用 `web` 组。固定 Laravel 的 API 安装与 Sanctum 流程、OpenRust 当前生成选项以及显式 JWT API profile 是不同对应范围；自有 JWT 不作为 Laravel API 默认认证方式。

## 尚待闭合的章节

本章先覆盖基本入口、默认路由文件、API 组和自动前缀。完整可用方法、依赖注入、路由参数、命名路由、路由组、模型绑定、fallback、限流及路由缓存，仍须按固定官方章节顺序逐项补齐生产行为、指南和验收；已有同名 API 或本页示例不证明这些范围全部完成。

固定来源：[Laravel 13.x 路由章节](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/routing.md)。路由配置的项目扩展已在上文单列，不冒称官方原生目录。
