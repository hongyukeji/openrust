# 构建产物增长排查与约束

2026-11-21，用户要求查明编译产物持续增长、既有清理规定为何没有阻止，并落实预防。

## 现场与原因

初次df可用38,496,212 KiB。根target占40,774,732 KiB，其中debug/incremental占31,158,664 KiB，约占76%；增量缓存包含1,648个条目。临时批次多数仅保留源码和日志；production-simulation的check-target约4.99 GB，后续Horizon仍在构建。不是所有历史/tmp目录都保存着编译缓存，不能按目录名整批删除。

现场捕获PID14666的stable Cargo `check --all-targets --workspace --keep-going`，父PID27519为RustRover。根目录当时没有.cargo/config.toml；验收shell里的CARGO_INCREMENTAL=0不会影响独立IDE进程。根debug还保存main二进制及依赖，说明它承载了历次构建，不能把全部占用都归因为这一条IDE check。已有模拟验收记录也明确记载过一次误从活跃工作树开始的编译后中断。

既有规范确实要求批次结束清理、增量关闭、可用空间32 GB停线，但存在执行和覆盖缺口：

1. 关闭增量仅靠各验收命令的环境变量，未在项目Cargo配置落实，IDE自动检查仍可产生缓存。
2. 清理聚焦本批独占/tmp target，根target作为IDE／手工构建缓存长期遗漏；“保留共享缓存”没有区分下载缓存和可重建的根构建产物。
3. 人工收尾缺少进程生命周期入口，失败／取消和多批并行的资源约束没有统一执行。df停线是准入检查，不能自动限制已启动的其他进程。

## 成熟机制与修正

核对[Cargo配置](https://doc.rust-lang.org/cargo/reference/config.html#buildincremental)、[profile增量机制](https://doc.rust-lang.org/cargo/reference/profiles.html#incremental)、[RustRover外部代码检查](https://www.jetbrains.com/help/rust/code-analysis.html)，以及[cargo-sweep现有清理方案](https://github.com/holmgr/cargo-sweep)。采用Cargo原生配置，不引入依赖或自己解析Cargo指纹来判断陈旧文件。增量配置被环境变量覆盖的优先级按官方说明处理，并在实际Rust1.94验证。

- 新.cargo/config.toml：build.incremental=false、jobs=3，覆盖正常使用项目配置的IDE检查；保留IDE默认调试信息，不新增rust-toolchain.toml，不修改Cargo.toml的rust-version。
- tests/build_session.py只管理自己mkdtemp创建的target，不是全盘清理器。主工作区调用、拒绝活跃工作树、独立源码执行、同项目受管构建串行、32 GB启动和运行中停线；传递Rust1.94.0、增量关闭、jobs=3、dev/test debug=0到子进程。根IDE target超过8 GiB拒绝新验收，保留其内容等待确认闲置后清理，不冒称IDE有硬配额。
- 成功、失败、SIGINT／SIGTERM均回收自建target；进程组先TERM，给后台子进程最多10秒执行服务清理，再KILL残留。SIGKILL／断电不能保证finally，规范要求下一批前盘点孤儿产物。旧的手工构建和已经运行的其他对话不会被新入口追溯接管。
- AGENTS及项目规范同步为必须使用本机受管入口，覆盖根IDE缓存、失败与中断收尾；导出API／站点和日志保存在target之外。此入口目前为macOS/Linux，未声称Windows原生已验。

## 验证与清理

证据目录 `/private/tmp/openrust-build-hygiene-20261121/`：

- lifecycle.log：6个测试函数通过。真实子进程验证成功／退出37、环境继承、活跃工作树拒绝、并发拒绝、SIGINT／SIGTERM及后台子进程清理宽限／锁释放。低磁盘准入、运行中触及停线和8 GiB根缓存拒绝分别验证；运行中低磁盘及大缓存用注入读数，不真实填满磁盘，目标进程和清理均实际执行。没有删另一个所有者的缓存。
- native-config.log：独立无依赖crate通过受管入口运行Rust1.94，两次修改源码后cargo check。显式移除CARGO_INCREMENTAL／CARGO_BUILD_INCREMENTAL，仅使用项目Cargo配置；真实rustc命令没有incremental参数，target没有增量产物。结束后target自动删除。
- 本次没有修改Rust业务代码，不运行整个workspace重建、数据库或文档站全量构建；内部Markdown检查链接目标及git diff --check。

两次清理均确认路径为本项目可重建产物，取得根debug/.cargo-lock排他锁并检查lsof。第一次仅删除闲置incremental，实际删除前占32,880,984 KiB；df可用37,524,852→68,710,288 KiB，净增加31,185,436 KiB。第二次清理剩余debug产物（9,626,904 KiB），仅清理进程自己的锁描述符在用；保留.cargo-lock原inode避免新Cargo绕过锁，df可用67,631,496→76,787,968 KiB，净增加9,156,472 KiB。

合计两次清理窗口净回收40,341,908 KiB（41.310 GB）；并行构建和APFS使实际回收量不等于du总和。清理后根target仅4 KiB。源码、所有未提交改动、日志、API交付、Cargo下载缓存、在用Horizon批次及全部数据库和数据卷保留。没有停止IDE或其他对话的构建，没有清理系统缓存、其他项目或Docker卷。此记录不宣称后续任何外部命令都无法绕过项目配置。

IDE随后重新生成必要的检查依赖，复查根target为358,040 KiB，incremental目录为空、占用0 KiB。正常检查仍会产生依赖输出；本次关闭增量并加入根缓存准入检查，不能把清理后的4 KiB描述为长期固定占用。
