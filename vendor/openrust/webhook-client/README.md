# Webhook client

独立的接收扩展，使用应用自己的 Provider、配置及共享基础设施。当前公开能力为配置、签名验证与扩展契约；可靠接收、持久化、处理与重放尚待补齐，不宣称完整收发模块已可生产部署。

通过 `WebhookClientServiceProvider` 装配；应用可独立启用该包，不要求安装另一收发包。用法见项目三语文档 `packages/webhook-client.md`。来源与许可见 [第三方说明](THIRD_PARTY_NOTICES.md)。
