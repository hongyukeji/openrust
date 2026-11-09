# Laravel 对照基线

核对日期为 2026 年 11 月 10 日。以固定源码和上游测试核对可观察行为，以固定官方文档导航组织三语指南。

| 用途 | 固定版本 | 提交 |
|---|---|---|
| 框架行为、默认值和测试 | [Laravel Framework v13.40.0](https://github.com/laravel/framework/releases/tag/v13.40.0) | [`1449bf241017087fbd38c118ecaabef3f6797fdc`](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc) |
| 应用目录、配置和启动入口 | [Laravel 应用骨架 v13.11.0](https://github.com/laravel/laravel/releases/tag/v13.11.0) | [`06d016a364a37430eec9cbc52209adce3d7667ce`](https://github.com/laravel/laravel/tree/06d016a364a37430eec9cbc52209adce3d7667ce) |
| 三语文档导航 | Laravel 13.x 固定文档快照 | [`960eedf0911155ad16326ade49c0ea5d18eda0b8`](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/documentation.md) |

应用骨架独立发布。官方仓库没有应用骨架 `v13.40.0` 标签，现用 `v13.11.0` 的框架依赖约束为 `^13.17`，覆盖框架 `v13.40.0`。 [应用骨架依赖清单](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/composer.json)

## 框架 v13.39.0 至 v13.40.0

官方比较包含 47 个提交、105 个变更文件，其中 47 个位于 `tests/`。框架 `config/` 没有文件变化。下列差异在 OpenRust 实现对应功能时纳入验收。 [官方版本比较](https://github.com/laravel/framework/compare/v13.39.0...v13.40.0)

| 范围 | 应核对的行为 |
|---|---|
| 并发进程 | 子进程任务的普通输出不能冒充结果。验收应覆盖伪造结果、校验失败和任务异常传播。 [上游安全修复](https://github.com/laravel/framework/pull/62297) |
| 数据库 | 核对嵌套事务的并发错误回滚到保存点、事务中丢失连接后的重连、`BelongsToMany::createOrFirst` 使用写连接，以及各数据库的查询和 Schema 差异。 [保存点修复](https://github.com/laravel/framework/pull/62248) · [重连修复](https://github.com/laravel/framework/pull/62211) · [写连接修复](https://github.com/laravel/framework/pull/62223) |
| HTTP 中间件 | 核对 `replace` 支持多个替换项，以及 `insertBefore`、`insertAfter` 的插入位置和执行顺序。 [替换接口](https://github.com/laravel/framework/pull/62230) · [插入接口](https://github.com/laravel/framework/pull/62261) |
| 队列与调度 | 核对任务运行时间超过设定阈值时的回调，以及定时任务已在其他节点执行时的跳过事件。 [队列时长回调](https://github.com/laravel/framework/pull/62141) · [调度时长回调](https://github.com/laravel/framework/pull/62233) · [调度跳过事件](https://github.com/laravel/framework/pull/62256) |
| 其他回归 | 随功能核对分页契约、Redis 标签缓存清除、图像验证默认规则、请求数据严格读取和加密广播频道。 [完整发布记录](https://github.com/laravel/framework/releases/tag/v13.40.0) |

## 文档导航核对

将固定快照与 Laravel 文档 `13.x` 的提交 `59da4453c4a39816246a7450c93aef1f645abf66` 比较。上游变更了九个正文文件，`documentation.md` 内容逐字节相同。三语导航继续使用原固定快照的章节顺序和页面路径。 [文档版本比较](https://github.com/laravel/docs/compare/960eedf0911155ad16326ade49c0ea5d18eda0b8...59da4453c4a39816246a7450c93aef1f645abf66)
