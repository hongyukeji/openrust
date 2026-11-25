# Webhook 接收记录保留期

固定 [Spatie WebhookCall](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/Models/WebhookCall.php) 的 `prunable` 由 `MassPrunable` 使用。它要求整型 `delete_after_days`，按 `created_at < now()->subDays(days)` 选记录。包配置默认 30 天，Laravel README 要求应用显式调度 `model:prune` 并指定包模型。当前 OpenRust 通用 `model:prune` 仍在计划清单中。本批提供包级 `webhook-client:prune` 与 `WebhookCall::prunable` 存储委托，采用相同的 30 天默认值。Rust 显式 `null` 关闭命令清理，其他非整数、负数及超界值报错。调度由应用选择。

本包的可靠接收要求清理保全在途数据，因此查询增加终态时间、范围和无租约约束。创建及终态时间均需早于截止。没有终态的旧记录、待关联、在途和活跃租约均保留。按创建时间和接收 ID 取有界批次，实际删除再次核对相同条件。去重键与处理尝试由现有外键级联清理。每次命令必须指定可信 scope，支持具名数据库、预演和 1–500 条上限。沿用既有 workspace 的 clap 版本，无新增 registry 依赖。

隔离源码使用 `4f19d7c29` 加本任务提交至 `6745be7fa` 及本批文件，Rust 1.94.0、locked 和项目 build_guard。实测真实接收、租约 Worker 和 Artisan。测试只在隔离数据库注入旧时间，复核批次顺序、最近终结、历史无终态、已领取、待处理、其他范围、预演、禁用及失败参数。外键联动实测同时保留关联队列。首次构建暴露测试夹具的 JSON 错误类型需要显式转换，修正后重建。日志保留于 `target/tmp/webhooks-prune`。

跨后端投递状态回收、Storage 附件删除、独立去重墓碑期限、主动调度的业务安装场景及多主机清理门禁仍待对应批次。已终结记录被清理后，非常晚的旧任务可能找不到模型。通用模型清理和完整 WH-02／WH-06 仍在实施。

SQLite、PostgreSQL 18.7、MySQL 8.4.13 每库最终通过 174 个真实 HTTP、120 次独立 Worker、5 次唤醒和 7 次保留期 Artisan 调用。本批新增 5 个 HTTP、4 个 Worker 与 7 次清理命令。旧签名／接收与许可回归、三库迁移回滚再迁移、全仓格式、两 Webhook feature 的 workspace/all-targets 严格 Clippy、三语 Rust 示例、站点构建和根／子目录浏览器均通过。既有块体积提示保留。清理隔离数据库、应用及站点输出，保留验收日志。
