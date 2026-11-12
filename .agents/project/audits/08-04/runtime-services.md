# 08-04 可选统一启动

当前行为见[默认同进程统一启动](#默认同进程统一启动)。前两节记录 `6b343058` 的可选模式验收。

## 对照与实现

Laravel 13.11.0 应用骨架把 HTTP 入口交给 `handleRequest`，Artisan 入口交给 `handleCommand`。OpenRust 的 `./main` 默认运行 HTTP 及其已配置的监听器。独立 Artisan 仍由 `handleCommand` 执行，支持 `queue:work`、`schedule:work` 和 `schedule:run`。 [Laravel HTTP 入口](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/public/index.php) · [Laravel Artisan 入口](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/artisan)

统一启动由 `RUNTIME_ENABLED=true` 开启，`RUNTIME_HTTP`、`RUNTIME_QUEUE` 和 `RUNTIME_SCHEDULE` 可分别关闭服务。对照 Goravel 固定版本的 `Application.Start` 和 Runner 取消、汇合逻辑。核对旧版 `6d1a8192` 的 `run_application`、聚合运行验收及调度器的可取消循环，沿用同一应用进程编排服务的做法。当前实现复用已有的 `RuntimeService`、Artisan 命令、Worker 和调度循环。 [Goravel Application.Start](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go#L216-L291)

受管队列使用默认连接和队列，以已解析连接的驱动判断是否跳过 `sync`。调度只在有任务时启动。其他监听器使用各自的配置。服务退出或收到关停信号时，运行时通知其余服务停止接新工作并等待在途工作结束。Worker 的非零退出码传递给进程。受管 Worker 使用运行时关停信号，独立 Worker 的 OS 信号处理和进程内存门槛仍由 Artisan 入口负责。受管 Worker 共享进程，关闭基于整个进程 RSS 的内存门槛。

强制超时保留独立执行子进程。Windows 使用独立的进程组，让 Ctrl+C 交给 Worker 所在进程处理，并继续使用 Job Object 在超时时回收执行器和后代进程。 [Windows 进程创建标志](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags)

## 实际验收

- macOS arm64、Rust 1.94.0。根应用二进制实测默认和显式统一启动的 `GET /up`、SIGINT 退出、无服务报错以及在统一启动配置下独立执行 `artisan inspire`。锁定离线工作区测试、最终进程夹具、全工作区全特性严格 Clippy 和格式检查通过。
- Parallels Windows 11 Pro ARM64 Build 26200 的 x64 兼容层。以 Rust/Cargo 1.94.0 和 MSVC 14.44.35207 编译并运行 `x86_64-pc-windows-msvc` 未优化验收二进制，PE Machine `0x8664`。运行 `cargo test --locked --offline --target x86_64-pc-windows-msvc --test queue_hard_timeout`，另跑框架 `queue_worker` 的 SQLite 实际 Artisan Worker 用例。
- 同一进程夹具验证默认 HTTP、统一启动三服务、分别关闭服务、`sync` 别名和空调度不启动、独立 `queue:work` 与 `schedule:run`。Windows 向独立控制台广播真实 `CTRL_C_EVENT`。HTTP 停止监听，隔离任务和调度回调继续完成，进程退出 0，未预约的下一项由独立 Worker 消费。独立 `schedule:run` 与受管调度共用文件锁并跳过重叠任务。
- 独立 Artisan 和统一入口使用同一夹具、SQLite 配置和隔离队列，分别运行等价任务。两边的硬超时都返回 124，验证执行器及后代回收、`failOnTimeout` 回调和按 UUID 查到的失败记录，以及可重试任务在租约后完成。受管 Worker 超时退出时，HTTP 监听也关闭。
- 三语部署页面构建通过，在根路径和 `/docs/` 分别完成三语桌面和移动端浏览器导航检查。验收覆盖 macOS arm64 与 Windows x64 MSVC；本批统一启动进程夹具使用 SQLite。

[English](../../../../docs/en/getting-started/deployment.md#run-services-together) · [简体中文](../../../../docs/zh-Hans/getting-started/deployment.md#一起运行服务) · [繁體中文](../../../../docs/zh-Hant/getting-started/deployment.md#一起執行服務)

## 默认同进程统一启动

应用骨架默认启用统一启动。`./main` 在同一进程运行 HTTP、默认异步队列的 Worker，以及注册了任务的调度器。`RUNTIME_ENABLED=false` 恢复 HTTP 启动及原有监听器，独立 Artisan 入口继续可用。`RUNTIME_HTTP`、`RUNTIME_QUEUE` 和 `RUNTIME_SCHEDULE` 可在统一启动时分别关闭服务。同步队列和空调度会跳过。应用骨架在 `config/runtime.rs` 设置默认值，框架继续兼容未声明该配置的应用。服务编排对照固定 Goravel `Application.Start` 和旧版 `6d1a8192` 的聚合运行测试。 [Goravel Application.Start](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go#L216-L291)

沿用已提交的同进程 Worker 和调度逻辑。共享进程的 RSS 包含 HTTP、队列和调度的占用，受管 Worker 不套用独立门槛。独立 `queue:work` 继续执行自己的内存上限。按任务启用的硬超时继续使用原有执行器隔离，Windows 使用 Job Object 回收后代进程。子进程 Worker 候选已单独保存，本次提交范围为应用配置、验收夹具和三语文档。

- 使用根应用真实 `main` 验证默认 HTTP `/up` 和 SQLite 队列消费。显式关闭统一启动后，HTTP 仍可访问，队列中的任务留给独立 Worker。
- 同一可执行夹具验证 HTTP、Worker 和调度同进程，Worker 和调度任务的 PID 均与启动进程一致，硬超时执行器 PID 独立。分别验证三项服务开关、`sync` 别名、空调度及独立 Artisan。
- 统一启动与独立 Artisan 在相同驱动及 Worker 配置下使用隔离队列执行等价任务。两边的强制超时退出码均为 124，回收执行器和后代进程。`failOnTimeout` 触发失败回调并可按 UUID 找到失败记录，可重试任务在租约到期后完成。独立 `schedule:run` 与受管调度通过同一文件锁跳过重叠任务。
- 真实 Windows Ctrl+C 和 macOS 终止信号验证停止接收新工作，在途队列与调度任务完成后退出 0，下一项任务仍可由独立 Worker 消费。内存对照中，独立 Worker 达到 128 MiB 门槛后以 12 退出并保留下一项，共享进程继续消费并在信号后正常退出。
- macOS arm64 和 Windows 11 Pro ARM64 虚拟机的 x64 MSVC 兼容层使用 Rust 1.94.0 及 SQLite 通过进程验收。Windows 工具链为 MSVC 14.44.35207，实际根二进制和验收夹具 PE Machine 均为 `0x8664`。本批使用未优化测试构建。macOS 锁定离线工作区测试、全目标全特性严格 Clippy 和格式通过。三语站点构建及根路径和 `/docs/` 下的 12 项桌面与移动端导航检查通过。

后续常驻服务在已装配、已配置且满足启动条件时默认纳入主应用，并保留关闭和独立运行入口。一次性 Artisan 命令由显式调用执行。本批范围为 HTTP、队列和调度。

配置测试使用平台原生路径组件核对公共存储链接。原有 HTTP 维护和 Reverb 验收夹具显式关闭其未配置的队列，macOS 嵌入式 Reverb、监听冲突、独立 Artisan 和重启验收通过。

Windows 另跑框架的 SQLite 独立 Worker 进程验收，真实 Ctrl+C、在途任务和退出状态的回归通过。

## Windows x64 MSVC 的 MySQL 与 Redis 同进程补验

以 `738b336d88d987fe4e1195fdd0f93480a1824d30` 为应用基线，将同一验收夹具扩展到 MySQL 与 Redis。使用现有 `RUNTIME_*` 配置契约。Parallels Windows 11 Pro ARM64 Build 26200 的 x64 兼容层使用 Rust/Cargo 1.94.0、MSVC 14.44.35207。执行 `cargo test --locked --offline --target x86_64-pc-windows-msvc --test queue_hard_timeout`，根应用和夹具均为 PE Machine `0x8664` 的未优化构建。源码清单和锁文件校验通过。

首轮 MySQL 同进程强制超时写入失败记录后触发 Debug 栈溢出，预约仍留在队列中。受管 Artisan 命令改由同一 Tokio 运行时中的独立任务轮询，减少监督循环叠加的调用栈。监督器继续等待各任务退出并传递退出码。进程 PID 对照和硬超时执行器的隔离验证通过。

- MySQL 8.4.13 使用专用测试库和账号保存队列与失败记录。Redis 8.10.3 使用专用容器保存队列，失败记录保存到独立 SQLite。原 SQLite 用例同时通过。
- 在相同后端和 Worker 配置下，用同一夹具对照无参数默认启动和独立 `artisan queue:work`。等价任务使用隔离队列。两种入口均验证强制超时退出 `124`、执行器及后代回收、无晚到副作用、`failOnTimeout` 回调、按 UUID 核对失败记录、可重试任务在租约后完成。
- 向两个入口分别发送真实 Windows `CTRL_C_EVENT`，在途任务完成后均退出 `0`。默认启动还验证 HTTP 关闭监听及在途调度任务完成。两边都保留下一项，随后由独立 Worker 消费。
- 根应用 `main.exe` 在两个后端上验证默认 HTTP `/up` 和 Worker 消费，并验证 `RUNTIME_ENABLED=false` 保留 HTTP、队列任务等待独立消费。

macOS 锁定离线工作区测试、严格 Clippy、格式和差异检查通过。

## Linux ARM64 同进程补验

在 macOS ARM64 宿主的 Docker Desktop Linux 虚拟机中运行 Debian 12 Bookworm ARM64 容器，实测内核 `7.0.14-linuxkit`。容器使用 `--init`，Rust/Cargo 1.94.0。基线为 `738b336d88d987fe4e1195fdd0f93480a1824d30`，并应用已本地提交的进程夹具扩展与受管 Artisan 栈修复 `f4452de5bdd2fd9025e7d5c1d7743928d9a0844b`。使用现有 `RUNTIME_*` 配置契约，锁文件和验收输入哈希与 Windows 候选一致。

- `cargo +1.94.0 test --locked --offline --target aarch64-unknown-linux-gnu --test queue_hard_timeout` 退出 `0`。根应用与验收夹具均为未优化 ELF64 AArch64。Linux 本批使用 SQLite。
- 根应用 `main` 验证默认 HTTP 和 Worker 消费、显式关闭。以同一夹具核对 HTTP、Worker 和调度同进程、分别关闭服务、同步连接及空调度跳过、独立 Artisan。Worker、调度和主进程的 PID 一致，硬超时执行器 PID 独立。
- 两种入口用隔离队列运行等价任务，强制超时均退出 `124`，验证进程树回收、失败回调及按 UUID 的 `failOnTimeout` 记录、租约后重试和后续消费。独立 `schedule:run` 使用同一文件锁跳过重叠任务。
- 向两个入口在任务进行中分别发送真实 SIGTERM，已开始的队列任务完成，下一项保留并由独立 Worker 消费。统一启动同时完成在途调度任务，停止 HTTP 监听，两种入口均退出 `0`。
- 三语发布说明记录 Linux ARM64 容器证据。三语站点构建通过，根路径与 `/docs/` 的 12 项浏览器导航检查通过。

## Goravel v1.18 Runner 生命周期

对照固定 Goravel v1.18 的 Runner 契约、Provider 注册、Builder 和运行循环。先收集 Provider 注册，再加载应用的 `withRunners` 回调。连续配置时最后一次回调生效。同签名保留首次注册项，按禁用列表过滤后才调用 `shouldRun`。可运行的 Runner 并发执行。收到停止信号或 Runner 失败时，按 `shutdownPriority` 从小到大关停，同组并发，默认优先级为零。成功返回的 Runner 仍会进入关停流程，运行失败的 Runner 跳过关停回调。 [Runner 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/foundation/application.go) · [Provider 注册](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/foundation/service_provider.go) · [Builder](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application_builder.go) · [生命周期](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go)

`APP_DISABLED_RUNNERS` 替代旧 `RUNTIME_*` 开关。示例 `APP_DISABLED_RUNNERS=queue,schedule` 位于 `.env.example` 的应用配置末尾。内置签名是 `http`、`queue`、`schedule`、`grpc` 和 `reverb`。自定义 Runner 使用同一签名去重与模式过滤。模式按 Goravel 使用的 Go `path.Match` 规则匹配，无效模式警告后跳过。没有符合条件的 Runner 时正常退出。

Rust 实现为每个 Runner 提供取消令牌，用同一运行时内的 Tokio 任务并发执行。监听器可以在报告就绪前绑定地址，HTTP 预渲染与其他服务共用关停流程。队列与调度继续复用 Artisan 执行机制，独立 Artisan 不受 Runner 禁用列表影响。Worker 的非零退出码会传递给应用进程，启用硬超时的任务继续在隔离执行器中运行。

### 实际验收

- macOS ARM64 和 Linux ARM64 运行同一份候选源码，Rust 1.94.0。用例核对最后一次 `withRunners` 生效、Provider 首次签名优先、内置 `queue` 签名与自定义 Runner 冲突、过滤在 `shouldRun` 之前、并发启动与分组关停、运行失败触发关停。
- macOS ARM64 工作区锁定离线测试、全目标全特性严格 Clippy 和格式检查通过。实际二进制覆盖 SQLite 队列、HTTP 维护预渲染、gRPC 监听冲突及信号关停。启用 Reverb 特性后，内嵌 HTTP 和 WebSocket、监听冲突、独立 Artisan 和重启用例通过。占用 Reverb 端口时，按短名禁用 `reverb` 后 HTTP 仍可启动，独立 `artisan reverb:start` 仍检查端口。根二进制在 `APP_DISABLED_RUNNERS=*` 时直接退出 0，同一设置下 `artisan inspire` 正常执行。
- Linux ARM64 使用 Debian 12 容器和 LinuxKit 7.0.14，`main` 为 ELF64 AArch64。SQLite 进程验收覆盖精确与通配符禁用、同进程 Worker 和调度、独立 Artisan、硬超时退出 124、执行器树回收、失败回调、按 UUID 查询失败记录及后续消费。向两种入口发送真实 SIGTERM，等待在途工作完成后均退出 0。
- Parallels Windows 11 ARM64 的 x64 兼容层运行 `x86_64-pc-windows-msvc` 未优化构建，Rust 1.94.0、MSVC 14.44.35207。9 个候选输入哈希相符，根应用和验收夹具均为 PE Machine `0x8664`。配置、Runner 契约及真实二进制的 SQLite、MySQL 8.4.13、Redis 8.10.3 对照通过。MySQL 同库保存队列与失败记录，Redis 队列使用 SQLite 保存失败记录。两种入口均验证真实 Ctrl+C 后退出 0、硬超时后退出 124、执行树回收、失败回调、按 UUID 查询失败记录、租约后重试和后续消费。统一入口还验证 HTTP 停止监听、在途调度完成和调度锁防重叠。
- 三语部署文档构建通过。根路径和 `/docs/` 的桌面及移动端导航共 12 项检查通过。
