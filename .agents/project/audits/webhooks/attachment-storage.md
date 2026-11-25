# Webhook 附件的私有 Storage 引用

本批补 WC-10。固定 [Spatie WebhookCall](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/Models/WebhookCall.php) 保留附件开关，并从已存文件元数据还原上传文件。Rust 端复用原生 multipart 解析和 Storage。已有内联模式继续可用。profile 显式配置 `attachment_disk` 且启用附件保存时，符合条件的原始 multipart 写入一个私有对象，数据库正文位置留空，仅保留对象引用、长度、SHA-256、原 multipart 类型和解析输入。对象路径由范围摘要与接收 UUID 导出。客户端文件名仅供显示。

对象写入前先登记暂存行。同库接收事务记录引用并移除登记。原子入队继续共用该事务，普通模式也在事务中绑定记录和登记。明确回滚时尝试删除对象。写入结果不确定、清理失败或进程崩溃时保留暂存行。提交返回不确定时避免删掉可能已提交的对象。Worker 和 Artisan 从已保存的磁盘名恢复原始正文和上传组，并检查读取上限、长度及摘要。常规保留期命令暂时保留有附件对象引用的接收记录。

隔离源码以 `4f19d7c29` 加本任务已验变更至 `f666ef7f8` 及本批文件为基线，Rust 1.94.0，构建和验收经项目 build_guard。验收使用真实私有本地磁盘和独立 HTTP、Artisan 及 Worker。核对二进制、文件名、上传组、原始正文、无附加模式、范围隔离、已确认回滚、并发去重、暂存结果不确定和入队后提交前的真实进程退出。缺失对象或同长度篡改均使读取失败。旧记录继续从数据库还原。日志位于 `target/tmp/webhooks-attachments`。

尚需通过暂存登记执行有界补偿，完成引用与附件的协同留存，并在跨主机共享磁盘及远端驱动上验收。WC-10、WH-02 及 WH-06 仍保留其余门禁。

最终 SQLite、PostgreSQL 18.7、MySQL 8.4.13 每库通过本批 7 个真实 HTTP 和 3 次独立 Worker。迁移分别在仅有暂存登记和仅有已提交引用时拒绝真实回滚，预演仍输出迁移语句，隔离夹具解除引用后可以回滚再迁移。迁移查询使用当前事务或写连接。旧接收、签名和许可回归、全仓格式、两 Webhook feature 的 workspace/all-targets 严格 Clippy、三语示例及站点浏览器通过。隔离应用、数据库、测试附件及站点输出已清理。
