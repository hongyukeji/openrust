# openrust（门面）

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：应用唯一依赖的 crate。

- 在 crate 根直接 re-export `ApiResponse<T>`、`PageMeta` 与 `#[openrust::resource]`；`openrust::http` = `openrust-http`；`openrust::ServiceExt` 与 `openrust::tokio` 供只依赖门面的应用做路由集成测试；`prelude` 含 `Application` / `AppBuilder` / `Error` / `Result` / `NoConfig` / `Inject` / `Validated` / `Routes` / `Resource` / `ResourceActions` / `Action` / `Created` / `NoContent` / `PageParams` / `Paginated` / `AppBuilderHttpExt` / OpenRust 的 `Json` / `Path` / `Query`、axum 的 `State` / `StatusCode`；原生 axum 类型可从 `http::axum` 访问。
- `run::<A>() -> ExitCode`：建 tokio 运行时 → `bootstrap` 执行应用显式注册的插件 → 第一个参数作为命令（默认 `serve`）→ 错误写 stderr 并返回失败码；应用需安装 `HttpPlugin` 才有 `serve` 命令。
- `db-postgres` / `db-mysql` feature 可选依赖并 re-export `openrust-db` 为 `openrust::database`，同时将 `DatabaseConfig` / `DatabasePlugin` / `Db` / `MigrationPlugin` 提供在门面根与 prelude；SeaORM migration API 位于 `openrust::database::sea_orm_migration`；默认 feature 不带数据库 driver。
