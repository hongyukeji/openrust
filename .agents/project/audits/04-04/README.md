# 04-04 Cookie、Session 与 CSRF

本次验收覆盖 Cookie HTTP 生命周期和数据库会话存储。请求级 Session、CSRF 与默认 Web 中间件组仍在开发。

## 上游与复用

- 对照固定 Laravel CookieJar、CookieValuePrefix、EncryptCookies、AddQueuedCookiesToResponse 和对应测试。复用旧版 `6d1a8192` 的 `http/cookie/{value,queue,parse,crypto}.rs` 中按名称和路径替换、PHP Cookie 解析及名称绑定用例。 [Laravel Cookie 中间件测试](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Cookie) · [Laravel HTTP Responses](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/responses.md#attaching-cookies-to-responses)
- 对照固定 Laravel DatabaseSessionHandler 与其集成测试，参考 Goravel 用独立中间件装配 Session 的方式。从旧版 `session/database_handler.rs` 提取写库读取、Base64 字节载荷、过期和元数据语义。 [Laravel 数据库会话测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Session/DatabaseSessionHandlerTest.php) · [Goravel StartSession](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/session/middleware/start_session.go)

## 实现与 Rust 适配

- `CookieJar` 继承 `session.path/domain/secure/same_site`。`Request::cookie` 复用 PHP 变量注册树。队列放在请求任务作用域，直接附加和排队写入均可由外层 `EncryptCookies` 处理。使用当前加密密钥生成响应，读取时允许配置的旧密钥。加密排除由中间件实例持有，避免多应用共享可变配置。
- `DatabaseSessionHandler` 使用现有 Query Builder 的写连接和原子 upsert，替代先查存在再写的过程。按固定版比较 `last_activity`，可独立查询行是否存在、销毁或清理。可选元数据没有提供时保留已有列。载荷上限为 1 MiB，损坏的 Base64 返回类型化错误。
- 回归定位到缺少 multipart boundary 的请求在尚未消费正文时返回，客户端可能收到 TCP reset。修复后在解析 boundary 前读取有界正文，已有 400 和 413 实测继续通过。
- 代理转发的 HTTPS 判定和旧版 PHP 序列化 Cookie 的可选兼容尚未接入。现有中间件支持固定 Laravel 默认的非序列化加密。Cookie 中间件已能按组装配，默认 Web 栈待 Session 和 CSRF 接线。

## 验收

隔离副本基线 `21a65e98` 加前序验收依赖及本次改动，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，工作区 411 项通过。
- 实际 TCP HTTP 覆盖并发请求队列隔离、同名路径替换、清除队列、直接附加、失效头、排除名单、旧密钥读取、不同应用拒绝、跨名称重放和篡改。应用进程 `GET /up` 为 200，`./artisan inspire` 退出 0。
- `cargo +1.94.0 test --locked --offline -p openrust-framework --test session_database` 的 SQLite 专项通过。MySQL 8.4.13 专项使用临时数据库及受限用户，通过后清除数据库和用户。两种引擎均验证写入和更新、并发同 ID 写入、失效与清理、销毁、元数据及载荷上限。此处验收的是存储接口。
- 三语 Cookie 示例分别注册为路由并通过真实 HTTP 往返。三语 Requests 和 Responses 正文通过 VitePress 构建、12 项通用浏览器检查和 12 项正文检查，覆盖根路径与子路径。

后续接通 `Store`、`SessionManager`、`StartSession`、CSRF、默认 Web 顺序与三语 Session、CSRF 正文，再用真实 HTTP 和 MySQL 验收跨请求会话。

## 请求级 Session

- 对照固定 Laravel Store 的加载、保存、闪存、迁移与令牌轮换，以及 StartSession 的请求装配和响应保存顺序。参考固定 Goravel 的请求存储装配，从旧版 `session/store.rs`、`session/middleware.rs` 复核 ID、闪存生命周期和 Cookie 保护边界。 [Laravel Store 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Session/SessionStoreTest.php) · [Laravel StartSession](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Session/Middleware/StartSession.php) · [Goravel StartSession](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/session/middleware/start_session.go)
- `SessionManager` 按应用创建数据库或内存驱动的请求级 Store。令牌与 ID 使用 40 位无偏随机字母数字字符。保存成功后推进闪存，失败保留原状态。Session Cookie 由外层 `EncryptCookies` 保护，缺少保护时拒绝启动已配置的会话。数据库驱动记录传输层客户端地址和 User-Agent，支持 `SESSION_ENCRYPT`。Rust Session 属性使用 JSON，PHP 默认序列化会话数据未做互读。
- 实际 MySQL 8.4.13 HTTP 测试覆盖闪存 `keep`、过期、旧 ID 状态删除、令牌轮换、invalidate、加密载荷、元数据和数据库故障时的 500。内存驱动 HTTP 专项覆盖相同生命周期及无 Cookie 保护时的拒绝。故障注入验证保存重试时闪存仍可读取一次。真实 MySQL 临时库执行 `./artisan migrate --force` 后确认 `sessions` 表存在，`./artisan inspire` 退出 0。
- 隔离工作区 Rust 1.94.0 格式、`cargo test --workspace --locked --offline`、`cargo clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，工作区 414 项通过。三语 Session 示例各自完成真实 HTTP 写读，文档构建、12 项通用浏览器组合、6 项 Session 导航与正文检查通过。
- 仍待接入默认 Web 栈的 Cookie、Session、错误共享、CSRF 与绑定顺序。会话请求锁、上一 URL 与路由名、其余存储驱动和认证用户元数据尚未验收。

## CSRF 与默认中间件组

- 对照固定 Laravel 的 PreventRequestForgery、Handler、默认中间件组和回归用例。复用旧版 `6d1a8192:vendor/openrust/framework/src/http/forgery.rs` 的来源判断、PHP 真值回退和空令牌拒绝边界。请求复用已经缓存的正文与解析结果，保留旧版 `csrf_replay.rs` 的下游可继续读取语义。 [Laravel CSRF](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/csrf.md) · [Laravel PreventRequestForgery 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Http/Middleware/PreventRequestForgeryTest.php)
- 会话已配置时默认 `web` 依次装配 Cookie 加密、队列、会话、错误共享、CSRF、绑定。API 组保留无状态绑定。错误袋通过请求作用域传给 Blade 的阻塞渲染线程。应用级配置支持路径排除、同站允许和仅来源模式。Rust 拒绝重复来源头及配置为明文的 Session/XSRF Cookie。没有配置 Session 的嵌入式应用沿用仅绑定的 Web 组。真实 HTTP 测试不会按环境名跳过 CSRF。
- HTTP 测试验证来源与令牌分支、伪造失败 419、仅来源失败 403、绑定前拒绝、请求正文复用、加密 XSRF Cookie、API 无会话 Cookie、应用间和请求间错误袋隔离。实际主程序在 MySQL 8.4.13 上连续两次 `GET /` 共用同一会话记录，`GET /up` 返回 200 且无 Cookie。 `./artisan migrate --force` 与 `./artisan inspire` 正常退出。
- Rust 1.94.0 隔离工作区格式、测试和严格 Clippy 通过，417 项测试通过。三语配置示例通过编译和真实 HTTP 验收，VitePress 构建、12 项通用浏览器检查和 18 项 CSRF、Middleware、Session 正文检查通过。
- 04-04 的计划验收项已通过。剩余差异包括代理 HTTPS 信任、PHP 序列化载荷互读、会话锁、上一 URL 和额外驱动。表单错误包的写入和旧输入回传由 04-06 接续。
