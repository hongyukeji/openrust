# Webhook 发送快照与 HTTP 固定目的地

对照固定 Spatie Server `6da6be3a721190bcdedcdef32d31d5980b77eb0e` 的 Builder 和 Job，覆盖 WS-01 至 WS-07 的请求准备、WS-08 的默认地址与连接边界、WS-09 与 WS-10 的设置保留。URL、方法、ID、JSON／原始字节、Header、配置、签名选择、队列、超时、尝试、退避、元数据和标签均在快照中稳定。GET 使用现有 RFC 3986 查询编码，按 Guzzle 的数组查询参数语义替换原 URL 查询，Spatie 签名继续使用同一份 JSON。JSON 转义处理 Unicode、斜杠和有序键，数值序列化沿用 Rust serde；需要指定线上字节时可用原始正文模式。明文签名密钥仅放在不可序列化的进程内凭据中，具名引用在尝试时由应用解析。[Spatie WebhookCall 固定源码](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/WebhookCall.php) · [Spatie Job 固定源码](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/CallWebhookJob.php) · [Guzzle 查询编码](https://github.com/guzzle/guzzle/blob/7.10.0/src/Client.php)

默认只接受 HTTPS，复核所有解析地址，排除私网、链路本地、回环、元数据地址与特殊协议地址。可信内部端点可限定来源、路径与地址。复用 native HTTP Client，固定传输使用仅返回已验证地址的 DNS 解析器，保持 URL 的 Host／SNI，禁用重定向和代理路径，复核中间件修改后的来源，拒绝自定义 Host 与 Unix socket。读取响应仍经过通用容量及超时限制。拒绝未知的签名器、退避实现、非法凭据、超限载荷和不受支持的代理／证书设置。[Reqwest 定制 DNS 接口](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html#method.dns_resolver) · [IANA IPv4 特殊地址](https://www.iana.org/assignments/iana-ipv4-special-registry/iana-ipv4-special-registry.xhtml) · [IANA IPv6 特殊地址](https://www.iana.org/assignments/iana-ipv6-special-registry/iana-ipv6-special-registry.xhtml)

## 验收

验收采用隔离源码 `4f19d7c29` 与本任务已验变更至 `15c9c1248`，再叠加本批文件，构建守卫固定 Rust 1.94.0。隔离源码保留自己的依赖图，离线恢复其 Pulse 既有锁定项后验证。工作树锁文件只增加 Webhook Server 对已锁定 serde、url、zeroize 的直接依赖。

真实回环 HTTP 验证固定地址连通、Host 保留、重定向保留 307、拒绝连接其他端口、中间件修改及 Host 覆盖，并验证响应上限。TLS 夹具验证固定连接保留 SNI、自签名证书在默认配置下被拒绝。HTTP Client 四组共 38 个测试、Webhook Server 八个测试均通过。独立 Artisan 在配置缓存前后分别准备 JSON、GET、原始二进制及具名签名／退避快照，并拒绝回环目的地。签名由 Python 独立 HMAC 核对。现有真实接收 HTTP、SQLite Worker、配置发布和许可证回归通过。全仓格式、双特性 workspace/all-targets 严格 Clippy、三语示例及文档站浏览器通过。日志在 `target/tmp/webhooks-send-builder`。

此后[持久领取与首次安全投递](first-delivery.md)已接入。WS-08 的 [TLS、mTLS 与自定义 CA](outbound-tls.md)已接入，显式代理仍待验。WS-09 至 WS-12 的任务扩展、Builder 派发变体、事件和失败钩子见后续记录。WH-04 保持开放。
