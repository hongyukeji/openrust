# gRPC 示例

该独立示例使用应用自有的 `proto/echo.proto` 和 `build.rs` 生成 typed client/server，并通过真实 TCP loopback 完成 unary、服务端流、客户端流和双向流调用。客户端流通过结束输入完成半关闭；所有连接都在本进程中运行，不需要数据库或全局 `protoc`。TLS 集成测试使用临时生成的 CA/server/client 证书，验证 hostname、信任链、过期证书和 mTLS 客户端身份拒绝；认证测试通过真实 TCP 验证 JWT/PAT Bearer、重复/无效凭据、Cookie Session 排除与标准 Status 映射。数据库授权测试以 PostgreSQL/MySQL 隔离库验证真实 PAT、Policy abilities 和撤销。

```bash
cargo run --locked
cargo test --locked
# 真实数据库认证测试分别使用当前 PostgreSQL / MySQL 管理连接
OPENRUST_TEST_DATABASE_URL="<隔离测试库管理 URL>" cargo test --locked --features db-postgres database_auth_tests::database_bearer_and_method_policy_work_over_real_tcp
OPENRUST_TEST_DATABASE_URL="<隔离测试库管理 URL>" cargo test --locked --features db-mysql database_auth_tests::database_bearer_and_method_policy_work_over_real_tcp
```

应用可从 `GrpcClientRegistry` 选择默认或具名共享 `Channel`，服务由 `GrpcPlugin::register_service` 注册。需要把配置的消息大小限额应用到 tonic 生成 server 时，使用 `GrpcPlugin::register_service_with_config`。启用 `grpc` feature 不会自动监听端口；示例仅在本机 loopback 显式允许明文，并通过 shutdown future 优雅停止。正式服务配置证书与私钥；mTLS 额外配置客户端 CA。证书轮换使用滚动重启。

生产 server 示例配置：

```toml
[grpc]
listen_addr = "0.0.0.0:50051"
max_concurrent_streams_per_connection = 128
max_concurrent_rpcs_per_connection = 64
max_concurrent_rpcs = 1024
max_metadata_size_bytes = 8192
max_decoding_message_size_bytes = 4194304
max_encoding_message_size_bytes = 4194304
default_timeout_secs = 30
max_timeout_secs = 300
reflection_enabled = false

[grpc.tls]
certificate_file = "config/certs/server.pem"
private_key_file = "config/certs/server-key.pem"
# 配置后会要求调用方提供由此 CA 签发的客户端证书。
client_ca_certificate_file = "config/certs/client-ca.pem"

[grpc.clients.billing]
endpoint = "https://billing.internal:50051"

[grpc.clients.billing.tls]
ca_certificate_file = "config/certs/billing-ca.pem"
# 两个客户端身份字段必须同时设置。
client_certificate_file = "config/certs/client.pem"
client_private_key_file = "config/certs/client-key.pem"
```

生成项目仅在 `config/development.toml` 显式开启明文 server；`config/production.toml` 不允许明文回退。证书与私钥由部署环境提供，不要提交私钥或把它们放进公共示例。

HTTP 与 gRPC 同进程运行时，在 builder 中先安装 `HttpPlugin`、`GrpcPlugin`，注册至少一个 gRPC 服务，再安装 `HttpGrpcPlugin`。它增加 `serve all` 命令；两个 listener 会先全部绑定，两个都开始服务后 readiness 才为 true。统一退出信号会同时摘除两个 listener，并在 `[server].shutdown_timeout_secs` 内 drain、join。原有 `serve` 与 `grpc serve` 命令仍可单独运行。该入口的真实 TCP 测试只证明单进程联合生命周期，不证明代理路由或跨节点集群。

```rust
builder
    .plugin(HttpPlugin)
    .plugin(GrpcPlugin::new())
    .plugin(HttpGrpcPlugin);
```

## 身份与方法授权

启用 `auth` 后，server 会验证 `Authorization: Bearer` 中的 JWT 或 PAT，并把 `CurrentUser` 放入 tonic RPC request extensions。Cookie Session 不用于 gRPC。未提供 Bearer 时，公共方法可继续执行；每个受保护 RPC 必须显式调用框架帮助入口，并由已有 Policy 检查用户角色和 PAT abilities：

```rust
openrust::grpc::GrpcAuth::authorize(&request, &policy, "invoices:read")
    .await
    .map_err(|error| openrust::grpc::GrpcAuth::status(&error))?;
```

认证失败使用 `UNAUTHENTICATED`；方法无权限使用 `PERMISSION_DENIED`。Status 只携带稳定错误类别，不返回字段值、业务消息或内部错误来源。示例中的认证 helper 演示框架接入；生产方法仍需按业务权限逐一授权。

## 调用期限与资源边界

