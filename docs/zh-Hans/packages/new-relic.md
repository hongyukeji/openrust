# New Relic APM（扩展）

## 启用遥测 {#configuration}

将 `"new-relic"` 加入应用 `Cargo.toml` 的 `[features].default` 数组，保留已有功能。若还没有该数组，先创建，再重新构建。包发现自动注册 `NewRelicServiceProvider`。独立使用扩展包的应用可以显式注册该 Provider。

为需要上报遥测的 HTTP 和 Worker 进程设置环境变量。

```dotenv
NEW_RELIC_ENABLED=true
NEW_RELIC_APP_NAME=Orders
NEW_RELIC_REGION=us
```

通过运行时环境提供 New Relic 摄取许可证 `NEW_RELIC_LICENSE_KEY`。Provider 启动时读取许可证，配置缓存不保存该值。服务名默认使用应用名称。按账号的摄取区域选择 `us`、`eu`、`jp` 或 `us-gov`。修改启用状态、服务名或区域后，重新生成应用配置缓存。

使用 OpenTelemetry Collector 时，将 `NEW_RELIC_COLLECTOR_ENDPOINT` 设置为其基础 URL。接受 HTTPS 或使用回环 IP 的 HTTP 地址，地址不得包含凭据、路径、查询或片段。应用使用 OTLP HTTP/protobuf 发送到 `/v1/traces` 和 `/v1/metrics`。在 Collector 的 New Relic 导出器中配置许可证。

需要向可信下游传递 W3C 链路时，将 `NEW_RELIC_PROPAGATE_ORIGINS` 设置为逗号分隔的精确源，例如 `https://api.example.com,https://internal.example.com:8443`。匹配协议、主机和有效端口。留空时关闭自动注入。修改后重新生成配置缓存。

## 采集内容 {#telemetry}

HTTP 遥测包含服务端 Span 和 `http.server.request.duration` 直方图。匹配请求按已注册的路由模板聚合，未匹配请求省略路由标签。记录响应状态码，5xx 标为错误。收到 W3C Trace Context 时可接续上游链路。HTTP 耗时统计到应用生成响应为止。

Artisan 命令完成和有计时信息的队列执行尝试都会生成 Span 和耗时指标。失败的命令或任务尝试标为错误。采集不包含请求或响应正文、URL 参数、Cookie、命令参数、任务载荷或异常消息。独立命令和 HTTP 进程正常退出时刷新遥测。导出使用有界 Span 队列并重试暂时性故障，过载或持续故障可能导致数据丢失。

执行期间的出站 HTTP 和 SQL 操作以当前 Span 为父节点。HTTP 客户端 Span 从网络尝试开始记录到响应头到达。数据库 Span 与耗时指标覆盖预备语句、事务查询和游标，属性只使用驱动和固定的操作名称，不采集 SQL 文本或绑定值。

本扩展目前不转发日志或采集异常堆栈。队列和命令耗时指标可分别按 `openrust.queue.job.duration` 和 `openrust.artisan.command.duration` 查询。
