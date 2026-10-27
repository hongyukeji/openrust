# ADR-040 gRPC 纳入 v1 通用框架

- 状态：Accepted；G1 基础与生成、G2a TLS/mTLS、G2b JWT/PAT/Policy 身份与错误映射、G2c 预算/资源/健康停机已实现并通过本地验收；G3 联合运行/集群仍待实施
- 日期：2026-10-26
- 依据：[ADR-001](adr-001-own-kernel-on-axum.md)、[v1 总计划](../plans/gen1-plan.md)、[能力覆盖](../plans/v1-capability-coverage.md)

## 决策与原因

v1 主攻通用框架底层能力，正式交付可选 gRPC 服务端与客户端，支持内部服务及跨语言调用。本决策替代旧计划中“gRPC 不纳入 v1、仅作 v2 候选”的范围限制；GraphQL 与后台业务的版本归属不变。使用者可只启用 HTTP，也可只启用 gRPC，或显式组合二者。

采用 `tonic` / `prost` 生态整合 HTTP/2、Protobuf 与代码生成，复用 Tokio、Tower、现有 Plugin/AppState/Command 和配置加载；不自研 RPC 协议、不建立第二套容器、ORM、身份或错误体系。依赖版本、TLS provider、MSRV 与生成工具在实施时核实并登记 `stack.md`；规划不代表依赖已经安装。

## 布局与入口

- 框架归属根 package 的 `openrust::grpc` 与 `src/config/` 对应配置模块，使用可选 `grpc` feature；具体子 feature 按实际依赖设计。未启用时不引入 gRPC 运行时或要求 `protoc`，不预建空模块或独立发布包。“只启用 gRPC”指只装配并运行 RPC 服务，不要求本簇从现有依赖树移除 Axum/HTTP。
- 应用持有 `proto/`、生成配置与普通 Rust 服务实现；复用官方生成工具，当前候选为 `tonic-prost-build`。明确生成源、descriptor、工具版本与可复现构建，工具缺失给修复说明，不自动全局安装工具。
- 服务显式注册，共享已构建 AppState；默认使用独立监听地址/端口。允许独立运行或由同一命令 supervisor 显式联合 HTTP 运行，不要求首版同端口复用，不因启用 feature 暗中监听端口或启动 worker。
- 客户端支持默认/命名端点、共享 Channel、配置校验及测试替换；直接复用生成的类型化 client/server，不逐方法包装。公开入口应允许独立 consumer 注册服务、替换客户端配置并运行，无需访问私有构造器。

## 协议与公共语义

| 范围 | v1 必须交付的行为 |
| --- | --- |
| 调用模型 | unary、服务端流、客户端流、双向流全部支持；半关闭、异常断连、消息顺序、流结束及错误有真实网络验证 |
| 验证与错误 | Protobuf 输入经业务 DTO/现有验证规则处理；复用框架错误类别，集中映射为标准 `Status` 及有界脱敏 details。认证失败、无权限、参数错误、配额耗尽、deadline、取消和内部故障分别映射；不套 HTTP JSON envelope 或 always-200 模式 |
| 认证与授权 | metadata Bearer 适配现有 JWT/PAT 与 Policy，区分用户身份和服务身份。mTLS 不自动授予业务权限；长流的令牌到期、撤销窗口及逐消息授权边界在 contract 明确。同步 interceptor 只处理同步 metadata；需要异步查库的认证使用 Tower layer，不 `block_on` |
| TLS 与 mTLS | 复用成熟 TLS 实现，支持服务证书、信任根和可选客户端证书。生产直连默认 TLS；明文仅允许显式配置的开发或受信 TLS 终止拓扑，错误证书/配置不得自动降级。给出证书滚动更新与信任重叠说明 |
| 时间与取消 | 明确连接、调用总预算、流空闲及整个流生命周期上限；传递剩余 deadline。`Endpoint::timeout` 不发送 `grpc-timeout`，须结合请求 timeout/metadata，并验证实际传播；只等到响应头不算流式总超时。取消通知关联工作并释放资源，不保证撤销已提交副作用 |
| 资源与背压 | 限制收发消息、metadata、连接、全局在途调用、每连接 stream、缓冲与发送等待；压缩启用时验证解压后限额。过载返回明确状态，不用无限排队承接慢消费者；单连接限制不能代替全局限制 |
| 重试 | 默认关闭应用层自动重试；只有声明幂等且可重放的 unary 调用可受控重试，重试与退避合计不超总预算。流式调用不自动缓存重放；客户端、代理与业务层避免叠加重试，复用通用出站策略原则 |
| 健康与诊断 | 接入标准 gRPC health Check/Watch，复用框架 readiness 状态；停机先置 NOT_SERVING。reflection 为显式选项，生产默认关闭，启用时控制暴露范围；二者不泄露密钥或私有配置 |
| 可观测性 | 记录服务/方法、状态、时延、在途/流资源，关联 HTTP、RPC 与任务的 trace；流指标覆盖至完成/错误。metadata 按白名单传播，身份凭据和业务载荷不默认记录，不使用高基数用户标签 |
| 协议演进 | 应用采用版本化 Protobuf package/service；删除字段保留编号/名称，兼容新增与破坏变更分别验证。生成客户端与服务端进行新旧版本互通测试，不能只证明同版本编译成功 |

