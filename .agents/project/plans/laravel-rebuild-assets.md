# 重构资产与回归入口

> 基线：`c63d7e8`，2026-10-28；仅盘点，原验证不等于新 contract 验收。

框架实现共 110 个源码文件、tests 下 75 个文件；下列文件迁移时必须保留行为覆盖。复用/适配后的能力均须重新验证。

## 源码与测试资产

| 当前路径 | 选择与归属 |
| --- | --- |
| `src/audit/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/audit/testing.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/audit.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/database.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/flows.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/jwt.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/mfa.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/migration.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/mod.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/oidc.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/password.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/pat.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/policy.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/query.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/refresh.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/session.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/testdata/oidc-es256-private.pem` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/verification.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/cache/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/cache/tags.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/cache.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/api.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/app.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/auth.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/cache.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/database.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/grpc.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/logger.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/mailer.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/mod.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/queue.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/realtime.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/security.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/server.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/signing.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/sms.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/storage.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/view.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/app.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/command.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/error.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/execution.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/lifecycle.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/mod.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/otel.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/plugin.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/rate_limit.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/readiness.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/data/exchange/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/data/exchange/tasks.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/data/exchange.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/data.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/batch.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/coordination.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/dialect.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/migration.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/migration_lock.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/mod.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/rate_limit.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/seed.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/table_names.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/events.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/grpc.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/client.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/client_ip.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/extract.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/health.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/metrics.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/middleware.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/mod.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/plugin.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/rate_limit.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/resource.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/response.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/routes.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/serve.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/site.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/sort.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/i18n.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/lib.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/mail/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/mail/mod.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/mail/template.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/main.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/notifications.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/queue/redis.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/queue.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/realtime.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/routes.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/runtime.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/schedule.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/security/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/security.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/signing/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/signing/migration.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/signing.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/sms.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/storage/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/storage.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/tenant/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/tenant.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/testing/database.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/testing.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/validation.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/view/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/view.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `tests/consumers/README.md` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/cache-custom-driver/Cargo.toml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/cache-custom-driver/src/main.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/communication-drivers/Cargo.toml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/communication-drivers/README.md` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/communication-drivers/src/main.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/data-exchange/Cargo.toml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/data-exchange/src/main.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/db-custom-sqlite/Cargo.toml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/db-custom-sqlite/README.md` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/db-custom-sqlite/src/main.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/Cargo.toml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/Dockerfile.g3c` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/README.md` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/build.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/compose.g3c.yaml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/envoy.g3c.yaml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/proto/echo.proto` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/proto/legacy/echo.proto` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/scripts/test-g3c.sh` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/src/bin/g3c-node.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/src/bin/g3c-probe.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/src/main.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/query-authorization/Cargo.toml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/query-authorization/README.md` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/query-authorization/src/lib.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/queue-custom-driver/Cargo.toml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/queue-custom-driver/README.md` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/queue-custom-driver/src/main.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/realtime/Cargo.toml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/realtime/README.md` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/realtime/src/main.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/realtime/tests/cluster.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/core/plugin_contract.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/http/resource.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/http/router.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/http/view.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/queue/driver_contract.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/queue/redis_driver.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/auth/database.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/auth/mfa.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/auth/oidc.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/auth/oidc_keycloak.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/auth/query_authorization.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/auth/routes.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/auth/runtime.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/data_exchange.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/data_exchange_tasks.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/database/connection.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/database/coordination.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/database/migration.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/fixtures/keycloak/openrust-realm.json` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/grpc.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/http/client.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/i18n/runtime.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/mail/templates.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/queue/crash_recovery.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/queue/notifications.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/queue/redis.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/queue/worker.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/realtime.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/schedule/calendar.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/schedule/coordination.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/server/aggregate.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/storage/driver_contract.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/storage/http.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/storage/s3.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tools/README.md` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/Cargo.toml` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/README.md` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/src/main.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/.gitignore` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/AGENTS.md` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/Cargo.toml` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/config/framework.toml` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/public/index.html` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/src/app.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/src/auth_routes.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/src/main.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/src/routes.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/tests/auth_flows.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/views/index.html` | 保留独立编译边界；R1 改路径/模板 |
| `tools/macros/Cargo.toml` | 保留独立编译边界；R1 改路径/模板 |
| `tools/macros/README.md` | 保留独立编译边界；R1 改路径/模板 |
| `tools/macros/src/lib.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/performance/Cargo.toml` | 保留独立编译边界；R1 改路径/模板 |
| `tools/performance/README.md` | 保留独立编译边界；R1 改路径/模板 |
| `tools/performance/benches/db_routing.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/performance/scripts/http_load.py` | 保留独立编译边界；R1 改路径/模板 |
| `tools/testing/Cargo.toml` | 保留独立编译边界；R1 改路径/模板 |
| `tools/testing/README.md` | 保留独立编译边界；R1 改路径/模板 |
| `tools/testing/src/lib.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/testing/tests/database.rs` | 保留独立编译边界；R1 改路径/模板 |
| `apps/README.md` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/.env.example` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/.gitignore` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/AGENTS.md` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/Cargo.toml` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/README.md` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/build.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/config/api.toml` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/config/app.toml` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/config/auth.toml` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/config/cache.toml` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/config/database.toml` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/config/grpc.toml` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/config/i18n.toml` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/config/mail.toml` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/config/security.toml` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/config/server.toml` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/config/session.toml` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/config/sms.toml` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/config/view.toml` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/migration/Cargo.toml` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/migration/src/lib.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/proto/echo.proto` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/public/index.html` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/src/app.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/src/controllers/mod.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/src/events/mod.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/src/grpc.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/src/jobs/example_heartbeat.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/src/jobs/mod.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/src/mailers/mod.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/src/main.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/src/middleware/mod.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/src/models/_entities/.gitkeep` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/src/models/mod.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/src/policies/mod.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/src/routes.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/src/schedule.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/src/seed.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/src/services/mod.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/tests/auth_flows.rs` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/views/index.html` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/views/layouts/app.html` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/views/pages/about.html` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/views/partials/footer.html` | 重写应用职责目录；保留综合示例能力 |
| `apps/mod.rs` | 重写应用职责目录；保留综合示例能力 |

## 已有证据与缺口

原验证的唯一入口是 [testing](../memory/testing.md)、[verification](../memory/verification.md) 与 [core](../README.md)，本轮没有运行这些测试。真实认证双库/OIDC/MFA、队列恢复、Redis、SMTP、S3、实时/gRPC 与公开 consumer 证据随文件保留。R14 仍需完整双库复制、集群、最低版本、性能和资源证据；Twilio 缺授权凭据/目标；SQLite 未完成。任何环境变量缺失造成 skip 不计为真实验收。
