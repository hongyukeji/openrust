# 08-03 gRPC 扩展

这是 OpenRust 扩展，对照固定 [Goravel 配置](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/config/grpc.go) 和 [gRPC Runner](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/grpc/runners.go)。复用旧版 `6d1a8192` 的 Tonic 服务注册、已绑定 TCP 监听器、Tonic Health 客户端与关闭验证，以及工作区现有的编译型装配。

`install:grpc` 在应用源码中加入 `routes/grpc.rs` 和 `withGrpc` 注册入口，重复运行保留现有代码。Provider 与 HTTP 路由装配完成后执行一次 gRPC 注册，应用启动阶段使用共享服务生命周期绑定监听，任一监听绑定失败时终止启动。Artisan 复用装配且无需绑定监听。当前服务端使用明文 HTTP/2，TLS 由反向代理终止。命名客户端、客户端凭据与拦截器配置待接入。

## 验收

- Rust 1.94.0、macOS Apple Silicon，锁定离线工作区启用 Reverb 后 533 项通过。5 项忽略测试由集成测试作为独立子进程启动。格式及严格 Clippy 通过。
- 实际 Artisan 安装、重复安装保留文件、Provider 后单次注册、真实 Tonic Health 的 SERVING 和 NOT_FOUND、端口冲突先于 HTTP 就绪、SIGTERM 后 HTTP 和 gRPC 端口释放均通过。
- 三语相同的 Tonic Health 示例从文档提取。在隔离应用执行 `./artisan install:grpc` 后编译并运行，真实客户端检查两种 gRPC 状态，HTTP `/up` 返回 200，`artisan inspire` 正常退出。
- 三语站点构建、12 项通用浏览器检查和 6 项 gRPC 侧栏链接及页面检查通过，覆盖根路径和 `/docs/`。

下一单元 08-04 安装、部署和发布资料。
