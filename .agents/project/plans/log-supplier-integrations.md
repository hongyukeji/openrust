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
