# 当前进度

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：R0 上游/资产盘点与 R1 骨架/默认启动已交付并实际验收；下一功能簇 R2。原完整生产/集群缺口继续保留。

## 当前交付

- 开发方式已按最新指令改为主工作区 `main` 直接开发；R0/R1 提交与主分支的长期 Laravel 跟进、已有优秀机制融合要求一并保留。后续一个写入者串行推进 R2，审查并行只读，不新建隔离分支。

长期目标已明确为用 Rust 忠实实现 Laravel，持续跟进其框架功能与行为演进；总计划定义版本基线、差异登记、必要 Rust 适配与后继更新闭环。此处为方向与文档交付，不代表重构实现或上游同步已完成。

已有优秀设计保留并融合进 Laravel 对应职责；统一返回格式已明确归入 R3/R5 的 Response / Resource 与异常渲染，不改变当前代码行为。R0 需逐项登记增强价值与回归证据。

- R0：固定官方版本与 commit，102 章/750 二级节及三级行为矩阵、源码/测试/consumer 资产和真实回归函数索引；没有将原行为证据转记为新 contract 通过。
- R1：独立 framework library、根 application 单 server bin、Laravel 职责目录、独立生成应用、综合示例、consumer/CI/文档与共享 IDE run 配置已迁移。框架 Rust 源文件除位置外逐字一致；R2 以后再改领域语义。
- 默认/all-features workspace 门禁、双库各 6 targets/31 项、双库认证生成应用真实 HTTP、公开 cache driver consumer、MSRV 1.97 与 cargo deny 已实际通过；详细命令与边界见 [测试基线](memory/testing.md#r1-目录迁移验收基线)。
- RustRover 2026.2.4 同一 Command=run 的普通运行、src/main.rs:12 断点与继续后的真实 HTTP 已由 Agent 实际 UI 验收；共享配置不携带验收专用端口或开发机绝对路径。项目已关闭，IDE 重建产物再 clean 回收 2.2 GiB；最终磁盘余量约 125 GiB。
- R2 Container/Provider/TestApp、R4 真实复制、R7 新认证 contract、完整集群、SQLite、供应商/最低后端版本等按依赖继续，不能把 R1 迁移算作后继语义验收。

## 重构前代码资产

- 根目录源码 workspace、启动入口、工具目录与生成项目流程已按 ADR-039、ADR-062、ADR-063 收敛；C1–C3 文档中文化已完成。
- 认证与安全、数据库/缓存/队列/存储、任务调度、国际化、数据交换、gRPC 与 WebSocket/SSE 等已完成相应功能簇验收。具体实现边界见 [`core/`](core)，验收命令和证据见 [`memory/testing.md`](memory/testing.md)。
- 公共扩展沿用 Plugin、driver 和应用显式组合机制；没有另建平行的生命周期或通用钩子系统。
- 服务端视图按 ADR-069 提供可选 MiniJinja feature、应用模板目录、`ViewPlugin` / `Views` 和 `view:` 根目标；5 项 HTTP 行为测试、CLI 生成快照、`examples/example` 以及独立生成项目的 `cargo check` / `cargo test` 均通过。API、WebSocket 和静态资源路由保持独立。

## 重构前专项验收基线

原 M5 可观测性与生产资源的验收输入如下，未关闭部分已转入 R12/R14。ADR-060 密码哈希容量与 ADR-061 依赖 readiness 有专项验收；已实现低基数 Prometheus 指标、可选 OTLP HTTP trace、HTTP 并发上限，以及 HTTP/gRPC/worker/realtime 指标入口。真实 Collector 已收到 HTTP 与 HTTP→gRPC spans；metrics endpoint 经真实 TCP scrape；PostgreSQL 18 与 MySQL 8.4 上完成受限连接池登录突发并验证拒绝后恢复。开发机 HTTP 基线和 CPU/RSS 样本及受控过载/恢复样本已记录于 `tools/performance/README.md`；本簇仍待独立审查及 v1 完整生产/集群门禁。

既有实际验证（本轮规划未重跑）：`cargo fmt --all --check`、`git diff --check`、默认与 all-features 的 workspace all-target Clippy、默认与 all-features 的 workspace tests、warnings-denied all-features workspace Rustdoc、`cargo deny check` 均通过。默认 230 项 openrust 单测、all-features 270 项通过/2 项忽略；双库登录突发、metrics feature TCP scrape、带 OTLP Collector 的跨协议 HTTP/gRPC 测试另有真实服务记录。`cargo deny` advisories/bans/licenses/sources 通过，保留既有重复 crate 版本与未匹配 MPL allowance 警告。本次验证后按规则清理了本仓库 target。

后续收口修复：HTTP 并发许可与请求耗时指标保持到响应 body 完成或丢弃；默认及 `metrics`/`otel`/`grpc` 组合下 middleware 测试通过，metrics scrape 脱敏测试通过，组合 Clippy 与 Rustdoc 通过。target 收尾 dry-run 预览 544.9 MiB，清理后 target 不存在，磁盘可用空间 97 GiB。

## 剩余门禁

- 按 [`todo.md`](todo.md) 从 R2 继续执行，旧观测审查与生产资源证据纳入 R12/R14。
- 原 M6 CLI/测试/生成/文档与更新体验纳入 R1/R2/R6/R13，不按旧阶段排序。
- 验证完整多节点集群、最低支持版本及代表性生产性能；外部短信实发需另具备授权凭据和目标。

详细计划和验收入口见 [`plan.md`](plan.md)、[`plans/gen1-plan.md`](plans/gen1-plan.md) 与 [`memory/testing.md`](memory/testing.md)。Git 记录实现历史；本文件只保留当前状态。

当前重构来源与 102 章/750 子节盘点见 [固定基线](plans/laravel-rebuild-baseline.md)；未将旧证据继承为新 contract 通过。
