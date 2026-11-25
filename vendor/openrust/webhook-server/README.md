# Webhook server

独立的发送扩展，使用应用自己的 Provider、配置及共享基础设施。当前公开能力为配置、请求快照、目的地规则、签名、退避、持久投递记录和安全 HTTP 投递。提供基于 Artisan Scheduler 的定时恢复。重放与端点管理仍在开发。

通过 `WebhookServerServiceProvider` 装配；应用可独立启用该包，不要求安装另一收发包。用法见项目三语文档 `packages/webhook-server.md`。来源与许可见 [第三方说明](THIRD_PARTY_NOTICES.md)。
