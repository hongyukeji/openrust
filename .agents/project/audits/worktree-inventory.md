# 未跟踪文件盘点

本次在任务中断批次 `d4923a5d` 后重新读取主工作区未跟踪清单。共 149 个文件，按内容及归属核对。现行交付计划和三语文档对照可从干净提交树独立检查。本批提交这两份文档，余下 147 个路径保留在原工作区。

- [正式版交付计划](../plan.md)
- [Laravel 13.x 文档对照](../plans/laravel-docs-delivery-map.md)

## 验收

从干净 Git 归档检查 69 个交付单元、492 个本地链接及单元锚点、214 处英文正文引用的两种中文对应路径。依赖图无环。

## 复核结果

在 `411e2d35` 上，逐项比对 Git 未跟踪清单与文件内容，仍有 147 个路径。84 个空文件和 12 份历史补丁原地保留。51 个非空路径均已复核，处理如下。

| 范围 | 路径数 | 处理 |
|---|---:|---|
| 模块说明文件 | 21 | 有效行全部是 Rust 模块注释，当前无独立可执行行为。随各模块正式接线时复核。 |
| 核心设计草稿 | 21 | 保留骨架时期的设计资料。多个文件互链未跟踪草稿，广播草稿还链接缺失的 `routes/channels.rs`。发布范围按正式交付计划和对应三语正文核对。 |
| 启动范围草稿 | 2 | 旧架构及文件清单互链本地资料，现行启动范围按正式交付记录核对。 |
| 早期路线与待办 | 2 | 两份内容早于现行交付顺序。正式依赖及状态记录以现行计划与逐批验收为准。 |
| 本地进度流水 | 1 | 包含本机候选路径、进程会话和历史草稿引用。保留为本地交接，发布证据使用各单元验收记录。 |
| 容器排除规则 | 1 | 同目录 Dockerfile 和 Compose 文件均为空，规则随完整容器构建和部署验收提交。 |
| 框架 README 草稿 | 1 | 13 个相对链接中有 12 个指向未跟踪资料。正文和示例随框架 README 单元重核。 |
| 数据库翻译扩展 | 2 | 当前分支缺扩展清单和运行实现，源文件只提供骨架。旧分支已有可评估实现，随翻译扩展交付核对。 |

本批入库的是复核结论。上表文件保留在原工作区，逐批正式交付时依据对应验收证据纳入。

## 文件与处理原因

