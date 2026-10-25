# openrust（门面）

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：应用唯一依赖的 crate。

- re-export `openrust-core` 全部公共项；`openrust::http` = `openrust-http`；`openrust::ServiceExt` 与 `openrust::tokio` 供只依赖门面的应用做路由集成测试；`prelude` 含 `Application` / `AppBuilder` / `Error` / `Result` / `NoConfig` / `Inject` / `Validated` / `Routes` / `Resource` / `ResourceActions` / `Action` / `Created` / `NoContent` / `PageParams` / `Paginated` / `AppBuilderHttpExt` / OpenRust 的 `Json` / `Path` / `Query`、axum 的 `State` / `StatusCode`；原生 axum 类型可从 `http::axum` 访问。
- `run::<A>() -> ExitCode`：建 tokio 运行时 → `bootstrap` 执行应用显式注册的插件 → 第一个参数作为命令（默认 `serve`）→ 错误写 stderr 并返回失败码；应用需安装 `HttpPlugin` 才有 `serve` 命令。
- 尚无 feature；数据库等能力进入后以 `db-postgres` 等 feature 接入并在此 re-export。
