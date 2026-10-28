# OpenRust

OpenRust 是采用 Laravel 概念与开发体验的 Rust 应用框架，当前正在全面重构。应用与框架源码分离，保留 Rust 类型、所有权、async、Cargo 和 IDE 工具能力。所有 package 暂不发布。

## 启动默认应用

```bash
cargo run
```

根 `application` package 只有一个 `server` bin。`src/main.rs` 是薄入口，`bootstrap/app.rs` 装配应用，`routes/web.rs` 注册路由；业务代码放在 `app/`。配置、数据库代码、资源、公开文件、运行数据和应用测试分别位于 `config/`、`database/`、`resources/`、`public/`、`storage/`、`tests/`。框架实现与 API 说明见 [framework](framework/README.md)。默认启动无需数据库等外部服务。

RustRover 打开根工程后可选择共享 `Default run` 配置（Command=`run`）普通运行与断点调试。通过 `APP_SERVER_PORT=4000 cargo run` 修改监听端口。`/health` 提供存活检查，`/ready` 提供就绪状态；配置和当前公共 API 以框架说明为准。

## 创建独立应用

```bash
cargo run -p cli -- new /tmp/my-application
cd /tmp/my-application
cargo check
cargo test
cargo run
```

生成项目拥有自己的 workspace 和 Cargo.lock，本地框架/工具依赖在 manifest 中明确解析；源框架目录需保持可达。生成器不修改源 workspace；参数、模型/迁移等命令见 [CLI](tools/cli/README.md)。当前生成默认启用 Redis 缓存，运行生成应用前需配置可用 Redis。综合示例见 [examples/example](examples/example/README.md)。

## 开发与验证

`cargo test --workspace` 发现根应用集成回归、framework 单元测试与独立工具测试；`tests/consumers/` 保留外部扩展接入。完整验证入口和资源清理规则见 [贡献指南](CONTRIBUTING.md)。真实服务、复制、集群与 IDE 验收分别记录，编译或 skipped 测试不能替代。

重构目标见 [全面计划](.agents/project/plans/laravel-rebuild.md)，当前任务以 [todo](.agents/project/todo.md) 为准。源码 API 迁移期间保留已实现能力，新的 Container/Provider、HTTP、Model 等语义按阶段交付。

## 许可证

[MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE)。
