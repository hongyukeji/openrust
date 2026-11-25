# Webhook client

独立的接收扩展，使用应用自己的 Provider、配置及共享基础设施。当前公开能力为配置、类型化扩展工厂、显式路由与控制器、签名验证、手动 Processor 与按范围隔离的数据库接收记录，应用显式提供任务派发实现；Storage 附件引用、正式处理 Job、去重、恢复与重放尚待补齐，不宣称完整收发模块已可生产部署。

通过 `WebhookClientServiceProvider` 装配；应用可独立启用该包，不要求安装另一收发包。用法见项目三语文档 `packages/webhook-client.md`。来源与许可见 [第三方说明](THIRD_PARTY_NOTICES.md)。
