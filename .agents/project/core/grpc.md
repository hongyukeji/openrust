# gRPC

> 受众：AI Agent
> 摘要：可选 tonic/prost runtime、服务注册、配置、typed clients、Protobuf 生成与 loopback 示例及 G2a TLS/mTLS 已交付并通过验收，身份/资源治理与 G3 联合运行/集群尚未交付。

## 当前契约

- 根 package 的 `grpc` feature 可选依赖 tonic 0.14、tonic-prost/prost 0.14 和 tokio-stream。默认构建不链接 tonic，CLI 默认应用模板不调用 protoc。
- `[grpc]` 配置包含 `listen_addr`、服务端 `tls`、具名 `clients` 与可选 `default_client`。服务端默认要求 TLS；只有显式 `allow_insecure_server = true` 才使用明文，适用于本机开发或可信 TLS 终止代理。插件只注册 `grpc serve` 命令和惰性共享 `Channel`；不因 feature 或空配置自动启动 listener/连接。
- 应用先调用 `GrpcPlugin::register_service` 注册 tonic `NamedService` 工厂，再安装 `GrpcPlugin`。`GrpcServer::from_app` 组装注册服务；`serve` 接受调用方 listener 与 shutdown future。
- `GrpcClientRegistry::channel(None)` 选择默认 endpoint，`channel(Some(name))` 选择具名 endpoint。`with_channel` 供测试和嵌入式应用显式注入 Channel；业务端使用 tonic 生成 typed client。
- CLI `openrust new --grpc` 生成应用持有的 proto、build.rs 和 typed module。构建使用 `tonic-prost-build` 与 `protoc-bin-vendored`，未安装全局工具。
- 客户端配置将 `endpoint` 与 TLS 信任材料逐名称绑定；HTTPS 必须提供显式 CA，可选客户端证书/私钥必须成对。证书材料只在启动时载入，错误消息与 Debug 不显示文件内容或路径，证书轮换通过进程滚动完成。G2a 的真实证书/信任失败用例见 `examples/grpc/`。
- 服务注册、配置与 loopback TCP 生命周期由 `apps/server/tests/grpc.rs` 覆盖；四种 RPC、半关闭和异常断连见 `examples/grpc/`。

## 未覆盖范围

Auth/JWT/PAT/Policy、deadline/cancellation budgets、消息/连接并发限额、health/reflection、有界 drain、HTTP+gRPC 联合 supervisor、多节点代理路由、滚动升级和跨节点流处理留在 G2b/G2c/G3 与 v1 集群验收。不得把 loopback 示例写成集群证据。
