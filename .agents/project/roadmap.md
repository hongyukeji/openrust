# 正式版开发路线图

记录日期为 2026 年 11 月 10 日，11 月 11 日恢复推进。00-01 启动基线与 00-02 默认 HTTP 已完成各自范围的隔离验收，00-03 Artisan 和发布文档也已完成本单元验收，01-01 容器与 Provider 基础也已验收，下一单元为 01-02。按用户最新授权继续依赖已满足的单元，阶段验收后可自主本地提交，推送仍等待授权。

## 先收尾启动基线

记录时的 Git 基线为 `197e7a0e`。启动、默认 HTTP、Artisan 及基础容器现已按各阶段候选本地提交；其他模块工作区改动保留。

1. 汇总最新版 `config/`、启动装配、默认 Web 和 Artisan 路由的框架依赖、回归测试和三语文档。按最新基线重建最小提交清单，区分运行必需文件与测试、文档，隔离其他模块的改动。
2. 用干净副本验证锁定离线构建。实测 HTTP 首页、健康检查、静态挂载顺序、HTML／JSON 异常、`./artisan inspire`、帮助和发布版入口。
3. 核对逐文件 tree 与实际验收结果，按用户最新授权完成本地阶段提交，汇报范围；推送单独授权。

## 后续交付顺序

开发按依赖顺序推进，三语文档导航沿用固定 Laravel 13.x 的章节顺序。[分批交付计划](plan.md)列出可独立验收的单元、Laravel 章节锚点和三语正文路径。[逐页对应表](plans/laravel-docs-delivery-map.md)可从上游页面反查交付单元，并登记需单独确认的生态范围。

| 阶段 | 模块 | 定稿验收 |
| --- | --- | --- |
| 0 | 最小启动基线 | 按前述清单收尾。后续能力通过现有应用装配入口接入。 |
| 1 | 框架基础、日志与异常 | 核对 Container、Provider、Facade、Contracts 的生命周期和应用隔离。接通[日志、Context 与异常 Handler](core/logging-exceptions.md)，实测 HTTP 和 Artisan 报告、默认日志及响应脱敏。 |
| 2 | 数据库基础与通用集合 | [数据库](core/database.md)依次交付连接、事务、Query Builder、Schema、迁移与填充。验收默认 SQLite 的迁移与读写，按项目约定用 MySQL 8.4 做日常验收。[Collection 和 LazyCollection](core/collections.md)先于 Eloquent 集合和分页接线。 |
| 3 | 应用基础服务 | 分批完成[文件系统](core/filesystem.md)、[加密与哈希](core/encryption-hashing.md)、[Redis、缓存与锁](core/cache-redis.md)、[国际化](core/localization.md)、[视图与模板](core/views-templates.md)。local/public、`storage:link`、Redis 和 S3 分别用实际入口验收。 |
| 4 | HTTP 请求链 | 补齐路由、请求、响应、控制器、中间件和 CORS。接通 [Cookie、Session、CSRF 与默认 Web 组](core/session-cookie-csrf.md)，再验收 [FormRequest](core/validation.md) 的 API 422 和 Web 表单回传。缓存就绪后接[限流](core/rate-limiting.md)，实测 429、响应头和 Redis 并发。 |
| 5 | Eloquent、身份与 API | [Eloquent](core/eloquent.md)按模型、关系、转换、序列化、工厂、集合推进，再接隐式模型绑定和[分页及 API Resources](core/pagination-api-resources.md)。在 Session、Hashing 和 UserProvider 就绪后交付 [Guard、Policy、登录及登出](core/auth.md)。 |
| 6 | 事件、异步任务与通知 | 先接[命名 URL、签名及重定向](core/url-generation.md)和[事件](core/events.md)，再交付[队列、Worker、调度](core/queue-scheduling.md)及[邮件、通知](core/mail-notifications.md)。验证 `queue:work` 的重试和失败记录，以及 `schedule:run` 的互斥。[密码重置与邮箱验证](core/password-reset-email-verification.md)在所需的通知、签名 URL 和限流就绪后验收。 |
| 7 | 后续框架专题 | 分批核定 Helpers、Strings、HTTP Client、Processes、Concurrency、Images、Search 和 Package Development 的 Rust 契约与验收。Concurrency 在进程和相关执行基础具备后推进，Search 在存储和队列接口具备后按驱动验收。 |
| 8 | 可选服务与发布验收 | [广播与 Reverb](core/broadcasting-websocket.md)在授权、事件和队列就绪后验收。gRPC 属于 OpenRust 扩展，按需安装并以真实客户端验收。补齐安装、部署、升级与发布资料，按目标平台、数据库版本和外部驱动完成发布验证。 |

HTTP、Console、数据库、Mocking 和浏览器验证随各批推进。前端、Vite、Starter Kits、MongoDB、AI 和 Laravel 独立生态包均在逐页表中登记，分别确认移植、Rust 对应方案或文档说明。Reverb 已纳入阶段 8。测试导航中的 Browser Tests 与 Packages 中的 Dusk 共用一个文档入口。

每批按[项目规范](conventions.md)对照固定 Laravel 契约，参考 Goravel 装配，先查 `openrust-v0.x` 的对应实现和测试。分别记录骨架、实现和验收状态，同步三语文档。暂存、提交和推送遵循当次授权。