框架默认调用预算为 30 秒，最大可配置为 300 秒（硬上限 24 小时）。客户端可按 gRPC 协议发送 `grpc-timeout`；服务端取其与配置上限的较小值，缺少期限时使用默认值。handler 和 server/client/bidirectional stream 的响应 body 共用同一绝对期限；到期返回 `DEADLINE_EXCEEDED`，不会在响应头返回后重置计时。处理函数如需将当前期限传播到下游 typed client，可在创建请求后调用：

```rust
let mut request = tonic::Request::new(message);
GrpcClientRegistry::propagate_deadline(&mut request);
client.call(request).await?;
```

配置的每连接并发与全局并发会快速拒绝过载请求，不建立无限等待队列；全局并发名额在流结束、deadline 或客户端断连时释放。连接与 metadata 使用 HTTP/2 限制，压缩后的 Protobuf 消息限制由 tonic 生成 server 在解码后应用。使用配置消息大小的服务工厂示例：

```rust
GrpcPlugin::register_service_with_config::<EchoServer<MyService>, _>(
    &mut builder,
    |state, config| {
        EchoServer::new(MyService::new(state))
            .max_decoding_message_size(config.max_decoding_message_size_bytes)
            .max_encoding_message_size(config.max_encoding_message_size_bytes)
    },
)?;
```

应用方法的业务配额仍由已有 `RateLimiter` 显式执行，应以已认证主体和方法组成稳定 key；框架不猜测公开 RPC 的客户端 IP 或业务配额策略。

## Protobuf 演进与 trace 传播

兼容演进保留现有 package、service、方法路径和字段编号；本示例的当前 schema 为 `EchoRequest` 增加 tag 2 `client_hint`、为 `EchoResponse` 增加 tag 2 `api_revision`，旧 schema 仍只认识 tag 1。示例测试在真实 TCP 上用旧 client 调新 server、再用新 client 携带未知请求字段调旧 server：旧 decoder 忽略新响应字段，旧 server 忽略新请求字段，新 client 读取旧响应时得到新字段的默认值。移除字段时必须在 `.proto` 中 `reserved` 对应编号和名称；破坏式契约使用新 package/service 版本，不原地改变旧 tag 的类型或含义。

HTTP 请求的 `traceparent` 只有符合 W3C version 00 格式时才进入 `ExecutionMetadata`。handler 调用 typed gRPC client 时，可显式向下游传播当前 trace parent：

```rust
let mut request = tonic::Request::new(message);
GrpcClientRegistry::propagate_trace_context(&mut request);
client.call(request).await?;
```

gRPC 入站也会将有效 `traceparent` 放入当前 `ExecutionContext`，可继续向下游传播。启用 `otel` feature 并配置 OTLP endpoint 后，HTTP、gRPC 和 worker span 会通过 OpenTelemetry SDK 导出。没有有效 trace parent 时调用照常处理；框架只传播单个 trace parent，不传播 `tracestate`、`baggage`、Authorization、Cookie 或任意 HTTP headers。未配置 exporter 时，现有 trace context 仍可用于框架内的显式传播，但不会导出 spans。

## Health 与 reflection

框架自动注册标准 health `Check`/`Watch`。总状态及已注册服务在 listener 可用后报告 `SERVING`；停机先报告 `NOT_SERVING`，随后在 `grpc.health_drain_grace_ms`（默认 250 ms）内继续响应健康查询并拒绝业务新 RPC，再关闭 Watch stream 并限时 drain。该宽限期必须短于 `[server].shutdown_timeout_secs`，且计入总停机 deadline。

### 双节点 HTTP/2 摘流验收

执行 `tests/consumers/grpc/scripts/test-g3c.sh` 可启动两个独立 Linux gRPC 节点和固定版本 Envoy 1.40.1。脚本用一个持久 `tonic::Channel` 发起 40 次 unary RPC，并检查 Envoy access log 中 downstream HTTP/2 connection ID 相同、upstream 分别为两个节点；接着对长流所在节点发送 SIGTERM，检查 Health `Check` 返回 `NOT_SERVING`、Envoy gRPC active health check 摘除节点、后续 RPC 只到存活节点、长流中断且同一 Channel 可重新调用。最后重启节点并验证其恢复分流。

脚本每次运行使用独立 Compose project，结束时移除该项目创建的容器、网络和本地节点镜像。长流进度不会迁移；客户端需在流中断后按业务幂等语义重新发起调用。该验收证明两个 API 节点通过真实 L7 HTTP/2 代理路由，不等同于 M6 的多 worker、共享依赖、滚动升级及全栈集群验收。

reflection 默认关闭。启用后，应用必须注册编译时生成的 FileDescriptorSet 和允许公开的完整服务名；未列入白名单的服务不会通过 reflection 暴露：

```rust
config.grpc.reflection_enabled = true;
GrpcPlugin::register_reflection_descriptor_set(
    &mut builder,
    include_bytes!(concat!(env!("OUT_DIR"), "/file_descriptor_set.bin")).to_vec(),
    ["example.v1.Echo"],
)?;
```

服务 descriptor 可能包含内部 schema，生产环境应只对受信网络或身份开放 reflection。
