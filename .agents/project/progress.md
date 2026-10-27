# 当前进度

> 全面重构状态：已记录 [ADR-070](decisions/adr-070-laravel-first-rebuild.md)、[总计划](plans/laravel-rebuild.md) 和 [执行任务](plans/laravel-rebuild-execution.md)；R0 尚未开始，下文均为重构前代码专项证据，不是新目标已验收。

> 受众：AI Agent
> 更新：2026-10-28
> 开发体验规划：M6 已细化为 [DX1–DX8](plans/v1-developer-experience.md)；服务端视图首个功能簇已实现并完成 workspace 与独立生成项目验收，其余开发体验工作仍待处理。
> 摘要：当前进入 Laravel 式全面重构规划交付；下一簇为 R0。原实现仍未通过完整生产/集群门禁。

## 本次规划交付

长期目标已明确为用 Rust 忠实实现 Laravel，持续跟进其框架功能与行为演进；总计划定义版本基线、差异登记、必要 Rust 适配与后继更新闭环。此处为方向与文档交付，不代表重构实现或上游同步已完成。

总计划、ADR-070 和执行任务已记录；AGENTS、执行路由、工程规范、相关旧 ADR 与两个项目技能已校准。gpt-6.1-sol 只读审查指出 R14 与 R15/R16 的矩阵门禁循环，已将 R14 限定为底座 C01–C31，C32/C33 留给后续阶段与整体关闭。仅完成文档检查，未运行代码测试、IDE 或服务验收。

## 重构前代码资产

- 根目录源码 workspace、启动入口、工具目录与生成项目流程已按 ADR-039、ADR-062、ADR-063 收敛；C1–C3 文档中文化已完成。
- 认证与安全、数据库/缓存/队列/存储、任务调度、国际化、数据交换、gRPC 与 WebSocket/SSE 等已完成相应功能簇验收。具体实现边界见 [`core/`](core/)，验收命令和证据见 [`memory/testing.md`](memory/testing.md)。
- 公共扩展沿用 Plugin、driver 和应用显式组合机制；没有另建平行的生命周期或通用钩子系统。
- 服务端视图按 ADR-069 提供可选 MiniJinja feature、应用模板目录、`ViewPlugin` / `Views` 和 `view:` 根目标；5 项 HTTP 行为测试、CLI 生成快照、`apps/example` 以及独立生成项目的 `cargo check` / `cargo test` 均通过。API、WebSocket 和静态资源路由保持独立。

## 重构前专项验收基线

原 M5 可观测性与生产资源的验收输入如下，未关闭部分已转入 R12/R14。ADR-060 密码哈希容量与 ADR-061 依赖 readiness 有专项验收；已实现低基数 Prometheus 指标、可选 OTLP HTTP trace、HTTP 并发上限，以及 HTTP/gRPC/worker/realtime 指标入口。真实 Collector 已收到 HTTP 与 HTTP→gRPC spans；metrics endpoint 经真实 TCP scrape；PostgreSQL 18 与 MySQL 8.4 上完成受限连接池登录突发并验证拒绝后恢复。开发机 HTTP 基线和 CPU/RSS 样本及受控过载/恢复样本已记录于 `tools/performance/README.md`；本簇仍待独立审查及 v1 完整生产/集群门禁。

既有实际验证（本轮规划未重跑）：`cargo fmt --all --check`、`git diff --check`、默认与 all-features 的 workspace all-target Clippy、默认与 all-features 的 workspace tests、warnings-denied all-features workspace Rustdoc、`cargo deny check` 均通过。默认 230 项 openrust 单测、all-features 270 项通过/2 项忽略；双库登录突发、metrics feature TCP scrape、带 OTLP Collector 的跨协议 HTTP/gRPC 测试另有真实服务记录。`cargo deny` advisories/bans/licenses/sources 通过，保留既有重复 crate 版本与未匹配 MPL allowance 警告。本次验证后按规则清理了本仓库 target。

后续收口修复：HTTP 并发许可与请求耗时指标保持到响应 body 完成或丢弃；默认及 `metrics`/`otel`/`grpc` 组合下 middleware 测试通过，metrics scrape 脱敏测试通过，组合 Clippy 与 Rustdoc 通过。target 收尾 dry-run 预览 544.9 MiB，清理后 target 不存在，磁盘可用空间 97 GiB。

## 剩余门禁

- 按 [`todo.md`](todo.md) 从 R0 开始执行，旧观测审查与生产资源证据纳入 R12/R14。
- 原 M6 CLI/测试/生成/文档与更新体验纳入 R1/R2/R6/R13，不按旧阶段排序。
- 验证完整多节点集群、最低支持版本及代表性生产性能；外部短信实发需另具备授权凭据和目标。

详细计划和验收入口见 [`plan.md`](plan.md)、[`plans/gen1-plan.md`](plans/gen1-plan.md) 与 [`memory/testing.md`](memory/testing.md)。Git 记录实现历史；本文件只保留当前状态。
