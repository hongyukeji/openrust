# Webhook 接收模型契约

核对固定 Spatie Client `ff886d1bae33d35ad98b6c91403f280df158129d` 的 `WebhookCall`，以及 OpenRust 的接收模型、Store 和运行时具名注册。证据包括当前源码和同一测试应用的真实三库接收、Artisan 与独立 Worker 验收。 [固定 Spatie 模型](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/Models/WebhookCall.php) · [租约和原生队列验收](processing-failed-hook.md)

| 固定契约 | OpenRust 对照及证据 |
| --- | --- |
| `storeWebhook`、输入解析和附件写入 | `WebhookCall::store_webhook` 接收已验签的 `VerifiedWebhook`，委托 `WebhookStore`。数据库适配保存完整 URL、方法、选定头、原始字节和输入快照，独立 Worker 按接收标识恢复。路由夹具用真实 JSON、重复字段表单、查询和 multipart 验证。 [存储及输入](receiver.md) · [输入快照和附件](payload.md) |
| `headersToStore`、`headerBag`、`headers` | 配置限制持久化头，模型返回原生 `HeaderMap`。`headers` 复用 `header_bag`，原始头值可由同一模型读取。 [头部与真实接收](receiver.md) |
| `saveException`、`clearException` 和时间字段 | 模型委托 Store 修改诊断，持久化成功后更新内存记录。默认处理流程保存限长脱敏诊断。创建、读取、旧行回退和更新时间不回退均已核对。 [更新时间](receipt-update-time.md) |
| `prunable`、`MassPrunable` | `prunable` 返回受范围和批次上限约束的记录 ID。通用 `model:prune` 具名注册及包清理命令复用 Store，包含预演、活跃租约、终态、附件引用和批次事件。 [模型清理](retention.md#model-prune) |
| `getAttachments`、旧载荷回退 | 从持久原文或私有对象返回原生上传对象分组。可信旧导出使用独立附件列优先、载荷字段回退，解析边界与在线入口分别验证。 [旧附件](legacy-attachments.md) · [共享私有存储](shared-object-storage.md) |
| `webhook_model` 自定义类、保护方法与 Eloquent 继承 | 名称解析进入 `register_webhook_store`，Store 可控制原文入库、查找、诊断、附件和清理。测试应用用具名工厂与独立 Worker 实测另一接收范围。实际工厂产出数据库 Store，自定义实现的接口另由 Rust 契约测试覆盖。 |

动态模型继承、通用查询转发和 `guarded` 批量赋值属于 Eloquent 的装配方式。Rust 公开记录和具名存储工厂保持接收与验签边界，应用需要的读写行为通过 `WebhookStore` 明确授权和实现。模型 ID 使用 UUID，时间用 Unix 秒，异常结构采用可控诊断。原 Laravel 整表迁移仍需逐行映射和行为验收。已覆盖的旧附件导出和本地共享私有 S3 证据沿用各自记录，目标对象存储及发布环境独立核验。
