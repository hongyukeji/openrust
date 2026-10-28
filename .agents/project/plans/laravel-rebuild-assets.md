# 重构资产与回归入口

> 基线：`c63d7e8`，2026-10-28；仅盘点，原验证不等于新 contract 验收。

框架实现共 110 个源码文件、tests 下 75 个文件；下列文件迁移时必须保留行为覆盖。当前目标按 ADR-073 指向根通用框架结构与普通 app/example 模块；示例专属迁移/资源/协议/测试已归模块，新布局完整门禁仍待执行；基线原路径仅为历史来源，不表示目录仍存在。复用/适配后的能力均须重新验证。

## 源码与测试资产

| 基线原路径 | 当前目标或责任归属 | 选择与归属 |
| --- | --- | --- |
| `src/audit/README.md` | `src/audit/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/audit/testing.rs` | `src/audit/testing.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/audit.rs` | `src/audit.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/README.md` | `src/auth/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/database.rs` | `src/auth/database.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/flows.rs` | `src/auth/flows.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/jwt.rs` | `src/auth/jwt.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/mfa.rs` | `src/auth/mfa.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/migration.rs` | `src/auth/migration.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/mod.rs` | `src/auth/mod.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/oidc.rs` | `src/auth/oidc.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/password.rs` | `src/auth/password.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/pat.rs` | `src/auth/pat.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/policy.rs` | `src/auth/policy.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/query.rs` | `src/auth/query.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/refresh.rs` | `src/auth/refresh.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/session.rs` | `src/auth/session.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/testdata/oidc-es256-private.pem` | `src/auth/testdata/oidc-es256-private.pem` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/auth/verification.rs` | `src/auth/verification.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/cache/README.md` | `src/cache/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/cache/tags.rs` | `src/cache/tags.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/cache.rs` | `src/cache.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/api.rs` | `src/config/api.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/app.rs` | `src/config/app.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/auth.rs` | `src/config/auth.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/cache.rs` | `src/config/cache.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/database.rs` | `src/config/database.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/grpc.rs` | `src/config/grpc.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/logger.rs` | `src/config/logger.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/mailer.rs` | `src/config/mailer.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/mod.rs` | `src/config/mod.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/queue.rs` | `src/config/queue.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/realtime.rs` | `src/config/realtime.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/security.rs` | `src/config/security.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/server.rs` | `src/config/server.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/signing.rs` | `src/config/signing.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/sms.rs` | `src/config/sms.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/storage.rs` | `src/config/storage.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/config/view.rs` | `src/config/view.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/README.md` | `src/core/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/app.rs` | `src/core/app.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/command.rs` | `src/core/command.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/error.rs` | `src/core/error.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/execution.rs` | `src/core/execution.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/lifecycle.rs` | `src/core/lifecycle.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/mod.rs` | `src/core/mod.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/otel.rs` | `src/core/otel.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/plugin.rs` | `src/core/plugin.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/rate_limit.rs` | `src/core/rate_limit.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/core/readiness.rs` | `src/core/readiness.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/data/exchange/README.md` | `src/data/exchange/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/data/exchange/tasks.rs` | `src/data/exchange/tasks.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/data/exchange.rs` | `src/data/exchange.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/data.rs` | `src/data.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/README.md` | `src/db/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/batch.rs` | `src/db/batch.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/coordination.rs` | `src/db/coordination.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/dialect.rs` | `src/db/dialect.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/migration.rs` | `src/db/migration.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/migration_lock.rs` | `src/db/migration_lock.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/mod.rs` | `src/db/mod.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/rate_limit.rs` | `src/db/rate_limit.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/seed.rs` | `src/db/seed.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/db/table_names.rs` | `src/db/table_names.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/events.rs` | `src/events.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/grpc.rs` | `src/grpc.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/README.md` | `src/http/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/client.rs` | `src/http/client.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/client_ip.rs` | `src/http/client_ip.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/extract.rs` | `src/http/extract.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/health.rs` | `src/http/health.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/metrics.rs` | `src/http/metrics.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/middleware.rs` | `src/http/middleware.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/mod.rs` | `src/http/mod.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/plugin.rs` | `src/http/plugin.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/rate_limit.rs` | `src/http/rate_limit.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/resource.rs` | `src/http/resource.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/response.rs` | `src/http/response.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/routes.rs` | `src/http/routes.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/serve.rs` | `src/http/serve.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/site.rs` | `src/http/site.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/http/sort.rs` | `src/http/sort.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/i18n.rs` | `src/i18n.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/lib.rs` | `src/lib.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/mail/README.md` | `src/mail/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/mail/mod.rs` | `src/mail/mod.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/mail/template.rs` | `src/mail/template.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/main.rs` | `src/main.rs` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `src/notifications.rs` | `src/notifications.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/queue/redis.rs` | `src/queue/redis.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/queue.rs` | `src/queue.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/realtime.rs` | `src/realtime.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/routes.rs` | `src/routes.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/runtime.rs` | `src/runtime.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/schedule.rs` | `src/schedule.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/security/README.md` | `src/security/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/security.rs` | `src/security.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/signing/README.md` | `src/signing/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/signing/migration.rs` | `src/signing/migration.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/signing.rs` | `src/signing.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/sms.rs` | `src/sms.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/storage/README.md` | `src/storage/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/storage.rs` | `src/storage.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/tenant/README.md` | `src/tenant/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/tenant.rs` | `src/tenant.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/testing/database.rs` | `src/testing/database.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/testing.rs` | `src/testing.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/validation.rs` | `src/validation.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/view/README.md` | `src/view/README.md` | 适配；R1 移框架，后继语义按章节矩阵 |
| `src/view.rs` | `src/view.rs` | 适配；R1 移框架，后继语义按章节矩阵 |
| `tests/consumers/README.md` | `tests/consumers/README.md` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/cache-custom-driver/Cargo.toml` | `tests/consumers/cache-custom-driver/Cargo.toml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/cache-custom-driver/src/main.rs` | `tests/consumers/cache-custom-driver/src/main.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/communication-drivers/Cargo.toml` | `tests/consumers/communication-drivers/Cargo.toml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/communication-drivers/README.md` | `tests/consumers/communication-drivers/README.md` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/communication-drivers/src/main.rs` | `tests/consumers/communication-drivers/src/main.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/data-exchange/Cargo.toml` | `tests/consumers/data-exchange/Cargo.toml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/data-exchange/src/main.rs` | `tests/consumers/data-exchange/src/main.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/db-custom-sqlite/Cargo.toml` | `tests/consumers/db-custom-sqlite/Cargo.toml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/db-custom-sqlite/README.md` | `tests/consumers/db-custom-sqlite/README.md` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/db-custom-sqlite/src/main.rs` | `tests/consumers/db-custom-sqlite/src/main.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/Cargo.toml` | `tests/consumers/grpc/Cargo.toml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/Dockerfile.g3c` | `tests/consumers/grpc/Dockerfile.g3c` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/README.md` | `tests/consumers/grpc/README.md` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/build.rs` | `tests/consumers/grpc/build.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/compose.g3c.yaml` | `tests/consumers/grpc/compose.g3c.yaml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/envoy.g3c.yaml` | `tests/consumers/grpc/envoy.g3c.yaml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/proto/echo.proto` | `tests/consumers/grpc/proto/echo.proto` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/proto/legacy/echo.proto` | `tests/consumers/grpc/proto/legacy/echo.proto` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/scripts/test-g3c.sh` | `tests/consumers/grpc/scripts/test-g3c.sh` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/src/bin/g3c-node.rs` | `tests/consumers/grpc/src/bin/g3c-node.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/src/bin/g3c-probe.rs` | `tests/consumers/grpc/src/bin/g3c-probe.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/grpc/src/main.rs` | `tests/consumers/grpc/src/main.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/query-authorization/Cargo.toml` | `tests/consumers/query-authorization/Cargo.toml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/query-authorization/README.md` | `tests/consumers/query-authorization/README.md` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/query-authorization/src/lib.rs` | `tests/consumers/query-authorization/src/lib.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/queue-custom-driver/Cargo.toml` | `tests/consumers/queue-custom-driver/Cargo.toml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/queue-custom-driver/README.md` | `tests/consumers/queue-custom-driver/README.md` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/queue-custom-driver/src/main.rs` | `tests/consumers/queue-custom-driver/src/main.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/realtime/Cargo.toml` | `tests/consumers/realtime/Cargo.toml` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/realtime/README.md` | `tests/consumers/realtime/README.md` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/realtime/src/main.rs` | `tests/consumers/realtime/src/main.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/consumers/realtime/tests/cluster.rs` | `tests/consumers/realtime/tests/cluster.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/core/plugin_contract.rs` | `tests/core/plugin_contract.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/http/resource.rs` | `tests/http/resource.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/http/router.rs` | `tests/http/router.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/http/view.rs` | `tests/http/view.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/queue/driver_contract.rs` | `tests/queue/driver_contract.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/queue/redis_driver.rs` | `tests/queue/redis_driver.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/auth/database.rs` | `tests/server/auth/database.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/auth/mfa.rs` | `tests/server/auth/mfa.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/auth/oidc.rs` | `tests/server/auth/oidc.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/auth/oidc_keycloak.rs` | `tests/server/auth/oidc_keycloak.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/auth/query_authorization.rs` | `tests/server/auth/query_authorization.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/auth/routes.rs` | `tests/server/auth/routes.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/auth/runtime.rs` | `tests/server/auth/runtime.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/data_exchange.rs` | `tests/server/data_exchange.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/data_exchange_tasks.rs` | `tests/server/data_exchange_tasks.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/database/connection.rs` | `tests/server/database/connection.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/database/coordination.rs` | `tests/server/database/coordination.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/database/migration.rs` | `tests/server/database/migration.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/fixtures/keycloak/openrust-realm.json` | `tests/server/fixtures/keycloak/openrust-realm.json` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/grpc.rs` | `tests/server/grpc.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/http/client.rs` | `tests/server/http/client.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/i18n/runtime.rs` | `tests/server/i18n/runtime.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/mail/templates.rs` | `tests/server/mail/templates.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/queue/crash_recovery.rs` | `tests/server/queue/crash_recovery.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/queue/notifications.rs` | `tests/server/queue/notifications.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/queue/redis.rs` | `tests/server/queue/redis.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/queue/worker.rs` | `tests/server/queue/worker.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/realtime.rs` | `tests/server/realtime.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/schedule/calendar.rs` | `tests/server/schedule/calendar.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/schedule/coordination.rs` | `tests/server/schedule/coordination.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/server/aggregate.rs` | `tests/server/server/aggregate.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/storage/driver_contract.rs` | `tests/server/storage/driver_contract.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/storage/http.rs` | `tests/server/storage/http.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tests/server/storage/s3.rs` | `tests/server/storage/s3.rs` | 保留回归/consumer，按框架与应用职责迁移 |
| `tools/README.md` | `tools/README.md` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/Cargo.toml` | `tools/cli/Cargo.toml` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/README.md` | `tools/cli/README.md` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/src/main.rs` | `tools/cli/src/main.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/.gitignore` | `tools/cli/templates/.gitignore` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/AGENTS.md` | `tools/cli/templates/AGENTS.md` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/Cargo.toml` | `tools/cli/templates/Cargo.toml` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/config/framework.toml` | `tools/cli/templates/config/framework.toml` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/public/index.html` | `tools/cli/templates/public/index.html` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/src/app.rs` | `tools/cli/templates/src/app.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/src/auth_routes.rs` | `tools/cli/templates/src/auth_routes.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/src/main.rs` | `tools/cli/templates/src/main.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/src/routes.rs` | `tools/cli/templates/src/routes.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/tests/auth_flows.rs` | `tools/cli/templates/tests/auth_flows.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/cli/templates/views/index.html` | `tools/cli/templates/views/index.html` | 保留独立编译边界；R1 改路径/模板 |
| `tools/macros/Cargo.toml` | `tools/macros/Cargo.toml` | 保留独立编译边界；R1 改路径/模板 |
| `tools/macros/README.md` | `tools/macros/README.md` | 保留独立编译边界；R1 改路径/模板 |
| `tools/macros/src/lib.rs` | `tools/macros/src/lib.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/performance/Cargo.toml` | `tools/performance/Cargo.toml` | 保留独立编译边界；R1 改路径/模板 |
| `tools/performance/README.md` | `tools/performance/README.md` | 保留独立编译边界；R1 改路径/模板 |
| `tools/performance/benches/db_routing.rs` | `tools/performance/benches/db_routing.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/performance/scripts/http_load.py` | `tools/performance/scripts/http_load.py` | 保留独立编译边界；R1 改路径/模板 |
| `tools/testing/Cargo.toml` | `tools/testing/Cargo.toml` | 保留独立编译边界；R1 改路径/模板 |
| `tools/testing/README.md` | `tools/testing/README.md` | 保留独立编译边界；R1 改路径/模板 |
| `tools/testing/src/lib.rs` | `tools/testing/src/lib.rs` | 保留独立编译边界；R1 改路径/模板 |
| `tools/testing/tests/database.rs` | `tools/testing/tests/database.rs` | 保留独立编译边界；R1 改路径/模板 |
| `apps/README.md` | `app/README.md` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/.env.example` | `.env.example` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/.gitignore` | `.gitignore` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/AGENTS.md` | `AGENTS.md` | 移除独立子应用指南，遵循根协作契约与模块 README |
| `apps/example/Cargo.toml` | `Cargo.toml` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/README.md` | `app/example/README.md` | 重写应用职责目录；保留综合示例能力 |
| `apps/example/build.rs` | `app/example/proto/generate.py` | 模块专属实现/资源，通过公开入口挂载；本布局完整门禁另行验证 |
| `apps/example/config/api.toml` | `config/api.toml` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/config/app.toml` | `config/app.toml` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/config/auth.toml` | `config/auth.toml` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/config/cache.toml` | `config/cache.toml` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/config/database.toml` | `config/database.toml` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/config/grpc.toml` | `config/grpc.toml` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/config/i18n.toml` | `config/i18n.toml` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/config/mail.toml` | `config/mail.toml` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/config/security.toml` | `config/security.toml` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/config/server.toml` | `config/server.toml` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/config/session.toml` | `config/session.toml` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/config/sms.toml` | `config/sms.toml` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/config/view.toml` | `config/view.toml` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/migration/Cargo.toml` | `app/example/database/migrations/mod.rs` | 模块专属实现/资源，通过公开入口挂载；本布局完整门禁另行验证 |
| `apps/example/migration/src/lib.rs` | `app/example/database/migrations/mod.rs` | 模块专属实现/资源，通过公开入口挂载；本布局完整门禁另行验证 |
| `apps/example/proto/echo.proto` | `app/example/proto/echo.proto` | 模块专属实现/资源，通过公开入口挂载；本布局完整门禁另行验证 |
| `apps/example/public/index.html` | `public/index.html` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/src/app.rs` | `app/example/providers/example.rs` | 模块专属实现/资源，通过公开入口挂载；本布局完整门禁另行验证 |
| `apps/example/src/controllers/mod.rs` | — | 原空占位文件不保留；业务与公开扩展按唯一 app/example 实际代码覆盖，不将占位计为能力 |
| `apps/example/src/events/mod.rs` | — | 原空占位文件不保留；业务与公开扩展按唯一 app/example 实际代码覆盖，不将占位计为能力 |
| `apps/example/src/grpc.rs` | `app/example/grpc.rs` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/src/jobs/example_heartbeat.rs` | `app/example/jobs/example_heartbeat.rs` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/src/jobs/mod.rs` | `app/example/jobs/mod.rs` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/src/mailers/mod.rs` | — | 原空占位文件不保留；业务与公开扩展按唯一 app/example 实际代码覆盖，不将占位计为能力 |
| `apps/example/src/main.rs` | `src/main.rs` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/src/middleware/mod.rs` | — | 原空占位文件不保留；业务与公开扩展按唯一 app/example 实际代码覆盖，不将占位计为能力 |
| `apps/example/src/models/_entities/.gitkeep` | — | 原空占位文件不保留；业务与公开扩展按唯一 app/example 实际代码覆盖，不将占位计为能力 |
| `apps/example/src/models/mod.rs` | — | 原空占位文件不保留；业务与公开扩展按唯一 app/example 实际代码覆盖，不将占位计为能力 |
| `apps/example/src/policies/mod.rs` | — | 原空占位文件不保留；业务与公开扩展按唯一 app/example 实际代码覆盖，不将占位计为能力 |
| `apps/example/src/routes.rs` | `app/example/routes/mod.rs` | 模块专属实现/资源，通过公开入口挂载；本布局完整门禁另行验证 |
| `apps/example/src/schedule.rs` | `app/example/jobs/mod.rs` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |
| `apps/example/src/seed.rs` | `app/example/database/seeders/mod.rs` | 模块专属实现/资源，通过公开入口挂载；本布局完整门禁另行验证 |
| `apps/example/src/services/mod.rs` | — | 原空占位文件不保留；业务与公开扩展按唯一 app/example 实际代码覆盖，不将占位计为能力 |
| `apps/example/tests/auth_flows.rs` | `app/example/tests/auth_flows.rs` | 模块专属实现/资源，通过公开入口挂载；本布局完整门禁另行验证 |
| `apps/example/views/index.html` | `app/example/resources/views/index.html` | 模块专属实现/资源，通过公开入口挂载；本布局完整门禁另行验证 |
| `apps/example/views/layouts/app.html` | `app/example/resources/views/layouts/app.html` | 模块专属实现/资源，通过公开入口挂载；本布局完整门禁另行验证 |
| `apps/example/views/pages/about.html` | `app/example/resources/views/pages/about.html` | 模块专属实现/资源，通过公开入口挂载；本布局完整门禁另行验证 |
| `apps/example/views/partials/footer.html` | `app/example/resources/views/partials/footer.html` | 模块专属实现/资源，通过公开入口挂载；本布局完整门禁另行验证 |
| `apps/mod.rs` | `app/mod.rs` | 迁入根应用职责或普通业务模块；保留并重新验证有效行为 |

