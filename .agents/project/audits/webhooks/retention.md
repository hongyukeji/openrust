# Webhook 接收记录保留期

固定 [Spatie WebhookCall](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/Models/WebhookCall.php) 的 `prunable` 由 `MassPrunable` 使用。它要求整型 `delete_after_days`，按 `created_at < now()->subDays(days)` 选记录。包配置默认 30 天，Laravel README 要求应用显式调度 `model:prune` 并指定包模型。当前 OpenRust 通用 `model:prune` 仍在计划清单中。本批提供包级 `webhook-client:prune` 与 `WebhookCall::prunable` 存储委托，采用相同的 30 天默认值。Rust 显式 `null` 关闭命令清理，其他非整数、负数及超界值报错。调度由应用选择。

本包的可靠接收要求清理保全在途数据，因此查询增加终态时间、范围和无租约约束。创建及终态时间均需早于截止。没有终态的旧记录、待关联、在途和活跃租约均保留。按创建时间和接收 ID 取有界批次，实际删除再次核对相同条件。去重键与处理尝试由现有外键级联清理。每次命令必须指定可信 scope，支持具名数据库、预演和 1–500 条上限。沿用既有 workspace 的 clap 版本，无新增 registry 依赖。

隔离源码使用 `4f19d7c29` 加本任务提交至 `6745be7fa` 及本批文件，Rust 1.94.0、locked 和项目 build_guard。实测真实接收、租约 Worker 和 Artisan。测试只在隔离数据库注入旧时间，复核批次顺序、最近终结、历史无终态、已领取、待处理、其他范围、预演、禁用及失败参数。外键联动实测同时保留关联队列。首次构建暴露测试夹具的 JSON 错误类型需要显式转换，修正后重建。日志保留于 `target/tmp/webhooks-prune`。

跨后端投递状态回收、Storage 附件删除、独立去重墓碑期限、主动调度的业务安装场景及多主机清理门禁仍待对应批次。已终结记录被清理后，非常晚的旧任务可能找不到模型。通用模型清理和完整 WH-02／WH-06 仍在实施。

SQLite、PostgreSQL 18.7、MySQL 8.4.13 每库最终通过 174 个真实 HTTP、120 次独立 Worker、5 次唤醒和 7 次保留期 Artisan 调用。本批新增 5 个 HTTP、4 个 Worker 与 7 次清理命令。旧签名／接收与许可回归、三库迁移回滚再迁移、全仓格式、两 Webhook feature 的 workspace/all-targets 严格 Clippy、三语 Rust 示例、站点构建和根／子目录浏览器均通过。既有块体积提示保留。清理隔离数据库、应用及站点输出，保留验收日志。

## 通用模型清理入口 {#model-prune}

