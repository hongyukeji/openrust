# 应用装配

`app/` 保留应用级装配；唯一综合业务示例位于 [`modules/example/`](../modules/example/README.md)。根 `src/main.rs` 只声明已编译模块与薄 server 入口，`bootstrap/app.rs` 通过 `Application::providers` 和 `Application::modules` 选择框架 Provider 与本次启动启用的模块。

根 `config/`、`routes/`、`database/`、`resources/`、`public/` 与 `storage/` 提供通用应用职责；示例业务迁移、种子、视图、语言资源、协议、生成脚本和测试 fixture 均在 `modules/example/`。增加业务模块需在 `modules/mod.rs` 声明编译入口并在应用清单显式启用；运行状态只选择已编译模块，不热加载 Rust 源码。

从仓库根运行 `cargo run` 与 `cargo test`。数据库、认证、队列等能力还需对应 Cargo feature、配置与外部服务；不会因启用模块而回退到内存模拟实现。
