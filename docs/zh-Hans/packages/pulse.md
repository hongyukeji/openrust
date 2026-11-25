# Pulse

Pulse 收集应用性能指标，提供慢请求、慢 SQL、慢任务、外部 HTTP、异常、队列、缓存和用户用量面板。

## 安装 {#installation}

在应用 `Cargo.toml` 中启用 `pulse` feature。通过 `./artisan` 执行时，将 `"pulse"` 加入 `[features].default` 并保留已有功能。包发现自动加载 `PulseServiceProvider`；默认应用无需登记扩展 Provider。

```sh
./artisan migrate
./artisan vendor:publish --tag=pulse-config
```

支持 PostgreSQL、MySQL、MariaDB 与 SQLite。Provider 登记三张指标表的迁移，启动时不查询这些表。配置与模板归包；发布的 Rust 配置须接入应用编译型加载器并重新构建。已有文件默认保留，配置缓存命中后不重读包环境变量。

## 配置 {#configuration}

`PULSE_ENABLED` 控制采集。默认直接写入数据库；`PULSE_DB_CONNECTION` 选择指标连接，`PULSE_STORAGE_KEEP` 默认 `7 days`。`PULSE_INGEST_BUFFER` 默认 5000，超过时尝试入库；普通 HTTP、Artisan 和 Worker 生命周期结束时也会入库。进程被强制结束或任务取消不保证入库。

需要分离采集与持久化时设置 `PULSE_INGEST_DRIVER=redis`，通过 `PULSE_REDIS_CONNECTION` 选择原生 Redis 连接，再运行消费命令。`PULSE_INGEST_KEEP` 接受保留时长，编译型配置也可传入条数。Redis 存储失败保留流条目供重试；多消费者和写库成功后删除流失败不保证严格一次处理。

`PULSE_CACHE_DRIVER` 选择原生缓存 store，用于服务器节流、检查锁和重启通知。多个进程应共享缓存连接与前缀；file 缓存仅在共享同一文件系统时跨进程生效。

## 面板与授权 {#dashboard}

默认访问 `/pulse`。`PULSE_PATH` 与 `PULSE_DOMAIN` 自定义入口。本地环境允许访问，其他环境需要原生 `viewPulse` Gate 授权；同时配置应用认证／会话中间件。JSON 指标入口为面板路径下的 `/api/metrics`，接受 `period=1h`、`6h`、`24h` 或 `7d`，使用相同授权。

```rust
let gate = app.make::<openrust::auth::access::GateRegistry>().await?;
gate.define_guest("viewPulse", |user, _| Box::pin(async move {
    Ok(if user.is_some() {
        openrust::auth::access::Response::allow()
    } else {
        openrust::auth::access::Response::deny("Forbidden")
    })
}));
```

面板使用原生 HTML、JavaScript 和 JSON 接口。通过 `./artisan vendor:publish --tag=pulse-dashboard` 发布模板；自定义模板保留 `<!-- PULSE_STYLES -->` 和 `<!-- PULSE_SCRIPTS -->` 可注入包资源。`Pulse::with_css` 接受可信应用 CSS 路径或 HTML，去重并追加；`css` 与 `js` 返回资源 HTML。Rust 将上游重载的 CSS 设置入口表达为 `with_css`。

## 记录器 {#recorders}

`pulse.recorders` 按类型名称配置 `CacheInteractions`、`Exceptions`、`Queues`、`SlowJobs`、`SlowOutgoingRequests`、`SlowQueries`、`SlowRequests`、`UserJobs`、`UserRequests` 和 `Servers`。`false` 或 `enabled=false` 在启动时关闭该记录器。`sample_rate` 为 0–1，慢记录的 `threshold` 单位为毫秒；`ignore` 接受 PHP 分隔符形式的 PCRE 模式。

对于路由名以 `livewire.update` 结尾的请求，`SlowRequests` 和 `UserRequests` 使用已解码的 `components[0].snapshot` JSON 中的 `memo.path`。慢请求记录的 `via` 字段保留处理路由。Pulse 使用 HTTP 内核已有的请求快照，保留不透明请求体的未读取状态。

