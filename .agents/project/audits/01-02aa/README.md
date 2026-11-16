# 01-02aa 服务管理器上下文属性

对照固定 Laravel 的 Cache、Database／DB、Log、Storage 上下文属性。复用 v0.x 的具名选择、DB 独立属性身份、缓存 memo 和日志重命名语义。本版通过当前容器的规范绑定取得服务，使用 `Arc` 注入仓库、连接、日志器和磁盘，并共用现有管理器。同步和异步解析遵循同一选择规则。日志重命名共享原频道的处理器及上下文，命名频道仍保持原名称。 [固定 Laravel 上下文属性](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/container.md#contextual-attributes)

在 `59897573d` 基线的隔离副本实测两个并发 HTTP 应用。各自的命名缓存、磁盘和日志文件相互隔离。请求内同步／异步注入共用 memo 缓存，后续请求重新读取。默认连接和命名连接通过 SQLite 验证，MySQL 8.4.13 复验命名连接上的真实查询。不存在的缓存、数据库和磁盘名称通过相应组件的容器错误返回。

通过七项容器和真实 HTTP 回归、四项 URL 回归、Rust 格式、框架库及本批测试目标严格 Clippy、三语 Rust 示例执行、VitePress 构建和 15 项浏览器检查。磁盘阶段只清理本批隔离产物。

01-02 仍需处理依赖当前 HTTP 请求的 Auth、Authenticated 和 CurrentUser，并复核整单元。
