# IDE 启动与编译器弃用警告

2026-11-24，用户反馈 IDE 启动应用报错。

- 实际读取 RustRover 运行控制台：cargo run 已完成构建，进程输出 OpenRust listening on http://127.0.0.1:8000。独立 HTTP 核对首页、/up、/api 均为200，/api 返回配置中的应用名称、版本及本地化成功消息。因此本次观察到的当前状态是启动正常，而非启动失败；无法据此追溯已经消失的更早错误。
- 当前控制台唯一可见编译警告是 framework/src/foundation/cloud/telemetry.rs:139 使用 AtomicU8::fetch_update 被较新编译器弃用。IDE Run 配置为默认工具链，实际 stable 是 Rust1.99.1；项目验收基线1.94.0。未调整用户IDE配置或工具链，也未停止其运行服务。
- 修复仅将重新入队状态更新改成 fetch_max(2, Ordering::AcqRel)。该私有原子状态只可能是0处理中、2重新入队、3失败，取最大值可保留失败优先级，不再调用被弃用的方法，也不使用仅较新版本可用的建议替代接口。
- 隔离快照复用 target/tmp/facade-alignment/source 及对应独占缓存，仅覆盖本次 telemetry 文件，保留日志模块协作者在途修改。通过 tests/build_guard.py 运行 Rust1.94.0 的 cargo test --locked -p openrust-framework --test cloud，15项通过，包含真实本地HTTP/Worker的既有消费者；本轮不扩展为托管云发布验收。
- 从正式源码提取实际 released 方法，分别使用 rustc +1.94.0 和 +stable（1.99.1）、-D warnings 编译；两版都通过3种状态及1000次并发重新入队/失败检查。证据及脚本在 target/tmp/ide-startup/{cloud.log,atomic.log,check_atomic.py}。临时测试可执行文件运行后直接删除，保留源码及日志，正常增量缓存继续复用。
- 本轮没有在IDE中重新启动应用：对照的是修复前运行控制台、实际运行服务和修复后的双版本编译／行为验收；下一次IDE构建会消费修复。未将工作区其他未验收修改或此后其他编译器警告算成本轮验证结果，未推送。
