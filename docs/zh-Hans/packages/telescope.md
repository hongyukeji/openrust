# Telescope 应用调试

## 安装与配置 {#configuration}

Telescope 将应用诊断信息写入独立数据库表，并提供需要授权的诊断面板。在应用 `Cargo.toml` 的 `[features].default` 中加入 `telescope`，保留已有功能，然后执行：

```sh
./artisan telescope:install
# 重新构建应用，加载发布的配置和 Provider。
./artisan migrate
```

安装器发布 `config/telescope.rs`、`app/providers/telescope_service_provider.rs`，登记模块和 Provider。重复安装保留现有文件。包发现负责注册包服务和迁移；Provider 启动不会查询 Telescope 表。

`TELESCOPE_ENABLED` 默认为 true；`TELESCOPE_PATH` 默认为 `telescope`；`TELESCOPE_DOMAIN` 可限制路由域名。`TELESCOPE_DRIVER` 默认为 `database`。存储使用 `storage.database.connection` 和 `storage.database.chunk`（默认 1000）。包迁移登记在默认连接；使用其他存储连接时，须先在该连接登记 `TelescopeServiceProvider::migrations()` 再迁移。配置缓存使用保存的快照；修改配置后重启常驻进程。

每个 `watchers.<Name>Watcher.enabled` 开关对应 `TELESCOPE_<NAME>_WATCHER` 环境变量；复合名称用下划线，如 `TELESCOPE_CLIENT_REQUEST_WATCHER`。发布的配置包含各 Watcher 选项。`only_paths`、`ignore_paths`、`ignore_commands` 限制采集范围，路径匹配不含开头斜杠。暂停状态与监控标签使用应用配置的缓存。

`init` 及其内部命令 `init:resources` 默认不采集，避免在数据库和包资源准备完成前访问诊断存储。

## 授权与过滤 {#authorization}

访问 `/telescope` 或自定义路径。安装的应用 Provider 默认允许本地环境，其他环境在自定义授权回调前拒绝访问。回调应使用应用已认证身份和权限。面板及所有 API 路由保留 `web` 和 Telescope `Authorize` 中间件；写操作使用框架请求伪造防护，接受同源 Fetch 元数据或有效会话 CSRF 令牌。

安装的 Provider 在本地记录全部诊断，其他环境默认保留可报告异常、失败请求和任务、定时任务，以及包含监控标签的记录。在应用 Provider 中使用 `Telescope::filter`、`filter_batch`、`tag` 定制策略。回调配置跨执行作用域共享；待写记录按 HTTP 请求、Artisan 命令和队列尝试隔离。`without_recording(future).await` 抑制嵌套记录，并在正常完成或取消后恢复原状态。

请求密码与授权头默认脱敏；安装的 Provider 在非本地环境额外隐藏 CSRF 与 Cookie。`hide_request_parameters`、`hide_request_headers`、`hide_response_parameters` 可增加名称，嵌套参数使用点路径。启用敏感业务采集前，应审查事件载荷、邮件和日志的内容。

## Watcher 与记录 {#watchers}

Watcher 覆盖请求、查询、异常、日志、任务、批次、缓存、HTTP 客户端、命令、定时任务、邮件、通知、模型、事件、Gate、视图、Redis 命令及 Dump。原生生命周期事件提供实际数据，待写记录通过框架延迟回调持久化；进程被强制终止时无法保证写入。

`openrust_telescope::watchers` 下的 18 类公开结构均实现 `Watcher`。应用 Provider 可使用 `new(options)` 构造并调用 `register(app).await?`；独立注册前先关闭对应默认配置项，避免重复监听。包 Provider 也使用同一入口。`QueryWatcher` 支持 `slow`，`EventWatcher` 支持 `ignore` 模式。默认忽略框架事件；在解析的服务上调用 `telescope.ignore_framework_events.store(false, std::sync::atomic::Ordering::Release)` 可开启，设置仅在当前应用内共享。

应用事件实现 `Event::properties()` 提供 JSON 诊断快照；Rust 不会反射任意结构体字段。监听器名称来自实际调度器注册。模型水合计数来自原生 Eloquent 生命周期。邮件保留原始 mailable 类型和排队标记；预览使用浏览器沙箱，下载包含捕获的 MIME 邮件。

