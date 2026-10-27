# OpenRust 综合示例应用

`apps/example/` 是 OpenRust v1 唯一面向开发者的完整示例项目，结构来自仓库 CLI 生成器，再通过真实路由、后台任务和配置展示能力组合。它是源码 workspace 成员，默认连接 PostgreSQL 与 Redis；运行它不会启动外部服务，开发者需要先准备服务。

## 能力路线

| 已接入 | 示例位置 |
| --- | --- |
| HTTP、统一响应、配置、i18n、邮件/SMS log driver | `src/app.rs`、`src/routes.rs`、`config/` |
| PostgreSQL、迁移、seed、认证流程、JWT/PAT、数据库审计和授权 | `migration/`、`src/routes.rs` |
| Redis cache、加密、出站 HTTP | `Cargo.toml`、`config/default.toml` |
| 数据库队列、typed job 与 API/worker/scheduler 分进程 | `src/jobs/`、`POST /api/jobs/example-heartbeat` |
| gRPC Protobuf/build scaffold | `src/grpc.rs`、`build.rs`、`proto/`；真实 RPC server 组合见 gRPC consumer |

存储上传、实时 WebSocket/SSE、双库矩阵及自定义 driver 的独立验收有不同服务拓扑或依赖边界，保留在 [`tests/consumers/`](../../tests/consumers/)；待综合示例能在本地配置下端到端展示时，再逐步接入。目录调整与不做 Rust 源码自动扫描的理由见 [ADR-064](../../.agents/project/decisions/adr-064-canonical-example-application.md) 和 [ADR-063](../../.agents/project/decisions/adr-063-explicit-app-module-registry.md)。

## 启动

准备 PostgreSQL、Redis 8，并在 `.env` 或进程环境设置数据库地址、JWT key 和加密 key。`.env.example` 给出配置键名，不含可用密钥。运行：

```sh
cd apps/example
cargo run -p cli -- key generate
cargo run -- migrate up
cargo run -- serve
```

`key generate` 的输出分别配置到 `APP_AUTH__JWT__KEYS__LOCAL` 和 `APP_SECURITY__ENCRYPTION__KEYS__LOCAL`。迁移前需先创建示例数据库。启动后访问 `GET /`；运行生成的认证流程需要配置可查看的 log mail driver。

提交一个后台任务：

```sh
curl -X POST http://127.0.0.1:3000/api/jobs/example-heartbeat
```

另开终端运行 worker 和 scheduler。它们使用同一数据库和配置：

```sh
cargo run -- queue work example
cargo run -- schedule run
```

当前生成的 `--grpc` 选项提供 Protobuf、`build.rs` 和类型模块脚手架；它不会自动注册 gRPC server。完整 listener、TLS 和真实路由验收见 [`tests/consumers/grpc/`](../../tests/consumers/grpc/README.md)。

## 检查

```sh
cargo check
cargo test
```

认证 HTTP 测试只有在设置 `OPENRUST_TEST_DATABASE_URL` 指向可创建/删除隔离测试库的 PostgreSQL 管理连接后才执行真实数据库路径；未设置而跳过不算数据库验收通过。独立 consumer 的真实拓扑、MySQL 与扩展边界见仓库根 `tests/consumers/`。
