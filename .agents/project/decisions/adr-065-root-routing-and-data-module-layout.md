# ADR-065 框架路由入口与数据模块分层

> 替代范围见 [ADR-070](adr-070-laravel-first-rebuild.md#替代范围)；本文旧目录/命名/默认值不阻止全面重写。代码是否已迁移以当前实现和 todo.md 为准。

- 状态：Partially Superseded（目标设计由 ADR-070 重新开放；未替代的行为/验收事实保留）
- 日期：2026-10-27
- 关联：[ADR-021](adr-021-loco-style-module-layout.md)、[ADR-063](adr-063-explicit-app-module-registry.md)、[ADR-064](adr-064-canonical-example-application.md)

## 背景

根宿主的 `apps/home.rs` 让框架级首页处理器看起来像业务模块；路由 registry 和 HTTP plugin 实现在 `src/http/plugin.rs`，框架顶层没有明显的路由装配入口。数据导入/导出代码放在 `src/data_exchange/`，与按领域分层的 `src/data/exchange/` 不一致。

## 决策

- 将路由注册契约、`RouteRegistry`、`AppBuilderHttpExt`、`HttpPlugin` 与最终 router 装配实现集中在 `src/routes.rs`；`src/http/plugin.rs` 仅保留兼容 re-export，`src/http/routes.rs` 继续负责 Axum `Routes` 构建器。
- 路由扩展复用 `AppBuilderHttpExt::routes` 的顺序注册、`http_layer` 的 Tower middleware 与现有启动错误语义；不新增平行 Hooks。业务 route builder 仍由 `Application::build` 或 `Plugin::build` 显式注册。
- 根集成宿主的 `/` 信息路由实现迁入框架 `src/routes.rs`，由宿主显式选择注册；生成业务应用继续自行定义 `/`。
- 数据交换源码迁至 `src/data/exchange/`。新内部引用使用 `crate::data::exchange`；`openrust::data_exchange` 通过 re-export 保持兼容。
- `apps/home.rs` 删除；应用目录只保留有明确业务模块职责的源码，不用 `home` 命名框架默认路由。

## 后果与验收

- 路由 builder 按注册顺序运行；冲突仍在启动装配时报告为 `http_route_registration_failed`。路由 callback 为同步构建闭包；异步初始化仍由插件 `finish` 负责，不在 route hook 中执行。
- `HttpPlugin`、`RouteRegistry`、`AppBuilderHttpExt` 和 `build_router` 的现有公开路径保持可用。
- 根 `/` 与 `/health` 的统一响应、扩展路由及 Tower layer 集成测试通过；`data-exchange` 默认/任务 feature 和 consumer 均可编译，旧 API 路径及文档链接可解析。