## 生命周期与集群

- 运行命令持有 listener、任务集合和退出信号，复用 ADR-037 的明确所有权原则。先撤就绪、停止新调用，再限时 drain，最后取消/abort 并 join；联合运行时一个 listener 启动或运行失败应关闭其他任务并返回失败，不能遗留后台服务。
- HTTP 与实时流同步补有界退出，仍不新增通用 Plugin shutdown 或平行 Hooks。每个调用的取消与应用退出分开；独立排队任务不继承已经结束的请求取消令牌。
- 支持静态/命名端点和 DNS/现有负载均衡部署，保留公开连接/端点解析扩展；至少验证一种能对 HTTP/2 多路复用正确分配 RPC 的方案。TCP 连接轮询不等于逐 RPC 均衡，单次 DNS 解析不等于动态服务发现。
- 至少两个 gRPC 节点验证健康摘流、滚动升级、节点退出与客户端重连。已有流不自动迁移到其他节点；中断和业务恢复策略明确，不能宣称跨节点恢复任意流进度。
- 本项不要求注册中心、配置中心、服务网格、RPC 网关或 gRPC-Web。后续按具体需求复用外部组件；gRPC 不改变单机部署的简单默认路径。

G3c 实施将 `GrpcConfig.health_drain_grace_ms` 加入公共配置，默认 250 ms。停机报告 `NOT_SERVING` 后，Health `Check`/`Watch` 在该宽限期内继续工作，业务 RPC 从 readiness 撤销时起拒绝；该宽限期必须短于并计入总停机 deadline。

## 实施顺序与验收

1. 保留当前中文整改及 M4 正确性任务；进入 M5 后先补跨协议验证、调用作用域与有界运行边界，不重写已验证内核。当前领取顺序只在 [todo.md](../todo.md) 维护。
2. G1：配置/注册、server/client、四种调用、Protobuf 生成与真实 TCP 示例；同簇完成生成项目 check/test，不把可运行入口拖到 M6。
3. G2a：TLS/mTLS 与明确的明文选择，server/client 信任材料；拒绝错误 CA、域名、过期证书及缺失客户端证书。
4. G2b：JWT/PAT/Policy、错误/验证与安全 Status 映射；拒绝无效/重复凭据，认证数据库路径双库验证。
5. G2c：deadline/取消、限额/背压、health/reflection 与有界退出；验收超限、慢消费、断连和 SIGTERM。
6. G3：宿主和独立 consumer、HTTP 与 RPC 显式联合运行、双节点代理/负载分配、滚动退出、新旧 proto 互通、观测链路；M6 做整体回归和性能收口。
7. 无数据库 gRPC 示例必须可独立运行；涉及数据库认证/事务的路径在 PostgreSQL/MySQL 分别验证。执行默认、仅 gRPC、HTTP+gRPC、认证及有效 TLS/生成组合、MSRV 与仓库源码门禁，不只测试 all-features。

