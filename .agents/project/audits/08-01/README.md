# 08-01 广播频道与事件分发

对照固定 [Laravel Broadcasting](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/broadcasting.md)、[BroadcastManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Broadcasting/BroadcastManager.php) 和 [BroadcastEvent](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Broadcasting/BroadcastEvent.php)。固定 Goravel 框架的树中未发现广播协议实现，参考 [Goravel Event Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/event/service_provider.go) 和队列的编译型装配。旧版 `6d1a8192` 无可提取的广播实现及测试。复用工作区已存在的频道注册骨架，以及当前 Auth、事件、队列和事务机制。

- 应用按配置解析并缓存广播连接，内置 `null`、`log`，支持显式注册驱动。`Routing::channels` 和 `withBroadcasting` 启用 Web 组的 GET／POST 授权路由，排除请求伪造防护中间件并保留频道守卫、命名参数和驱动授权。Artisan `channel:list` 显示当前应用的规则。
- Rust 通过编译型事件注册把具体类型接到既有 Dispatcher，以显式 `broadcastData` 表达可广播字段。同步投递及队列投递共用一份交付数据；队列序列化在分发时捕获的载荷和频道。事务回滚丢弃延迟操作；提交后由既有事务回调或队列处理。来源套接字由 `EventContext::toOthers` 读取并转交驱动。
- Reverb 签名、客户端订阅、在线成员名单协议和跨进程 Redis 属于 08-02。Pusher、Ably 和 Mercure 的外部传输按各驱动和生态范围跟踪。当时模型频道参数绑定、广播唯一任务及匿名广播仍待实现。

## 验收

- macOS Apple Silicon、Rust 1.94.0，锁定离线工作区 522 项通过。4 项忽略用例由集成测试作为真实子进程启动。全量格式和严格 Clippy 通过。
- 7 次真实 HTTP 请求覆盖私有授权、在线成员数据、无身份、规则拒绝、公共频道无需授权、未知频道和不支持的方法。启用 session 中间件，验证 POST 授权路由豁免请求伪造检查。Artisan `channel:list` 实际列出两个规则。
- 真实 SQLite 和独立 MySQL 8.4.13 验证应用连接选择、三类频道投递、事务提交与回滚、事件条件、来源套接字透传、指定队列及投递失败后重试。Artisan `queue:work` 处理释放后的真实数据库任务。
- 三语 VitePress 构建、12 项通用浏览器检查及 6 项广播页面检查通过。

下一单元 08-02 Reverb 与客户端协议。

## 2026-11-12 遗漏回归补交

独立频道注册作用域、GET/POST 授权、无驱动拒绝及 Web 中间件轨迹，新增 1 项。 与同文件其他单元按测试项拆分。隔离副本用 Rust 1.94.0 锁定离线运行当前分块测试及严格 Clippy，均通过。三语正文沿用原提交。

## 频道授权与模型参数绑定

- 固定 Laravel 依注册顺序匹配频道，先尝试同名显式路由绑定，再尝试频道参数的模型解析。隐式模型未找到返回 403，显式模型未找到沿用路由错误。Rust 通过 `ChannelBindings` 声明模型，复用 `UrlRoutable`、现有路由解析器及模型自定义键。守卫沿用首个匹配规则的顺序。回调拒绝立即终止，弃权继续匹配。 [固定 Broadcaster](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Broadcasting/Broadcasters/Broadcaster.php)
- 真实 TCP HTTP 端点以 SQLite 验证 slug 模型绑定、成员数据、模型不存在、其他所有者和无身份拒绝、守卫回退、显式绑定优先与缺失、重叠规则的拒绝和继续。实际 Artisan `channel:list` 列出七个规则，现有数据库广播与 `queue:work` 回归通过。
- Rust 1.94.0 锁定离线运行 2 项广播回归及工作区全目标全特性严格 Clippy。三语站点构建和浏览器检查通过。

## 匿名广播与广播唯一任务

- 固定 Laravel 的匿名事件可指定一个或多个频道、广播连接、名称、载荷和来源套接字，默认经队列投递，`sendNow` 同步执行。Rust 由 `Broadcast::on`、`private`、`presence` 构造，交给现有事件 Dispatcher；`toOthers(&request)` 显式传入 HTTP 请求。 [固定 AnonymousEvent](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Broadcasting/AnonymousEvent.php) [固定 BroadcastManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Broadcasting/BroadcastManager.php)
- 固定 Laravel 按事件类和唯一 ID 定位缓存锁，支持租期与指定缓存，重复时派发 `UniqueJobSkipped`。Rust 将同等策略交给既有队列锁，区分 Rust 事件类型与唯一 ID，并沿用队列的锁所有者、重试及永久失败释放逻辑。同步广播跳过唯一锁。 [固定 UniqueBroadcastEvent](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Broadcasting/UniqueBroadcastEvent.php) [固定 UniqueLock](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/UniqueLock.php)
- Laravel 在送入队列前获取锁。显式 Rust 事务沿用队列现有的提交后获取策略，回滚时没有锁或任务需要回收。活跃事务中的唯一投递若显式关闭提交后调度，队列返回配置错误。
- 真实 TCP HTTP 和独立 Artisan 应用共享 SQLite 队列与文件缓存。验收涵盖匿名同步和队列投递、公共／私有／在线频道、自定义／默认名称、载荷、指定连接、来源套接字、并发去重与跳过事件、不同事件类型及 ID、提交与回滚、重试持锁、成功与永久失败释放、过期锁所有者隔离，以及同步广播直接投递。

## 08-01 整单元验收

- 前置 05-09、06-02、06-04 已闭合。真实 HTTP 与 Artisan 验证了频道授权、模型绑定、广播分发、匿名发送和唯一任务完整生命周期。
- Rust 1.94.0 锁定离线运行 3 项框架广播回归和 2 项 Reverb 频道回归，独立 MySQL 8.4.13 复验广播事务和 Artisan 重试，工作区全目标全特性严格 Clippy 通过。改动文件格式通过，全仓格式留有既有 Sanctum 测试排版差异。
- 三语 VitePress 站点构建、12 项浏览器检查与三语广播章节锚点通过。隔离站点、数据库夹具和本批测试可执行文件已清理。08-01 核心单元闭合。
