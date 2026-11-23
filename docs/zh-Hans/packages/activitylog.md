# 操作审计（扩展）

## 安装与配置 {#configuration}

启用应用的 `activitylog` feature，或添加 `openrust-activitylog` 依赖并使用包发现。`ActivitylogServiceProvider` 登记服务及迁移，启动时不访问数据库。在应用 `Cargo.toml` 的 `[features].default` 中加入 `activitylog`，保留已有功能，再运行 `./artisan migrate`。

`ACTIVITYLOG_ENABLED` 控制默认启用状态；`ACTIVITYLOG_BUFFER_ENABLED` 控制 `buffer.enabled`，默认关闭。`include_soft_deleted_subjects` 默认关闭。非法布尔值使配置加载失败；命中配置缓存时保留缓存值。

运行 `./artisan vendor:publish --tag=activitylog-config` 发布配置，在应用加载器登记模块与配置组后重新编译。重复发布保留定制，`--force` 覆盖。默认 `enabled: true`、`default_log_name: "default"`、`default_auth_driver: null`、`connection: null`、`clean_after_days: 365`；日志名允许为空。配置缓存命中时使用保存的快照。变更配置后重启常驻进程。

指定 `activitylog.connection` 时，由应用在目标连接显式登记 `ActivitylogServiceProvider::migrations()`。包跳过默认连接的迁移登记。

## 记录操作 {#recording}

`ActivityLogger` 提供流式记录；`ActivityRepository` 负责查询、事务写入和清理。操作者及对象使用可信业务身份。显式传入的属性默认保留；应用负责选择可记录的数据。

上游 Support 命名空间对应 `openrust_activitylog::support::{ActivityLogger, PendingActivityLog, ChangeDetector}`，也保留 crate 根导入。

```rust
use openrust_activitylog::{ActivityLogger, CauserResolver, Subject};
use serde_json::json;

let mut logger = ActivityLogger::new(connection.clone());
logger.setCauserResolver(CauserResolver::new(Some(
    Subject::new("user", authenticated_user_id)?,
)));
let record = logger.on(Subject::new("settings", "1")?)
    .inLog("settings").event("updated")
    .withProperties(json!({"module": "site"}))
    .withChanges(json!({"old": {"enabled": false}, "attributes": {"enabled": true}}))
    .log("settings.updated").await?;
```

同一个 Logger 成功记录后清空主体、事件、属性、时间和显式操作者，下一条恢复默认日志名与操作者。`byAnonymous()` 明确使用匿名身份；`by(None)` 保留默认解析。`tap(callback, event)` 在解析默认操作者后执行回调，可修改待保存数据。`createdAt` 接受 `jiff::Timestamp`。

`onModel(&model)`、`byModel(&model)?` 捕获模型真实 morph 身份与属性，供 `:subject.name`、`:causer.name` 等占位符使用。在应用作用域中，`byId(id).await?` 通过配置的认证 Provider 查找用户；不存在的 ID 返回错误且不写日志。

## 作用域与模型事件 {#model-events}

`activity()` 与 `facades::Activitylog::logger()` 每次创建新记录器。HTTP 请求、Artisan 命令和队列任务通过容器原生执行作用域共享日志状态与操作者解析器。独立调用者可以用 `logger.scope(async { ... })` 将显式创建的上下文共享给 helper 与模型事件。`setLogStatus` 与 `setCauserResolver` 接受显式作用域对象，不共享进程级当前用户。

`ActivityLogStatus` 支持开关；`withoutLogging(|| async { ... })` 在原来启用时临时关闭，结束、错误、panic 或取消时恢复。原来已关闭时保留回调对状态的操作。`CauserResolver::withCauser` 支持嵌套覆盖及恢复。默认操作者读取当前请求中已完成认证的身份，遵循 `default_auth_driver` 或选中／默认 guard，并使用模型真实 morph 映射。记录前须通过认证中间件或 guard 完成认证；同步模型回调不能发起尚未完成的异步认证查询。独立调用者显式提供解析器。

模型实现 `models::concerns::LogsActivity`，在应用 Provider 登记 `ModelType::bootLogsActivity(app, repository)`。`getActivitylogOptions` 返回 `LogOptions`：可选 `logAll`、`logOnly`、`logExcept`、`logFillable`、`logUnguarded`、`logOnlyDirty`、`dontLogEmptyChanges`、`dontLogIfAttributesChangedOnly`、`useAttributeRawValues` 与描述回调。默认不选择属性，但允许空变化日志。模型日志使用原生 Eloquent 事件，显式事务内同连接写入随业务一起回滚；其他日志连接在业务提交后写入。

模型还须实现 `Clone`，保存以 `Default::default()` 初始化的 `models::LoggingState`，并在 `activitylogState` 返回该字段。`disableLogging()`、`enableLogging()` 仅影响该实例；克隆复制当前开关值。用已登记的关系路径如 `owner.name` 选择关联属性；旧快照按原外键读取，显式事务也能读取事务内关系。应用采用不同名称时，在原生命名关系中显式登记别名。

