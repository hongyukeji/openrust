# Roadmap

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：v1 版本之后的方向。当前执行任务只写 `todo.md`。

## v1 版本

框架底座：见 [v1 版本计划](plans/gen1-plan.md)。

## v1 之后的候选

- JWT 无状态认证 driver（`auth-jwt` feature）。
- OAuth / 社交登录 Provider。
- 通知中心：在 v1 版本邮件/短信独立能力之上，评估多渠道编排、站内信与推送；v1 版本短信范围见 [ADR-011](decisions/adr-011-sms-and-communication-baseline.md)。
- SQLite 作为开发与测试数据库。
- 页面渲染脚手架与表单 CSRF 示例（在 `templates` feature 之上）。
- 缓存 tag 失效、读写分离连接池。
- SSE 与跨实例实时广播；WebSocket 基础能力已纳入 v1，见 [通用能力计划](plans/v1-common-capabilities.md)。

## 更远

- 官方扩展包目录（基于 `Plugin` 的 crate 集合）。
- 多租户与数据范围（DataScope）扩展包。
- 管理后台脚手架。

## 维护规则

- 只描述版本目标，不记录完成历史。
- 候选能力进入开发前先拆入 `todo.md` 并在计划中界定范围。
