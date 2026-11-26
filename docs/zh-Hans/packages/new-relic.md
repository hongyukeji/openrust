# New Relic APM（扩展）

## 启用遥测 {#configuration}

将 `"new-relic"` 加入应用 `Cargo.toml` 的 `[features].default` 数组，保留已有功能。若还没有该数组，先创建，再重新构建。包发现自动注册 `NewRelicServiceProvider`。独立使用扩展包的应用可以显式注册该 Provider。

为需要上报遥测的 HTTP 和 Worker 进程设置环境变量。

```dotenv
NEW_RELIC_ENABLED=true
NEW_RELIC_APP_NAME=Orders
NEW_RELIC_REGION=us
```

通过运行时环境提供 New Relic 摄取许可证 `NEW_RELIC_LICENSE_KEY`。Provider 启动时读取许可证，配置缓存不保存该值。服务名默认使用应用名称。按账号的摄取区域选择 `us`、`eu`、`jp` 或 `us-gov`。修改启用状态、服务名或区域后，重新生成应用配置缓存。 直连日本区域时，使用 HTTPS 通过 443 端口将 OTLP 发送到 `otlp.jp.nr-data.net`。

使用 OpenTelemetry Collector 时，将 `NEW_RELIC_COLLECTOR_ENDPOINT` 设置为其基础 URL。接受 HTTPS 或使用回环 IP 的 HTTP 地址，地址不得包含凭据、路径、查询或片段。应用使用 OTLP HTTP/protobuf 发送到 `/v1/traces` 和 `/v1/metrics`，启用日志导出时也会发送到 `/v1/logs`。在 Collector 的 New Relic 导出器中配置许可证。

需要向可信下游传递 W3C 链路时，将 `NEW_RELIC_PROPAGATE_ORIGINS` 设置为逗号分隔的精确源，例如 `https://api.example.com,https://internal.example.com:8443`。匹配协议、主机和有效端口。留空时关闭自动注入。修改后重新生成配置缓存。

## 采集内容 {#telemetry}

HTTP 遥测包含服务端 Span 和 `http.server.request.duration` 直方图。匹配请求按已注册的路由模板聚合，未匹配请求省略路由标签。记录响应状态码，5xx 标为错误。收到 W3C Trace Context 时可接续上游链路。HTTP 耗时统计到应用生成响应为止。

Artisan 命令（含帮助、列表和版本快捷入口）及队列执行尝试都会生成 Span 和耗时指标。失败的命令或任务尝试标为错误。采集不包含请求或响应正文、URL 参数、Cookie、命令参数、任务载荷或异常消息。独立命令和 HTTP 进程正常退出时刷新遥测。导出使用有界 Span 队列并重试暂时性故障，过载或持续故障可能导致数据丢失。

执行期间的出站 HTTP 和 SQL 操作以当前 Span 为父节点。HTTP 客户端 Span 覆盖每次网络尝试，直到响应正文传输完成或该次尝试出错。数据库 Span 与耗时指标覆盖预备语句、事务查询和游标，属性只使用驱动和固定的操作名称，不采集 SQL 文本或绑定值。

通过队列管理器派发的任务可以携带可选链路元数据。批量派发时，每个任务使用独立的创建 Span。异步 Worker 的 Span Link 关联任务的来源。已有任务缺少该字段时可照常执行。队列和命令耗时指标可分别按 `openrust.queue.job.duration` 和 `openrust.artisan.command.duration` 查询。

## 日志与异常 {#logs-and-errors}

设置 `NEW_RELIC_LOGS_ENABLED=true`，并在 `logging.channels` 中显式添加渠道。下例是渠道的配置值。

```json
{
  "driver": "monolog",
  "handler": "new_relic",
  "formatter": "default",
  "level": "error",
  "bubble": true,
  "with": {"fields": ["context.operation"], "report_errors": true}
}
```

只有选用该渠道的日志会发送。默认格式器使用结构化记录及其消息。通过 `with.fields` 明确列出允许导出的顶层 context 或 extra 字段。处理器保留有限长度的标量和简单数组，附带日志产生时的 Trace ID 和 Span ID。仅需转发日志时设置 `report_errors=false`。已经由应用异常处理器报告的错误，经本渠道转发时不会再创建一个错误 Span。使用默认格式器可避免把其他字段写入导出消息。可用 `with.transaction_name` 设置固定事务名称，或在 `with.fields` 中允许 `context.transaction_name` 让日志覆盖该名称。日志只能重命名自身仍在执行的事务。

异常报告使用错误类型和安全摘要。设置 `NEW_RELIC_EXCEPTION_STACKTRACE=true` 后，只有应用已经通过异常上下文提供 `stacktrace` 字段，例如由 `ExceptionBehavior` 提供在错误源捕获的堆栈时才发送截断后的堆栈。开启前确认该字段适合导出。

## 运行控制 {#runtime-controls}

扩展启用后默认发送链路和指标。可用 `NEW_RELIC_TRACES_ENABLED=false` 或 `NEW_RELIC_METRICS_ENABLED=false` 单独关闭。将 `NEW_RELIC_TRACE_SAMPLE_RATIO` 设为 0 到 1，为根 Span 设置采样比例，并沿用父 Span 的采样决定。指标计数不受链路采样影响。需要显式区分服务副本时设置 `NEW_RELIC_INSTANCE_ID`。使用配置缓存的应用修改这些设置后需重建缓存。

从应用容器解析 `openrust_new_relic::NewRelic`。在事务作用域内，可用 `name_transaction` 和 `add_attribute` 更新 Span 名称及有限的自定义属性，HTTP 指标继续按路由聚合。嵌入式消费者可在自己的工作结束后调用 `flush().await` 或 `shutdown().await`。`export_stats()` 统计实际 HTTP 请求尝试及失败，重试单独计数。`dropped_spans` 和 `dropped_logs` 统计应用的导出队列已满时丢弃的记录。`rejected_items` 统计接收端在成功 OTLP 响应中明确拒收的记录。部分接收不会由本扩展重新发送。
