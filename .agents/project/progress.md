# 当前进度

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：M0 已完成，M1 进行中；HTTP 与仓库聚合启动入口已实现。

| 项 | 状态 |
| --- | --- |
| 第一代计划与框架设计 | 已定稿，见 `plans/` |
| ADR-001 至 ADR-007 | 已接受 |
| 仓库基线（许可证、README、工具链声明、Agent 契约、CHANGELOG、贡献与安全政策） | 已就位 |
| workspace、lint 基线、CI 工作流 | 已就位；CI 的双库 job 预置但禁用，M2 启用 |
| `openrust-core` | 已实现并测试：配置分层、错误与 Problem 渲染、注册表、plugin 级联、空格嵌套命令 |
| `openrust-http` | 已实现并测试：`Routes`、`Resource` / `ResourceActions`、`Created` / `NoContent` / `PageParams` / `Paginated`、`Inject`、`Validated`、含 gzip/br 与 CORS 的中间件栈、`/health`、`serve` |
| `openrust` / `openrust-cli` | 门面 `run` 与 CLI 转发可用 |
| `apps/server` | M1 聚合入口已实现：默认 HTTP、配置文件、`/health`、根路由与无端口集成测试 |
| 其余 8 个 crate | 空壳，README 标注规划 surface 与里程碑 |
| 验证证据 | 本机 `cargo fmt / clippy -D warnings / test / doc` 通过，见 `memory/testing.md` |
