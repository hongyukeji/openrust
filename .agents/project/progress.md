# 当前交付与验证

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：R0 已固定上游；R1 按 ADR-072 完成同仓框架与应用 workspace，下一功能簇 R2 待开始。

根 openrust/src 与 examples/demo 位于同一仓库、同一 Cargo workspace，根 `default-members=["examples/demo"]` 默认启动应用，共用根 Cargo.lock；CLI 模板仍是新应用骨架唯一来源。原框架 Rust 源码保持不变，原 33 个测试 target 完整保留为根 6 个与 demo 27 个；注册表依赖 name/version/checksum 集合不变。

新布局实际通过 fmt、默认/all-features workspace Clippy/test/doc、deny、MSRV/no-default；默认 361 passed/26 ignored，全 feature 481 passed/32 ignored，显式库单测 146 passed。PostgreSQL 18.6/MySQL 8.4.13 各完成 11 targets/46 项真实集成、testing 1 项及最终 CI 两组 workspace 命令。三类干净生成应用 check/test、真实认证双库、默认 HTTP 启动与八个 consumer 的适用门禁通过。根与 demo 无参数 cargo run 均验证健康页、公开 HTML 与正常退出；根 CLI config show 转发默认应用通过。详见 [测试基线](memory/testing.md#r1-当前工程边界与验收入口)。

不同模型 gpt-6-sol 对最终代码只读复审，无 blocker/should-fix；审查未重跑构建或服务测试，实际执行证据由实现者记录。本任务根 target 与临时生成项目已清理；根 target 随后被后台重新生成约 344 MiB，归属未确认，连同预存 consumer 缓存保留，未中断共享服务或改配置。

旧 ADR-071 与中间布局的验收仅保留为历史输入。R2 尚未实现；本轮目录迁移不等于 Laravel 完整等价，真实复制/完整集群、SQLite 完整支持、供应商外部条件等继续按总计划逐项验收。
