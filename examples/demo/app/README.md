# 应用业务模块

业务 Rust 模块放在 `app/`，由 `src/main.rs` 的编译期模块声明连接。`bootstrap/app.rs` 负责应用装配，`routes/web.rs` 负责路由；不扫描或动态加载 Rust 源码。

配置位于 `config/`，数据库迁移/种子位于 `database/`，视图与前端资源位于 `resources/`，公开文件位于 `public/`，运行数据位于 `storage/`。框架库位于仓库根 `src/`，应用通过 `openrust::...` 公共 API 使用。

在本 demo 应用根目录运行 `cargo run`；`cargo test` 发现本应用集成测试。框架 workspace 的单元/公开契约及工具测试从仓库根另行执行。独立生成应用使用同样职责目录和自己的 workspace，允许在指定目标目录创建；生成器不会注册为框架 workspace 成员。
