# 应用代码

`app/` 可承载 Laravel 式应用控制器、模型和 Provider；`modules/<名称>/` 是可选并存的业务组织。综合示例位于 [`example` 分支](https://github.com/hongyukeji/openrust/tree/example/modules/example)，无需把普通应用业务放入模块。两处代码共用框架的 Container、ServiceProvider 生命周期和路由机制。

根 `main.rs` 是薄 server 入口。框架启动链从同次配置仓储装载默认 Provider，随后接入显式编译的包 Provider、`bootstrap/providers.rs` 的应用 Provider 与模块；`bootstrap/app.rs` 保持启动和模块装配。`app/mod.rs` 只声明 Rust 编译模块。应用控制器在 `app/http/controllers/` 编译声明后由 `routes/web.rs` 或 `routes/api.rs` 注册；模型在 `app/models/`；Provider 类型在 `app/providers/`，使用公开的 `ServiceProvider::register` 与 `boot`。这些 `mod.rs` 是 Rust 编译所需适配，不是另一套运行时。

生成应用可用 `openrust make:controller PostController`、`openrust make:model Post`（需数据库选项）和 `openrust make:provider CatalogServiceProvider`。控制器由开发者选择具体处理函数并在路由注册；Provider 生成后写入 `bootstrap/providers.rs`。生成器拒绝覆盖同名文件。`openrust generate model` 仍保留原字段驱动实体生成路径；`make:model` 创建应用根 `app/models/` 中的模型，可省略字段。

根 `config/`、`routes/`、`database/`、`resources/`、`public/` 和 `storage/` 保持应用职责。新增模块在 `modules/mod.rs` 声明编译入口，并通过应用模块清单选择启用。数据库、认证、队列等能力仍需对应 Cargo feature、配置与实际服务。

根 `config/*.rs` 是可编辑的 Rust 配置函数，由 `config/mod.rs` 显式声明和登记。`ApplicationDefinition::config()` 把清单接入同一配置仓储；函数接收同次 dotenv 快照的 `ConfigContext`，可构造类型化值、条件分支和明确的环境默认值。`APP_DEBUG` 经严格布尔解析，非法值会使启动失败；部署默认关闭服务端错误详情。TOML 可通过公开来源 API 显式附加，默认启动不扫描 TOML。

根应用 API 前缀由 `config/api.rs` 的 `ApiConfig.prefix` 决定，默认 `/api`，部署变量为 `API_PREFIX`；`AppServiceProvider` 从同一仓储读取它，同时配置挂载与 Handler 的统一错误包装，不需手动同步两个字面量。`/v1` 不匹配 `/v10`；空值挂载到根。模块可经 `or_api_envelope_when` 保留自身路径的策略，HTML/Web 继续使用内容协商。

`GET /api` 的应用信息由根应用 Controller 提供；`routes/api.rs` 声明相对API路径，`bootstrap/app.rs` 用公开 API 路由组配置有效前缀，`routes/web.rs` 承担应用 Web 路由，`routes/mod.rs` 是 Rust 所需的模块导出。`AppServiceProvider` 在 boot 时登记它们；框架 HTTP Provider 本身不提供该应用端点。禁用业务模块不移除宿主 API。

`bootstrap/app.rs` 保存应用启动配置和 `ApplicationDefinition` 装配。宿主与模块资源的测试主体放在 `tests/server/`，bootstrap 中只保留访问私有宿主所需的测试模块挂载；该编译适配不增加生产公共 API。
