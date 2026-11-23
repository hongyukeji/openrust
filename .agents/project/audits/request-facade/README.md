# Request 门面逐项核对与验收（2026-11-23）

## 依据与范围

- [固定 Laravel 基线](../../plans/laravel-rebuild-baseline.md)：Framework v13.40.0，提交 `1449bf241017087fbd38c118ecaabef3f6797fdc`。
- 固定 [Goravel ContextRequest 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/http/request.go) 用显式 ContextRequest 访问原生请求、输入、Session 与验证；参考其请求所有权边界，不引入进程全局可变 Request。
- [官方 Request 门面](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Facades/Request.php)、[请求实例](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Request.php)、[条件和类型化输入](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Traits/InteractsWithData.php)。Symfony 原生请求接口仅用于核对其被固定框架继承的行为，不替换框架基线。
- 已检查 `openrust-v0.x/vendor/openrust/framework/src/http/request.rs` 的 `client_ip`／`client_ips` 及对应请求测试。当前框架已具备代理、输入解析、上传、Session、验证和签名实现；本批优先接通这些实际消费者，未复制旧代理架构。
- [192 项声明清单](request-facade-inventory-2026-11-23.json)分别记录入口、实现、原生适配和验收边界。声明数量仅用于防遗漏，不作为行为通过率或整个框架进度。

## 结果与正式契约

| 范围 | 裁决 |
| --- | --- |
| 176 个运行时静态入口 | 已实现当前请求转发及必要的实例能力，按功能组验收；不把方法声明命中当作每一参数组合均已验证 |
| 6 个构造入口 | 显式原生输入／所有权适配，保留类型、签名或复制语义差异，见下文 |
| 10 个 PHP 全局环境／启动策略入口 | 明确使用原生传输、持有的配置和 `with_middleware`；不增加空方法或并行的全局请求状态 |
| 通用 Facade | `request` 名称及 `RequestHandle` 类型真实绑定；非缓存解析、根句柄、替换及清除沿用现有 Facade／容器机制 |

`support::facades::Request` 以现有请求任务作用域及 `with_container` 借用通道访问原请求，不复制 Body、输入缓存、上传或 Session，也不使用裸指针别名。无请求参数的同步／异步路由、ApiResponse、模型响应，以及 `ControllerDispatcher::without_request` 自动建立借用。显式接收 `&mut Request` 的处理器使用实例，或在调用门面服务前显式 `with_container`。请求外、原借用结束后和重复独占借用均返回错误；取消恢复原请求。

- `RequestHandle` 复用既有请求身份，可在同一 HTTP 请求的构造注入与动作借用之间复用，但仅在活动借用内可访问；HTTP 请求结束或跨请求时失效。句柄不暴露可逃逸的锁或可变引用；访问闭包不能将借用带出回调。
- `UserResolver` 是接收原请求及 Guard 的异步请求级用户回调，支持等待数据库等业务查询；`RouteResolver` 返回包含名称、元数据、参数及模型的 `ResolvedRoute`。原请求的 `route`、`route_is`、模型读取、字段回退和容器路由参数消费者均接入此结果。
- Session 通过闭包借用，不复制 Store。验证复用 Factory、请求 Data、用户解析和 ValidationException：API 422，Web 重定向并保存具名错误袋。
- 宏按应用容器隔离，异步调用在释放注册表锁后执行。条件输入回调也先释放内部请求借用，再执行业务代码。
- `MethodOverride` 按应用配置参数开关及允许列表；覆写请求头保留独立语义，不允许 GET／HEAD／CONNECT／TRACE。可信代理沿用已有策略并保存真实 trusted 判定，不能仅按是否提供 XFF 判断代理可信度。
- 请求语言独立于应用翻译语言，遵循当前 BCP 47 规则；格式扩展按请求隔离，避免在并发应用之间共享可变全局表。

## 明确保留的 Rust 适配差异

这不是 PHP 全局运行环境的字面复刻，也不宣称 192 个入口拥有完全相同的签名。

