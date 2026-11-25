# Webhook 固定地址 HTTPS CONNECT

应用通过 `PinnedHttpProxy::https` 提供代理的数字地址、证书名称和非空信任根。代理使用该地址建立 TCP，名称用于代理 TLS 证书校验及 SNI。代理 TLS 握手完成后才发送 CONNECT 和可选 Basic 认证，CONNECT 只携带已通过目的地策略校验的数字地址与端口。收件端 TLS 继续使用原 URL 的主机名及其自身的信任配置与客户端身份。代理不接收收件端客户端证书。投递只保存代理名称，授权缺失和代理错误遵循现有失败路径。

固定 Laravel HTTP Client 通过 Guzzle options 提供传输配置。固定 Spatie 发送器的 `useProxy` 继续映射到 OpenRust 的具名策略入口。原 Reqwest 0.13.5 的 HTTPS 代理使用从目标克隆的 TLS 配置。本批复用已有 CONNECT 补丁及 Hyper 隧道，使用 hyper-rustls 0.27.10 的 `FixedServerNameResolver` 独立选择代理 TLS 名称，并用 Rustls 0.23.46 构建只接受代理信任根的独立 TLS 配置。旧版工作树 `6d1a81924` 未发现相同机制。 [固定 Laravel HTTP Client](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Client/PendingRequest.php) · [固定 Spatie WebhookCall](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/WebhookCall.php) · [Reqwest 0.13.5](https://docs.rs/reqwest/0.13.5/src/reqwest/connect.rs.html) · [hyper-rustls 0.27.10](https://docs.rs/hyper-rustls/0.27.10/hyper_rustls/struct.FixedServerNameResolver.html) · [Rustls 0.23.46](https://docs.rs/rustls/0.23.46/rustls/client/struct.ClientConfig.html)

[代理映射、排除列表与 IPv6 本地拓扑](proxy-map.md)已补验。代理跳的客户端证书认证、外部代理和真实跨主机部署仍未覆盖。代理跳支持 Basic 认证和服务端证书校验，收件端支持 mTLS。WH-04／WH-05 保持开放。

## 验收

SQLite、PostgreSQL 18.7、MySQL 8.4.13 共通过 31 个 HTTPS CONNECT 用例。隔离应用执行 Artisan 迁移、配置及路由缓存，真实 HTTP 登记后由独立 Worker 投递。测试核对代理 SNI、CONNECT 的数字目标地址与 Host、Basic 认证、收件端 SNI／原始 HTTP Host、原始字节和 HMAC。分别以收件端 CA 冒充代理 CA、代理域名错误及代理 CA 冒充收件端 CA 验证拒绝。代理主动索要收件端可接受的客户端证书，未收到证书，收件端仍完成 mTLS。覆盖代理许可缺失、许可名不符、407、拒绝重定向、明确选择直连和目标 IP 字面地址。SQLite 额外通过原有 SOCKS5、明文 HTTP CONNECT 和普通发送回归。凭据未进入接收请求、队列及 SQL 快照。

隔离源码通过格式检查、18 个定向 Rust 测试（含六组三语文档消费者），以及包含 Webhook 验收应用的严格 Clippy。单独检查了 vendored Reqwest 补丁文件格式。
