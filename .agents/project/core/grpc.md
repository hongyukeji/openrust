# gRPC

> 受众：AI Agent
> 摘要：可选 tonic/prost runtime、服务注册、配置、typed clients、Protobuf 生成、G2a TLS/mTLS、G2b 身份/错误映射及 G2c deadline、资源限额、health/reflection 和有界停机已交付；G3 联合运行/集群仍待验收。

## 当前契约

- 根 package 的 `grpc` feature 可选依赖 tonic、tonic-health、tonic-reflection 0.14、tonic-prost/prost 0.14、http-body-util 与 tokio-stream。默认构建不链接 tonic，CLI 默认应用模板不调用 protoc。
- `[grpc]` 配置包含 listener/TLS/具名客户端、每连接及全局并发、metadata、消息、默认/最大调用预算和 reflection 开关。默认值为每连接 128 streams/64 个在途 RPC、全局 1024 个 RPC、metadata 8 KiB、收发消息各 4 MiB、默认期限 30 秒/最大 300 秒；超范围值启动时失败。服务端默认要求 TLS；只有显式 `allow_insecure_server = true` 才使用明文，适用于本机开发或可信 TLS 终止代理。插件只注册 `grpc serve` 命令和惰性共享 `Channel`；不因 feature 或空配置自动启动 listener/连接。
- 应用先调用 `GrpcPlugin::register_service` 注册 tonic `NamedService` 工厂，再安装 `GrpcPlugin`。`GrpcServer::from_app` 组装注册服务；`serve` 接受调用方 listener 与 shutdown future。
- `GrpcClientRegistry::channel(None)` 选择默认 endpoint，`channel(Some(name))` 选择具名 endpoint。`with_channel` 供测试和嵌入式应用显式注入 Channel；业务端使用 tonic 生成 typed client。
- CLI `openrust new --grpc` 生成应用持有的 proto、build.rs 和 typed module。构建使用 `tonic-prost-build` 与 `protoc-bin-vendored`，未安装全局工具。
- 客户端配置将 `endpoint` 与 TLS 信任材料逐名称绑定；HTTPS 必须提供显式 CA，可选客户端证书/私钥必须成对。证书材料只在启动时载入，错误消息与 Debug 不显示文件内容或路径，证书轮换通过进程滚动完成。G2a 的真实证书/信任失败用例见 `examples/grpc/`。
- `auth` feature 启用时 server Tower layer 验证 Bearer JWT/PAT、拒绝重复 Authorization，并把 `CurrentUser` 放入 tonic request extensions；没有 Bearer 时允许公开 RPC 继续执行。应用方法以 `GrpcAuth::authorize` 复用 `Policy` 与 PAT abilities，并通过 `GrpcAuth::status` 将框架错误映射为不含业务/内部消息的标准 Status。Cookie Session 不适用于 RPC。PostgreSQL/MySQL 真实 TCP、Policy 授权、PAT 撤销验收命令见 `memory/testing.md`。
- `ExecutionContext` 由框架层创建并覆盖 handler 与响应 body 生命周期；请求 `grpc-timeout` 缺省使用配置默认值，过长预算被最大值截断，错误/重复 metadata 返回 `INVALID_ARGUMENT`。期限从调用开始贯穿 unary 和流式响应；流期限结束发送 `DEADLINE_EXCEEDED` trailers，客户端断开释放全局 permit 并触发合作取消。出站调用可由 `GrpcClientRegistry::propagate_deadline` 写入当前上下文的剩余预算。超限的全局并发快速返回 `RESOURCE_EXHAUSTED`；HTTP/2 每连接 streams、在途 RPC 和 metadata 另有限制。
- 生成服务的 tonic 消息上限由应用使用 `GrpcPlugin::register_service_with_config` 将配置值应用到生成 server 的 `max_decoding_message_size` / `max_encoding_message_size`；旧 `register_service` 保留 tonic 默认 4 MiB 行为。超限请求按 tonic 协议返回 `OUT_OF_RANGE`。`RateLimiter` 不擅自按 IP/身份建立猜测性策略；业务 handler 使用已有 limiter 并选择受信用户/方法 key。
- 框架自动注册标准 gRPC health `Check`/`Watch`，总状态及每个已注册服务在 listener 可用后为 `SERVING`；停机先更新为 `NOT_SERVING`，Watch 收到更新后关闭，同时保留 Check 的 `NOT_SERVING` 回答。reflection 默认关闭；开启时必须注册应用 descriptor set 和明确服务白名单，不公布白名单外的服务。
- gRPC 停机切换共享 `core::Readiness`、拒绝新 RPC、限时 drain；超过 `[server].shutdown_timeout_secs` 后合作取消在途调用、abort 并 join server task，再返回 `grpc_shutdown_timeout`。响应流和 health Watch 均覆盖在停机生命周期内。
- 服务注册、配置与 loopback TCP 生命周期由 `apps/server/tests/grpc.rs` 覆盖；四种 RPC、半关闭和异常断连见 `examples/grpc/`。

## 未覆盖范围

HTTP+gRPC 联合 supervisor、多节点代理路由、滚动升级、proto 新旧互通、跨节点流处理、生产观测与性能基线留在 G3 与 v1 集群验收。不得把 loopback 示例写成集群证据。
