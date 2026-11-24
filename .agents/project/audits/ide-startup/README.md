# IDE 启动与编译器弃用警告

2026-11-24，用户反馈 IDE 启动应用报错。

- 实际读取 RustRover 运行控制台：cargo run 已完成构建，进程输出 OpenRust listening on http://127.0.0.1:8000。独立 HTTP 核对首页、/up、/api 均为200，/api 返回配置中的应用名称、版本及本地化成功消息。因此本次观察到的当前状态是启动正常，而非启动失败；无法据此追溯已经消失的更早错误。
- 当前控制台唯一可见编译警告是 framework/src/foundation/cloud/telemetry.rs:139 使用 AtomicU8::fetch_update 被较新编译器弃用。IDE Run 配置为默认工具链，实际 stable 是 Rust1.99.1；项目验收基线1.94.0。未调整用户IDE配置或工具链，也未停止其运行服务。
- 修复仅将重新入队状态更新改成 fetch_max(2, Ordering::AcqRel)。该私有原子状态只可能是0处理中、2重新入队、3失败，取最大值可保留失败优先级，不再调用被弃用的方法，也不使用仅较新版本可用的建议替代接口。
- 隔离快照复用 target/tmp/facade-alignment/source 及对应独占缓存，仅覆盖本次 telemetry 文件，保留日志模块协作者在途修改。通过 tests/build_guard.py 运行 Rust1.94.0 的 cargo test --locked -p openrust-framework --test cloud，15项通过，包含真实本地HTTP/Worker的既有消费者；本轮不扩展为托管云发布验收。
- 从正式源码提取实际 released 方法，分别使用 rustc +1.94.0 和 +stable（1.99.1）、-D warnings 编译；两版都通过3种状态及1000次并发重新入队/失败检查。证据及脚本在 target/tmp/ide-startup/{cloud.log,atomic.log,check_atomic.py}。临时测试可执行文件运行后直接删除，保留源码及日志，正常增量缓存继续复用。
- 本轮没有在IDE中重新启动应用：对照的是修复前运行控制台、实际运行服务和修复后的双版本编译／行为验收；下一次IDE构建会消费修复。未将工作区其他未验收修改或此后其他编译器警告算成本轮验证结果，未推送。

## 最新稳定版复查与本机默认升级（2026-11-24）

- 用户要求再次核查最新 Rust 的弃用调用，并明确授权把本机默认更新到官方最新版。官方发布页 https://blog.rust-lang.org/2026/11/12/Rust-1.100.0/ 与 static.rust-lang.org 的 stable manifest 均指向1.100.0（2026-11-12）。先单独安装1.100.0作验收，再使用 rustup update stable --no-self-update 和 rustup default stable 升级默认通道，保留原有组件及1.94等其他工具链。实测默认 rustc/cargo 均为1.100.0，Clippy0.1.100、rustfmt1.11.0；未修改项目 rust-version、IDE启动配置或重启用户服务。
- 从已提交082c867be冻结源码到 target/tmp/rust-deprecations/source，保留工作区正在开发的日志、依赖锁与发布版本等改动。Rust1.100.0首次 cargo check --locked --workspace --all-targets --all-features 通过，没有编译器诊断；不把在途代码计为已通过。
- 额外扫描本项目源码、测试、模板及显式弃用豁免。邮箱域名转换 dns.rs 两处、诊断标签长度 rfc.rs 一处 #[allow(deprecated)] 已无对应弃用API，属于过时豁免，予以删除；保留现有Punycode实现、Intl默认选项和校验行为，不更换依赖。其他常见 std 弃用符号扫描无命中。
- 严格验收仅在隔离副本的workspace lint中启用 deprecated=deny、deprecated_in_future=forbid，正式项目lint没有改变。strict.jsonl显示30个本地包（含现有路径补丁依赖）、511个去重Cargo构建目标，build-finished success=true、零compiler-message；61秒通过全部工作区/目标/特性。default-final.jsonl验证根应用默认特性及全部构建目标，42.09秒通过，零compiler-message。Cargo的all-targets指库、二进制、测试等，不代表跨操作系统目标。
- 初次把deprecated设为forbid时，会拒绝第三方thiserror派生宏自动插入的allow(deprecated)，并非发现了调用点弃用；保留失败日志，改用deny并结合项目手写豁免全量扫描。没有修改外部派生宏或为本项目增加豁免。最初两轮失败不计通过。
- Rust1.94.0再次 cargo check --locked -p openrust-framework --lib 通过（msrv.log，1分07秒）。本次只移除注解，无运行逻辑变化，不重复已验收的DNS与邮箱运行路径。所有Cargo均经tests/build_guard.py，显式选择工具链；最新Rust使用独占rust-deprecations/cargo-target，1.94复用前批facade-alignment缓存。源码、命令输出、工具链manifest及JSON诊断保存在target/tmp/rust-deprecations/。没有触及32GB停线，正常缓存保留，未删除数据库、数据卷或共享依赖。
- 范围：macOS aarch64、冻结的已提交源码；未声称Windows/Linux条件分支、未执行的文档Rust代码、其他协作者在途修改或所有第三方依赖内部均无弃用。生成模板额外静态扫描常见弃用名称，不将静态搜索当作生成应用的完整编译验收。未推送。