四类调用的真实网络证据、故障和资源回收、生成项目、集群与文档全部齐备才算交付；本 ADR 不新增任何测试通过记录。

## G1 实施 Spec

## G3c 双节点 HTTP/2 路由与摘流 Spec

- 复用 tonic/Tower、标准 gRPC health 与现有 `GrpcServer::serve` 有界停机语义；不在框架中重复实现服务发现或负载均衡器。
- 使用固定版本 Envoy HTTP/2 L7 代理和两个独立 OpenRust 节点进程，health check 按已注册 Echo 服务状态剔除节点。
- 新增节点二进制以环境变量选择节点身份和监听地址；两个节点使用同一 schema，以响应字段识别实际上游。
- 新增单一 Channel 探针，连续发起 RPC 并确认同一 HTTP/2 连接上的实际后端分配；通过 Envoy access log 记录 downstream connection ID 和 upstream host。
- 验收脚本先验证双节点分布，再在流式 RPC 活跃时向其所属节点发送 SIGTERM，验证流中断、NOT_SERVING 后不再向该节点分配新 RPC。
- 同一持久 Channel 在摘除期间必须继续到达存活节点；重启被摘除节点后，验证健康状态恢复及双节点重新分配。
- 真实 Envoy + TCP 节点拓扑作为本地/CI 可复跑验收；不以 loopback、fake、独立互不复制的进程或 TCP 连接轮询代替。
- 不承诺流进度迁移；业务客户端在流中断后负责重新发起调用及业务级恢复。
- 修改范围为 `src/grpc.rs`、`src/config/grpc.rs`、`根目录集成宿主` 集成测试、`examples/grpc`、对应 CI、示例文档及当前项目事实/验收记录；新增公开 `health_drain_grace_ms` 配置。

## G3c 验收结果

`examples/grpc/scripts/test-g3c.sh` 已在真实 Docker Linux 容器中运行通过：固定 Envoy 1.40.1 路由到两个独立 OpenRust 节点；同一客户端 Channel 的 40 次 unary RPC 共用一个 Envoy downstream HTTP/2 connection ID，并在两节点间分配。对长流所属节点发送 SIGTERM 后，标准 Health `Check` 返回 `NOT_SERVING`，Envoy gRPC active health checker 将目标标记为不健康，10 次后续 unary RPC 全部到存活节点；长流中断，同一 Channel 重连成功。重启节点后健康恢复，40 次 RPC 再次覆盖两节点。末次实测 Envoy 为 0.54% CPU、19.78 MiB；该数值仅是本次验收观测，不构成容量基线。

## G1 实施 Spec

- 采用 `tonic`、`tonic-prost`、`prost` 与 `tonic-prost-build` 0.14.6 兼容系列；Cargo 当前解析 tonic/tonic-prost 0.14.7，锁定 MSRV 1.88，低于 workspace MSRV 1.97。运行时依赖只由根 `grpc` feature 启用；根默认构建不链接 tonic。
- 新增 `[grpc]` 配置：`listen_addr` 默认为 `127.0.0.1:50051`，`clients` 为名称到 endpoint URI 的映射，`default_client` 可选；G1 只接受明文 `http://` endpoint，TLS 在 G2 交付。配置无服务/客户端时仍不自动监听或连接。
- `GrpcPlugin` 显式注册 `grpc serve` 命令并构建默认/具名共享 `Channel` 注册表。应用用 `GrpcPlugin::register_service(builder, factory)` 注册 `AppState -> GeneratedServer` 工厂；工厂只在构造运行时 server 时调用，直接复用 tonic 生成类型，不包装每个 RPC。
- `GrpcServer::from_app` 构造 tonic 路由，`serve(listener, shutdown)` 接收调用方 listener 与 shutdown future；CLI 命令绑定配置地址并监听进程信号。此入口支持真实 TCP 验收与自定义进程管理；健康/就绪接线、TLS、限额和联合 supervisor 留在 G2/G3。
- `GrpcClientRegistry::channel(None)` 返回默认共享 `Channel`，传入名称返回具名 channel；`with_channel` 提供明确的测试或嵌入式连接注入入口。业务代码继续直接使用生成的 typed client。
- 应用持有 `proto/` 和 build.rs，使用官方 `tonic-prost-build`。示例使用可选 `protoc-bin-vendored` build dependency 设置 `PROTOC`，不安装开发者全局工具；只有启用 gRPC 的包才包含代码生成依赖。生成项目和无数据库 loopback TCP 示例覆盖 unary、server-stream、client-stream 与 bidirectional-stream、半关闭及异常断连。
- CLI `openrust new --grpc` 启用框架 feature，生成应用自有的 `proto/echo.proto`、build.rs 和 typed Protobuf 模块；默认模板不引入 gRPC 或 protoc 依赖。

