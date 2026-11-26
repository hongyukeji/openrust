# Tinker 原生交互控制台

## 选型与范围

将 `./artisan tinker` 作为可选开发扩展。复用 Evcxr 0.21.1 处理 Rust 片段编译、执行和会话变量。应用适配层构建为 Rust 共享库，每个执行会话启动一次应用，复用同一个应用容器和 Tokio 运行时。采用现有启动和 Facade 作用域，不启动 HTTP、队列或调度服务。默认应用和部署二进制无需引入 REPL 依赖。

固定基线为 [Artisan 教程](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md#tinker-repl)。借鉴 [Tinker 的应用适配](https://github.com/laravel/tinker/blob/v3.2.0/src/Console/TinkerCommand.php) 与 [Evcxr 求值机制](https://github.com/evcxr/evcxr/blob/v0.21.1/evcxr/HOW_IT_WORKS.md)。教程中的介绍、安装、使用、命令允许清单、自动别名排除五节仍随扩展逐项验收。原生类型按 Rust 可见性和 `use` 导入处理，短名、冲突和排除由显式配置决定。

## 最小验证证据

基于 `9e60f8a6e0f69061850b00578aa0141c4aa6211a` 的隔离快照，在 macOS ARM64 上验证。Evcxr 0.21.1 本身可用 Rust 1.94.0 构建。最终验收使用同版本上游优化二进制，应用与 REPL 输入均由 Rust 1.94.0 编译。24 条真实 REPL 输入通过并正常退出：

- 定义并调用开发者函数，跨输入保留和修改变量，最终值为 42。
- 复用应用原有启动链，使用 Eloquent 创建、读取并更新自定义模型 `NotebookEntry`。
- 分别经 `Application::make` 与 `App::make` 解析自定义单例，`Arc::ptr_eq` 通过，跨输入状态为 7。
- 独立查询 SQLite，最终只有一行，触发器记录恰好一次 INSERT、一次 UPDATE。

证据在项目内 `target/tmp/tinker-probe.hog9oh/`。通过日志为 `repl-app-dylib.log`，输入为 `input`，驱动为 `drive.py`，适配层为 `source/repl_app.rs`，验收数据库为 `probe.sqlite`。

已观察到的失败和适配：

- 默认链接加 Evcxr 顶层 `.await` 在模型写入时触发 SQLx Tokio 上下文 panic，但 SQLite 已提交 INSERT。保留 `repl-acceptance.log` 与 `static-attempt.sqlite`。任何求值失败后都不得自动重放写入。
- Evcxr 整体强制动态链接在 Rust 1.94.0 上报 `core` 元数据错误，见 `repl-dylib.log`。采用仅应用适配层共享链接、应用持有 Tokio 的已验方案。`session.run(...)` 已通过，直接在提示符用顶层 `.await` 待验证。
- REPL 生成项目采用独立 Cargo 工作区和项目内缓存。在实际工作区根传播项目锁文件及本地补丁，验证解析结果，避免静默换用注册表里的同版本依赖。
- 本机关闭 REPL 动态库的调试符号剥离，并为执行进程提供对应 Rust 标准库和应用库的加载路径。只在扩展进程设置所需参数。验证正式实现的工具链和平台边界。

## 正式交付与验收

1. 用独立可选包接入 Artisan 注册与配置。应用提供最小的可导入模型、服务和启动入口，保持现有二进制入口职责。无须自建 Rust 求值器、反射系统或第二套容器。
2. 用真实 `./artisan tinker` 验证安装、进入和退出、用户 Rust 代码、会话变量、实库模型读写、跨片段单例与 Facade 身份、历史写入次数、命令允许清单和别名排除。允许清单限制控制台命令入口，任意 Rust 代码按本地开发者权限运行。
3. 核对编译错误后的继续输入、panic、Ctrl+C、执行进程重启和变量状态，验证数据库副作用不会被自动重放。验证默认应用、生成项目、依赖补丁和部署产物边界。记录首启与热会话延迟，待验的平台单列。
4. 同步英、简中、繁中教程和 Artisan 逐节账本。隔离原型只证明上述链路，五节在正式入口通过验收前保留待完成状态。按项目规范验收后单独本地提交，先不推送。
