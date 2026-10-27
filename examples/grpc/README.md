# gRPC 示例

该独立示例使用应用自有的 `proto/echo.proto` 和 `build.rs` 生成 typed client/server，并通过真实 TCP loopback 完成 unary、服务端流、客户端流和双向流调用。客户端流通过结束输入完成半关闭；所有连接都在本进程中运行，不需要数据库或全局 `protoc`。TLS 集成测试使用临时生成的 CA/server/client 证书，验证 hostname、信任链、过期证书和 mTLS 客户端身份拒绝。

```bash
cargo run --locked
cargo test --locked
```

应用可从 `GrpcClientRegistry` 选择默认或具名共享 `Channel`，服务由 `GrpcPlugin::register_service` 注册。启用 `grpc` feature 不会自动监听端口；示例仅在本机 loopback 显式允许明文，并通过 shutdown future 优雅停止。正式服务配置证书与私钥；mTLS 额外配置客户端 CA。证书轮换使用滚动重启。

生产 server 示例配置：

```toml
[grpc]
listen_addr = "0.0.0.0:50051"

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