## 一手参考

- [gRPC 核心概念](https://grpc.io/docs/what-is-grpc/core-concepts/)、[deadline](https://grpc.io/docs/guides/deadlines/)、[health](https://grpc.io/docs/guides/health-checking/)、[reflection](https://grpc.io/docs/guides/reflection/)：协议行为参照。
- [tonic](https://docs.rs/tonic/latest/tonic/)、[Endpoint](https://docs.rs/tonic/latest/tonic/transport/struct.Endpoint.html)、[Interceptor](https://docs.rs/tonic/latest/tonic/service/trait.Interceptor.html)、[生成工具](https://docs.rs/tonic-prost-build/latest/tonic_prost_build/)：Rust 集成与能力边界参照。
- [Protobuf 演进规则](https://protobuf.dev/programming-guides/proto3/)、[Goravel gRPC](https://www.goravel.dev/the-basics/grpc.html)：兼容约定和框架使用体验参照；具体实现以 OpenRust 的安全约定与验收为准。


## G2 分簇 Spec

- G2a 先交付传输安全：复用 tonic 的 Rustls `ServerTlsConfig`/`ClientTlsConfig`，显式启用 ring provider，不自建 TLS 栈。
- 服务端配置证书与私钥文件，mTLS 通过显式 client CA 启用并要求客户端证书；文件只在启动时读入内存，Debug 不含路径内容或密钥，配置/读取失败关闭。
- 服务端无 TLS 时必须显式设置 `allow_insecure_server = true`，仅适用于本机开发或已有可信 TLS 终止代理；无自动降级。具名客户端使用独立 HTTPS endpoint/CA/可选 client identity，HTTP URI 本身是显式明文选择。
- TLS 凭据轮换通过有界重叠的进程滚动完成；G2a 不承诺进程内热加载。客户端 hostname 默认取 endpoint host，可显式覆写用于受控 DNS/SNI。
- 验收用真实 TCP 自签 CA/证书：可信链、hostname、mTLS 双向身份通过；错误 CA、hostname、过期证书和缺失 client certificate 失败；证书文件权限和密钥内容不出现在错误或 Debug。
- G2b 后续复用 `JwtTokens`、`Authenticator`、`CurrentUser` 与 `Policy`；G2c 复用 `ExecutionContext`、`Cancellation`、`RateLimiter` 和 readiness 语义。

## G2c 实施 Spec 与验收

- 配置为 server-stream 等长响应统一设置 deadline，并以同一剩余预算约束 handler future 与 HTTP body；deadline 结束流时发送 `DEADLINE_EXCEEDED` trailer，取消和断连释放并发名额。
- 为每连接 stream/RPC、全局 RPC、metadata 与收发消息设置显式上限。全局并发采用 fail-fast semaphore；每连接由 HTTP/2/Tower 限制。protobuf 消息上限通过 `register_service_with_config` 交给 tonic 生成服务应用；旧注册入口仍采用 tonic 默认消息限额。
- 复用统一 `core::Readiness`；health Check/Watch 初始为 `NOT_SERVING`，listener 就绪后转为 `SERVING`，停机时先报告 `NOT_SERVING` 并结束 Watch，再在配置期限内 drain。超时取消剩余调用、abort 并 join server task。
- reflection 默认关闭；启用必须提供 descriptor set 和显式服务全名白名单。已有 RateLimiter 不自动套用到匿名 RPC，业务方法按身份与方法显式执行配额。
- 示例真实 TCP 测试覆盖整条流 deadline、全局并发拒绝与释放、protobuf 消息限额、reflection 白名单/配置错误、health Watch 停机状态及非终止流超时退出；server 集成测试覆盖 readiness 与子进程 SIGTERM。
- 本簇不声称 HTTP 与 gRPC 联合 supervisor、多节点负载均衡、生产观测或 proto 新旧互通已完成；这些仍由 G3/M6 验收。

## G3a HTTP/RPC 联合生命周期 Spec

- 对常见应用启动路径提供公开 `HttpGrpcPlugin` 与 `openrust::runtime::serve_http_and_grpc(&App)`；插件登记嵌套命令 `serve all`，不替换或删除既有 HTTP `serve` 与独立 `grpc serve`。
- 联合入口必须确认应用已安装 `HttpPlugin`、`GrpcPlugin` 并注册至少一个 gRPC service。预先构建 HTTP router 与 gRPC routes/TLS，再依次绑定两个 listener；任一绑定失败时已绑定 socket 自动 drop，禁止留下后台 listener/task。
- 联合 supervisor 配置共享 readiness 需要两个 listener 都开始服务；只有两个都 active 才报告 ready，任一个结束/失败即 not-ready。HTTP/gRPC 独立启动仍以一个 listener 为 ready。
- supervisor 只持有一份 SIGINT/SIGTERM signal；正常信号令两个 listener 同时停止接入并按共同 deadline drain。任一 listener 意外结束时，停止并等待另一 listener；到期取消合作请求、abort 并 join 两个 task。
- HTTP 的 app-owned background tasks 在两个 listener 都结束后由联合 supervisor 停止并按同一 deadline join，不能因 HTTP 子任务先进入 drain 而提前取消 gRPC handler 依赖。
- 真 TCP 验收覆盖两个协议并行服务、联合 readiness、退出时两个端口均关闭、listener 绑定失败不残留另一端口、服务异常退出清理 sibling，以及共享 shutdown timeout。G3a 不覆盖双节点 L7 路由、proto 新旧互通和跨协议 trace，它们分别由 G3b/G3c 交付。

## G3b Proto 兼容与 trace 传播 Spec

- 兼容验证在同一 `openrust.example.v1` package 与既有 `Echo` service/method 上运行两套生成 schema：旧版本保留现有 tag 1；当前版本只新增 tag 2，不重用/改变既有编号、类型或 RPC 路径。
- 用真实 loopback TCP 分别验证旧 client → 新 server 和新 client → 旧 server；未知字段按 Protobuf 规则忽略，旧端读取新端响应时忽略扩展字段。示例的当前 server 对新字段有可观察处理，测试不得仅比较序列化字节或编译成功。
- HTTP middleware 接受且仅接受格式正确的 W3C version 00 `traceparent`，写入既有 `ExecutionMetadata`；缺失/非法值不拒绝业务请求、不进入 span 或传播。trace context 保持 bounded、可序列化，并随已有 ExecutionContext 子任务/队列传播。
- `GrpcClientRegistry` 暴露显式 trace context 传播方法，只向 tonic metadata 写入 `traceparent`；不传播 `tracestate`、`baggage`、身份凭据或任意 HTTP headers。gRPC 入站层仅提取校验后的 `traceparent` 到 ExecutionContext，使 handler 再发起 typed RPC 时可继续传播。
- 不新增 OpenTelemetry feature 或依赖；框架不伪造 trace ID/span 生命周期，也不声称导出了分布式 trace。已有 tracing span 可将 request ID 与 traceparent 作为受限关联字段，测试证明日志/span 不包含 Authorization、Cookie 或任意业务 metadata。
- 验收覆盖合法/非法/边界 traceparent、HTTP→RPC→RPC 传播、无 trace 时兼容、敏感 header 不传播，以及两方向新旧 schema 真 TCP 互通。提交前运行根 workspace 源码门禁、示例真实网络测试和 docs/deny 检查；无数据库路径不需要 PostgreSQL/MySQL 专项。