使用 `openrust::support::dump(&value).await?` 记录 Debug 内容；需要面板 Dump Watcher 活跃，或设置 `TELESCOPE_DUMP_WATCHER_ALWAYS=true`。该辅助方法不拦截 `dbg!`、`println!` 或同步集合 dump。请求载荷仅使用已解码数据，不读取尚未消费的请求体流。Redis 观察覆盖 `Connection::query_command` 成功执行的命令，不包含流水线和专用阻塞传输。

运行时未提供可靠来源时，源码位置和调用栈为空。Gate 条目使用 Rust 授权入口捕获的最近合格调用位置。同一应用内嵌套授权时，可回退到外层授权调用位置。`ignore_packages` 排除依赖包来源，`ignore_paths` 按路径子串过滤。没有通过过滤的来源时，仍会记录授权结果，文件和行号留空。Query 条目使用已捕获的最近一处合格数据库调用位置。一次分页调用触发的计数、页面数据和预加载查询都可指向同一行应用代码。Eloquent 写入也可记录应用的调用位置，包括静默写入和事务内写入。直接关系查找、批量读取和游标也可记录应用调用位置。首次懒加载、模型与集合的显式关系加载和关系计数可记录应用调用位置。关联创建、静默保存、批量写入和读后写操作产生的 SQL 可归到应用调用行。一次多对多中间表操作产生的读取和写入可归到同一行。Schema 的建表、改表及目录查询也可归到应用调用位置。通过外键约束包装器执行的回调查询保留回调内的调用行。已捕获的应用调用位置全部被过滤时，查询不写入。入口尚未提供应用调用位置的查询仍可记录，文件和行号留空。视图回调不提供 composer 名称。服务端记录已缓冲的 JSON／文本响应，应用响应参数脱敏及字符长度限制；不读取不透明正文或流，标记为 `Not Captured`。处理器／控制器名称和实际路由中间件类型来自路由器。HTTP 客户端记录传给 `as_multipart()` 的字段，以及通过 `attach_body()` 附加的文件名、十进制 KB 大小和头部。附件流由传输层消费，未知长度显示为 `null`。原生 `Part` 仍可通过 `attach()` 发送，同名字段的最后一项为不透明部件时，Telescope 省略该字段。面板提供列表、过滤、详情、监控标签和采集控制，不复刻上游 Vue 界面或头像。

出站诊断记录 JSON 或表单字段，也可记录 GET 地址中传入的查询参数。表单字段保留原始标量类型，同名字段在诊断中显示最后一项。构建器的查询选项仍用于实际发送的 URL。原始正文仅在内容类型为受支持的 JSON 或表单格式时才会解码。

发生重定向后，响应诊断显示最终地址，并保留首次调用的 GET、JSON 或表单输入。连接失败使用异常携带的请求，仅解码媒体类型受支持的缓冲正文。

## 查询与命令 {#commands}

```sh
./artisan telescope:list request --limit=20
./artisan telescope:list exception --tag=production --json
./artisan telescope:show latest:exception --full
./artisan telescope:show latest --type=query,log --json
./artisan telescope:pause
./artisan telescope:resume
./artisan telescope:prune --hours=48 --keep-exceptions
./artisan telescope:clear
```

`list` 支持 `--tag`、`--batch`、`--family` 和 `--before` 序列游标。`show` 接受 UUID、无歧义 UUID 前缀、`latest`、`latest:<type>`，可用 `--type` 过滤同批次记录。普通输出截断长内容，`--full` 保留完整值；JSON 始终保留捕获内容。控制台使用普通表格和时间戳，未采用 PHP 控制台的样式及相对时间描述。

`prune` 的小时数必须为非负整数；`clear` 删除所有 Telescope 记录、标签及监控行。保留策略由应用调度器执行。`telescope:publish` 发布配置，`--force` 覆盖该文件。独立代码可用 `EntriesRepository` 实现构造 `Telescope` 并显式调用 `store().await`；应用 Provider 可在首次解析 Telescope 前替换此契约。