1. `capture`、`create`、`create_from_base` 接收显式 `http::Request<Body>`；`create_from` 转移原框架请求所有权。PHP 全局变量捕获、全局工厂、全局数组回写没有同名空方法。
2. `duplicate().await` 返回**传输快照** `http::Request<Bytes>`，保留原始请求体及有效方法／头／URI，清除框架扩展，仅保留原生连接地址和传输安全信息。它不是上游完整请求对象的克隆，不复制后续输入覆盖、Session、模型及中间件状态；访问原状态使用句柄。`initialize(native)` 替换当前借用的请求。
3. `route()` 返回可拥有的 `ResolvedRoute`，具名参数／模型由 `route_parameter` 读取。`session`／`get_session` 使用闭包，避免可变引用逃逸。条件操作以 `Option<T>` 表达未选择分支，缺键以现有 `Option` 契约表达。
4. 可信代理、主机及方法覆写设置由应用启动配置持有，不提供跨线程可变的 Request 全局设置或缓存；方法覆写有效值有静态读取入口。`get_script_name` 在无 CGI 的原生服务中为空。
5. 显式独占请求的处理器不能同时经门面再借用同一请求。原实例操作及 `with_container` 都是正式支持方式，未用请求副本掩盖别名冲突。

三语正文已逐项说明上述公开边界，见 `docs/{en,zh-Hans,zh-Hant}/the-basics/requests.md#request-facade`。

## 验收证据

最终候选基于命名提交 `6c879c658fc5818da377155bd34fd8a175c0eb4f`，临时源码与日志均位于项目 `target/tmp/request-facade-full/`。候选只覆盖本批文件，保留 HEAD 中旧 lint 注解，将其他代理的注解清理留在工作区；不包含用户环境模板、模型记录或既有许可文件暂存删除。源码 SHA 见本目录 [source-sha256.json](source-sha256.json)，临时范围清单为 `target/tmp/request-facade-full/files.json`，不是只用工作树组合冒充独立提交验收。

前置 IP 单元的证据见 [IP 历史记录](request-facade-ip-2026-11-23.md)。完整门面最终验收：macOS arm64、Rust 1.94.0，Docker 独立 MySQL **8.4.13**。在独占缓存窗口触碰候选 first-party Rust 源码（内容 SHA 不变），日志确认实际编译本候选 `source/vendor/openrust/framework`，避免跨源码快照的旧 mtime 产物复用。

| 检查 | 实际结果／日志（均在 `target/tmp/request-facade-full/`） |
| --- | --- |
| 8 组正常并发 HTTP 消费者 | 100 项通过，1 项需固定浏览器夹具的既有用例 ignore；`exclusive-consumers.log` |
| Session 消费者 | 独立 MySQL 下 17 项通过；同一日志后半段 |
| 最终 API 局部修正 | IP 11 项＋完整门面 17 项再次通过；`final-api.log`；验证异步用户解析器、非法 JSON 设置不破坏原输入 |
| workspace 全目标严格 Clippy | `cargo +1.94.0 clippy --workspace --all-targets --locked -- -D warnings` 退出 0；`clippy-delivery.log`，51.37s |
| 全 workspace 格式 | `cargo +1.94.0 fmt --all --check` 通过；`fmt-delivery.log` |
| 三语文档 | 命令入口测试 2 项通过，每语 109 个导航章节／111 篇正文内容检查通过，VitePress 构建通过；`docs-delivery.log` |

117 项消费回归**已包含**28项 IP／完整门面专项，不能相加计作145项。覆盖真实 TCP 与并发隔离、输入修改回到原实例、multipart 上传、Session 闪存、API 422、Web 具名错误重定向、签名篡改与忽略参数、宏隔离及重入、控制器构造注入、请求取消和替换后的 URL／IP／容器刷新。

主要命令通过项目 `tests/build_guard.py` 运行，Cargo 指定 `--manifest-path target/tmp/request-facade-full/source/Cargo.toml`，复用 `target/tmp/rust-naming/cargo-target`。MySQL 使用 `tests/mysql_acceptance.py`，向 Session 用例显式映射临时连接；未读取开发库。具体装配脚本为同目录 `exclusive-consumers.py`。

初次将三个既有 MySQL Session 用例并发指向同一临时库时，固定 `sessions` 表建表冲突发生在请求处理前；未改断言或被测实现，改按既有固定表夹具顺序执行。早期失败日志与两项独立复验日志 `mysql-session-recheck.log` 保留；独占最终批次完整17项通过。

未验证范围：Windows／Linux 原生部署矩阵、PostgreSQL／SQLite、Session 的 Redis／MySQL+Redis 可选路径及需浏览器夹具的前端用例。本批未把环境缺失时提前返回的 Redis 分支计作真实 Redis 通过。三语页面构建与锚点检查属于静态文档交付，不宣称新增浏览器端交互验收。

## 后续接口调整

- [Request 输入读取简化](input-ergonomics-20261123.md)：直接字段名、默认值与类型读取的 Rust 入口调整；历史清单与上方验收只代表当时接口，最新签名及验收边界见该记录。
