# 已验签接收的共享额度

固定 Laravel 节流中间件采用计数窗口和 HTTP 429 `Retry-After`。OpenRust 复用框架已有的原子 `RateLimiter::reserve`，并在 Webhook 处理的验签、Profile 筛选及必要事件 ID 校验后，持久接收或排队前扣减。该配置默认关闭。启用时须提供可信范围，缓存键由范围和配置名摘要组成，配置修订及副本共用一个一分钟窗口。每个已接受的请求计一次，合法重试也参与计数。 [固定 Laravel 节流中间件](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/Middleware/ThrottleRequests.php)

正式 Controller 的 Runtime 从当前应用解析限流服务。应用手动装配 `WebhookProcessor` 时显式提供该服务，已配置额度但缺失服务按错误处理。缓存出错或额度耗尽时不会保存接收记录或创建队列任务。验签失败、Profile 忽略和无签名提示沿用各自既有流程。Worker 中的[执行限流和并发锁](processing-middleware.md)及同任务租约分别在处理环节执行。

## 验收

SQLite、PostgreSQL 18.7、MySQL 8.4.13 的共享数据库缓存均通过真实 HTTP、Artisan 迁移、配置及路由缓存、接收查询和独立 Worker。两个独立节点目录使用同一范围和不同配置修订，在十个已验签请求中只允许三个进入存储与队列，剩余返回 429 和有效等待时间。其他 profile／范围额度独立。另验忽略与无效签名保持额度、缓存表不可用时拒绝、手动 Processor 装配与缺失服务、未扩展原默认配置行为。PostgreSQL 和 MySQL 的 Redis 8.10.3 共享缓存重复该矩阵。接收契约、格式、严格 Clippy 及三语文档构建与浏览器检查通过。
