# 出站代理映射与 IPv6 目标

固定 Spatie Server 的 `useProxy(array|string|null)` 接受映射、单个代理或空值，Job 将配置交给 Laravel HTTP Client。Guzzle 的 cURL handler 按目标协议选择代理并咨询 `no` 列表。OpenRust 沿用现有具名代理和出站策略，以类型化 `ProxyMap` 表示协议与排除列表，同时接受配置对象和原有单个代理名。 [固定 Spatie WebhookCall](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/WebhookCall.php) · [固定 Spatie CallWebhookJob](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/CallWebhookJob.php) · [Guzzle v7.10.0 CurlFactory](https://github.com/guzzle/guzzle/blob/7.10.0/src/Handler/CurlFactory.php)

先校验目标，再按最终 URL 的协议和规范化主机选择代理。普通域名匹配自身与子域，前导点限定子域，IP 字面量按地址精确匹配。排除列表最多 128 项。命中排除项或缺少协议对应代理时，通过同一目的地策略直连。持久快照沿用原 `proxy` 字段，只保存由 Worker 当前授权的具名代理引用。代理地址、凭据、排除列表和未选中的代理名均不入队。本批覆盖 HTTPS 目标，后续已支持[明确授权的 HTTP 收件端](connect-http-origin.md)通过固定地址 CONNECT。

首轮 IPv6 验收中，域名目标可以经 IPv6 代理投递，`https://[::1]:端口` 在两种数据库上得到未知结果。定位到 vendored Reqwest 的 CONNECT 和 SOCKS5 路径把带括号的 URI host 直接交给 Rustls。两条路径改用现有 hyper-rustls 0.27.10 `DefaultServerNameResolver` 去除 IPv6 URI 括号，并继续使用目标 URL 校验证书中的 IP SAN。CONNECT 和 SOCKS5 继续向代理发送策略固定的数字地址。 [hyper-rustls 0.27.10 连接器源码](https://docs.rs/hyper-rustls/0.27.10/src/hyper_rustls/connector.rs.html)

## 验收

- SQLite、PostgreSQL 18.7、MySQL 8.4.13 以真实 Artisan 迁移、配置和路由缓存、HTTP 提交及独立 Worker 验证。三库通过 IPv4 HTTPS CONNECT 代理映射。修复后 PostgreSQL 通过 IPv6 代理与域名目标组合，MySQL 通过 IPv6 字面目标、IP 排除直连和目标 CA 错误拒绝。
- SQLite 修复后复测 IPv4 域名和 IPv6 IP 目标各经 HTTP CONNECT、HTTPS CONNECT 及 SOCKS5 的投递。另验 IPv4 IP 目标经 HTTPS CONNECT。夹具核对代理认证、目标数字地址、TLS 名称、IP 目标无 DNS SNI、原始正文和签名。对象配置及 Builder 都能选择 `https` 代理。错误凭据、未知许可、不同代理 CA／名称、重定向和退出代理的路径各有断言。
- Rust 包与应用消费者严格 Clippy、映射及发布契约测试、许可固定检查、三语文档构建和根／子路径 Chrome 检查通过。未使用外部代理账号。代理跳的客户端证书认证和真实跨主机部署保持开放。
