# Webhook 接收载荷与附件

本批对应 WC-09／WC-10 的数据库接收实现。固定 Spatie Client `ff886d1bae33d35ad98b6c91403f280df158129d` 的 `WebhookCall::storeWebhook` 先建立输入快照，按配置保留上传文件。固定 Laravel `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 `InteractsWithInput::input` 合并输入与查询，同名键优先使用输入源。OpenRust 复用原生 Request 解析，不引入新依赖。旧版仓库未发现通用接收模型可复用。

数据库存储在验签和 Profile 通过后，使用原始方法、URI、Content-Type 和原文建立独立请求快照。解析输入与原文在同一行写入。该快照保留签名原文中的空白和空字符串，不从全局中间件改写后的输入重建。解析错误阻断持久化与派发。附件保留框架原有字段分组，通过同一行原始 multipart 正文和受控媒体类型重建为原生上传类型。应用可取文件名及原始字节，无需恢复服务器临时路径，也不另存一份附件字节。附件禁用时读取视图为空，完整原文仍包含附件字节。

新增可回滚迁移保留既有原文记录。迁移前记录的解析载荷为 None，无附件视图。模型读取继续委托范围受限的存储适配。第三方适配器可实现新方法，未实现时返回明确的 `PayloadUnsupported`，保留原有 trait 消费者的源代码兼容性。数据库内原文仍使用原有编码，访问权限和留存由应用配置。

后续补验了 [Storage 磁盘引用](attachment-storage.md)、[附件清理](attachment-cleanup.md)和[旧版附件恢复](legacy-attachments.md)。正式处理 Job、去重与恢复继承既有待办。本批不关闭 WC-09／WC-10 或 WH-02。

隔离快照基线 `cfa3c473a93f7e3131737a69aabf2bd05798220d`。Rust 1.94.0，使用项目 build_guard 和锁文件。PostgreSQL 18.7、MySQL 8.4.13、SQLite 各通过 114 次真实 HTTP 请求与 72 次独立 Artisan Worker 消费，覆盖原始二进制附件、字段分组、显示用文件基名、关闭附件视图、查询与正文冲突、重复表单字段、缓存与唯一路由名。三库均实际执行迁移、回滚新增列、重新迁移，并读取旧记录的原文与空快照。旧版 SQLite 接收验收及双协议签名、包发现、重复发布、缓存和 Artisan 回归通过。

接收包契约、工厂、Processor 和 Provider 共 31 项 Rust 回归及六段三语文档编译通过。全仓格式与两项 Webhook feature 的 workspace/all-targets 严格 Clippy 通过。三语文档站内容校验和构建通过，保留原有产物大小提示。实测日志保留在 `target/tmp/webhooks-payload`。
