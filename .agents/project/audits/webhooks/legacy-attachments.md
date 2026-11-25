# 接收模型恢复旧版附件

固定 Spatie Client 的 `WebhookCall::getAttachments()` 读取 `attachments` 列，列缺失或为 `null` 时读取 `payload.attachments`。空数组仍是有效附件列。旧记录用 Base64 携带原始字节，并保存文件名、媒体类型、大小、错误和临时路径。固定 Laravel `UploadedFile::createFromBase` 包装已有上传对象。OpenRust 复用原生 `UploadedFile` 的字节表示，由应用将可信导出的两列交给 `WebhookCall::restore_legacy_attachments`。 [固定 Spatie 接收模型](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/Models/WebhookCall.php) · [固定 Laravel UploadedFile](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/UploadedFile.php)

恢复过程返回原生附件分组，保留数组顺序或对象键，最多 128 个文件，合计最多 16 MiB 解码内容。错误上传、无效 Base64、文件名中的路径和标明大小与实际内容不符的记录会被拒绝。旧临时路径只属于源系统，读回以 Base64 内容为准。现行 `get_attachments` 仍委托范围受限的 Store，只读取真实 multipart 附件。普通 JSON 中的同名字段交给业务代码处理。该入口用于应用控制的归档读取，整表跨框架迁移、重签旧记录及回放仍由迁移应用决定。

## 验收

隔离应用经 Artisan 读取旧版 JSON 导出，验证独立列和载荷回退、独立空列覆盖载荷、具名文件、Unicode 文件名、二进制原内容和错误分类。真实 HTTP 将同样形状的 JSON 交给正常接收路由，SQLite、PostgreSQL 18.7、MySQL 8.4.13 上的独立 Worker 和 Artisan 读回均保留原始载荷而无附件视图，multipart 附件和关闭附件的既有行为继续通过。Rust 合约覆盖文件数与字节预算及非法元数据。原生 Request 读写回归、严格 Clippy、格式和三语文档也随批核对。
