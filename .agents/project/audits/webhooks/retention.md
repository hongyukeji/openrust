# Webhook 接收记录保留期

固定 [Spatie WebhookCall](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/Models/WebhookCall.php) 的 `prunable` 由 `MassPrunable` 使用。它要求整型 `delete_after_days`，按 `created_at < now()->subDays(days)` 选记录。包配置默认 30 天，Laravel README 要求应用显式调度 `model:prune` 并指定包模型。当前 OpenRust 通用 `model:prune` 仍在计划清单中。本批提供包级 `webhook-client:prune` 与 `WebhookCall::prunable` 存储委托，采用相同的 30 天默认值。Rust 显式 `null` 关闭命令清理，其他非整数、负数及超界值报错。调度由应用选择。

本包的可靠接收要求清理保全在途数据，因此查询增加终态时间、范围和无租约约束。创建及终态时间均需早于截止。没有终态的旧记录、待关联、在途和活跃租约均保留。按创建时间和接收 ID 取有界批次，实际删除再次核对相同条件。去重键与处理尝试由现有外键级联清理。每次命令必须指定可信 scope，支持具名数据库、预演和 1–500 条上限。沿用既有 workspace 的 clap 版本，无新增 registry 依赖。

隔离源码使用 `4f19d7c29` 加本任务提交至 `6745be7fa` 及本批文件，Rust 1.94.0、locked 和项目 build_guard。实测真实接收、租约 Worker 和 Artisan。测试只在隔离数据库注入旧时间，复核批次顺序、最近终结、历史无终态、已领取、待处理、其他范围、预演、禁用及失败参数。外键联动实测同时保留关联队列。首次构建暴露测试夹具的 JSON 错误类型需要显式转换，修正后重建。日志保留于 `target/tmp/webhooks-prune`。

跨后端投递状态回收、Storage 附件删除、独立去重墓碑期限、主动调度的业务安装场景及多主机清理门禁仍待对应批次。已终结记录被清理后，非常晚的旧任务可能找不到模型。通用模型清理和完整 WH-02／WH-06 仍在实施。

SQLite、PostgreSQL 18.7、MySQL 8.4.13 每库最终通过 174 个真实 HTTP、120 次独立 Worker、5 次唤醒和 7 次保留期 Artisan 调用。本批新增 5 个 HTTP、4 个 Worker 与 7 次清理命令。旧签名／接收与许可回归、三库迁移回滚再迁移、全仓格式、两 Webhook feature 的 workspace/all-targets 严格 Clippy、三语 Rust 示例、站点构建和根／子目录浏览器均通过。既有块体积提示保留。清理隔离数据库、应用及站点输出，保留验收日志。

## 通用模型清理入口 {#model-prune}

固定 Laravel 的命令按 `--model` 或 `--path`／`--except` 选模型，默认发现应用模型；`--pretend` 只计数，实际执行派发开始、每批累计删除及完成事件。Rust 从当前应用的编译策略注册表找到模型及源路径，不运行时加载源文件。已有 `ModelPruner` 按已删除条数推进批次，Webhooks 对应批量清理路径。Laravel 的逐实例 `Prunable`、Soft Deletes 和 `pruning()` 钩子映射仍需单独对照，当前由策略所属服务提供这些行为。旧版 `6d1a81924` 无模型清理实现可复用。[固定 PruneCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/PruneCommand.php) · [固定 MassPrunable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/MassPrunable.php)

Webhook 包注册 `openrust_webhook_client::models::WebhookCall`，默认应用目录扫描不会选中包模型。显式选择后，使用有效配置中内置数据库存储的可信范围和具名连接；同名 profile 按后值覆盖，按连接名称和范围去重，不读取签名凭据。包中的原生数据库注册被替换时跳过它。没有声明在数据库配置中的历史范围仍可通过包命令单独清理。

模型命令整批预演走主库计数，不受删除批次上限影响。删除每批最多 500 条，复用原有终态、租约、去重与附件持久登记规则；附件对象由既有包命令回收。已报告的删除数量仅包含本次实际删除。

隔离源码以 `eeb858574` 叠加本批，使用 Rust 1.94.0。从实际 HTTP 接收、独立 Artisan Worker 到模型清理的 SQLite、PostgreSQL 18.7、MySQL 8.4.13 均通过。各库基础消费者覆盖 136 次 HTTP 和 83 次独立 Worker，另有清理及恢复场景。验证缓存前按路径预演及缓存后按名分批删除，缺签名凭据也可运行，默认目录与排除选项不处理包模型。旧重复名称不扩大范围，覆盖数据库工厂后不触碰数据库；保留在途、近期终结和其他范围记录，外键清理有效。按批派发累计 1、2，预演无生命周期事件。注入附件登记失败时删除一起回滚，补偿命令随后回收对象。

框架注册与应用隔离回归、接收包 33 项测试、三语六组示例编译、完整隔离工作区格式检查、框架／包及消费者严格 Clippy 均通过。
