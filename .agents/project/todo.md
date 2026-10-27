# 活跃待办

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：Laravel 式全面重构的唯一活跃队列；实现尚未开始。

## 领取规则

先核对 Git/WIP、代理/进程、[执行校准](plan.md#执行校准) 与 [ADR-070](decisions/adr-070-laravel-first-rebuild.md)。每次一个主功能簇，完成 Spec → Implement → Verify → Commit 后再领取后继簇。整体依赖见 [R0–R16](plans/laravel-rebuild.md#5-按依赖实施)，不按旧 M5/M6 顺序执行。

## 当前可领取

| 功能簇 | 状态 | 交付与验收 |
| --- | --- | --- |
| R0 上游与资产基线 | 待开始 | 固定 Laravel/Goravel 版本与 commit；逐章能力/子行为矩阵；逐模块代码、调用方、回归与服务证据清单；复用/适配/重写及旧规则替代清单；归属/依赖/外部缺口明确。按长期上游跟进规则确定后继版本差异登记与必要 Rust 适配证据。文档门禁通过后提交并立即领取 R1，不把全量重构停在盘点 |

## 后继路由

R1 骨架/默认启动 → R2 内核/测试底座 → R3 HTTP → R4 数据 → R5 Model → R6 Factory/SQLite → R7 安全 → R8 Cache/Storage → R9 视图/通信 → R10 异步/事务 → R11 网络/组合 → R12 搜索/特性/观测 → R13 开发体验 → R14 底座门禁 → R15 V2 → R16 已纳入生态包。阶段内部按总计划依赖拆成可验证的提交。

## 必须带入新矩阵的现有缺口

- 统一返回格式等已有优秀机制 → R0 登记对应 Laravel 职责与保留价值，R3/R5 在 Response / Resource / 异常渲染融合并回归，见[融合计划](plans/laravel-rebuild.md#已有优秀机制的融合)；重写不等于删除能力。
- 原 M5 可观测性/资源独立审查与完整生产门禁 → R12/R14。
- 原 DX1–DX8、F9/F10：默认 run/debug、TestApp/assertions、factory、命令/生成/stubs、只读 doctor、dev 编排、文档和更新体验 → R1/R2/R6/R13。
- 双库负载均衡、多 API/worker/scheduler、共享状态/文件/实时、滚动升级/依赖故障、最低版本与性能 → R14。
- [SQLite 完整支持](plans/v1-sqlite-backend.md) → R6 及各能力适用验收；不能只接通连接。
- [V2 后台/App](plans/v2-admin-platform.md) → R15；PC 仅规划；独立 AI 产品未启动。

## 外部条件与证据缺口

Twilio 实发缺授权账号/凭据/目标；其他供应商与生态包实测条件在 R0 盘点。遇到外部缺口继续无依赖工作，但不能标记真实发送、支付、完整生态或全部验收完成。
