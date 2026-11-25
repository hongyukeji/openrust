# Webhook 迁移发布与账本身份

两个包分别注册 `webhook-client-migrations` 和 `webhook-server-migrations`。发布文件位于应用的 `database/migrations/`，以原文件名提供项目构建入口所需的 `Definition`，继续使用现有 `build.rs` 的迁移发现。普通发布遵循 `vendor:publish` 的保留行为。

既有 Webhook 编译迁移曾实际在数据库中执行，文件名及排序是账本身份。因此这些已加载迁移使用保留文件名的发布方式。具名包显式允许应用覆盖，迁移器只在应用注册了 `database/migrations/` 下的同名编译条目时选用它，同时保留原包路径作为 `--path` 选择别名。以往的 `load_migrations_from` 继续保持重复文件名检查语义。未发布的 Webhook 迁移仍由包自身注册。固定 Laravel 的 Provider 资源发布与加载作为职责基线，保留迁移身份是对已交付 Rust 应用的迁移适配。旧版 `6d1a81924` 无同类 Webhook 发布流程。 [固定 Laravel ServiceProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/ServiceProvider.php) · [固定 Spatie 接收包 Provider](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/WebhookClientServiceProvider.php)

## 实际验收

- SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 分别初始化正式应用并从两个标签发布 30 条迁移。标签之间隔离，文件名与包中的条目相同。重复发布保留已修改的应用迁移。发布后使用原制品运行迁移，原账本继续有效。
- 重建正式应用并运行迁移，原账本完全一致，已执行的建表没有再次运行。通过原包目录 `--path` 回滚两包，随后从应用目录重新迁移，原有账本身份恢复，自定义列真实生效。生成配置缓存与路由缓存后，实际 HTTP `/up` 返回 200。
- 验收入口为 `tests/webhooks_install_acceptance.py --profile both --publish-migrations`。源码和验收数据库均隔离，回滚只用于该验收环境。
- 使用发布文件重新构建的应用在三库分别运行独立 Artisan Worker，Spatie 和 Standard Webhooks 两种签名均通过。框架迁移测试目标报告 8 项通过。定向格式、Python 语法和框架及两个 Webhook 库的严格 Clippy 均通过。全工作区格式检查仍报告[三个文件的既有差异](backend-interruption.md)。
