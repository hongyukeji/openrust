# ADR-003 数据库队列为默认，Redis 可选

- 状态：Accepted
- 日期：2026-10-25

## 背景

开箱即用要求 `openrust dev` 不引入非必要的基础设施。PostgreSQL 14+ 与 MySQL 8.0+ 都支持 `SELECT ... FOR UPDATE SKIP LOCKED`，足以实现可靠的单表队列。

## 决策

- 队列、会话、缓存默认不依赖 Redis：队列使用数据库 `jobs` / `failed_jobs` 表，会话使用数据库 store，缓存使用进程内 `moka`。
- Redis 作为 `queue-redis` / `cache-redis` feature 与 session store 选项提供，用于多实例共享与高吞吐场景。
- 队列引擎倾向自研（拉取、重试退避、延迟、失败表、优雅停机），范围有界且可完全控制 Schema 与双库行为；M0 Spike B 对比 `apalis` 后定稿。

## 影响

- 调度器多副本去重同样用数据库锁实现。
- 事件监听器的 `queued` 模式复用同一队列。

## 验证

待 M0 Spike B 补充：两库 `SKIP LOCKED` 行为、重试计数与延迟任务；`apalis` 对比结论。