定制审计模型时，在应用 Provider 用 `app.container().instance_named("application.activity", value)` 登记 `Arc::new(models::ActivityModel::of::<CustomActivity>())`，并将 `activitylog.activity_model` 设为 `"application.activity"`。保留审计列与 JSON casts；模型元数据选择表及可选连接，显式 `activitylog.connection` 优先。普通写入执行原生 Eloquent 保存事件。定制动作时，以原生容器名称登记 `LogActivityAction` 或 `CleanActivityLogAction`，再通过 `actions.log_activity`、`actions.clean_log` 选择；用 `beforeLogging` 修改日志，用 `CleanActivityLogAction::using` 定制清理。

## 事务边界 {#transactions}

显式事务日志先调用 `repository.prepare(activity)`，再用 `PreparedActivity::logIn` 写入业务事务。传入事务选择数据库，该库须已有审计表。事务失败或取消会回滚；`repository.logIn(tx, activity)` 合并准备和写入。独立的 `repository.log(activity)` 不属于业务事务。外部通知或发货由应用负责。

```rust
use openrust_activitylog::{Activity, ActivityRepository};

let repository = ActivityRepository::new(connection.clone());
let prepared = repository.prepare(Activity::new("settings.updated"))?;
connection.transaction(move |tx| {
    let prepared = prepared.clone();
    Box::pin(async move {
        // Apply the business mutation through tx.table(...).
        if let Some(record) = prepared { record.logIn(tx).await?; }
        Ok(())
    })
}, 1).await?;
```

## 脱敏与存储边界 {#redaction}

默认保存显式属性，不自动掩盖值。需要脱敏时显式使用 `withRedaction(Redaction::new().logProperties(&["module"]).logOnly(&["name"]))`，或启用 `activitylog.redaction.enabled` 并配置 `properties`、`attributes`、`except`。此策略只保留允许的顶层字段，递归掩盖密码、token、密钥等名称；`logExcept` 增加掩盖名称。只有启用此策略时才施加16 KiB及16层限制。普通字符串中的任意秘密无法自动识别。

默认迁移采用自增主键、可空数值型 subject／causer morph 标识、可空 JSON `properties`／`attribute_changes` 以及原生 `created_at`／`updated_at` 时间戳。`Subject` 在 API 边界以字符串表示 ID；应用主键为 UUID 或字符串时，须在迁移前定制两个 morph ID 列。`ActivityRecord::created_at` 是从存储解码的 UTC Unix 毫秒；默认 SQL 时间戳精度会丢弃秒以下部分。额外的可空 `request_id` 用于请求关联。查询不自动添加租户过滤，返回记录前由应用授权。Debug 隐藏载荷与身份。

## 查询与保留 {#queries}

`models::Activity` 是带 JSON casts 的 Eloquent 模型；`getProperty("order.number", default)` 读取嵌套属性，`ActivityRecord` 也提供此方法。`subject::<Order>()`、`causer::<User>()` 返回原生类型化 morph 关系，在应用作用域中使用。设 `include_soft_deleted_subjects: true` 后可读取软删除对象；操作者保留正常软删除过滤。

```rust
let page = repository.query().inLog("settings")
    .forSubject(&Subject::new("settings", "1")?)
    .limit(50).get().await?;
// Pass the final record's id to .before(id) for the next page.
```

支持 `inLog`、`inLogs`、`forEvent`、`forSubject`、`causedBy`、`for_request`，身份使用精确文本匹配。每页1–1000条，默认50条。包不自动暴露后台或 HTTP 接口。

`./artisan activitylog:clean accounts --days=365` 清理指定日志组的过期记录；省略 `accounts` 清理所有日志组。省略天数使用 `clean_after_days`，天数须为正整数。生产环境需要交互确认，或使用 `--force`。`cleanBefore(utc_milliseconds, limit)` 拒绝未来截止时间。包不提供防篡改存储。

启用 `activitylog.buffer.enabled` 后，每个 HTTP 请求、Artisan 命令或队列执行尝试中的 helper 与模型事件共享一个缓冲区。生命周期结束后刷新原缓冲区，失败响应、命令和执行尝试也刷新。缓冲记录持久化前没有 ID。刷新采用原子批量插入，跳过逐条 Activity 模型保存事件；模型事件日志在业务事务提交后才入队，事务或保存点回滚会丢弃对应日志，嵌套提交保持原记录顺序。未传事务句柄的显式 Logger 调用仍独立写入；显式日志要求加入业务事务时使用 `PreparedActivity::logIn`。

响应前取消、进程终止或强杀不能保证刷新。硬超时 Worker 子进程只在原有截止时间内执行本地刷新。独立进程使用 `ActivityBuffer::new(repository)`、`logger.withBuffer(buffer)` 并显式等待 `buffer.flush()`，没有异步进程退出钩子。刷新失败保留待写记录，可显式重试。