固定 Laravel 的命令按 `--model` 或 `--path`／`--except` 选模型，默认发现应用模型；`--pretend` 只计数，实际执行派发开始、每批累计删除及完成事件。Rust 从当前应用的编译策略注册表找到模型及源路径，不运行时加载源文件。已有 `ModelPruner` 按已删除条数推进批次，Webhooks 对应批量清理路径。逐实例 `Prunable` 已在[后续批次](#prunable)接入。旧版 `6d1a81924` 无模型清理实现可复用。[固定 PruneCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/PruneCommand.php) · [固定 MassPrunable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/MassPrunable.php)

Webhook 包注册 `openrust_webhook_client::models::WebhookCall`，默认应用目录扫描不会选中包模型。显式选择后，使用有效配置中内置数据库存储的可信范围和具名连接；同名 profile 按后值覆盖，按连接名称和范围去重，不读取签名凭据。包中的原生数据库注册被替换时跳过它。没有声明在数据库配置中的历史范围仍可通过包命令单独清理。

模型命令整批预演走主库计数，不受删除批次上限影响。删除每批最多 500 条，复用原有终态、租约、去重与附件持久登记规则；附件对象由既有包命令回收。已报告的删除数量仅包含本次实际删除。

隔离源码以 `eeb858574` 叠加本批，使用 Rust 1.94.0。从实际 HTTP 接收、独立 Artisan Worker 到模型清理的 SQLite、PostgreSQL 18.7、MySQL 8.4.13 均通过。各库基础消费者覆盖 136 次 HTTP 和 83 次独立 Worker，另有清理及恢复场景。验证缓存前按路径预演及缓存后按名分批删除，缺签名凭据也可运行，默认目录与排除选项不处理包模型。旧重复名称不扩大范围，覆盖数据库工厂后不触碰数据库；保留在途、近期终结和其他范围记录，外键清理有效。按批派发累计 1、2，预演无生命周期事件。注入附件登记失败时删除一起回滚，补偿命令随后回收对象。

框架注册与应用隔离回归、接收包 33 项测试、三语六组示例编译、完整隔离工作区格式检查、框架／包及消费者严格 Clippy 均通过。

## 逐实例模型清理 {#prunable}

`Prunable` 将固定上游的实例清理契约接到既有 Eloquent 查询和删除机制。`prunable()` 选择候选，未提供查询时返回建模错误。`pruning()` 是默认空操作的异步钩子，先于 `delete()` 或 `force_delete()`；软删除模型的候选包含已删除记录。Rust 通过 trait 方法表达 PHP 扩展钩子，按应用注册 `register_prunable::<M>()` 后可由 Artisan 发现。直接调用 `M::prune_all(app, chunk)` 也会派发每批进度，直接 `model.prune()` 返回该模型的原生删除结果。[固定 Prunable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Prunable.php)

逐实例跑主键游标，先报告单条错误，报告成功后继续下一条；无法解析异常处理器时保留原错误，报告操作本身失败时传播错误。固定上游将未抛错的实例计入进度，包括观察者取消删除后返回 false 的实例，故逐实例 Artisan 输出采用「已处理」计数。零成功批次仍派发累计事件，游标继续推进，避免重扫失败记录。包自己的批量策略仍按实际删除数推进，附件和保留期业务继续由包存储负责。

本轮沿用 Webhook 的租约和保留期策略。通用 `MassPrunable` 已在[后续批次](#mass-prunable)接入。真实跨主机的支撑环境和完整 Webhook 对照仍待验收。

隔离基线为 `37e471407`，Rust 1.94.0。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 的实际 Artisan 消费者均验证默认发现、只读预演、普通删除、软删除模型永久删除、失败首批报告后继续、取消删除保留行、每批累计及直接调用。框架应用隔离回归、隔离工作区格式检查和框架及验收目标严格 Clippy 已通过。首次全矩阵在共同磁盘停线中断，磁盘守卫持锁释放约 89.9 GB 后重建。日志保留于 `target/tmp/eloquent-prunable`。

重建后，三库各完成 136 次真实接收 HTTP、83 次独立 Worker 及额外恢复与清理场景。Webhook 原有范围、活跃租约、配置缓存及附件事务回滚均通过回归。

## 模型批量清理 {#mass-prunable}

新增 `MassPrunable`、`ModelPruners::register_mass_prunable::<M>()` 和直接调用入口。批量执行在初始查询保留正数 limit，空或零 limit 取命令批次。普通模型使用带已应用作用域的查询，软删除模型按固定 Eloquent 永久删除语义直接使用显式条件。预演包含软删除记录并应用其余全局作用域，所以预演与软删除模型的实际永久删除范围可能不同。批量没有实例化或单条回调，按原生 SQL 报告的删除数逐批派发 `ModelsPruned`，零删除终止。Webhooks 继续使用存储策略维护范围、租约和附件事务。[固定 MassPrunable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/MassPrunable.php) · [固定 Eloquent Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Builder.php)

Rust 复用同一连接与候选查询，保留显式写入限制。查询回调把删除上限取消或设零时返回错误。已有 MySQL 联结限量写入限制继续生效。SQL Server 的批量联结和正偏移在发出 SQL 前拒绝，该驱动未做本批实机验收。PostgreSQL 继承及分区表中 `ctid` 只在所属物理表内标识行位置，限量更新和删除改用 `(tableoid, ctid)`，避免相同行位置匹配另一个分区。[PostgreSQL 18 系统列](https://www.postgresql.org/docs/18/ddl-system-columns.html)

隔离基线为 `1c0dbe8b5`，Rust 1.94.0。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 的 Artisan 专项通过预演、显式两条上限、默认单条分批、回调取消上限时拒绝写入、作用域、零删除时无进度和直接调用。通过观察者反例确认没有实例化及逐实例删除事件。PostgreSQL 专项实际构造两个分区的相同 `ctid`，限量删除和带表前缀及别名的限量更新都只触及目标行。框架隔离测试、隔离工作区格式检查及目标严格 Clippy 通过。日志位于 `target/tmp/eloquent-mass-prunable`。

首次 PostgreSQL 接收回归的延期用例在提前领取断言处失败。夹具改用明确的数据库到期状态，分别验证提前的重复 Worker 无法领取及到期后成功领取，仍通过实际派发创建延期及新队列任务。生产延期逻辑保持原样。

后续 PostgreSQL 回归在延期、附件等场景已通过后触及共享磁盘停线。清理构建产物并重建同一隔离源码后，单独重跑完整 PostgreSQL 矩阵通过。修正夹具后三库各通过 136 次真实接收 HTTP、83 次独立 Worker 及额外的恢复和清理场景。
