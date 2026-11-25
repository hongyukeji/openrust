# Webhook 出站 TLS 与客户端身份

## 固定契约与实现

固定 Spatie Server 的 Builder 支持 `verifySsl`、`doNotVerifySsl`、`mutualTls` 和 `useProxy`。Job 将这些选项交给 Laravel HTTP Client。本批复用 OpenRust HTTP Client 中已有的固定目的地机制及 reqwest 0.13.5 的 rustls 支持。自定义 CA 替换当前 HTTPS 对端的系统根证书库。客户端证书链和私钥组成当前固定 HTTPS 请求的客户端身份。HTTP Host、TLS SNI、主机名验证及实际连接地址各自保留原有约束。未新增依赖。[Spatie 固定 Builder](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/WebhookCall.php) · [Spatie 固定 Job](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/CallWebhookJob.php) · [reqwest 0.13.5 TLS 接口](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html#method.tls_certs_only)

HTTPS 默认验证证书与主机名。只有投递快照显式请求关闭校验且当前应用策略许可、目标为 HTTPS、同时未附带自定义 CA 或客户端身份时，才允许关闭。关闭验证的选择随投递保存并由重放重新授权，默认快照字段保持兼容。PEM 材料由应用按范围和固定目的地运行时提供，私钥不进入快照、队列或事件。Rust 入口接收解密后的 PEM，不读用户输入中的文件路径，也不支持将口令跟随 Job 存储。证书与私钥不匹配等 HTTP Client 配置错误记为拒绝；远端握手中断继续遵循既有未知结果和重试预算。

## 验收

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 共完成 41 次真实 HTTP 登记与独立 Artisan Worker 投递，含迁移、配置及路由缓存。正例验证私有 CA、客户端认证、实际证书匹配、原始正文与 HMAC、原始 Host 与 SNI。反例验证默认拒绝未知 CA、错误 CA、缺失或错误客户端证书、过期客户端证书、证书与私钥不匹配、未授权跳过校验、跳过校验时附带客户端证书，以及收到重定向仍不跟随。SQLite 另验错误主机名和过期服务端证书。队列和快照中均无证书材料。夹具用 OpenSSL 3.6.4 临时签发证书，以本地 TLS 对端验证请求。

Rust 1.94.0 隔离工作区格式检查、HTTP Client 三项选项回归、Webhook Server 八项合约测试及六组三语示例编译均通过。框架相关入口、发送包及启用收发特性的应用夹具均通过严格 Clippy。SQLite 原有发送、订阅扇出和接收套件通过，接收套件包含 136 个真实 HTTP 请求及 83 个独立 Worker。

[具名 SOCKS5 代理及凭据](outbound-proxy.md)已在后续批次接入。WS-08 仍包含普通 CONNECT 代理和部署边界。默认不读取系统代理。本轮使用隔离 HTTPS 对端，未覆盖生产证书链、外部代理、第三方账户或多主机 Worker。WH-04／WH-05 保持开放。
