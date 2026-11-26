# 固定地址 CONNECT 的受信 HTTP 收件端

固定 Spatie 发送 Job 把字符串或按协议配置的代理交给 HTTP Client。OpenRust 的公开目的地策略要求 HTTPS，应用可按可信配置批准固定的内部 HTTP 地址。对于这种地址，应用授权的 CONNECT 代理沿用已校验的数字 IP 和端口。HTTP Client 使用现有 Reqwest 与 Hyper 隧道，收件端仍收到原始 Host 和请求路径。 [固定 Spatie 发送 Job](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/Jobs/CallWebhookJob.php) · [RFC 9110 CONNECT](https://www.rfc-editor.org/rfc/rfc9110.html#name-connect)

代理认证和代理专用请求头只用于 CONNECT。HTTP 收件端这一段使用明文。HTTPS 代理自身继续使用单独的信任根和服务名，目标 mTLS 资料仍限于目标 HTTPS。代理授权失败、拒绝连接或证书错误返回原失败路径，不改用直连。代理和目标传输组合在 Worker 发送和人工重放前核对。具名 SOCKS5 继续要求目标 HTTPS。

## 验收

SQLite、PostgreSQL 18.7、MySQL 8.4.13 从实际 HTTP 提交，独立 Artisan Worker 经 HTTP 与 HTTPS 两种 CONNECT 代理发送至受信 HTTP 收件端。验证固定数字地址、原始 Host／路径和 HMAC、代理凭据隔离、代理授权拒绝、407、代理证书和主机名不符，以及无重定向或直连降级。SQLite 另验 IPv6 和 SOCKS5 拒绝该明文目标。三库复跑 HTTPS 目标的两类代理，SQLite 复跑原 SOCKS5、IPv6 字面 HTTPS 及原客户端证书路径。

隔离累计快照叠加 `8883114d` 的 HTTP Client 模块及当前改动。首轮编译发现旧快照缺少已合入的 multipart 配套模块，按实际提交补齐。首次 HTTP 用例随后定位到 Builder 和发送授权的同类限制，接通三处后复验。框架 HTTP Client 三项、发送包契约、框架／包及应用严格 Clippy、相关格式及三语浏览器通过。外部正式代理仍待验证。
