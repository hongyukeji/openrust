# ARM64 原生构建验证

验收从 `ed8b63c7` 的 Git 归档开始，同时启用接收和发送特性，Rust 1.94.0。使用隔离测试应用、SQLite、真实 HTTP、Artisan 和独立 Worker。目标平台间共用源码与入口。

## ARM64 Linux

在本机 Docker 的 ARM64 Linux 环境编译，得到动态链接的 AArch64 ELF。接收回归通过原有 139 个 HTTP 用例、86 个独立 Worker，并执行具名工厂、补派、恢复和缓存用例。常规出站回归通过原始字节签名、事务、发送控制、超时、权限拒绝、重试及应用 Job。

第一次并行链接有链接子进程被系统终止。对同一源码和锁文件将该次 Cargo 构建改为单任务，随后编译及运行通过。日志保存在 `target/tmp/webhook-platform-native/`。此项覆盖 Linux ARM64 的原生目标程序在本机容器中的实际运行。物理跨主机、Linux x86_64 发布制品和托管网络仍按发布矩阵验收。

## Windows 11 ARM64

在 Parallels 的 Windows 11 ARM64 虚机以 `aarch64-pc-windows-msvc` 编译。专用 NTFS 构建盘在持锁清理后有 38.16 GB 可用。工作目录和 SQLite 数据均隔离。实际运行 Artisan 迁移、配置缓存、路由缓存，随后通过真实 HTTP 登记投递。独立出站 Worker 向应用的实际 HTTP 接收路由发送签名正文，独立接收 Worker 读取完全一致的正文。Artisan 查询确认投递成功。

PE 核对正式入口 `main.exe` 预留 16 MiB 主线程栈。首次通用测试二进制预留 1 MiB，出站 Worker 实际溢出。单独装箱运输 Future 后复现了同一问题，撤回该尝试。原源码使用 `cargo rustc --test webhooks_app -- -C link-arg=/STACK:16777216` 复现正式入口的栈设置，核对 PE 后完整往返通过。临时启动脚本改用夹具要求的 `WEBHOOK_HTTP` 和 TCP 就绪检测。按请求的 JSON 类型提供合法 JSON 后，记录收到的原文以逐字节比较。

本项覆盖 Windows ARM64 虚机中的原生程序。Windows x86_64 实机、Docker Desktop 和正式制品仍按发布环境验收。运行结束后删除本批隔离应用目录。

### 默认测试构建复验

将栈设置收敛到 Webhook 验收程序的 Windows MSVC 入口。测试在预留 16 MiB 的线程启动现有单线程 Tokio 运行时。从同一隔离源码叠加该入口改动，使用 Rust 1.94.0 和普通的 `cargo test --locked --offline --features webhook-client,webhook-server --test webhooks_app --no-run` 在 Windows 11 ARM64 复验。无栈相关的环境变量和命令行链接参数，PE 主线程仍预留 1 MiB。Artisan 迁移、配置与路由缓存、真实 HTTP 登记、独立发件及收件 Worker、原文比较和投递查询全部通过。删除隔离 SQLite 应用目录，日志保存在 `target/tmp/webhook-windows-test-stack/`。
