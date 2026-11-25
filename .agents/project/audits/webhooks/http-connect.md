# Webhook 固定地址 HTTP CONNECT

应用授权具名代理时可返回 `PinnedHttpProxy`。代理从可信配置的数字 Socket 地址创建，Basic 认证材料由 Worker 运行时提供。投递仍只保存代理名称。目的地策略先校验收件地址，HTTP Client 保留 URL，明文 HTTP 代理 CONNECT 只收到获准的数字 IP 和端口。目标 TLS 使用原主机名进行 SNI 和证书校验，客户端证书只用于目标 TLS。无授权、代理拒绝或连接失败时保持现有失败与重试结果，不转为直连。代理收到的认证资料未加密，适用于受信代理网络。

固定 reqwest 0.13.5 的 CONNECT 默认使用原 URI authority，而其 SOCKS5 路径可执行本地解析。CONNECT 补丁仅在 opt-in 时替换隧道 URI，仍复用 Hyper 的 CONNECT 实现和原 Reqwest 请求、中间件、事件、TLS 及响应链路。上游 crate 下载 SHA-256 与原锁文件一致，原始源提交为 `de55373434f07f42926599dbb5a88550d8e55112`。两个传输源文件接入补丁，原 MIT 与 Apache-2.0 文件逐字节保留。根 Cargo 固定同版本的本地副本。独立组装下游 Cargo workspace 时须带上相同补丁。 [Reqwest 0.13.5 连接源码](https://docs.rs/reqwest/0.13.5/src/reqwest/connect.rs.html) · [RFC 9110 CONNECT](https://www.rfc-editor.org/rfc/rfc9110.html#name-connect)

当前验证 Rustls 后端和 HTTP 到代理的路径。代理自身的 TLS 和独立信任已在[HTTPS CONNECT 后续批次](https-connect.md)接入。代理映射及排除列表、IPv6 代理拓扑、外部正式代理和跨主机部署仍待验收。

## 验收

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 通过 22 个 CONNECT 用例。隔离应用执行 Artisan 迁移、配置及路由缓存，通过实际 HTTP 入队，由独立 Worker 投递给本地 TLS 收件端。验证了 CONNECT 数字地址和 Host、原始域名的目标证书校验、Basic 认证及 407、代理不可用、拒绝重定向、显式直连、TLS 主机名不符和目标 mTLS。验证认证资料未进入收件请求、队列及数据库。SQLite 的原有 SOCKS5 和普通发送回归通过。

验收快照通过工作区格式检查、18 个定向 Rust 测试，以及涵盖 Webhook 验收应用的严格 Clippy。上游 MIT 许可证保留原始末尾空行。
