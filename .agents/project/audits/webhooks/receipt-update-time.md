# 接收模型更新时间

固定 Spatie 接收模型公开继承 `created_at` 与 `updated_at`，保存或清除异常均走模型保存。Rust 保留已有模型字段和自定义构造，使用 `WebhookCall::updated_at(&store)` 读取持久更新时间。数据库适配器在接收时将两个字段设为同一 Unix 秒值。保存或清除异常、领取结算、延期／唤醒和显式重放在原有数据库写入中更新时间。写入取数据库时钟与已有时间中的较大值，空列先回退到 `created_at`。 [固定 Spatie 模型](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/Models/WebhookCall.php) · [固定 Laravel HasTimestamps](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasTimestamps.php)

迁移添加可空列，旧记录在第一次受支持的写入前用接收时间响应。自定义 `WebhookStore` 可实现同一入口，既有适配器使用默认空值。更新时间只反映当前所接入的持久写入，仍按原接收时间计算留存期限。迁移回滚删除更新时间列，保留接收记录及原始时间。

非原子接收在插入后、派发前会清空异常，因此 HTTP 成功后读取的更新时间可能晚于接收时间。租约夹具按非递减契约断言，保留后续旧行回退和异常写入的精确断言。

## 验收

SQLite、PostgreSQL 18.7、MySQL 8.4.13 的真实 HTTP 接收、Artisan 和独立 Worker 覆盖创建值、旧行迁移、第一次修改、异常清除、范围隔离、时钟落后情况下的非回退，以及显式终态处理后保存时间。通过带真实记录的单列迁移回滚和重跑验证原有数据。缓存、并发、接管、重放、原始字节、现行附件及可信旧附件继续通过接收回归。相关格式、包契约、应用与包严格 Clippy 及三语文档随批验证。

从 `93c44a984` 全新归档叠加本批迁移，SQLite 正式应用通过双包 `init`、重复安装、真实 HTTP、Artisan 发布 33 个迁移文件、编译覆盖及旧账本回滚重建。另一次独立安装跑通 Spatie 与 Standard Webhooks 的 Artisan 发送、HTTP 接收和独立 Worker。发布夹具逐文件核对两个包的实际迁移，并单独断言新文件与新列。
