# gRPC 示例

该独立示例使用应用自有的 `proto/echo.proto` 和 `build.rs` 生成 typed client/server，并通过真实 TCP loopback 完成 unary、服务端流、客户端流和双向流调用。客户端流通过结束输入完成半关闭；所有连接都在本进程中运行，不需要数据库或全局 `protoc`。TLS 集成测试使用临时生成的 CA/server/client 证书，验证 hostname、信任链、过期证书和 mTLS 客户端身份拒绝；认证测试通过真实 TCP 验证 JWT/PAT Bearer、重复/无效凭据、Cookie Session 排除与标准 Status 映射。数据库授权测试以 PostgreSQL/MySQL 隔离库验证真实 PAT、Policy abilities 和撤销。

```bash
cargo run --locked
cargo test --locked
# 真实数据库认证测试分别使用当前 PostgreSQL / MySQL 管理连接
OPENRUST_TEST_DATABASE_URL="<隔离测试库管理 URL>" cargo test --locked --features db-postgres database_auth_tests::database_bearer_and_method_policy_work_over_real_tcp
OPENRUST_TEST_DATABASE_URL="<隔离测试库管理 URL>" cargo test --locked --features db-mysql database_auth_tests::database_bearer_and_method_policy_work_over_real_tcp
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

## 身份与方法授权

启用 `auth` 后，server 会验证 `Authorization: Bearer` 中的 JWT 或 PAT，并把 `CurrentUser` 放入 tonic RPC request extensions。Cookie Session 不用于 gRPC。未提供 Bearer 时，公共方法可继续执行；每个受保护 RPC 必须显式调用框架帮助入口，并由已有 Policy 检查用户角色和 PAT abilities：

```rust
openrust::grpc::GrpcAuth::authorize(&request, &policy, "invoices:read")
    .await
    .map_err(|error| openrust::grpc::GrpcAuth::status(&error))?;
```

认证失败使用 `UNAUTHENTICATED`；方法无权限使用 `PERMISSION_DENIED`。Status 只携带稳定错误类别，不返回字段值、业务消息或内部错误来源。示例中的认证 helper 演示框架接入；生产方法仍需按业务权限逐一授权。
