# 构建产物增长排查与约束

2026-11-21，用户要求查明编译产物持续增长、既有清理规定为何没有阻止，并落实预防。

## 停线自动清理的长期授权（2026-11-21）

用户随后明确：“以后触发停止线了，就清理本项目所有的编译产物”。现行规则已同步根AGENTS、项目规范与当前交接：触及32 GB停线即自动清理本项目根／隔离验收目录的全部Cargo编译产物及文档dist／cache，清理无需再次确认；先协调构建安全退出、核对占用并持Cargo锁执行，保留源码／WIP／日志／依赖下载／独立交付／数据库和卷。清理后复查实际空间，仍不足则保持停线。正常空间下继续复用增量缓存与原生并发，不恢复关闭增量、固定jobs或每批清空的规则。下文“一次性”及“仅清废弃”描述为此前授权和策略的历史时点，以本条和项目规范为准。用户随后明确“现在恢复开发”，当前继续执行；本文下方暂停描述为清理当时的状态，以当前交接的最新指令为准。

## 用户授权的一次性完整产物清理（2026-11-21）

用户先要求保持停线处理磁盘，随后明确要求“直接清理干净本项目所有关联编译产物腾出空间”，本次授权覆盖此前保留复用缓存的默认策略。此为本次清理，不恢复全局关闭增量、固定并发或每批删除缓存的已撤回规则。

清理根应用、core-ecosystem与extensions三处Cargo target内的全部构建产物及八处隔离指南dist，含联合站点中的生成副本；两个独立api-delivery、源码、日志、依赖下载和全部数据库／数据卷保留。已先通知扩展对话保持暂停，核对无Cargo／rustc／文档构建进程及lsof占用，取得三处debug/.cargo-lock排他锁后执行；锁文件及inode保留，避免其他Cargo通过重建锁文件进入。未停用IDE或修改构建配置。

清理窗口Available从31,607,232至84,386,244KiB，实测净释放54.046GB（50.334GiB），恢复可用86.412GB（80.477GiB）。目录清单与实际读数见 `/private/tmp/openrust-cleanup-builds-result-20261121.json`。复查三处target只余锁文件、八处dist已不存在，Docker卷列表与清理前一致（删除0卷），清理前后受跟踪改动文件清单一致。未启动构建／测试，不将删除缓存算作功能验收；下一次构建需要重新生成产物。按用户选择保持开发暂停，等待恢复指令。

## 用户纠正后的现行策略

用户指出上述限制损害编译效率。确认先前处置将“空间占用大”直接等同于“应关闭增量、降低并发、每批删缓存”，缺乏编译耗时与复用收益评估；这是不合理的取舍。正常IDE增量缓存不是配置缺陷，真正需要治理的是已废弃副本和遗漏的生命周期收尾。

撤回146aff771引入的项目.cargo/config.toml、定制build_session入口及配套测试，取消固定jobs3、全项目强制串行、8 GiB根缓存拒绝和每批退出清空target。AGENTS／规范改为保留日常增量与正常并发，独立源码验收按工作流复用独占target；清理前核对用途与占用，只删除已废弃产物。32 GB可用空间停线及数据库保护保留。无需引入新的清理依赖或缓存管理器。

上轮删除的缓存不能原地恢复，后续正常编译会重建；本轮不再清理现有IDE／其他任务的缓存。下文保留最初排查、回收量与旧入口测试证据，属于历史处置，不能继续作为现行执行规则。Cargo原生默认和复用机制仍参考下列官方资料，以Rust1.94实际检查验证恢复行为。

### 纠正验收

证据目录 `/private/tmp/openrust-cache-correction-20261121-wchpk8z3/`。从主工作区调用Rust1.94的Cargo，manifest与target均为独立无依赖小项目；清除探针环境中的增量／jobs覆盖，读取恢复后的项目配置：

- initial.log：原生dev profile生成真实incremental参数和增量产物。
- unchanged.log：同一源码和target再次检查为Fresh，没有调用rustc。
- changed.log：修改源码后使用同一个target增量编译通过。
- result.json记录三次实际结果；只回收探针自身的一次性target，现有IDE和其他任务缓存未删除。不把小项目行为验证宣称为完整项目耗时提升测量，也未再次全量编译框架。

删除旧入口的引用检查及git diff --check通过；业务源码、依赖和其他协作者改动保留。

## 原始处置记录（相关限制已撤回）

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


## API导出脚本残留策略清理（2026-11-21）

复查发现 `docs/scripts/build_api.py` 仍强制 `CARGO_INCREMENTAL=0` 且默认 `CARGO_BUILD_JOBS=3`，与用户已撤回的全局限制不符。仅移除这两项环境覆盖；独占target、Rust1.94、主机目标、锁文件、源码指纹、严格文档链接和清除过时Rustdoc页面的原有契约保留。

`python3 -B docs/tests/check_api_cli.py` 七项真实CLI拒绝路径通过。另在 `/private/tmp/openrust-api-cargo-policy-20261121/source` 建立无依赖、已提交的独立小crate，用真实Rust1.94执行两次完整导出，透明Cargo包装器记录传入环境：第一轮未设置incremental／jobs时，两项均保持缺省；第二轮调用者显式设置incremental=1／jobs=2时，两项均原样保留，两份API导出均成功。证据为该目录的 `native.log`／`explicit.log` 及两份 `*-environment.jsonl`；不拿小项目结果替代最终全workspace API快照验收。未改项目Cargo配置、工具链或依赖，不关闭应用／IDE，不清理正常复用缓存。

## 用户再次授权清空编译产物（2026-11-21）

用户明确要求清理三个大型target及本项目全部编译产物。已协调Activitylog／Telescope、Horizon及Pennant停止构建，取得六个Cargo锁并保留原inode，清空五处target及三处文档dist；保留源码、未提交改动、日志、依赖下载、独立交付文件、数据库及数据卷。清理前后df Available分别为61,328,464／112,306,812KiB，实测净增50,978,348KiB（48.617GiB），清理后可用115.002GB（107.104GiB）。清理前空间已受其他活动影响回升，因此不将本轮开始至结束全部变化算作本项目释放量。

Pennant、core-ecosystem及API小crate目录已同盘迁入项目tmp/同名目录；全部现行工作流使用项目内路径，不保留外部写入软链。最终递归复查项目及/private/tmp/openrust-*，未发现剩余target编译文件（仅保留锁文件），未发现剩余VitePress dist/cache。正常空间仍沿用Cargo增量与并发；此次清理不恢复已撤回的每批强制冷构建规则。

## 扩展验收触发32GB自动清理（2026-11-21）

Pulse原生配置与默认骨架3项测试已通过后，df Available降至30,331,816KiB，触发既定32GB自动清理。核对无本项目Cargo／rustc／rustdoc或文档构建进程、lsof无目标目录占用后，持有五处target中的六个Cargo排他锁，清空全部编译产物及三处指南dist，保留锁文件原inode。源码、WIP、日志、依赖下载、独立交付、数据库与数据卷均保留；Docker卷列表前后完全相同。

清理窗口实测可用31,061,225,472→122,468,278,272字节，净增91,407,052,800字节；清理后122.468GB（114.057GiB）。记录与安全脚本位于项目tmp/openrust-pulse-20261121/cleanup-result.json及cleanup_builds.py。macOS同一探针句柄同时持有flock与lockf也会冲突，已实测确认并仅使用Cargo兼容flock，不将该探针冲突误记为外部锁占用。用户仍授权持续开发，空间恢复后接续验收；未联系其他对话。
