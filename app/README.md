# 应用业务模块

`app/` 保存业务模块。唯一综合示例位于 [example](example/README.md)，展示如何在 `app/<业务>/` 通过公开 `openrust::...` API 实现业务；框架核心 `src/` 不依赖这些业务模块。

根 `src/main.rs` 提供唯一 server 薄入口，编译期连接 `app/mod.rs`，由根 `bootstrap/app.rs` 与 `routes/` 显式装配。业务模块只保留实际业务文件，不复制 Cargo、入口、bootstrap、config、routes、database、resources 或 storage 的完整应用骨架，不通过目录扫描自动加载源码。

根 `config/`、`routes/`、`database/`、`resources/`、`public/` 与 `storage/` 只保留通用结构及公开加载/挂载。示例业务专有迁移、种子、模板、协议、生成脚本与测试 fixture 留在 `app/example`；根 `tests/` 的通用验收可通过公开入口挂载示例。从仓库根运行 `cargo run` 和 `cargo test`，完整 workspace 门禁使用 `--workspace`。数据库、授权、队列等按已有 feature 与应用配置启用真实服务，不为业务模块新增独立示例运行模式。
