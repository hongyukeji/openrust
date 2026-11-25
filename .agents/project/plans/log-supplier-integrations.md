# 日志供应商原生集成

## LOG-S01：New Relic APM（首版实现，真实账号待验）

2026-11-25，用户明确选择“单列供应商集成（推荐）”。固定Logging教程的核心渠道／格式器／处理器按本机实际消费者收尾，本项不阻塞核心章节；不把供应商实现缺失降格为仅缺真实账号或发布验收。

固定文档960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md的formatter:default示例使用Monolog3.10.0 NewRelicHandler，依赖PHP newrelic扩展。当前原生专用Agent及该处理器尚未提供，不能以register_handler或register_formattable_handler回调存在代替完成。

[官方C SDK仓库](https://github.com/newrelic/c-sdk)已于2022年归档，[官方文档](https://docs.newrelic.com/docs/apm/agents/c-sdk/get-started/introduction-c-sdk/)明确停止支持，原目标为64位Linux。不能为照搬PHP示例而默默引入停维SDK，也不能把Log API或OTel的新实现说成原APM调用完全等价。

原独立选型要求核对：维护中的官方接入方式、受支持平台与部署组件、APM事务／错误上下文、结构化Normalizer结果和现有文本Formatter的类型边界、同步／异步时序、凭据与错误恢复、许可和维护成本。先形成具体方案与影响，再由用户确认实施范围。正式实现后按实际类型／Trait／Provider／配置入口，验证正常／拒绝／重试／清理与真实供应商验收；只有本地协议模拟时明确标记边界。

Slack／Papertrail已有原生网络实现和本地真实协议消费者；其真实账号投递与跨平台发布属于原发布矩阵，不与LOG-S01的功能开发缺口混合。历史已闭合功能保持，不推送。

### 已确认范围与选型

用户确认首版覆盖 HTTP 服务端、独立 Artisan、Worker 和关闭时刷新。采集范围为链路和耗时指标，日志转发、异常堆栈、出站 HTTP／SQL 子链路与专属 Agent 功能另行裁决。

| 参考 | 采用或舍弃理由 |
| --- | --- |
| [Laravel New Relic](https://github.com/JackWH/laravel-new-relic/tree/974bc5b0f61740719c7f9697bd2c2e7d2bead04a) | 借鉴包 Provider 装配、HTTP 路由与后台操作分别命名的做法。其事务调用依赖 PHP 扩展。 |
| [OpenTelemetry PHP Laravel 自动观测](https://github.com/opentelemetry-php/contrib-auto-laravel/tree/f9a368e1d5db2bd2eefeddbf3acea63b168f5b25) | 对照 HTTP 和队列生命周期边界。动态钩子和 PHP 扩展依赖不适用于 Rust。 |
| [Axum Tracing OpenTelemetry](https://github.com/davidB/axum-tracing-opentelemetry/tree/ed9676e707f011c6c595899f2251cd42052ab2bb) | 参考路由模板生成低基数名称、提取 W3C 上游上下文的做法。当前主线由框架 Router 匹配路由，使用现有 `RouteMetadata` 与完成事件可获得实际匹配和最终响应状态。 |
| [OpenTelemetry Rust SDK 0.33.1](https://github.com/open-telemetry/opentelemetry-rust/releases/tag/opentelemetry-0.33.1) | Apache-2.0，导出器最低 Rust 1.75。采用 SDK 的聚合、采样、批处理、退避和 OTLP protobuf。旧版 `6d1a81924451` 已使用同版 SDK，保留其批次导出经验，改用应用级 Provider 和有界退出刷新。 |
| [New Relic OTLP](https://docs.newrelic.com/docs/opentelemetry/best-practices/opentelemetry-otlp/) 与 [APM 指标语义](https://docs.newrelic.com/docs/opentelemetry/get-started/apm-monitoring/opentelemetry-apm-ui/) | 使用分区域 HTTPS OTLP、摄取许可证和 Delta 指标。HTTP 指标使用 `http.server.request.duration`，配合服务端 Span 和 `service.name`，为 New Relic APM 提供服务及事务输入。 |

### 实现与本地验收

- 可选 `openrust-new-relic` 包经包发现注册 `NewRelicServiceProvider`。由配置与环境在应用级启用，许可证在启动时读取。独立 Collector 支持无供应商凭据的本机验收与应用外密钥管理。
- 复用 `RequestHandled`、`CommandFinished` 和 `JobAttempted`。HTTP 记录框架生成响应的时点，非传输结束。HTTP Span 接续入站 W3C 上下文，后台 Span 记录已结束操作。异常分类只取状态和结果，不携带原始业务异常。新增 `ApplicationTerminating` 供编译二进制的退出流程在已有工作结束后刷新。
- 限制每次 HTTP 导出少于 1 MB，使用有界 Span 队列和 SDK 的暂态重试。HTTP 请求线程及任务事件不等待外发。配置错误指明配置项，不记录许可证；普通日志处理器与这条导出链独立。
- Rust 1.94.0 隔离快照使用 `tests/new_relic_acceptance.py` 在 OTLP/HTTP 接收端解码实际 protobuf，验证真实 TCP HTTP、独立 Artisan 和 SQLite Worker 成功与失败。覆盖默认关闭、路由模板与上游父链路、Delta 指标、5xx、400 不重试、503 重试、超时、凭据和敏感数据隔离以及缓存配置。
- 验收边界为本地协议和应用消费者。真实 New Relic 区域账户的摄取、服务与事务呈现、跨平台实机，以及既定排除功能仍单项跟踪。
- 最低工具链下定向严格 Clippy 和全仓格式检查通过，根应用 `main artisan inspire` 经包发现向本机接收端实测导出。三语站点构建、两种部署路径的 12 个浏览器组合和全部导航页面检查通过。编译缓存与日志留在项目内 `target/tmp/new-relic/`。未运行无关数据库方言的全量验收。

### 完整版实施方案（待确认范围）

**交付目标。** 用现有 `openrust-new-relic` 交付可独立启用的 New Relic APM 集成。一个 HTTP 请求能看到下游 HTTP／数据库操作、相关结构化日志和异常，并追到独立 Worker 的执行尝试。在真实 New Relic 账号验证服务、事务、依赖与错误视图。核心 Logging 和供应商集成分别核算。

**技术边界。** 延用 [OpenTelemetry Rust 0.33.1](https://github.com/open-telemetry/opentelemetry-rust/releases/tag/opentelemetry-0.33.1)、包发现、Provider 和 OTLP/HTTP。使用 [New Relic 的 APM 映射](https://docs.newrelic.com/docs/opentelemetry/get-started/apm-monitoring/opentelemetry-apm-ui/)核对实际平台支持，区分标准 OTLP、平台可见效果及 PHP Agent 专属行为。CRON 任务、手动后台操作等如果没有平台认可的事务语义，提供可查询的 Span 和指标，并在验收记录中注明。浏览器 RUM、Synthetics、主机监控、持续性能分析、NerdGraph 部署标记和 PHP Agent 二进制兼容各有独立接入边界，本方案只覆盖服务端 APM。部署标记如另行纳入，使用[官方 Change Tracking](https://docs.newrelic.com/docs/change-tracking/config/nerdgraph/)及单独权限。

#### 实施顺序及退出条件

| 批次 | 实现与复用 | 退出条件 |
| --- | --- | --- |
| NR-01 执行作用域 | 将当前完成后创建的 Span 改为进入 HTTP 内核、Artisan 命令及 Worker 尝试时创建，执行期间启用上下文，结束时填充实际路由、结果和耗时。复用应用作用域与 [SDK FutureExt](https://docs.rs/opentelemetry/0.33.1/opentelemetry/context/trait.FutureExt.html)；只有事件无法包裹执行时，才在核心增加最小、供应商无关的接缝。用正常返回、错误、取消及超时控制结束时机，流式响应按现有响应生成边界记录，流体后续阶段另行注明。 | 并发和多应用下无串线。HTTP、Artisan、Worker 实际执行中的下游操作可获取父上下文。核对独立命令、Facade 嵌套调用及 list／help 等快捷路径，明确每个入口的采集边界。首版指标无重复。 |
| NR-02 出站 HTTP／数据库 | 在实际出站尝试建立 CLIENT Span，按 [HTTP 语义](https://opentelemetry.io/docs/specs/semconv/http/http-spans/)向允许的目标注入 W3C traceparent／tracestate。按实际协议尝试处理重试与重定向，跨域时重新判断传播策略。现有出站事件可佐证响应与连接错误，其字段不足以可靠配对并发、注入请求头或识别全部失败时，从传输处补齐。在数据库统一执行边界记录成功、失败及流式路径，`QueryExecuted` 继续保留原成功事件语义。使用构造器或驱动已有元数据生成 [SQL 操作摘要](https://opentelemetry.io/docs/specs/semconv/db/database-spans/)，无可靠摘要时省略。 | 两个真实 HTTP 服务互证父子关系和出站指标，覆盖超时、重试、重定向、拒绝传播。PostgreSQL 18、MySQL 8、SQLite 分别验证查询、失败、事务与流式接口。解码核对 `http.client.request.duration`、`db.client.operation.duration`，无绑定值、原始 SQL、DSN、查询串或请求正文。 |
| NR-03 队列跨进程关联 | 用可选、向后兼容的消息元数据保存有限的 W3C 创建上下文，沿用现有 Payload 序列化与 Worker 执行入口。每次真实处理生成独立 CONSUMER Span，按 [Messaging 规范](https://opentelemetry.io/docs/specs/semconv/messaging/messaging-spans/)用 Span Link 关联消息创建上下文；同步执行保留有效的本地父上下文。核对 New Relic 实际识别的发送和处理属性版本，并保留已有任务时长指标。 | HTTP 或 Artisan 生产、独立 Worker 消费，验证成功、失败、释放、重试、超时和有／无上下文的旧任务。sync、database、Redis 实测，其他承诺支持的内置异步驱动逐项用真实协议夹具验证。滚动升级与隔离子进程无数据丢失或错误传播。 |
| NR-04 异常与告警关联 | 从正式异常报告入口复用现有过滤、限流和类型链。用实际错误类型和脱敏内容记录异常及可用堆栈，标记 Span 状态，按独立策略处理预期错误、忽略和分组。原始 backtrace 不可用时注明缺失，禁止把报告器调用栈写为故障源。无活动 Span 的显式报告使用独立错误 Span；同一异常的日志、报告和外层 5xx 使用作用域内身份去重。 | 实测 HTTP、Artisan、Worker 三条主路径和独立报告，核对有／无堆栈、异常链、忽略规则、同源异常去重及不同异常保留。Errors Inbox 以错误 Span 状态判定，错误率以结构化指标判定，两条链分别验收。见 [New Relic Errors Inbox 语义](https://docs.newrelic.com/docs/opentelemetry/get-started/apm-monitoring/opentelemetry-apm-ui/)。 |
| NR-05 结构化日志与 Laravel 示例替代 | 在扩展 Provider 注册可选 `NewRelicHandler`，候选配置为 `driver=monolog`、`handler=new_relic`，沿用渠道、`level`、`bubble` 和处理器顺序。默认结构化映射复用 `LogRecord` 的级别、时间、正文、channel、context、extra 和异常数据，`trace_id`／`span_id` 写入 [OTLP LogRecord](https://docs.newrelic.com/docs/opentelemetry/best-practices/opentelemetry-best-practices-logs/) 原生字段。核心的 `FormattableHandler` 只提供文本格式，结构化适配优先留在扩展内；`formatter=default` 的文本格式输出不能替代结构化 Normalizer。配置若不支持某类 formatter，显式校验并说明原因。 | 从真实 Log 渠道及 `stack` 验证级别、冒泡、结构化字段、截断、脱敏及 trace 关联；覆盖缓冲渠道，在事件产生时保留关联，避免按导出时的活动上下文取值。禁用、无活动 Trace、出口故障、外部日志收集共存均有测试。 |
| NR-06 生产运行与交付 | 保持现有 `NEW_RELIC_ENABLED`、服务名、区域、摄取许可证及 Collector 配置。新增信号开关与采样配置，沿用 SDK 的有限批处理和 Delta 聚合；日志需显式启用渠道。服务名、版本和实例身份放在 Resource，业务 ID 不作指标维度。三类信号分别限额和冲刷。Exporter 自身错误不进入受观测 Log／HTTP 管线。提供应用嵌入场景的显式限时刷新／关闭入口，原二进制收尾继续自动调用。 | 验证采样只影响链路，指标计数仍代表实际业务操作。过载、限流、拒收、部分接收、超时、SIGTERM／正常退出、凭据轮换重启及配置缓存各有测试。验收实际 payload 上限、重试预算和丢弃统计。使用官方 Collector 的队列与持久化机制提供需落盘的部署方式。 |

#### Monolog 3.10.0 逐项裁决

依据 [NewRelicHandler 固定源码](https://github.com/Seldaek/monolog/blob/3.10.0/src/Monolog/Handler/NewRelicHandler.php)，处理器的主要副作用包含报告异常和变更当前事务，并不等同于普通日志转发。

| 固定入口或行为 | 计划中的 Rust 表达 |
| --- | --- |
| 默认 Error 门槛、冒泡与异常报告 | 兼容渠道以 Error 为默认门槛，保留冒泡。事件触发错误报告的路径与单独显式日志转发分别验收；默认无活动 Span 时也能报告。 |
| `NormalizerFormatter` 及 context／extra | 保留结构化默认适配。标量按类型输出；数组可选择保留为有界编码或受限展开。设定深度、数量、长度、字段保留规则与碰撞顺序，并覆盖空值和保留字段。 |
| 每条记录选择应用名 | 服务身份属于应用 Provider 的 Resource。迁移用独立受控服务配置，记录自带的任意 `appname` 不能把数据路由到另一应用或账号。该差异明确列入兼容账本。 |
| 当前事务命名和自定义参数 | 支持在活动作用域中命名业务操作和给 Span 添加有限键值；HTTP 指标保留真实路由模板。兼容渠道的 `transaction_name` 与 context／extra 按明确的允许字段转换，避免改变同进程其他事务或引入不受控的指标维度。 |
| Agent 缺失、高安全模式和内部错误 | 以扩展装配和配置校验为入口，以明确的数据收集策略实现保护。用实际 OTLP 字段和结果核对，不模拟 PHP 扩展状态或专有错误事件。 |

**完成标准。** 逐批使用 Rust 1.94.0 从包发现与公开入口实测，覆盖配置缓存、旧应用／任务和禁用状态。保持英文、简体中文、繁體中文文档同步，执行格式、严格 Clippy、文档站与浏览器检查。真实账号验收需要授权的隔离 New Relic 服务，逐项从 NRQL 和界面核对服务身份、HTTP 与 Worker 事务、跨服务 Trace、外部服务、数据库、Errors Inbox 和关联日志。在存在采样、过滤或区域差异时记录预期与实测；无法验证的项目独立列出。本机可先完成代码和协议验收，真实账号通过后才关闭平台验收。平台版本、OTLP 信号映射和已承诺的平台矩阵分别给出证据。
