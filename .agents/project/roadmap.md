# Roadmap

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：第一代之后的方向。当前执行任务只写 `todo.md`。

## 0.1（第一代）

框架底座：见 [第一代计划](plans/gen1-plan.md)。

## 0.2 候选

- JWT 无状态认证 driver（`auth-jwt` feature）。
- OAuth / 社交登录 Provider。
- 通知中心：邮件、短信、推送统一 `Notification` 抽象。
- SQLite 作为开发与测试数据库。
- 页面渲染脚手架与表单 CSRF 示例（在 `templates` feature 之上）。
- 缓存 tag 失效、读写分离连接池。
- WebSocket / SSE 约定。

## 更远

- 官方扩展包目录（基于 `Plugin` 的 crate 集合）。
- 多租户与数据范围（DataScope）扩展包。
- 管理后台脚手架。

## 维护规则

- 只描述版本目标，不记录完成历史。
- 候选能力进入开发前先拆入 `todo.md` 并在计划中界定范围。
