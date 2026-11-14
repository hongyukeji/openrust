# 目录结构

## 应用目录

根目录的 `main.rs` 连接 Tokio 和 `bootstrap::createApplication`。`Cargo.toml` 与 `Cargo.lock` 声明应用、工作区和锁定依赖。应用代码位于 `app/`，编译型配置加载器位于 `config/`，Web 和控制台路由声明位于 `routes/`。

框架源码位于 `vendor/openrust/framework/`。视图位于 `resources/views/`，网站文件位于 `public/`，公共存储磁盘位于 `storage/app/public/`。测试放在根应用或所属 crate 的 `tests/` 目录。

## Bootstrap 目录

应用的 `bootstrap/` 目录存放构造应用所需的装配声明。

`bootstrap/app.rs` 声明配置、路由、中间件和命令入口。`bootstrap/mod.rs` 向二进制入口导出 `bootstrap::createApplication`。`bootstrap/providers.rs` 列出应用的[服务提供者](../architecture-concepts/providers.md#注册服务提供者)。

装配入口通过 `withRouting` 声明 Web 路由、控制台路由和健康路径。Rust 用 `Routing` Builder 表达这些选项，并通过 `withConfig`、`withProviders` 和 `withCommands` 注册编译进应用的配置、Provider 和命令。

框架使用这些声明的顺序见[请求生命周期](../architecture-concepts/lifecycle.md)。
