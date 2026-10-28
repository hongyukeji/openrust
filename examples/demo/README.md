# OpenRust 聚合回归应用

本目录是根 Cargo workspace 的应用成员，与框架位于同一仓库、同一项目，共用根 Cargo.lock；package 为 demo，只有默认 server bin。它完整保留原根应用的启动、HTTP、认证、数据库、队列、调度与真实服务回归责任。通用框架在仓库根 src/；完整业务能力示例见 [example](../example/README.md)，新应用骨架唯一来源见 [CLI](../../tools/cli/README.md)。

## 运行

```bash
cd examples/demo
cargo run
# 可选覆盖监听端口
APP_SERVER_PORT=4000 cargo run
```

从 workspace 根运行时，薄入口先切至本应用目录，确保 config/public/storage 与 .env 均按应用根解析；其他启动目录保留已有部署约定。

默认无外部服务依赖，/health 与 /ready 提供探针。src/main.rs 为薄入口，bootstrap/app.rs 装配，routes/web.rs 路由，app/ 业务；其他职责目录采用 Laravel 布局。配置由本目录 config/ 与环境变量加载。


## 验证

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo test --all-features
cargo doc --no-deps --all-features
```

根 default-members=["examples/demo"]，可从仓库根运行 cargo run；框架测试显式用 cargo test -p openrust，完整 cargo test --workspace 包含本目录。tests/server 中数据用例按 feature 与授权测试 URL 运行；TestDatabase 使用隔离数据库，不复用生产业务数据。真实复制/集群和供应商实发保留独立门禁，跳过不能算通过。
