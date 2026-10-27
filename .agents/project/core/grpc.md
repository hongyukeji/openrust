# gRPC

> 受众：AI Agent
> 摘要：可选 tonic/prost G1 runtime、服务注册、配置、typed clients、Protobuf 生成与 loopback 实例；G2 安全/资源治理和 G3 联合运行/集群尚未交付。

## 当前契约

- 根 package 的 `grpc` feature 可选依赖 tonic 0.14、tonic-prost/prost 0.14 和 tokio-stream。默认构建不链接 tonic，CLI 默认应用模板不调用 protoc。
- `[grpc]` 配置包含 `listen_addr`、具名 `clients` 与可选 `default_client`。插件只注册 `grpc serve` 命令和惰性共享 `Channel`；不因 feature 或空配置自动启动 listener/连接。
- 应用先调用 `GrpcPlugin::register_service` 注册 tonic `NamedService` 工厂，再安装 `GrpcPlugin`。`GrpcServer::from_app` 组装注册服务；`serve` 接受调用方 listener 与 shutdown future。
- `GrpcClientRegistry::channel(None)` 选择默认 endpoint，`channel(Some(name))` 选择具名 endpoint。`with_channel` 供测试和嵌入式应用显式注入 Channel；业务端使用 tonic 生成 typed client。
- CLI `openrust new --grpc` 生成应用持有的 proto、build.rs 和 typed module。构建使用 `tonic-prost-build` 与 `protoc-bin-vendored`，未安装全局工具。
- 当前仅支持明文 `http://` endpoint。服务注册、配置与 loopback TCP 生命周期由 `apps/server/tests/grpc.rs` 覆盖；四种 RPC、半关闭和异常断连见 `examples/grpc/`。

## 未覆盖范围

TLS/mTLS、Auth/JWT/PAT/Policy、deadline/cancellation budgets、消息/连接并发限额、health/reflection、有界 drain、HTTP+gRPC 联合 supervisor、多节点代理路由、滚动升级和跨节点流处理留在 G2/G3 与 v1 集群验收。不得把 loopback 示例写成集群证据。