- `.agents/project/README.md` · 空占位
- `.agents/project/architecture.md` · 早期启动范围草稿，现行最小范围待重核
- `.agents/project/audits/01-02f/excluded-workspace-wip.patch` · 历史工作区补丁
- `.agents/project/audits/01-02f/preexisting-wip.patch` · 历史工作区补丁
- `.agents/project/audits/01-02g/preexisting-wip.patch` · 历史工作区补丁
- `.agents/project/audits/01-02h/preexisting-wip.patch` · 历史工作区补丁
- `.agents/project/audits/01-02i/preexisting-wip.patch` · 历史工作区补丁
- `.agents/project/audits/01-02j/preexisting-wip.patch` · 历史工作区补丁
- `.agents/project/audits/01-02k/preexisting-wip.patch` · 历史工作区补丁
- `.agents/project/audits/01-02l/preexisting-wip.patch` · 历史工作区补丁
- `.agents/project/audits/01-02m/preexisting-wip.patch` · 历史工作区补丁
- `.agents/project/audits/01-02n/preexisting-wip.patch` · 历史工作区补丁
- `.agents/project/audits/01-02o/preexisting-wip.patch` · 历史工作区补丁
- `.agents/project/audits/01-02p/preexisting-wip.patch` · 历史工作区补丁
- `.agents/project/context.md` · 空占位
- `.agents/project/core/artisan.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/auth.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/broadcasting-websocket.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/cache-redis.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/collections.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/database.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/eloquent.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/encryption-hashing.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/events.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/filesystem.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/localization.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/logging-exceptions.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/mail-notifications.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/pagination-api-resources.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/password-reset-email-verification.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/queue-scheduling.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/rate-limiting.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/session-cookie-csrf.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/startup-files.txt` · 早期启动范围草稿，现行最小范围待重核
- `.agents/project/core/url-generation.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/validation.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/core/views-templates.md` · 骨架阶段设计记录，现状和链接待重核
- `.agents/project/decisions/README.md` · 空占位
- `.agents/project/decisions/adr-001-framework-principles.md` · 空占位
- `.agents/project/memory/testing.md` · 空占位
- `.agents/project/memory/verification.md` · 空占位
- `.agents/project/plan.md` · 本批提交的交付计划
- `.agents/project/plans/laravel-docs-delivery-map.md` · 本批提交的交付计划
- `.agents/project/plans/laravel-rebuild.md` · 空占位
- `.agents/project/progress.md` · 本地进度流水，包含候选路径并引用历史草稿
- `.agents/project/roadmap.md` · 早期路线或待办记录，进度已变化
- `.agents/project/stack.md` · 空占位
- `.agents/project/style.md` · 空占位
- `.agents/project/todo.md` · 早期路线或待办记录，进度已变化
- `.github/workflows/ci.yml` · 空占位
- `app/broadcasting/mod.rs` · 空占位
- `app/events/mod.rs` · 仅有模块说明的占位
- `app/http/controllers/mod.rs` · 空占位
- `app/http/requests/mod.rs` · 空占位
- `app/http/resources/mod.rs` · 空占位
- `app/jobs/mod.rs` · 仅有模块说明的占位
- `app/listeners/mod.rs` · 仅有模块说明的占位
- `app/mail/mod.rs` · 仅有模块说明的占位
- `app/notifications/mod.rs` · 仅有模块说明的占位
- `app/policies/mod.rs` · 仅有模块说明的占位
- `app/rules/mod.rs` · 仅有模块说明的占位
- `app/view/components/mod.rs` · 空占位
- `database/factories/mod.rs` · 空占位
- `database/factories/user_factory.rs` · 空占位
- `docker/.dockerignore` · 依赖仍为空的容器构建文件，尚无独立构建验收
- `docker/Dockerfile` · 空占位
- `docker/docker-compose.yml` · 空占位
- `resources/css/app.css` · 空占位
- `resources/js/app.js` · 空占位
- `vendor/openrust/framework/README.md` · 框架 README 草稿，旧签名和路径待核对
- `vendor/openrust/framework/src/ai/console/agent_make_command.rs` · 空占位
- `vendor/openrust/framework/src/ai/console/tool_make_command.rs` · 空占位
- `vendor/openrust/framework/src/cache/console/cache_table_command.rs` · 空占位
- `vendor/openrust/framework/src/database/connectors/connector_interface.rs` · 空占位
- `vendor/openrust/framework/src/database/connectors/mariadb_connector.rs` · 空占位
- `vendor/openrust/framework/src/database/connectors/mysql_connector.rs` · 空占位
- `vendor/openrust/framework/src/database/connectors/postgres_connector.rs` · 空占位
- `vendor/openrust/framework/src/database/connectors/sql_server_connector.rs` · 空占位
- `vendor/openrust/framework/src/database/connectors/sqlite_connector.rs` · 空占位
- `vendor/openrust/framework/src/database/console/migrations/migrate_make_command.rs` · 空占位
- `vendor/openrust/framework/src/database/eloquent/concerns/has_global_scopes.rs` · 仅有模块说明的占位
- `vendor/openrust/framework/src/database/grammar.rs` · 空占位
- `vendor/openrust/framework/src/database/mariadb_connection.rs` · 空占位
- `vendor/openrust/framework/src/database/mysql_connection.rs` · 空占位
- `vendor/openrust/framework/src/database/postgres_connection.rs` · 空占位
- `vendor/openrust/framework/src/database/query/grammars/mariadb_grammar.rs` · 空占位
- `vendor/openrust/framework/src/database/query/grammars/mysql_grammar.rs` · 空占位
- `vendor/openrust/framework/src/database/query/grammars/postgres_grammar.rs` · 空占位
- `vendor/openrust/framework/src/database/query/grammars/sql_server_grammar.rs` · 空占位
- `vendor/openrust/framework/src/database/query/grammars/sqlite_grammar.rs` · 空占位
- `vendor/openrust/framework/src/database/query/processors/mariadb_processor.rs` · 空占位
- `vendor/openrust/framework/src/database/query/processors/mysql_processor.rs` · 空占位
- `vendor/openrust/framework/src/database/query/processors/postgres_processor.rs` · 空占位
- `vendor/openrust/framework/src/database/query/processors/processor.rs` · 空占位
- `vendor/openrust/framework/src/database/query/processors/sql_server_processor.rs` · 空占位
- `vendor/openrust/framework/src/database/query/processors/sqlite_processor.rs` · 空占位
- `vendor/openrust/framework/src/database/schema/blueprint_state.rs` · 空占位
- `vendor/openrust/framework/src/database/schema/grammars/mariadb_grammar.rs` · 空占位
- `vendor/openrust/framework/src/database/schema/grammars/mysql_grammar.rs` · 空占位
- `vendor/openrust/framework/src/database/schema/grammars/postgres_grammar.rs` · 空占位
- `vendor/openrust/framework/src/database/schema/grammars/sql_server_grammar.rs` · 空占位
- `vendor/openrust/framework/src/database/schema/grammars/sqlite_grammar.rs` · 空占位
- `vendor/openrust/framework/src/database/schema/schema_state.rs` · 空占位
- `vendor/openrust/framework/src/database/sql_server_connection.rs` · 空占位
- `vendor/openrust/framework/src/database/sqlite_connection.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/cast_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/channel_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/class_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/component_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/config_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/enum_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/event_cache_command.rs` · 仅有模块说明的占位
- `vendor/openrust/framework/src/foundation/console/event_clear_command.rs` · 仅有模块说明的占位
- `vendor/openrust/framework/src/foundation/console/exception_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/interface_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/job_middleware_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/listener_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/mail_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/notification_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/observer_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/package_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/request_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/resource_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/rule_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/scope_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/trait_make_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/view_cache_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/view_clear_command.rs` · 空占位
- `vendor/openrust/framework/src/foundation/console/view_make_command.rs` · 空占位
- `vendor/openrust/framework/src/queue/connectors/database_connector.rs` · 仅有模块说明的占位
- `vendor/openrust/framework/src/queue/connectors/redis_connector.rs` · 仅有模块说明的占位
- `vendor/openrust/framework/src/queue/connectors/sync_connector.rs` · 仅有模块说明的占位
- `vendor/openrust/framework/src/queue/console/batches_table_command.rs` · 空占位
- `vendor/openrust/framework/src/queue/console/failed_command.rs` · 仅有模块说明的占位
- `vendor/openrust/framework/src/queue/console/failed_table_command.rs` · 空占位
- `vendor/openrust/framework/src/queue/console/flush_failed_command.rs` · 仅有模块说明的占位
- `vendor/openrust/framework/src/queue/console/forget_failed_command.rs` · 仅有模块说明的占位
- `vendor/openrust/framework/src/queue/console/restart_command.rs` · 仅有模块说明的占位
- `vendor/openrust/framework/src/queue/console/retry_command.rs` · 仅有模块说明的占位
- `vendor/openrust/framework/src/queue/console/table_command.rs` · 空占位
- `vendor/openrust/framework/src/queue/jobs/database_job.rs` · 仅有模块说明的占位
- `vendor/openrust/framework/src/queue/jobs/redis_job.rs` · 仅有模块说明的占位
- `vendor/openrust/framework/src/queue/jobs/sync_job.rs` · 仅有模块说明的占位
- `vendor/openrust/framework/src/routing/console/controller_make_command.rs` · 空占位
- `vendor/openrust/framework/src/routing/console/middleware_make_command.rs` · 空占位
- `vendor/openrust/framework/src/session/console/session_table_command.rs` · 空占位
- `vendor/openrust/framework/src/validation/console/filter_make_command.rs` · 空占位
- `vendor/openrust/translation-database/src/database_loader.rs` · 翻译扩展骨架，尚无运行实现
- `vendor/openrust/translation-database/src/lib.rs` · 翻译扩展骨架，尚无运行实现
