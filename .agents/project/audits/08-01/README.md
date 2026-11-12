# 08-01 广播频道与事件分发

对照固定 [Laravel Broadcasting](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/broadcasting.md)、[BroadcastManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Broadcasting/BroadcastManager.php) 和 [BroadcastEvent](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Broadcasting/BroadcastEvent.php)。固定 Goravel 框架的树中未发现广播协议实现，参考 [Goravel Event Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/event/service_provider.go) 和队列的编译型装配。旧版 `6d1a8192` 无可提取的广播实现及测试。复用工作区已存在的频道注册骨架，以及当前 Auth、事件、队列和事务机制。

- 应用按配置解析并缓存广播连接，内置 `null`、`log`，支持显式注册驱动。`Routing::channels` 和 `withBroadcasting` 启用 Web 组的 GET／POST 授权路由，排除请求伪造防护中间件并保留频道守卫、命名参数和驱动授权。Artisan `channel:list` 显示当前应用的规则。
- Rust 通过编译型事件注册把具体类型接到既有 Dispatcher，以显式 `broadcastData` 表达可广播字段。同步投递及队列投递共用一份交付数据；队列序列化在分发时捕获的载荷和频道。事务回滚丢弃延迟操作；提交后由既有事务回调或队列处理。来源套接字由 `EventContext::toOthers` 读取并转交驱动。
- Reverb 签名、客户端订阅、在线成员名单协议和跨进程 Redis 属于 08-02。Pusher、Ably 和 Mercure 的外部传输、模型频道参数自动绑定、广播唯一任务及匿名广播仍待实现。

## 验收

- macOS Apple Silicon、Rust 1.94.0，锁定离线工作区 522 项通过。4 项忽略用例由集成测试作为真实子进程启动。全量格式和严格 Clippy 通过。
- 7 次真实 HTTP 请求覆盖私有授权、在线成员数据、无身份、规则拒绝、公共频道无需授权、未知频道和不支持的方法。启用 session 中间件，验证 POST 授权路由豁免请求伪造检查。Artisan `channel:list` 实际列出两个规则。
- 真实 SQLite 和独立 MySQL 8.4.13 验证应用连接选择、三类频道投递、事务提交与回滚、事件条件、来源套接字透传、指定队列及投递失败后重试。Artisan `queue:work` 处理释放后的真实数据库任务。
- 三语 VitePress 构建、12 项通用浏览器检查及 6 项广播页面检查通过。

下一单元 08-02 Reverb 与客户端协议。

## 2026-11-12 遗漏回归补交

独立频道注册作用域、GET/POST 授权、无驱动拒绝及 Web 中间件轨迹，新增 1 项。 与同文件其他单元按测试项拆分。隔离副本用 Rust 1.94.0 锁定离线运行当前分块测试及严格 Clippy，均通过。三语正文沿用原提交。