## 已有证据与缺口

原验证的唯一入口是 [testing](../memory/testing.md)、[verification](../memory/verification.md) 与 [core](../README.md)；R0 盘点时没有运行这些测试，后续迁移实际执行结果见 testing 当前边界。真实认证双库/OIDC/MFA、队列恢复、Redis、SMTP、S3、实时/gRPC 与公开 consumer 证据随文件保留。R14 仍需完整双库复制、集群、最低版本、性能和资源证据；Twilio 缺授权凭据/目标；SQLite 未完成。任何环境变量缺失造成 skip 不计为真实验收。

## 回归测试函数索引

以下是现有测试的真实函数名，供逐行为 Spec 选择/补齐回归；不表示每个测试已验证新目标。真实服务条件仍遵循对应 core/testing 文档。

| 当前测试文件 | 已有测试函数 |
| --- | --- |
| `src/audit.rs` | `policy_filters_redacts_and_hides_debug_values`, `audit_records_capture_and_redact_trusted_tenant_context`, `sink_errors_propagate_without_fallback`, `details_are_allowlisted_and_bounded`, `plugin_registers_the_database_sink_service`, `plugin_registers_a_custom_sink`, `public_fake_captures_only_prepared_records_and_injects_failures` |
| `src/auth/flows.rs` | `session_login_rotates_id_preserves_app_data_and_redacts_csrf`
| `src/auth/jwt.rs` | `access_token_signs_and_verifies_with_redacted_secret_debug`, `key_rotation_accepts_old_tokens_and_signs_with_the_active_key`, `rejects_invalid_key_length_and_active_key_configuration`, `rejects_wrong_token_type_algorithm_time_audience_and_subject` |
| `src/auth/mfa.rs` | `totp_matches_rfc_6238_sha1_known_vectors`, `code_comparison_checks_length_and_value`, `matching_step_accepts_only_the_adjacent_window_and_six_ascii_digits` |
| `src/auth/mod.rs` | `route_mode_ignores_unselected_credentials_and_selects_one_source`, `optional_missing_credentials_are_distinct_from_malformed_credentials`, `combined_mode_rejects_any_two_sources_and_repeated_credentials`, `unsafe_session_requests_require_one_csrf_header_before_authentication` |
| `src/auth/oidc.rs` | `id_token_requires_signature_issuer_audience_nonce_and_access_token_hash`, `authorization_constructor_rejects_malformed_or_unsafe_urls`, `token_debug_output_redacts_id_token`, `external_identity_session_id_is_validated_and_redacted`, `external_login_handle_can_be_restored_from_secure_storage`, `backchannel_logout_token_requires_signature_audience_event_and_no_nonce` |
| `src/auth/password.rs` | `hashes_use_distinct_random_salts_and_verify_passwords`, `rejects_stored_hashes_with_unsafe_resource_parameters`, `rejects_out_of_range_parameters_and_malformed_hashes`, `rejects_empty_and_oversized_passwords`, `rejects_hash_operations_when_the_service_capacity_is_full`, `default_capacity_allows_two_operations_and_rejects_the_third`, `rejects_invalid_operation_capacity` |
| `src/auth/query.rs` | `nested_or_user_filters_remain_inside_authorized_and_scope`, `empty_and_failed_rules_fail_closed`, `pagination_rejects_unbounded_and_overflowing_requests` |
| `src/auth/verification.rs` | `binds_codes_to_purpose_and_identity_and_consumes_once`, `invalid_codes_are_bounded_and_issue_redacts_the_code`, `rejects_unbounded_code_policies` |
| `src/cache.rs` | `memory_store_roundtrips_namespaces_ttl_and_removal`, `file_store_replaces_values_expires_and_uses_hashed_names`, `file_store_concurrent_replacements_leave_a_complete_value`, `memory_tags_isolate_namespaces_and_reject_stale_generation_writes`, `shared_store_keys_and_tag_generations_are_tenant_scoped`, `singleflight_merges_loads_and_scopes_contract_keys`, `cancelled_singleflight_leader_releases_waiter_to_retry`, `singleflight_enforces_key_capacity_and_loader_timeout`, `singleflight_rejects_excess_waiters`, `singleflight_broadcasts_loader_failure_without_retrying_waiters`, `swr_returns_stale_during_refresh_and_shares_the_refreshed_value`, `swr_does_not_serve_stale_after_tags_are_invalidated`, `swr_refresh_failure_keeps_stale_only_until_the_maximum_window`, `swr_shutdown_cancels_and_joins_a_running_refresh`, `swr_caps_concurrent_refreshes_across_keys`, `plugin_requires_config_and_registers_named_memory_stores`, `external_driver_is_collected_before_cache_initialization`, `redis_connection_failure_does_not_fall_back_to_memory`, `redis_store_roundtrips_ttl_on_live_service` |
| `src/config/grpc.rs` | `validates_client_names_and_default_reference`, `rejects_inconsistent_tls_and_redacts_material_paths`, `rejects_endpoint_credentials_and_redacts_endpoint_path`, `rejects_unbounded_or_inconsistent_runtime_limits` |
| `src/config/mod.rs` | `domain_modules_preserve_capability_config_paths`, `domain_files_override_defaults_and_environment_files_are_ignored`, `config_dir_environment_override_loads_selected_directory`, `config_dir_defaults_to_working_directory_config`, `wrong_type_reports_key_name`, `unknown_environment_is_rejected`, `secrets_are_redacted_recursively`, `configuration_source_errors_do_not_echo_invalid_string_values`, `server_middleware_defaults_are_complete`, `api_http_status_defaults_overrides_and_rejects_invalid_value`, `partial_cors_toml_keeps_field_defaults`, `grpc_toml_loads_server_and_named_client_configuration`, `cors_list_environment_override_uses_figment_array_syntax`, `application_namespace_values_map_to_sections_and_override_defaults`, `laravel_style_database_and_redis_variables_build_connection_urls`, `environment_selection_uses_app_env_by_default`, `custom_namespace_selects_config_dir_and_values_without_mixing_defaults`, `custom_namespace_bootstraps_environment_from_dotenv`, `invalid_namespace_is_rejected_before_loading`, `invalid_custom_environment_reports_selected_key` |
| `src/config/realtime.rs` | `config_bounds_and_redacts_redis_credentials`
| `src/core/app.rs` | `services_are_typed_and_shared`, `get_returns_none_for_missing_service`, `try_get_reports_missing_type_name`, `extensions_accumulate_across_registrations`, `unknown_command_is_reported`, `command_exit_cancels_and_joins_application_background_tasks`, `serve_is_unknown_without_http_plugin`, `nested_command_consumes_its_name_words`, `unknown_nested_command_names_input_and_available_command`, `config_show_rejects_extra_argument`, `sensitive_section_error_does_not_echo_invalid_secret_value` |
| `src/core/error.rs` | `expected_http_overload_rejections_do_not_emit_error_logs`, `client_error_has_code_and_message`, `internal_error_hides_source`, `validation_errors_are_listed_per_field`, `custom_status_defaults_to_bad_request`, `page_meta_is_preserved_in_response` |
| `src/core/execution.rs` | `concurrent_scopes_keep_request_metadata_isolated`, `child_budget_never_exceeds_parent_and_expiry_cancels`, `cancellation_is_observable_to_cooperative_child`, `run_installs_its_context_for_the_future`, `metadata_rejects_unbounded_or_unsafe_request_ids`, `metadata_accepts_only_w3c_version_00_traceparents` |
| `src/core/lifecycle.rs` | `production_bootstrap_loads_dotenv_without_overriding_process_environment`, `dotenv_selected_production_loads_other_keys`, `malformed_dotenv_fails_without_exposing_its_contents`, `development_bootstrap_loads_dotenv_then_resolves_environment`, `finalize_without_http_exposes_only_builtin_commands`, `plugins_added_during_build_are_processed_and_finished`, `app_background_tasks_are_cancelled_joined_and_closed`, `app_background_tasks_abort_after_shutdown_deadline` |
| `src/core/otel.rs` | `extractor_reads_only_the_validated_traceparent_field`
| `src/core/rate_limit.rs` | `permits_the_limit_then_returns_retry_after`, `rejects_invalid_policy_and_fails_closed_on_store_error` |
| `src/core/readiness.rs` | `dependency_probe_failure_recovers_without_exposing_error`, `dependency_probes_run_with_bounded_concurrency`, `concurrent_readiness_calls_do_not_start_parallel_probe_rounds`, `dependency_probe_names_are_validated_and_unique`, `readiness_waits_for_each_expected_listener_and_clears_when_one_stops` |
| `src/data/exchange.rs` | `csv_stream_converts_rows_and_collects_bounded_field_errors`, `csv_rejects_duplicate_headers_and_byte_limit_overflow`, `csv_stops_when_the_row_callback_fails`, `csv_error_reports_respect_row_and_global_limits`, `csv_export_escapes_formula_prefixes_and_enforces_output_limit`, `xlsx_stream_roundtrips_text_and_rejects_formula_cells`, `xlsx_shared_string_declared_count_is_bounded`, `xlsx_preflight_rejects_macro_entries_and_expansion_overflow` |
| `src/db/batch.rs` | `batch_options_enforce_the_documented_row_bound`
| `src/db/dialect.rs` | `schema_helpers_emit_the_required_backend_types`, `schema_helpers_reject_unimplemented_backends` |
| `src/db/migration.rs` | `migration_actions_default_to_all_up_or_one_down`, `migration_actions_accept_positive_step_counts_and_status`, `migration_action_selects_an_explicit_database`, `invalid_migration_arguments_are_rejected` |
| `src/db/mod.rs` | `defaults_define_a_small_pool_without_a_connection_url`, `debug_redacts_connection_url`, `debug_does_not_render_replica_urls`, `named_instance_config_debug_redacts_primary_and_replica_urls`, `named_instances_reject_invalid_names_and_missing_urls`, `database_config_deserializes_complete_named_instances`, `database_selector_keeps_default_handle_and_rejects_unknown_names`, `replica_selection_round_robins_without_skipping_configured_entries`, `connection_is_unavailable_before_plugin_initialization`, `invalid_pool_settings_are_rejected`, `database_url_requires_its_backend_feature`, `replica_backend_requires_its_feature_and_matches_primary`, `official_backend_detection_rejects_unknown_url_schemes`, `provisional_minimum_database_versions_are_checked_explicitly` |
| `src/db/table_names.rs` | `resolves_defaults_empty_prefix_and_full_mapping`, `rejects_unsafe_names_and_backend_length_overflow`, `quotes_identifiers_for_each_supported_backend`, `validates_and_quotes_sqlite_names_for_custom_plugins`, `migration_scope_is_explicit_and_task_local` |
| `src/events.rs` | `listeners_run_in_order_and_stop_after_the_first_error`, `unrelated_event_types_are_not_delivered`, `queued_listener_maps_event_to_registered_job_payload`, `queued_listener_mapping_error_stops_later_listeners`, `queued_dispatch_error_stops_later_listeners` |
| `src/grpc.rs` | `health_drain_grace_must_fit_inside_shutdown_deadline`, `parses_and_caps_grpc_timeout_and_rejects_ambiguous_values`, `client_deadline_propagation_uses_the_current_execution_budget`, `client_trace_propagation_is_explicit_and_redacted`, `configured_clients_share_lazy_channels_and_validate_endpoint_scheme`, `grpc_status_mapping_uses_stable_codes_and_redacts_messages` |
| `src/http/client.rs` | `local_http_json_and_body_limit`, `invalid_timeout_configuration_is_rejected`, `breaker_does_not_count_local_policy_rejections_as_dependency_failures`, `retry_after_values_are_parsed_and_capped`, `retry_jitter_stays_within_the_exponential_ceiling`, `configuration_debug_redacts_proxy_credentials`, `total_timeout_is_enforced`, `injected_transport_respects_the_configured_timeout`, `total_timeout_covers_injected_response_body_reads`, `fake_transport_never_opens_a_network_connection`, `fake_body_without_content_length_is_still_bounded`, `fake_transport_accepts_form_and_multipart_requests`, `explicit_retry_is_bounded_for_idempotent_requests`, `explicit_retry_retries_transport_errors`, `explicit_retry_stops_at_the_attempt_limit`, `one_shot_stream_body_is_not_replayed`, `dropping_request_future_cancels_the_transport_future`, `retry_policy_rejects_unbounded_or_inconsistent_values`, `restricted_target_policy_rejects_unsafe_addresses_and_userinfo`, `restricted_resolver_rejects_mixed_results_and_allows_explicit_internal_network`, `restricted_client_connects_to_the_single_checked_dns_result` |
| `src/http/client_ip.rs` | `untrusted_peers_cannot_supply_a_forwarded_address`, `trusted_proxy_chain_is_walked_from_the_peer_toward_the_client`, `spoofed_prefix_before_the_client_is_not_selected`, `cidr_boundaries_support_ipv4_ipv6_and_reject_invalid_prefixes`, `malformed_forwarding_header_is_rejected_only_for_trusted_peers` |
| `src/http/health.rs` | `ready_returns_503_without_leaking_dependency_errors`
| `src/http/metrics.rs` | `metric_labels_are_restricted_to_fixed_values`, `scrape_route_is_opt_in_and_does_not_export_request_secrets` |
| `src/http/middleware.rs` | `request_span_does_not_capture_identifiers_or_full_uri`, `concurrent_request_limit_rejects_without_waiting_and_recovers`, `concurrent_request_limit_covers_unconsumed_response_bodies`, `request_context_accepts_only_valid_traceparent` |
| `src/http/serve.rs` | `readiness_changes_after_bind_and_before_graceful_drain`, `non_cooperative_request_is_aborted_at_shutdown_deadline`, `bind_failure_never_marks_server_ready` |
| `src/http/sort.rs` | `parses_only_unique_allowlisted_fields_in_order`, `rejects_unknown_duplicate_malformed_and_unbounded_fields` |
| `src/i18n.rs` | `canonicalizes_language_and_region`, `chooses_highest_quality_supported_language_and_skips_invalid_values`, `merges_application_json_and_falls_back_by_language_then_default`, `request_locale_isolated_between_concurrent_scopes`, `rejects_malformed_json_resources`, `formats_plural_and_select_messages_from_validated_json`, `rejects_invalid_structured_message_resources_atomically`, `formats_decimal_and_dates_with_explicit_locale_and_timezone`, `i18n_config_deserializes_from_the_standard_toml_section` |
| `src/lib.rs` | `prelude_json_is_a_response`
| `src/mail/mod.rs` | `localized_email_renders_subject_and_body_from_explicit_locale`, `production_rejects_log_driver`, `smtp_submits_to_local_mailpit`, `fake_driver_captures_composed_messages`, `mail_config_debug_redacts_password`, `unknown_mail_driver_is_rejected`, `smtp_driver_builds_tls_transport_and_requires_complete_credentials`, `plaintext_smtp_is_limited_to_unauthenticated_loopback` |
| `src/mail/template.rs` | `renders_locale_html_escaped_and_rejects_missing_variables`, `validates_closed_template_names_and_duplicate_registration`, `message_uses_alternative_mime_and_keeps_attachment_composition_available`, `smtp_submits_rendered_template_to_mailpit`, `rendered_template_is_accepted_by_the_public_fake_mailer` |
| `src/queue.rs` | `registry_execute_installs_scope_and_enforces_deadline`, `execution_metadata_round_trips_and_legacy_payloads_stay_raw`, `malformed_tenant_metadata_is_rejected_without_running_as_legacy_payload`, `malformed_tenant_envelope_shape_is_rejected`, `queue_dispatch_rejects_conflicting_execution_and_task_tenants`, `metadata_envelope_rejects_reserved_payload_key`, `malformed_legacy_envelope_shape_remains_an_application_payload`, `cron_registration_rejects_invalid_schedule_configuration` |
| `src/realtime.rs` | `user_channel_delivery_and_revocation_close_online_session`, `slow_consumer_is_closed_at_message_queue_bound`, `connection_expiry_origin_and_payload_limits_fail_closed` |
| `src/routes.rs` | `invalid_shutdown_timeout_fails_during_plugin_build`
| `src/runtime.rs` | `early_clean_listener_exit_is_reported_as_failure`, `failed_listener_stops_and_joins_its_sibling` |
| `src/schedule.rs` | `interval_slots_are_stable_and_daily_slots_use_utc_day`, `frequency_bounds_reject_fractional_and_out_of_range_values`, `cron_skips_nonexistent_spring_minutes_and_deduplicates_repeated_fall_minutes`, `cron_calendar_handles_leap_days_and_bounded_catch_up_configuration` |
| `src/security.rs` | `encrypts_binary_and_empty_plaintext_and_generates_fresh_nonces`, `context_is_authenticated_and_must_come_from_the_caller`, `key_rotation_writes_only_with_active_key_and_reads_old_ciphertexts`, `malformed_or_tampered_envelopes_fail_without_echoing_input`, `rejects_bad_key_rings_and_redacts_configuration_debug`, `rejects_oversized_inputs_before_encryption`, `known_aes_256_gcm_siv_vector_matches_rfc_8452`, `independent_encryptors_with_the_same_ring_can_decrypt_each_other` |
| `src/signing.rs` | `link_binds_purpose_subject_and_expiry`, `shared_store_revokes_valid_link`, `revocation_requires_shared_store`, `retained_key_verifies_old_links_and_new_key_signs_new_ones` |
| `src/sms.rs` | `production_rejects_log_driver`, `recipient_and_content_are_validated`, `debug_does_not_disclose_message_data`, `twilio_config_requires_credentials_and_exactly_one_sender`, `fake_captures_messages_without_provider`, `twilio_posts_form_with_basic_auth_and_returns_acceptance_id`, `provider_rejection_is_generic_and_does_not_leak_message_or_credentials`, `provider_timeout_is_bounded_and_never_retries_sms`, `twilio_configuration_debug_redacts_secrets_and_addresses` |
| `src/storage.rs` | `concurrent_filesystem_driver_builds_reserve_distinct_staging_directories`, `storage_path_rejects_absolute_and_traversal_keys`, `local_disk_streams_objects_and_aborts_cancelled_uploads` |
| `src/tenant.rs` | `validates_tenant_identifiers_without_disclosing_them`, `scopes_are_isolated_and_cannot_switch_nested_tenant`, `resolver_failure_rejects_without_running_the_handler` |
| `src/testing/database.rs` | `database_url_replacement_preserves_auth_and_query_options`, `generated_database_names_fit_postgresql_identifier_limits` |
| `src/validation.rs` | `validates_application_dtos_and_preserves_nested_field_paths`, `returns_valid_values_without_copying` |
| `tests/consumers/communication-drivers/src/main.rs` | `application_plugin_registers_custom_drivers_through_public_services`
| `tests/consumers/data-exchange/src/main.rs` | `public_data_exchange_flow_imports_and_exports`
| `tests/consumers/grpc/src/main.rs` | `tls_verifies_ca_and_server_name_over_real_tcp`, `mtls_requires_a_trusted_client_certificate`, `expired_server_certificate_is_rejected`, `jwt_bearer_authentication_runs_over_real_grpc_tcp`, `old_and_new_proto_clients_interoperate_over_real_tcp`, `grpc_deadline_covers_stream_lifetime_and_global_limit_holds_until_end`, `reflection_is_opt_in_and_lists_only_registered_descriptor_services`, `health_watch_reports_not_serving_before_graceful_drain`, `reflection_requires_a_descriptor_set_when_enabled`, `configured_message_size_limit_rejects_oversized_requests_over_tcp`, `shutdown_timeout_cancels_and_joins_a_non_terminating_stream`, `combined_runtime_serves_both_protocols_and_drains_together`, `combined_startup_binding_failure_releases_the_first_listener`, `combined_shutdown_timeout_aborts_and_joins_both_listeners`, `combined_plugin_rejects_missing_http_or_grpc_registration`, `database_bearer_and_method_policy_work_over_real_tcp` |
| `tests/consumers/realtime/tests/cluster.rs` | `two_real_api_processes_route_websocket_and_sse_through_redis`
| `tests/core/plugin_contract.rs` | `independent_plugin_registers_config_services_command_and_route`, `missing_plugin_dependency_fails_during_build`, `duplicate_service_registration_fails_without_replacing_first_value`, `legacy_service_registration_replaces_by_type_deterministically`, `duplicate_command_registration_fails_during_build`, `invalid_plugin_config_reports_key_without_echoing_value`, `duplicate_plugin_routes_return_startup_error_instead_of_panicking`, `legacy_command_registration_replaces_by_name_deterministically` |
| `tests/http/resource.rs` | `all_resource_actions_map_to_the_expected_methods_and_paths`, `trailing_slash_resource_path_uses_one_collection_and_member_base`, `empty_and_slash_resource_paths_work_inside_groups`, `empty_and_slash_nest_prefixes_merge_at_root`, `absent_actions_do_not_create_paths_and_registered_paths_reject_other_methods`, `only_and_except_filter_registered_actions_in_nested_groups`, `filters_apply_even_when_declared_before_handlers`, `repeated_only_calls_intersect_registered_actions`, `created_and_no_content_have_their_status_and_body_contract`, `pagination_defaults_and_json_shape`, `pagination_per_page_only_keeps_default_page`, `pagination_accepts_largest_database_offset`, `pagination_rejects_database_offset_overflow_as_page_error`, `pagination_rejects_out_of_range_and_invalid_fields_as_envelopes`, `total_pages_cover_zero_exact_and_partial_pages`, `created_location_is_set_and_invalid_value_is_safe` |
| `tests/http/router.rs` | `health_and_request_id_are_installed_by_the_plugin`, `app_builder_http_layer_wraps_routes`, `route_rate_limit_uses_scoped_custom_keys_and_returns_retry_after`, `authenticated_rate_limit_rejects_a_missing_identity`, `inject_resolves_registered_services`, `inject_of_unregistered_service_is_a_envelope_not_a_panic`, `validated_json_reports_each_field`, `validated_json_accepts_valid_input_and_rejects_malformed_bodies`, `panics_become_500_envelopes`, `compression_negotiates_gzip_and_br_and_respects_absence`, `public_site_serves_index_and_maps_files_from_configured_directory`, `root_target_supports_internal_forward_and_redirect`, `invalid_root_target_is_rejected_during_router_build`, `static_fallback_does_not_capture_api_or_websocket_namespaces`, `disabled_compression_ignores_accept_encoding`, `cors_is_disabled_by_default`, `cors_allows_listed_origin_and_preflight_with_request_id`, `cors_preflight_rejects_unlisted_origin`, `cors_origin_error_is_wrapped_outside_preflight`, `cors_timeout_retains_allow_origin`, `cors_wildcard_and_credentials_have_expected_headers`, `invalid_cors_config_returns_keyed_errors`, `invalid_cors_origin_reports_escaped_value`, `cors_wildcards_with_credentials_return_errors`, `cors_mixed_wildcards_return_errors`, `invalid_cors_methods_and_headers_return_keyed_errors`, `disabled_cors_ignores_other_invalid_fields`, `json_syntax_is_400_envelope`, `json_content_type_is_415_envelope`, `json_field_type_is_422_envelope`, `oversized_json_is_413_envelope`, `path_and_query_parse_errors_are_400_envelopes`, `unknown_route_is_404_envelope_with_request_id`, `wrong_method_is_405_envelope_with_allow`, `handler_404_keeps_its_detail`, `cors_applies_to_fallback_envelopes`, `missing_json_field_is_422_with_field_error`, `always_ok_wraps_success_errors_and_fallbacks_with_matching_request_id`, `http_execution_context_contains_request_id_budget_and_timeout_cancels_children`, `semantic_keeps_created_error_and_rate_limit_statuses`, `always_ok_maps_extractor_errors_and_timeout_to_envelopes` |
| `tests/http/view.rs` | `root_view_is_html_and_escapes_context`, `view_root_requires_plugin_and_existing_template`, `renderer_loads_named_views`, `development_reload_reads_updated_template`, `template_symlink_cannot_escape_template_directory` |
| `tests/queue/driver_contract.rs` | `public_registry_executes_typed_jobs_with_custom_context_behavior`, `registry_execute_is_an_explicit_awaited_in_process_path`, `progress_without_driver_reporter_fails_explicitly`, `public_context_rejects_invalid_attempt_metadata`, `legacy_custom_driver_rejects_nonempty_execution_metadata` |
| `tests/queue/redis_driver.rs` | `redis_queue_fails_closed_for_bad_namespace_or_unavailable_service`, `redis_queue_shares_atomic_claims_uniqueness_progress_and_retry`, `redis_queue_recovers_a_task_after_its_worker_process_is_killed` |
| `tests/server/auth/database.rs` | `signed_link_revocation_is_shared_by_database_instances`, `auth_migration_and_shared_revocation_work_on_the_configured_database`, `auth_migrations_use_default_empty_prefix_and_full_name_mapping`, `audit_and_permission_change_share_a_transaction_on_the_configured_database` |
| `tests/server/auth/mfa.rs` | `mfa_lifecycle_is_shared_secure_and_reachable_over_real_tcp`
| `tests/server/auth/oidc.rs` | `external_identity_flows_enforce_browser_binding_single_use_and_explicit_linking`
| `tests/server/auth/oidc_keycloak.rs` | `keycloak_two_client_sso_and_backchannel_logout_tokens_are_validated`, `keycloak_api_node_process` |
| `tests/server/auth/query_authorization.rs` | `query_scope_filters_mutations_and_relations_on_real_database`, `tenant_scope_is_trusted_and_shared_between_real_api_nodes` |
| `tests/server/auth/routes.rs` | `facade_application_authentication_coexists_with_dependency_injection`, `facade_application_authenticates_jwt_without_session_plugin` |
| `tests/server/auth/runtime.rs` | `jwt_authentication_uses_the_configured_key_ring_without_session_middleware`, `current_user_and_injected_service_share_the_route_request`, `cookie_authentication_uses_the_shared_session_and_preserves_csrf_checks`, `redis_sessions_are_shared_between_apps_and_revoke_across_instances`, `optional_identity_is_anonymous_only_when_selected_credentials_are_absent`, `default_database_authenticator_requires_database_at_startup`, `auth_plugin_rejects_invalid_password_capacity_during_startup`, `duplicate_auth_plugin_is_rejected` |
| `tests/server/data_exchange.rs` | `server_feature_exposes_bounded_csv_import_api`
| `tests/server/data_exchange_tasks.rs` | `import_batches_are_atomic_idempotent_and_export_is_streamed`
| `tests/server/database/connection.rs` | `plugin_opens_pool_and_executes_a_query`, `runtime_database_readiness_fails_and_recovers_through_a_proxy`, `transaction_commits_success_and_rolls_back_callback_errors`, `transaction_events_rollback_before_commit_and_ignore_after_commit_errors`, `supported_backends_skip_rows_locked_by_another_transaction` |
| `tests/server/database/coordination.rs` | `f4c_swr_uses_shared_locks_across_apps_and_redis_connections`
| `tests/server/database/migration.rs` | `migration_command_applies_repeats_reports_and_rolls_back`, `migration_commands_serialize_timeout_and_recover_after_process_exit`, `child_migration_process_runs_migrations`, `named_database_instances_keep_migrations_and_transactions_isolated` |
| `tests/server/grpc.rs` | `plugin_registers_service_and_serves_a_bound_tcp_listener`, `plugin_rejects_duplicate_services_by_name`, `plugin_rejects_health_drain_grace_that_exhausts_shutdown_deadline`, `server_host_composes_http_and_grpc_under_one_lifecycle`, `server_requires_an_explicit_service_registration`, `plaintext_server_requires_an_explicit_development_or_proxy_opt_in`, `grpc_serve_signal_path_drains_on_sigterm`, `grpc_sigterm_child` |
| `tests/server/http/client.rs` | `aggregated_server_can_use_outbound_http_client`, `restricted_client_allows_declared_local_target_and_does_not_follow_redirects`, `named_dependencies_bound_waiters_without_exhausting_each_other`, `optional_circuit_breaker_opens_and_recovers_after_half_open_probe`, `cancelling_a_queued_dependency_request_releases_its_queue_slot`, `dependency_permit_covers_streaming_response_and_queue_wait_budget` |
| `tests/server/i18n/runtime.rs` | `aggregate_server_selects_plural_forms_per_request_locale`, `aggregate_server_localizes_errors_and_scopes_concurrent_requests`, `aggregate_server_exposes_request_locale_and_explicit_override`, `validator_rule_messages_localize_without_changing_code_or_field_path`, `untranslated_application_error_keeps_its_safe_message`, `app_catalog_can_override_builtin_messages_with_parameters` |
| `tests/server/mail/templates.rs` | `server_application_can_render_and_build_localized_multipart_mail`
| `tests/server/queue/crash_recovery.rs` | `tenant_metadata_survives_a_separate_worker_process_and_retry`, `stale_worker_token_cannot_acknowledge_a_reclaimed_job`, `child_queue_worker_drains_on_sigterm`, `process_crash_releases_job_after_lease_expiry`, `child_worker_holds_crash_probe`, `child_worker_processes_tenant_retry_probe` |
| `tests/server/queue/notifications.rs` | `notification_channels_share_transaction_but_run_as_independent_jobs`, `queued_notification_is_accepted_by_configured_smtp_sink` |
| `tests/server/queue/redis.rs` | `queue_plugin_selects_redis_without_a_database_plugin`, `redis_queue_rejects_database_transactional_schedules_at_build` |
| `tests/server/realtime.rs` | `http_auth_origin_and_sse_headers_are_enforced`
| `tests/server/schedule/calendar.rs` | `cron_plan_revisions_are_separate_and_shared_ticks_dispatch_once`, `child_calendar_scheduler_ticks_due_slot`, `cron_dst_gap_and_fold_slots_are_persisted_correctly`, `cron_catch_up_is_bounded_per_tick_and_continues_backlog` |
| `tests/server/schedule/coordination.rs` | `independent_scheduler_processes_dispatch_one_job_per_slot`, `child_scheduler_dispatches_due_slot`, `child_scheduler_stops_on_sigterm`, `child_scheduler_exits_after_committing_slot` |
| `tests/server/server/aggregate.rs` | `mailer_plugin_is_registered_by_the_assembled_server`, `cache_plugin_uses_the_configured_shared_redis_store`, `health_has_request_id`, `serve_process_exits_successfully_after_sigterm`, `metrics_scrape_is_served_over_tcp_only_when_explicitly_enabled`, `api_root_reports_server_name_and_version`, `root_serves_the_default_public_index`, `resource_macro_registers_controller_actions`, `database_plugin_is_available_through_the_facade`, `config_show_runs_and_redacts_secrets`, `default_config_loads_from_package_directory`, `unknown_route_returns_envelope` |
| `tests/server/storage/driver_contract.rs` | `consumer_registers_custom_named_disks_without_private_framework_access`
| `tests/server/storage/http.rs` | `assembled_server_streams_upload_and_download_over_tcp_with_authorization`
| `tests/server/storage/s3.rs` | `s3_streams_multipart_cancellation_and_exact_version_presigned_reads`
| `tools/testing/tests/database.rs` | `each_test_database_has_its_own_migrated_schema_and_rows`
| `app/example/tests/auth_flows.rs` | `generated_auth_routes_run_over_http_with_isolated_database_and_fake_mail`