`groups` 和按模式设置的 `threshold` 用有序的 `[[pattern, value], ...]` 表达首次匹配规则。例如 `[["/^orders:\\d+$/", "orders:*"]]`；对象形式按 JSON 对象的键顺序匹配。任务采样以任务 UUID 保持生命周期一致。请求指标使用匹配路由，避免为每个动态 ID 创建指标键；外部 HTTP 去掉 URL 用户名和密码。

慢查询 `location` 尝试恢复应用源码位置；不含调试符号的构建可能返回空位置。`Exceptions` 接收原生带类型的异常日志、任务异常以及 `Pulse::report`，普通 JSON 日志上下文不冒充异常。Rust 错误未必携带调用栈，需要时显式传入位置。

`Servers` 默认每实例 15 秒采样一次，保存 CPU、内存和磁盘快照；`PULSE_SERVER_NAME` 与 `PULSE_SERVER_DIRECTORIES` 自定义名称及冒号分隔的目录。`Servers::detect_cpu_using`、`detect_memory_using` 可替换检测器。

## 自定义指标 {#custom-metrics}

在已启动的请求、命令或任务中使用 Facade。指标在当前生命周期缓冲，设置和过滤器在应用内共享。

```rust
use openrust_pulse::facades::Pulse;

Pulse::record("orders", "completed", Some(120), None).await?
    .count().sum().max().avg();
Pulse::set("deployment", "version", "v1", None).await?;
Pulse::ingest().await?;
let totals = Pulse::aggregate_total(&["orders".into()], "sum", 3600).await?;
```

`record` 返回 Entry，可选择 `count`、`min`、`max`、`sum`、`avg`；`only_buckets` 仅写聚合。返回 Entry 与缓冲项共享聚合标志，其他字段遵循 Rust 值所有权，修改副本不会改写缓冲项。`set` 为同类型／键保留最后传入的值。四种窗口为 1 小时、6 小时、24 小时和 7 天，图表填充 60 个时间点。

`filter` 筛选入库项；`ignore(future)` 临时忽略采集并在取消时恢复；`lazy` 延迟生成指标；`handle_exceptions_using` 接收入库与记录器错误。`flush` 丢弃当前缓冲，`ingest` 立即提交。查询接口包括 `values`、`graph`、`aggregate`、`aggregate_types` 和 `aggregate_total`。后者接收类型切片并返回每类型结果，空集合结果为 `None`。

实现 `recorders::Recorder` 的 `name`、`listens` 与异步 `record`，在包启动后的应用 Provider 调用 `pulse.register(vec![recorder])`，即可监听原生事件。使用 `recorders()` 查看注册实例。Rust 通过编译型类型与显式事件名称替代 PHP 类反射；自定义 HTML 卡片通过发布模板实现。

## 用户 {#users}

用量记录读取原生认证已经解析的用户。先运行认证中间件或 `Auth::user(request).await?`；队列投递可用 `remember_user` 明确关联操作者。默认面板通过认证 Provider 读取姓名、邮箱和头像，找不到时显示 ID。`Pulse::user` 定制字段，或在扩展启动前绑定 `ResolvesUsers` 自定义用户键与加载行为。默认通用认证 Provider 按 ID 加载；需要批量优化时实现该契约。

## 命令 {#commands}

```sh
./artisan pulse:check
./artisan pulse:check --once
./artisan pulse:work
./artisan pulse:work --stop-when-empty
./artisan pulse:restart
./artisan pulse:clear --type=slow_request --type=slow_query --force
./artisan pulse:purge --force
```

`pulse:check` 每秒发送检查事件，服务器记录器按采样周期工作；Vapor 环境或 `--once` 执行一次。`pulse:work` 消费 Redis 流并周期清理旧指标；直接存储模式不需要该进程。`pulse:restart` 通知长期运行的两类命令退出，由进程管理器重启。`pulse:clear` 与 `pulse:purge` 清理指定类型或全部指标，生产环境需要确认或 `--force`。部署后使用 `./main artisan`。
