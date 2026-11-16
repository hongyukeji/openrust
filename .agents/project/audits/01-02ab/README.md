# 01-02ab 认证上下文属性

`Auth` 解析当前请求选择的默认或具名 Guard。`Authenticated` 和 `CurrentUser` 共享认证管理器的用户解析及请求内缓存，保留独立属性类型。访客返回 null。沿用 v0.x 的具名属性和空值映射。 [固定 Laravel Auth](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Attributes/Auth.php) · [Authenticated](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Attributes/Authenticated.php) · [CurrentUser](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Attributes/CurrentUser.php)

Rust 的认证驱动借用 `&mut Request`。控制器解析期间独占借用同一个请求，解析完成或取消后归还。闭包路由可以通过 `Request::withContainer` 提供该请求。`GuardHandle` 在操作时借用请求且校验来源，用户属性注入共享的 `Arc<dyn Authenticatable>`。认证解析沿用原有 Cookie、Session 和 Guard 状态。

隔离基线 `f761c154c`。实际 TCP HTTP 验证两个应用的并发请求、默认和具名 Guard、访客空值、身份对象复用、拒绝绑定到其他请求、无效 Guard 和认证 Future 取消后同请求再次解析。Rust 1.94.0 通过 22 项容器、请求输入和会话认证定向测试，含独立 MySQL 8.4.13 真库及 SQLite。通过本批源码格式、框架库和两项 HTTP 测试目标的严格 Clippy、同一隔离副本的实际 `main artisan inspire`。

三语容器示例分别编译运行。VitePress 构建通过，根路径及 `/docs/` 下通过 12 项浏览器回归与 6 项认证属性锚点检查。01-02 核心整单元仍需复核固定测试与既有交付记录。
