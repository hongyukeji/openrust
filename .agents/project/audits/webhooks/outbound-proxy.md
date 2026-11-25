# Webhook 具名 SOCKS5 出站代理

## 固定契约与实现

Spatie Server 固定版 `useProxy` 将字符串或映射交给 HTTP Client。OpenRust 的同名 Rust 入口接收具名代理引用，也可通过 `webhook-server.proxy` 配置默认名称。同步、Worker 和重放均由当前应用的投递策略核对范围、原始目的地与代理名称。策略可以覆写 `authorize_transport` 检查所请求的名称。快照只记录名称，代理地址及可选用户名和口令由当前进程从可信配置提供。启用前，各 Worker 需升级到支持代理名称的版本并配置一致的代理授权。[Spatie 固定 Builder](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/WebhookCall.php)

复用 reqwest 0.13.5 的 SOCKS5 本地解析。受限解析器只取已通过目的地策略的地址。代理收到数值 IP，URL、HTTP Host、TLS SNI 和证书的服务端身份仍使用原目标。代理本身使用应用配置的固定数字 Socket 地址。需使用已开启证书及主机名校验的 HTTPS，仍可叠加自定义 CA 和双向 TLS。无许可或代理失败不会改走直连。自动系统代理、重定向和 SOCKS5 远程 DNS 关闭。代理认证在 SOCKS5 对端间未经加密，凭据应仅用于可信网络。[reqwest 固定连接源码](https://docs.rs/reqwest/0.13.5/src/reqwest/connect.rs.html)

[固定地址 HTTP CONNECT](http-connect.md)已在后续批次接入。TLS 到代理的实现和验收仍开放。代理映射及排除列表未开放。本批未验外部托管代理、IPv6 代理拓扑和多主机环境。WH-04／WH-05 保持开放。

## 验收

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 共通过 22 组代理相关 HTTP 登记、Artisan 和独立 Worker 用例，均执行真实迁移、配置缓存及路由缓存。受控 SOCKS5 对端断言 CONNECT 字节包含获准的数字 IP 和端口，TLS 对端核对原始 Host、SNI、正文及 HMAC。覆盖代理缺失、名称不符、认证失败、代理成功、拒绝跟随重定向及显式直连。SQLite 另验 IP 字面目标、通过代理的双向 TLS、缺失客户端证书、原域名证书不匹配和原有直连 TLS。凭据未出现在持久快照、队列及接收端 HTTP 头。原有直接发送及订阅扇出套件也通过。

Rust 1.94.0 隔离工作区格式检查通过。HTTP Client 三项、发送包九项、三语六组公开示例均通过，受影响目标及 Webhook 应用夹具严格 Clippy 通过。共享构建触及 32 GB 停线后，按守卫规程恢复；最终复测仅在本批省略调试符号，保留默认增量与并发。
