# 活跃待办

> 当前代码与目标：业务示例已迁至 `modules/example/` 并经模块清单接入；R1-E CRUD/能力门禁已由 bc3fa01 提交；附件 PG/MySQL 全量各 15/15，真实 MinIO CopyObject 延迟响应时的双库删除竞争各 1/1；deadline/EOF 原子仲裁后，远端结果不确定的迟到发布仍属未保证边界。OIDC 确认页已完成 Chromium/mock 分支验收，真实 Keycloak/双库 `AlwaysOk` 错误密码、已消费 state 与成功夹证各 1/1；Collector PostgreSQL 隔离 sink 的 delivery/unavailable 各 1/1 且保留 16 MiB 限制，runner 路径和 fixture run ID 已修正。R2-N 已由 48be854 提交；R3-A/B/C/D/E/F 已按适用范围验收，C04-A/B1/B2/C已按子簇验收，后继C04-D Session/flash与Web栈契约待领取。

> 受众：AI Agent
> 更新：2026-10-29
> 摘要：Laravel + laravel-modules 的 Rust 1:1 实现唯一活跃队列；R0旧基线已记录，模块包逐项盘点待补；R1-S、R2-A至R2-N及 R1-E 真实 CRUD/当前公开能力示例已分别交付；R1-E 提交 bc3fa01。R2-N 由 48be854 提交；R3-A 语义 HTTP 状态默认已按子簇交付，继续 R3 HTTP/Web 路由、请求、资源与异常行为矩阵。远端结果不确定后的迟到发布边界保留。

## 领取规则

先核对 Git/WIP、代理/进程、[执行校准](plan.md#执行校准) 与 [ADR-070](decisions/adr-070-laravel-first-rebuild.md)。按总控依赖图领取；契约明确、前置满足的独立功能簇可在本地任务分支/worktree并行，各自完成 Spec → Implement → Verify → Commit，main集成与组合门禁串行。整体依赖见 [R0–R16](plans/laravel-rebuild.md#5-按依赖实施)，不按旧 M5/M6 顺序执行。

## 从零重建与并行执行授权

用户再次明确按 Laravel + laravel-modules 的完整 Rust 实现从零设计、重写核心；旧 API、目录和实现不构成兼容约束。新内核以固定上游行为为依据，不继续堆叠旧 Plugin 装配；旧源码仅作事实与回归输入，由对应功能簇替换后通过 git rm 清除失效实现。此授权不删除他人 WIP、共享服务、数据或原始测试证据，也不要求重造成熟密码学和数据库驱动。

R2-M 模块闭环、R1-E 综合示例与 R2-N 模块依赖顺序均已在 main 限定提交；当前在途主线为R3 HTTP/Web后继，现有WIP由原负责人闭环；总控可从唯一队列另选前置已满足的独立簇，在本地分支/worktree并行。共享Cargo清单、锁文件、验证服务和main集成串行协调；各任务登记负责人/依赖/分支/目录/验收/清理状态，细则见[并行开发与集成](conventions.md#并行开发与集成)；子代理不得递归派发。

## 当前可领取

当前执行优先级（恢复完整计划）：①R03/typed/CC/Signed四簇五父main限定组合已交付 `e105dbf`，来源与分支已安全退役，证据见[四簇记录](memory/testing.md#r2-四簇-main-组合限定验收)；②R2-P `4ac8099` 与S-DV `6ffb5c8` 已由三父main提交 `9e5a94198b421987851b3580b27fa90370feeb77` 完成限定组合门禁：默认943/0/34忽略、全特性1748/0/108忽略，双严格Clippy/docs、MSRV、no-default、deny、consumer/生成/MySQL/TCP按[最终记录](memory/testing.md#r2-p-s-dv-main-limited)通过。S-DV与P工作区/分支已安全退役；P两份root文档事实已迁入main，原稿与迁移核对回执保全于 `/tmp/openrust-provider-root-docs-wip-preserved/`，正常退役回执 `/tmp/openrust-provider-registration-safe-retirement.json`。③ST-V来源`2262df9`已由两父提交`8d18021b630ea10652316e025ef6019c7772bbb0`完成main限定组合验收，来源工作区/分支已安全退役；真实S3 ACL、迟到CopyObject、`throw=false`仍开放，见[本簇记录](memory/testing.md#st-v-main-限定组合验收)。本轮用户已明确恢复完整计划，覆盖此前仅收尾限制；当前下方C04-F Request文件家族已完成任务分支限定验证，待main组合验收。两guard worktree受保护。完整Config、Provider发现/Facade、Session/Web、签名URL复杂参数/代理、动态Settings和发布全后端矩阵仍开放。

<a id="c04-f-request-files"></a>
### C04-F Request文件家族（分支限定验收通过，main待集成）

- 当前续作（收口）：MIME 家族21文件冻结v7，608构建输入零漂移；15代码门禁通过，默认workspace955/34忽略、全特性1770/111忽略，双严格Clippy/docs/MSRV/no-default/deny通过。services九阶段fresh生成/真实TCP/MySQL链全部exit0，回执`/tmp/openrust-request-mime-services-prepare/run-20261101T080437Z/receipt.json`；生成test_app8/TCP1、Request fixture1、真实MySQL模块1均通过，无新增库残留。原owner `/root/config_runtime_callers`正汇总最终证据，services owner `/root/request_files_services_prepare`已完成本轮target清理：30,115,557,376B→0，磁盘16.968%→19.971%，未知外置65,982,464B保留，回执`/tmp/openrust-request-mime-final-cleanup-retry/receipt.json`；源码保持冻结，待限定任务分支提交；不冒称已合main或完整Request对应完成。受控gate已修复Duration::ZERO测试竞争；原失败保留。首Request历史默认数纠正为945/34（原922漏计CLI父23），旧全特性1760/111不变。守护独立修复已合main `31a6d38`，plain重载PID71755，原Goal active、working/task_running及三项存活/心跳均true；默认0.8/baseline1793413935.123801、账号历史、已结束grant保留，38问答ID无丢失，四行因正常复核变化。

- 已创建限定任务分支本地提交`2f901272ec6fcc8cdd7d3a2b4ec6624a7445aaaf`（43文件），提交信息校验与工作区干净检查通过。源码未合入main；保留任务工作区等待最终验收，暂不退役分支。

- 沿用原完整Goal、默认80%可用/20%保留和原baseline；不新建Goal/守护，不启动冻结产品，不push/tag/发布。
- 同仓任务位置：OpenRust项目`prj_34a40ee7b674b2b6`，workspace `wks_748133ad70032a5e`，branch `tasks/request-files-family`，base `a9b057e78f35c42955dd99401526a5d1eda60ba1`，cwd `/Users/Home/.paseo/worktrees/02joa2lr/openrust-request-files-family`。原核心owner `/root/config_runtime_callers`，当前生成/真实服务owner `/root/request_files_services_prepare`；root独占索引、依赖/锁、共享文档和提交。
- 范围：唯一Request管线的multipart、递归文件树、DataValue身份、UploadedFile独立reader/暂存与取消清理、store字符串路径、闪存/Redirect公开链路，贯通唯一模块、consumer、生成模板和文档。Spec `/tmp/openrust-request-files-family-spec.md`；固定Laravel/Symfony实际行为优先，普通选择自主，不造独立上传内核。
- 流式契约：网络正文读一次，沿原Axum默认/覆盖/禁用限额；小正文内存、越阈值受管暂存，multipart逐chunk写入。异步`content()`受管reader与`content_bytes(max)`显式投影，原文32MiB/结构化8MiB额外预算属可配置Rust资源策略，不能冒称Laravel默认。writer/在途open/read均持文件与路径owner，取消后小ReadBuf续读保持缓存/offset。
- 已核代码门禁：Request lib41、http_input38、公开consumer Session2、CLI23均通过；默认workspace945通过/34忽略，全特性1760通过/111忽略，双严格Clippy/docs、no-default、Rust1.97和deny均通过。CLI子进程摘要已去重；无服务环境的忽略/跳过不算真实后端。root日志SHA审计`/tmp/openrust-request-files-code-gates-root-audit.json`、`/tmp/openrust-request-files-all-features-root-audit.json`；原失败记录保留，测试数量不是整体完成率。
- 最终808冻结`/tmp/openrust-request-files-generated-json-freeze.json`，tree `e8ff0cac0e66bfa04a92ea5aaa165fce307904cf`，36候选文件；全部SHA无漂移。相对全特性已过输入仅模块multipart用例失败清理与模板/快照三路径变化，生产`src/`、manifest、locks不变；受影响server测试严格Clippy、CLI/生成/MySQL已补验。证据索引`/tmp/openrust-request-files-final-evidence-index.json`。
- 最终真实链已通过：CLI主23、全新生成应用check/test_app8+TCP1、Request精确1/TCP精确1、唯一模块附件真实MySQL精确1且无skip；同MySQL server UUID和schema集合前后相等，进程组无残留。最终wrapper`/tmp/openrust-request-files-services-third-20261101T042452Z/receipt.json`、driver`/tmp/openrust-request-files-services-prepare/run-20261101T042456Z/receipt.json`；旧模板JSON失败和首次临时fixture编译失败原证据保留，不能改写为原命令通过。生成源码已由成功driver清理，不能现时重算原字节。
- 依赖root已统一：Axum官方multipart、基础tempfile仅新增multer3.1.0；http-body-util提升基础direct。12 consumer实际native metadata校准，11锁仅补该direct边，全部既有包版本集合不变；证据`/tmp/openrust-request-files-consumer-direct-lock-sync/receipt.json`。最初离线wasm解析与未缓存平台依赖失败均保留，不能冒称成功。
- 资源：本轮已完成归属/进程/打开句柄核查与正常Cargo清理，根target实测29,937,872,896B→0，磁盘可用167,465,619,456B→197,155,180,544B（19.821%）；回执`/tmp/openrust-request-files-final-cleanup-retry/receipt.json`。首轮因缺缓存标记安全中止的回执保留；确认归属后exclusive create标准CACHEDIR.TAG，dry-run及正常clean均exit0；全项目50GB软/100GB硬与磁盘10%软/5%硬保持。后续仅经root核归属/占用后清可再生产物，保未知外置缓存、guard工作区、服务和数据卷。
- 开放边界：任务分支限定交付，尚未集成main；完整Symfony MIME、真实S3 ACL/迟到CopyObject、Storage `throw=false`、Windows原生取消清理及Laravel完整Request/Filesystem能力继续未完成，不因本簇限定验证关闭。

### 上轮收尾执行窗口（已完成）

- main Provider/S-DV源码与集中验证已收口，本轮仅同步Agent事实文档；ST-V main门禁、真实尾链和确认归属的产物清理均已结束；旧owner与分支已安全退役，后续新阶段仍须独立预检资源。
- ST-V默认945/34 ignored与全特性1756/111 ignored、双严格Clippy/docs、MSRV、deny、公开consumer及真实MySQL/生成/TCP已有本次main证据；真实S3 ACL、迟到CopyObject与`throw=false`继续开放。50GB/10%软与100GB/5%硬阈值、未知缓存归属保护不变。
- 本次清理回执`/tmp/openrust-stv-main-cleanup/receipt.json`：根target已正常`cargo clean` exit0，登记3个现存target合计67,485,491,200B→65,982,464B，磁盘可用126,622,199,808B→193,684,373,504B。小生成源码与166个文件哈希保留，外置65,982,464B归属不足未删；两guard工作区和服务/数据库卷未碰。

### 本轮组合产物回收（已完成）

- 四簇main验收与本地合并后，资源owner `04cca925-e59f-42f1-800b-424e7b4cd1b8` 已核占用并正常执行 `cargo clean`（exit 0，8.89秒）。本轮清理前后根target实测55,298,695,168B→0B，已登记target合计55,494,791,168B→65,982,464B，磁盘可用139,102,576,640B→193,909,436,416B；不同时间采样不与上方验收收口样本混算。外置db-plugin-consumer的65,982,464B归属不足且无CACHEDIR.TAG，未删；源码/WIP/证据哈希前后一致。回执 `/tmp/openrust-four-family-cache-cleanup/RESULT.json` 与 `clean.log`；当时S-DV owner `7611b92b` 获独占根target重建窗口，此为历史采样，当前资源归属见上方收尾执行窗口。

### 集中产物回收（已完成，保留原始日志缺口）

- 资源owner `04cca925-e59f-42f1-800b-424e7b4cd1b8` 独占清理窗口，driver先保存；root已通知S-DV/ST-V/driver禁止新增构建，源码修复与caller迁移继续。root当次核cargo/rustc/rustdoc均无进程；main与两历史target顶层为标准Cargo结构、非symlink。owner已按锁/占用/归属复核后集中清理，非仅几百MB精准删。
- 清单基于既有resource_paths、main/worktree/consumer与外置目标去重；共享main target获准完整清后按需重建，未知root-site/M5集中核一次来源后明确清/留理由，不盲删、不无限搁置。保护未合分支源码/WIP、依赖源码、PHP来源、数据库/卷与证据。现已完整清13个确认OpenRust target（含主target、root-site/M5及consumer），逐项实测共93,855,621,120B产物归零，登记剩余Cargo缓存65,982,464B；磁盘可用约101.17→193.60GB。PHP参考源码/证据244.76MB单列保留，不算Cargo cache。回执 `/tmp/openrust-concentrated-cache-retirement/RESULT.json`；root实核13路径已无、caller冻结35文件无漂移。auth-pg一项第二轮同basename覆盖两份原始日志，原预期SHA/退出码/字节记录保留且明确不可恢复；其余24份日志SHA核对一致。原失败ROOT-AUDIT及RESULT.log_integrity_audit保留，不改expected hash或重跑伪造历史。
- 下一唯一构建目录 `/Users/Home/Codes/Rust/openrust/target`；该S-DV重建窗口属历史阶段，现S-DV已完成main组合并安全退役；当前target归属以本页收尾窗口和ST-V owner实际交接为准。跨簇只留下一已安排步骤明确需要的一份缓存，不为历史feature持续堆积；50GB软/100GB硬不变。

执行节奏：先完成本簇代码及配套测试代码，ready_for_verify后由总控安排提交前集中验证；分支限定门禁、main完整组合验收，未通过不提交。不得继续按每个方法/小改动重复跑测试或Clippy；当前在途命令自然结束，已有有效证据按实际影响复用，规则见[集中验证](conventions.md#功能簇提交前集中验证)。

持续提速约束：每次worker交接、门禁结束或阻塞变化时，总控按e25ec75核对关键路径、下一独立簇是否实际编码、重复审查/构建和逐阶段停派、资源归属及释放条件；发现可避免等待立即调整，用现有日志记录前后事实，并向架构协调会话发送一次含瓶颈、调整、下一动作/owner及证据位置的简短事件回执。无变化不发、不高频轮询，不新建计划/Goal/守护；默认20%额度保留及真实授权边界不变。

| 功能簇 | 状态 | 交付与验收 |
| --- | --- | --- |
| R1-S 根结构与普通模块挂载 | 已完成结构子交付34dfb5e | 根Laravel职责、同package lib/server、资源内聚、模块公开挂载；按范围实际门禁与独立审查通过后可限定提交，仅结构子交付 |
| R2 基础：Container/Provider/模块配置/TestApp | R2-A/A2/A3、R2-B、R2-C、R2-D、R2-M 各子簇已验收 | 全体register后boot、统一Container与typed配置、最小TestApp及模块闭环；先形成真实CRUD/扩展接入的稳定API，不依赖完整R1-E |
| R2-M 模块最小闭环与目录迁移 | 真实服务与生成项目已验收，本簇提交 | modules/ 声明注册、模块配置/资源/统一迁移、依赖与冲突错误；示例迁至 modules/example，真实HTTP及双库验证；完整管理命令归R13 |
| R1-E 真实CRUD与当前能力示例 | 已验收并提交 bc3fa01 | 真实 Article CRUD；当前公开能力进入唯一模块示例；PostgreSQL 18/MySQL 8.4、生成项目、真实服务与整组门禁已验收。Collector PostgreSQL 隔离 sink delivery/unavailable 已复验通过，详见 testing |
| R2 后继内核与测试底座 | R2-N 模块依赖图与顺序已完成 | `.requires` 硬依赖、稳定拓扑顺序、register/boot 一致；真实 PostgreSQL/MySQL HTTP、生成项目、根无参数 TCP、工作区门禁均有本簇证据 |
| R3 HTTP 与 Web | R3-F 已提交163bb49；C04-A 已提交08be69d；C04-B1输入基础已验收；C04-B2类型便捷方法已通过适用验收；C04-C Cookie/响应子簇已验收；C04-D1子簇适用验收通过；C04-D2基础Store API子范围已验收，扩展生命周期后继 | 按ADR-078实现无正文请求检查与固定上游协商；C03其余及完整C04范围继续保留 |
| 模块包固定范围细化 | 待逐项盘点 | 对正式laravel-modules固定源码/文档的导航、源码与测试拆行为矩阵，分类必要Rust差异/用户约定/缺口；不把目录规范或显式Plugin等同完整模块管理 |

## 后继路由

- **高价值测试精简与入口整合（已授权，T01已验收）**：T01已由 `b7afcfe` 提交，extensions15例纳入既有http_session26例、全部41项逐名保全，显式入口46→45、Session family8→7，环境子进程/DB/Redis等隔离入口保留。当前 `tests/http/session.rs` 已公开挂载私有扩展模块，不能再按旧只读审计重复实施；实际双feature、完整门禁及计时限制见[验收记录](memory/testing.md#t01-session纯行为测试入口整合限定试点已验收提交见git)，单样本未证明整体开发显著提速。后继只按[测试分层与入口整合](conventions.md#测试分层与入口整合)核独有缺陷、真实编译入口、feature及全局状态后领取；不按数量配额删、不预报倍数、不重排在途冻结测试。

- **宏观主线**：[两阶段框架主线](plans/laravel-rebuild.md#两阶段框架主线)；先验阶段一Laravel+modules核心基线，再做已纳入阶段二通用增强，均是框架main而非产品可选快速开发模块。R0–R16仅技术分解；现增强保留复用，不搬走核心Config/Queue/Scheduler/安全，当前B2不变，发布整理须两阶段既定范围验收。

- **可选模块边界**：原V2版本阶段取消，后台/App需求转入[可选快速开发模块](plans/optional-rapid-development-modules.md#开发与使用边界)，暂不开发，待用户明确开工。R14、两个框架阶段或持续Goal“全部完成”不能自动启动；main只推进框架/唯一示例，不清理现有WIP，不为暂不开发业务模块预建分支或默认合并；已授权框架任务分支按并行规范执行。框架配置管理、Queue/Scheduler及官方扩展继续；官方工具自身界面属于对应可选包，不受业务后台冻结限制。

- 阶段一R2公共声明/覆盖/错误契约→R13包发现与生成工具：[Provider编译期发现Spec](plans/laravel-rebuild.md#r2r13-provider-编译期发现-spec)（L044.02/模块M016），复用R2-M/N清单、拓扑和register→boot；仅规划未实现，不抢占当前HTTP。
  - 当前前置只读Spec：复用已交付Q会话 `ea706795-768f-43c9-9a56-33b38c0abfac`，只核公开声明/退出/覆盖/错误与现ModuleManifest接线，无源码/构建/新分支变更；候选Spec已交，auto/explicit同Provider重复是否应去重需补核固定Application::register/force与RegisterProviders顺序，不能把上游正常重复误判冲突；root冻结后才分配实施，不能把显式清单冒称自动发现已完成。 后续重复注册契约与真实PHP证据已收：`/tmp/openrust-provider-discovery-registration-contract.md`、`/tmp/openrust-provider-registration-oracle/receipt.json`；普通重复首次实例、force启动前/后与dont-discover不阻止显式均由真方法实跑并重放一致，configured三段顺序仍仅源码证据。自动发现仍未实现；普通/force生命周期已由R2-P `4ac8099`及main组合`9e5a941`限定交付，见[当前core事实](core/core.md)，不能继续把该历史探针结论当作force待实现。

- 阶段一R2容器Binding历史缺口（矩阵L012.02）：保留原factory的公开`extend`及作用域/实例装饰已由R2-X `d299ca4`与Q/X main组合限定验收，并在当前core事实记录；不再按旧D3结论重复领取。完整Binding/Container其余上游对应仍按矩阵逐项闭合，不由已完成子范围外推。

- 阶段二R11 MQTT按[总计划可选扩展契约](plans/laravel-rebuild.md#r11-明确待办mqtt-可选服务端与客户端)排期：client/Broker按需独立引入，沿既有Provider加载，client操作/订阅CLI/Broker CLI才产生网络副作用；仅规划，不抢占当前HTTP簇。

R1-S → R2基础 → R2-M模块闭环 → R1-E当前CRUD/能力贯通；R2后继及R3 HTTP → R4 数据 → R5 Model → R6 Factory/SQLite → R7 安全 → R8 Cache/Storage → R9 视图/通信 → R10 异步/事务 → R11 网络/组合 → R12 搜索/特性/观测 → R13 开发体验 ；阶段一核心门禁后，R16官方框架扩展及工具自身界面按各自前置在阶段二分批领取，最终证据汇入R14；原R15版本阶段取消，业务后台/App转为可选模块并暂不开发，官方工具自身UI随包阶段二交付，待用户明确开工。每个后继能力同簇补示例；R1-E不等待未来全框架完成，R2基础不等待完整示例，消除循环依赖。阶段内部按总计划拆可验证提交。

## 必须带入新矩阵的现有缺口

- 统一返回格式等已有优秀机制 → R0 登记对应 Laravel 职责与保留价值，R3/R5 在 Response / Resource / 异常渲染融合并回归，见[融合计划](plans/laravel-rebuild.md#已有优秀机制的融合)；重写不等于删除能力。
- 原 M5 可观测性/资源独立审查与完整生产门禁 → R12/R14。
- 原 DX1–DX8、F9/F10：默认 cargo run 与 Cargo 正常兼容设计、TestApp/assertions、factory、命令/生成/stubs、只读 doctor、dev 编排、文档和更新体验 → R1/R2/R6/R13。
- 双库负载均衡、多 API/worker/scheduler、共享状态/文件/实时、滚动升级/依赖故障、最低版本与性能 → R14。
- [SQLite 完整支持](plans/v1-sqlite-backend.md) → R6 及各能力适用验收；不能只接通连接。
- [可选快速开发模块 后台/App](plans/optional-rapid-development-modules.md) → R15；PC 仅规划；独立 AI 产品未启动。

## 外部条件与证据缺口

Twilio 实发缺授权账号/凭据/目标；其他供应商与生态包实测条件在 R0 盘点。遇到外部缺口继续无依赖工作，但不能标记真实发送、支付、完整生态或全部验收完成。

## 当前 R1-S 结构子簇 Spec

1. 框架与应用保持同仓、main集成；独立任务可由总控分配本地分支/worktree；根 package 为 openrust，src/lib.rs 提供框架，薄 src/main.rs 提供默认 server。
2. default-members=["."]、default-run="server"；根无参数 cargo run 启动唯一应用，不需要选择 package/bin。
3. app/bootstrap/config/database/public/resources/routes/storage/tests 直接位于根，根工作目录自然加载配置和资源。
4. 当前结构子簇保全 app/example 代码和测试；最终唯一示例为 modules/example，随 R2-M 模块契约迁移并由根 server 统一装配，不再次独立做一轮仅改名的搬迁。
5. 业务集中在 modules/；其余目录均为框架通用结构/代码；示例 migration/seed、模板、proto/生成脚本及测试 fixture 留模块内，根只提供通用结构和公开挂载；根不得有示例 build.rs、build 依赖或样例业务特判。
6. 工具与公开 consumers 保持其编译/验收职责；CLI new 是框架能力，仅在 /tmp 验证用户应用，不新建长期源码仓库。
7. ADR-074 替代 ADR-073 的模块位置目标，其他有效根结构边界保留；保留旧布局非界面真实证据，不改 Git 历史，不当作本次通过。
8. 同步全部 README、当前计划、资产目标、CI 与模板路径；不改变已确认的底层框架语义。
9. 实际执行 fmt、default/all-features workspace clippy/test/doc、deny、MSRV/no-default、公开 consumer 与适用双库门禁。
10. R1-S盘点并保留R1-E当前公开能力示例缺口，不把完整CRUD/全能力示例作为结构子簇退场条件；代码/消费者存在不代表示例已覆盖。
11. 根默认 cargo run 与 HTTP、唯一综合示例行为、三类生成应用 check/test/HTTP 须重新验证，缺外部条件明确保留。
12. 结构门禁/审查通过可限定本地提交并结束R1-S；R1整体及R1-E真实CRUD/当前能力示例不关闭，下一主簇进入R2基础再回来补齐，不推送/发布/tag。

## 扩展示例后续接入

- 原 `app/example/extensions.rs` 已随示例迁至 `modules/example/extensions.rs`；其扩展参考代码仍不代表所有扩展能力已在业务场景验收，后继 R1-E 按公开能力逐项接入。
- 归 R2-M/R1-E：统一 Provider 与模块契约稳定后，由示例负责人改写所需实现，在 `modules/example/` 内挂载、配置并验证后提交；旧 Plugin 示例不作为目标接口，代码与测试按有效需求取舍。

## 综合示例完成边界

- 硬交付一个真实业务实体的列表/详情/新增/更新/删除，包含校验、分页、未找到、授权、事务，并在 PostgreSQL/MySQL 各执行真实 HTTP 闭环；/api/example 常量响应不算 CRUD。

- 当前所有已实现的框架公开能力都应有 modules/example 业务调用或公开扩展实现，并映射根装配、配置/迁移/资源与可执行测试；逐项范围以根 src/lib.rs、Cargo features 与 core 事实为准。
- 示例业务、公开扩展及专属资源/协议/生成脚本/测试 fixture 全在 modules/example；其余目录只维护框架通用结构和加载/挂载，不放样例实现或专属 build 依赖；不复制整套框架骨架。
- 当前基础路由/信息响应没有完整业务实体迁移与CRUD；多项实际调用/扩展/测试仍缺。结构子提交不关闭R1整体；R1-E在R2基础后继续，不用旧测试或命名接近冒充1:1。
- 后继功能簇完成定义同时要求：公开框架行为、综合示例接入、通用根挂载/模块专属配置资源与行为测试同步；未实现能力继续保留后继阶段，不宣称全框架等价。

## 模块扩展后继路由

- R2：统一 Container/ServiceProvider、register/boot、模块依赖与装配顺序，迁移实际 ExampleServiceProvider 与应用注册清单；复用当前薄 Plugin，不保留双内核。
- R13：模块生成、启停/管理、依赖诊断及开发体验的目标与验收；当前显式静态声明/注册只证明基础挂载，不能当作自动发现或管理已实现。
- laravel-modules是用户指定正式模块化实现范围；独立来源/冻结版本与待盘点行为见[模块矩阵](plans/laravel-modules-matrix.md)，同一Provider/Container落地，不无限增加其他产品。

## R2 配置能力审计输入（审计历史；typed 子范围已交付）

- 已对照 Goravel 官方源码和现有 ConfigLoader/section<T> 完成只读审计；具名连接/store与类型化读取是已有能力，不能重复实现。
- 审计范围包括完整领域配置说明、env 映射、typed 读取/启动校验、模块配置的公开注册与覆盖；先说明现有能力、真实缺口和依赖，不凭空叠加第二配置层或改变 env 语义。
- 本项归 R2，依赖只读审计交付与统一 Container/ServiceProvider contract；R1 不混入配置实现改动，当前配置源、默认运行值和 Redis 默认无静默回退保持。 R2-T六类严格读取及`*_or`/`*_or_else`已在同项目任务分支限定提交`b1f4bf7`；仓储43/集合4/模块1/consumer6、严格Clippy3/docs1、fresh普通7+fixture1、真实TCP200/正常关闭及精确产物回收通过，分支证据不关闭完整L005.03；现已通过四簇main限定组合门禁。owner `8ca4236f-2c8c-4f7c-887b-8dab60f6e54e`，workspace `wks_44ece7d45e1cfb13`，branch `tasks/config-typed-helpers`，base `a5e6461`，cwd `/Users/Home/.paseo/worktrees/02joa2lr/openrust-config-typed-helpers`；源码与文档工作区干净、target窗口已释放，原worktree/branch已正常退役，workspace已归档。最终owner证据 `/tmp/openrust-config-typed-helpers-final-owner-receipt.json`、root核证 `/tmp/openrust-config-typed-helpers-root-final-audit.json`；聚合登记56.83GB/磁盘可用86.95GB均仅软告警，100GB/5%硬线保持；未知缓存未清。Provider普通/force注册后继真实PHP与Rust候选已交`/tmp/openrust-provider-registration-rust-contract.md`，不同模型审查在途，未实现。

## ST-V Storage visibility 家族（main限定组合已交付，完整Storage开放）

- 来源`2262df9`由两父提交`8d18021b630ea10652316e025ef6019c7772bbb0`合入main，来源工作区/分支已按`/tmp/openrust-storage-visibility-safe-retirement.json`安全清理；两guard工作区未动。冻结Spec仍见`/tmp/openrust-storage-visibility-frozen-spec.md`，分支旧失败与修复证据保留于`/tmp/openrust-storage-visibility-attempt{5,6}-manifest.json`，不改写为首次通过。
- 本次805输入/33日志SHA与独立核证见[ST-V main记录](memory/testing.md#st-v-main-限定组合验收)：双严格Clippy、默认/全特性workspace测试、双docs、no-default、Rust1.97双feature、当前锁deny、公开consumer、唯一模块真实MySQL、生成项目与根TCP均按限定范围通过。ignored不算实服务。
- 完整Request文件输入、MIME判定、Storage/Filesystem 1:1继续在后继队列；真实S3 ACL启用/禁用bucket、远端迟到CopyObject与Laravel `throw=false`失败值语义尚无等价服务证据，不能从本次local/TCP或被忽略S3用例推断完成。

## S-DV 有序对象值与 Session 保存边界（main限定组合已交付）

以下 owner、冻结与失败描述保存分支执行过程，不代表现时运行窗口；最终main状态见本节末尾及顶部组合记录。

- Request 文件家族的实际前置；固定 Laravel 真探针14案表明 Request flashOnly/Except 保留对象到 save，Redirect 则递归剔除，不能在 put/flash 时提前转JSON。冻结Spec `/tmp/openrust-session-data-values-frozen-spec.md`，oracle `/tmp/openrust-request-files-flash-oracle/receipt.json`；只推进通用值/Session保存边界，multipart/DefaultBodyLimit/Storage visibility与MIME继续后继。
- owner `7611b92b-f279-4a2f-ab5f-d134c2263926`，workspace `wks_1735f33899c42ec3`（既有OpenRust项目），base `9eb54ce02a09b48db31c31b57384976628da4050`，branch `tasks/session-data-values`，cwd `/Users/Home/.paseo/worktrees/02joa2lr/openrust-session-data-values`；common-dir已核同仓。当时正式run状态与模型记录为历史；源工作区/分支现已安全退役，不重复本地不可用执行器探测。
- 当前公共签名补充冻结 `/tmp/openrust-session-data-values-canonical-api.md`：canonical get/all/old_input等返回原始DataValue，JSON投影显式，禁止保旧JSON get再只靠get_value读对象。core owner独占src/session、值类型、Session配置/就地unit；复用native `config_runtime_callers` 已实际派发同worktree其余调用方/唯一模块/外部测试/consumer/CLI模板快照/说明，保全core已写caller WIP接续，不重叠编辑。manifest/lock/共享Agentdocs及main索引root独占；其后已完成不同模型独审与main集中Verify；当时默认编译子范围不代表最终行为，最终真实服务证据见顶部组合记录。
- 默认workspace check已实际exit0（140.593秒），前后50SHA无漂移；默认feature仅cache-redis，不证明Session gated源码通过。核心3并发/预算blocker、caller原对象读取及Cookie未排响应/old_or断言均已静态闭合，36 caller最终清单 `/tmp/openrust-session-data-values-callers-ready-final.json`。
- 严格Clippy已通过；lib首次557通过/2失败已修，第二次559通过/1失败，剩同句柄并发named errors误报busy。同步准入方案独审发现跨线程析构join死锁；已按真实PHP5案与回收顺序Spec完成state.rs整批修复：先释放Operation再析构候选/旧树/输入，成功回调写入保留，失败候选原子回滚，原并发与投影busy断言不弱化。当前仅静态通过，不宣称运行时修复；terra原reviewer已静态复审无blocker（`/tmp/openrust-session-data-values-retirement-independent-review.md`），unwind/返回值及真实I/O取消/poison保留后续验证缺口。原attempt3/4失败保留；attempt5第三次lib已563通过/0失败/0忽略，50输入无漂移；HTTP首轮5项configuration_error已定位并补Provider封闭schema serializer；attempt7严格Clippy通过、HTTP五target共82项全过、模块session_flows8项通过，consumer5项/CLI精确快照1项也通过，attempt7链全部通过。服务链在session+auth+db-mysql严格Clippy因account_security Density→DataValue漏迁移停止，生成/MySQL/TCP未运行；原caller已补公开put_typed写入，旧36caller无漂移，root核新增37th caller；新458输入attempt2服务driver已通过：普通生成check/test7项、MySQL生成check与真实auth2项无skip，状态passed_with_root_tcp_pending；根server新构建已过；首次TCP在启动前因freeze漏modules_statuses.json失败，未发业务请求；root补459输入后build成功，TCP第二次在ready前退出；静态定位runner误用未合R03的领域相对格式，第3次TCP六业务步骤全过且SIGINT exit0，但即时端口重绑失败，总结果仍failed；用户已明确授权先受控socket探针核重用选项再单次严格重验，受控probe已复现plain bind失败/reuseaddr成功并拒绝活监听，root差异复核候选不改六业务断言；此为候选解释非历史唯一根因。新真实buildreceipt绑定runtime-final459，用户获准单次TCP已passed6/6、SIGINT exit0、无残留、重绑1次0.558秒成功；原三失败保留。allfeatures check另暴露runtime.rs四标量调用已显式Into修复，旧37caller无漂移；allfeature第二次又定位database.rs三标量，caller连同Keycloak三处同批修完且其余36caller无漂移；新459最终feature freeze后第3次allfeature类型检查与严格Clippy已过，服务owner续组合lib/view/MySQL/条件Redis批次，失败停，TCP证据按生产未变范围复用，不重跑已过服务driver，core冻结。caller未降断言；schema与测试lint差异分别绑定新freeze。证据 `/tmp/openrust-session-data-values-attempt5-verify-manifest.json`。
- 分支最终459输入及运行失败历史保留于 `/tmp/openrust-session-data-values-final-feature-freeze.json` 等原证据；Session对象身份/native拒绝/JSON显式投影与caller迁移已在三父main提交 `9e5a94198b421987851b3580b27fa90370feeb77` 的组合门禁重新验证。原MySQL认证/迁移与上游超时失败及后续定向修复不回填旧回执；main最终默认943/全特性1748、真实MySQL/TCP与consumer/生成证据见[最终记录](memory/testing.md#r2-p-s-dv-main-limited)。源worktree/branch已安全退役（`/tmp/openrust-session-data-values-safe-retirement.json`），完整Session/Web仍开放。
- 资源：只在已清理的 `/Users/Home/Codes/Rust/openrust/target` 按需重建，jobs2；不占端口/库。构建前重测全登记缓存/预计增量，50GB软/100GB硬、磁盘10%软/5%硬；独立执行者受总控暂停/停止协调，原默认80%额度不变。完成后按真实归属清理，不动未合入分支。

## R2-CC 配置缓存家族（分支限定验收历史）

- 本分支复用唯一Application/Provider/ConfigSources/ConfigRepository实现config:cache与config:clear。首次命令引导保持正常缓存屏障，实际派发才清选定缓存；fresh启动、资源关闭、原子写/回读、失败无半写、双阶段env与模块屏障同簇验证，不热改在途App。
- 最终18文件 `/tmp/openrust-config-cache-command-bootstrap-script-delta2.json` 已独立复审。修后默认851、全feature1638/68ignored、双严格Clippy、配置77/生命周期18/模块1/CLI1、no-default/MSRV及普通生成8/两类真实进程脚本通过；ignored不算服务。首链驱动忽略规则断言失败及旧benchmark失败均保留；强制重编CLI的续跑与原失败分开索引。
- 修后双doctest/docs均通过，离线deny仅复用相同依赖图/策略输入且注明2026-10-30 advisory快照；根fmt/diff通过，限定本地提交见Git。root核61项证据及42生成文件。两任务temp已退役，未实际执行范围过宽的cargo clean，约1GiB同名包缓存留待root下次资源交接按精确归属复核；不误删其他产物。以上为CC分支阶段证据；CC已与R03/typed/Signed完成四簇main限定组合，P另与S-DV完成三父main限定组合，发布全后端矩阵保持开放。


## R2-P Provider 完整注册生命周期（main限定组合已交付）

以下96/97源、生成、资源与Clippy计数保存分支执行过程；当前main结果见顶部组合记录，P源分支仅待保全文档事实核对后安全退役。

- 已冻结普通/force与boot前后完整家族；固定Laravel真实正常/失败探针、Rust接口候选及独审在 `/tmp/openrust-provider-registration-{frozen-spec,rust-contract,rust-contract-review}.md`。不以启动前去重代替晚注册；异步register/原位置rebinding、绑定generation与缓存槽同锁准入为明确Rust实现约束；失败已发生贡献按PHP保留，不造Provider全局事务。
- 同项目workspace `wks_f7e111d2b503053b`，branch `tasks/provider-registration-family`，base `3aba0a3`，cwd `/Users/Home/.paseo/worktrees/02joa2lr/openrust-provider-registration-family`。`04cca925-e59f-42f1-800b-424e7b4cd1b8`负责provider/app/resources与配置owner/schema；`0b1cd54b-3828-4eff-bf97-5c519973a41e`仅负责container.rs及container子目录；既有callers负责其余Provider/模块/consumer/模板迁移，禁止重叠编辑。
- 共享API已收敛：ServiceProvider::register改BoxFuture；Context涉及绑定/回调的方法原位置await；rebinding在已绑定时立即解析并返回Option<Arc<T>>。configure_after_register改FnOnce(Container)→BoxFuture，原register后覆盖时点不变；启动前建立唯一Container/StartupResources，配置登记当场可读但保留boot前校验，默认合并不得重复应用。
- 当前冻结96个变更文件，生成验收另外绑定实际运行输入；core125、仓储47、公开Container契约24，以及独立container-extension2、test-app1、config读取consumer6均实际通过。Register重放覆盖后续set的缺陷已修并经不同模型静态复审。默认严格Clippy三次历史失败保留；用户授权单次重验已通过，源码限定分支提交 `4ac8099`；main三父组合提交 `9e5a94198b421987851b3580b27fa90370feeb77` 已通过限定门禁。root精确刷新3个consumer锁，新增包匹配根锁，无第三方升级。
- 生成首轮view遗漏await已修；第二轮确认共享target旧CLI复用，未误改正确异步模板。attempt3 `/tmp/openrust-r2p-generated-acceptance-attempt3/` 实际强制重编当前CLI并绑定97输入，13阶段通过：普通生成/Provider/TCP/公开fixture、MySQL认证2项无skip；root独立SQL审计前后61库集合摘要一致。前两轮失败保留。其后Cookie生产修复已用保留生成项目补验3项plain及2项真实MySQL auth具名测试，均通过/无skip，97源与生成锁无漂移、前后61库集合一致。`/tmp/openrust-r2p-cookie-generated-supplement/result.json` 与root五日志核验留证，不重跑未受影响生成/CLI/Marker TCP。
- Cookie当前节读取改has(session)，保留已解析singleton快照；固定PHP真实CookieServiceProvider探针确认早解析不热建，新增身份回归后cookie9、默认workspace lib/tests862、全feature1650/68ignored、doctest22/34ignored及24/40ignored、双warnings-denied docs、no-default/MSRV/deny均有新回执。`/tmp/openrust-r2p-cookie-matrix-handoff.json` 关联10日志，root逐哈希核对全匹配；ignored不计真实服务，严格Clippy按用户授权单次重验通过；本行数字仍只属分支证据，main新结果见顶部组合记录。
- 资源补验中的unregistered_count79已确证是对unknown字符串按字符计数，不是79target，原result保留且resource-audit.md纠正；限定main/worktree/tmp扫描无新增target，登记15路径约94.30GB仍为覆盖有界的小计。P补验已收尾8个确证自有测试二进制，释放379,628,016B；root逐项核缺失状态、来源/.d/日志SHA，97源与224保护项由owner前后核不变，生成源码/锁/证据/共享依赖保留，回执 `/tmp/openrust-r2p-generated-cleanup/result.json` 和root-audit.json。
- 分支当时owner回执主target76,380,396KiB、磁盘可用101.3GB；其他登记target仍需合计，不能冒称全量已核。后续构建均重新预检合计及预计增长，50GB/10%仅软告警、100GB/5%硬线保持；未知缓存不清。派发/停止ID入口 `/tmp/openrust-provider-registration-dispatch.json`，总控用正式Paseo stop逐任务协调；不另建Goal/guard、不推送、不提前退役未合分支。

## R2-CC 配置缓存家族（分支提交7be446d；main组合见本页顶部）

- 稳定main `9581a53` 起点，不依赖未提交R2-P；固定Laravel ConfigCache/Clear、LoadConfiguration/LoadEnvironmentVariables 与现来源/命令机制，完成官方 `config:cache/config:clear`、同Application fresh配置、缓存命中与两阶段env路径、原子文件/失败清除/资源关闭、模块/生成/真实复启验收；Spec `/tmp/openrust-config-cache-frozen-spec.md`，候选与来源 `/tmp/openrust-config-cache-contract.md`，未实现/验收项不得提前关闭L005.04。
- owner `cbc80e79-d801-4d4e-aef0-c4e7f4551346`，同项目workspace `wks_d752446d3d277b26`，branch `tasks/config-cache-family`，cwd `/Users/Home/.paseo/worktrees/02joa2lr/openrust-config-cache-family`；本lane独占自身源码/模块/模板/说明，manifest/lock/共享Agentdocs与main索引root协调。不复制其他worktree WIP，后继R2-P异步API/R03文件来源在实际集成时组合验证。
- 已完成多轮生产批修及历史分支验收：配置77、生命周期16、默认workspace849、双严格Clippy/docs、MSRV/no-default/deny、生成与真实缓存复启具证据；全特性all-targets的额外复制benchmark曾失败，测试目标补验与原失败分别保留，不把整条命令写成通过。后续独审发现维护命令应先正常bootstrap再执行清缓存，原owner已修该生产顺序，18文件新冻结 `/tmp/openrust-config-cache-command-bootstrap-delta.json`；原结果须按输入影响复用，命令/缓存真实验收需新输入重验。
- 限定分支提交 `7be446d81c6c1aa0269c4f2aa36b0ce5a4e27aa1`，26文件（18实现/用户文档+8共享记录），worktree干净。固定命令引导/fresh/原子缓存/env与模块屏障、生成器和两类真实复启由不同模型审查及实际门禁验证。默认workspace lib/tests851、全feature1638/68ignored、doctest22/34ignored与24/40ignored、双严格Clippy/docs、no-default/MSRV、普通生成8通过；旧offline deny仅复用相同依赖图/策略与2026-10-30 advisory快照。
- 最终证据 `/tmp/openrust-config-cache-final-manifest.json`（SHA `73da1b7712e35176dff0bd30b63004d07e7f35f5e6e73603c99b6005c3508119`）；root核61项和42生成文件。原driver断言失败/旧benchmark失败保留；共享target后续更换server，原真实脚本首尾SHA与expected一致，不声称现在仍可重算旧binary。
- 两自有temp目录已退役，磁盘实测回收13,570,048B，共享target变化0；Cargo忽略包URL/版本限定，会清全部同名sample-app，未执行实际clean，约1GiB混合归属缓存由root下次资源交接精确核对，未知产物保护。分支提交时未main集成；现已纳入R03/typed/Signed四簇main组合；P后续与S-DV另经main限定组合交付；原工作区/分支已正常退役、workspace已归档，发布全后端矩阵保持开放。

## R2/R10 运行时配置与动态调度（仅规划，按依赖领取）

- 依据 [ADR-080](decisions/adr-080-runtime-configuration-scheduling.md) 与 [运行时配置与动态调度计划](plans/runtime-configuration-scheduling.md)；当前 HTTP 主功能簇不抢占，本轮没有代码实现或真实验收。
- R2：V1必须交付Laravel简洁typed分组DB业务设置与权限保护管理API/CLI（宿主启用可选）；开发者注册字段、管理员后台保存，白名单/权限/完整校验/CAS审计/快照/通知轮询/故障期限封装内部，持久实现依赖R4事务、R7权限审计及适用R8，基础设施启动项不热换；邮件/SMS/支付/存储业务secret加密、driver受控激活或pending_restart，区分DB保存/节点应用。
- R9/R10 Notification/SMS细化沿[短信渠道与多服务商交付计划](plans/sms-delivery.md)：先SMS-0来源/SDK审计与Notification公开渠道契约，再按前置做具名连接、官方Vonage对应及阶段二国内/其他服务商增强；队列、动态设置和回执复用R10/ADR-080。候选SDK未选型，跨attempt配置版本策略待Spec与既定运行时contract统一；真实发送需获准凭据/号码，当前不抢占D10-G或启动供应商实现。
- R10：复用Job注册/connection/queue与同一Scheduler；先明确after_commit/同事务及onOneServer/withoutOverlapping，再实现DB动态计划/版本审计/目标队列/允许策略、变更竞争与worker并发。具体公开API与schema先Spec；默认DB队列、Redis显式可选。
- R14：PG18/MySQL8.4真实多节点、Redis通知中断补偿、DB故障/权限/CAS、任务重试崩溃及仅应用+DB迁移恢复；各簇同步modules/example、生成项目、README/config/测试，未验收不关闭。
- R15才做可选快速开发模块管理界面；V1先交付Rust公开管理服务/必要traits与Gate/Policy接入，由宿主自定路由/controller显式传认证授权上下文；框架不预置HTTP endpoint，CLI显式复用同服务不绕授权，不引入配置平台、第二总线或平行调度器。

## 当前确定的待修风险

- 配置：未知配置键可能退回默认。R2 按已注册 section 与合法动态字段设计校验，保持敏感值脱敏。
- 路由：src/lib.rs 的 routes 与 src/http 的路径引用可能重复编译注册类型；R2/R3 统一公开入口并用实际注册/请求测试锁定，不能因两条 import 都可编译就视为同一服务表。
- R1-S 的 MySQL 整组、MSRV、consumer 和最终文档门禁已实际补齐，结果见 memory/testing.md；这些结果只归当前 app/example 过渡结构，不转记 modules/ 或新 Provider 已验收。

## 当前 R2-A Spec

1. R1-S 已以 34dfb5e 限定提交，整体R1/模块系统/CRUD不关闭。
2. 修复R2草稿的root scoped/reset、模块boot overlay时点与数组留尾语义，交不同模型复审。
3. 新容器基元统一typed bind/singleton/scoped/instance，15项回归与完整门禁/公开下游已实际通过，限定提交；未切生产Registry。
4. root/手动scope/reset在A验证，HTTP/job/console入口集成和完整关闭在后继；不把基元当生产Provider已切换。
5. 现生产Plugin/Registry暂仍唯一启动路径；B1一次替换，不增加legacy/new运行开关。
6. 不动Redis默认/现有DB行为，不增加依赖；按源码门禁和独立审查验收后再提交。

## 当前 R2-A2 配置目录失败语义 Spec

1. R2-A容器基元已提交26df75c；下一子簇只修ConfigLoader目录/来源错误，不实现Provider或schema/合并语义。
2. figment()改Result，显式new路径/APP_CONFIG_DIR/自定义namespace CONFIG_DIR为空、缺失、非目录、不可读或条目错误时失败。
3. 未显式选择的默认config目录不存在时保留内置默认，目录存在但读取失败仍报错；不改变当前Redis/DB/env优先级或运行配置值。
4. 已发现TOML文件必须实际可读/可解析，不因缺失静默跳过；错误只含安全来源key和类别，不泄漏路径/文件内容/密钥。
5. 同步全部loader调用与真实既有fixture，新增显式坏目录、默认缺目录、有效空目录、损坏文件和启动传播回归。
6. 源码门禁、独立consumer/生成与根HTTP及适用双库重新执行，不用R2-A或R1旧结果冒充；不同模型复审后限定提交。

## 当前 R2-B Spec

状态：生产切换与领域 Provider 已落盘；默认/all-features、双库、生成项目、消费者和根实际入口门禁已实际运行，详见 [testing](memory/testing.md#r2-b-统一-provider-内核验收证据)。本簇实现与验证完成，后继按 R2-C/D/M 依赖顺序推进。Figment 来源字典原始插入顺序与第三方 driver 无专属 validator 的限制须继续公开，不归零宣称严格 PHP/Laravel 等价。

1. 统一ServiceProvider的register仅绑定/schema，所有register后seal，再顺序boot贡献，最终配置/贡献校验后Ready。
2. 先并行实现ConfigRepository/schema与可显式await的资源关闭基元；这些前置不宣称生产已切换。
3. 资源清理复用既有受管任务/Cancellation：关闭先拒新登记，取消并join任务，逆序尝试全部回调；共同deadline、失败继续且诊断不泄密。
4. 并发shutdown只启动一次，调用future被取消后已启动的有界关闭继续；弱注册句柄不得延长所有者寿命，Drop仅取消不伪装async清理。
5. B0逐族拆分即时构造与贡献；B1实际替换私有Registry/旧Plugin循环、AppState/Inject/命令读同Container，不增双运行开关，不删除领域贡献registries。
6. 配置worker仅src/config/repository.rs；集成者持core/resources/provider/app/lifecycle等与mod导出、Cargo/共享文档/串行真实门禁；不同模型独立审查。
7. R2-C再接HTTP body/job/console scope，R2-D同生产TestApp；R2-M再迁modules/example，随后R1-E真实CRUD，不能跳依赖或将基元当整体完成。

## 当前 R2-C Spec：入口 scope 与清理

状态：代码、真实 HTTP/Redis/PostgreSQL 18/MySQL 8.4、生成双库项目、公开队列 consumer、默认/全特性与 MSRV 门禁已实际通过；提交与资源收尾见本簇 Git/验收记录。R2-D TestApp、R2-M 模块目录和 R1-E 真实业务 CRUD 仍待后继，不计入本簇完成。

1. 复用 R2-B 的 Container、Scope、ResourceOwner 与 ExecutionContext；HTTP 已有请求 scope/Body 关闭，只补实际缺失的后台任务与取消边界。
2. 一次命令、一次队列 attempt、一次 scheduler 触发各建独立 scope；成功、错误、超时、取消均封闭解析并执行有界回收，App 可连续运行多个命令。
3. `Scope::run` 仅在当前 future 安装与所属 Container 匹配的 task-local scope；`AppState::resolve` 只在匹配时取该 scope，普通 `tokio::spawn` 不继承它。
4. `Scope::spawn` 使用同一 ResourceOwner 管理任务，显式传递允许的 ExecutionMetadata、预算和取消；关闭时取消/join，禁止跨入口继承身份或已关闭 scope。
5. Queue 的 `JobContext::service` 优先本次 attempt scope，worker 与自定义 driver 都经实际 handler 入口建 scope；retry 必须得到新实例，租约失败和超时也清理。
6. Scheduler 每次触发才进入 scope，不把任务服务缓存进 Scheduler singleton；命令自身 scope 与任务触发 scope 分离。
7. 受管回调逆序且仅一次；入口 future 取消后由 App 持有的子资源关闭队列接管，App.shutdown 等待并聚合失败，不以 `Drop` 冒充异步关闭。
8. 用真实 HTTP stream/drop、连续 console 命令、queue 成败/超时/取消/重试与 scheduler 多次触发验证隔离和清理；适用 PostgreSQL 18/MySQL 8.4、生成项目及完整门禁后限定提交。
9. 本簇只处理入口生命周期；R2-D TestApp、R2-M 模块目录及 R1-E CRUD 不计入完成，保留既有 Figment/第三方 driver 差异。

## 当前 R2-D Spec：TestApp 最小路径

状态：生产与测试共用 Provider 构建路径、示例和生成应用已接入；默认/全特性、真实双库与 Redis 回归及 MSRV 门禁见 [R2-D 测试证据](memory/testing.md#r2-d-testapp-验收证据)。本簇完成后继续 R2-M，不把 TestApp 当模块系统或 CRUD 已交付。all-features workspace 一次 fixture 启动 `internal_error` 的原因仍未证实，保留日志及安全阶段诊断。

1. 复用生产 `Application::providers` 清单、CoreCommands、应用配置绑定及唯一 `ProviderBuilder::start`；测试显式传 Figment，不修改全局环境或工作目录。
2. 在清单排序前允许按具体 ProviderId 移除；仍执行依赖检查，被移除模块不得注册、启动或贡献路由。
3. 全体 `register` 完成后、容器封口前执行一次类型化绑定覆盖，支持 instance/singleton/scoped；任意 `boot` 都看到最终绑定，不建第二服务表。
4. `TestApp` 持有真实 App 与 Router，`request` 走生产 build_router/oneshot；响应正文有字节上限和截止时间，断言错误不回显敏感值。
5. 流式响应可由调用方持有并显式 Drop；App 关闭等待请求 scope 清理，失败向测试传播；重置先关闭旧 App 再重建，不能仅 reset 根 scope。
6. 验收 fake Mailer/业务接口在 boot 与真实 HTTP 可见、同进程两个 App 隔离、模块移除 404/health 保留、取消与资源回收、status/header/JSON 断言。
7. 同步当前示例、公开 consumer、CLI 生成应用和用户文档；默认/全特性门禁、适用双库真实测试、MSRV/no-default/deny 与限定提交后关闭本簇。
8. 本簇只提供测试宿主与覆盖/移除契约；R2-M 目录和 R1-E CRUD 仍未交付。

## 当前 R2-M Spec：模块最小闭环与目录迁移

状态：源码、真实服务与适用门禁通过，随本簇限定本地提交收口；固定 laravel-modules v13.2.0 的 B007–B010、B016–B020 与 ADR-074 为当前实现依据，其余矩阵条目按实际能力保留缺口。

1. 编译期描述 `modules/<name>` 的身份、来源、priority、按序 Provider 与资源；启动时读取独立启用快照，未编译名称、同名/规范化碰撞均拒绝，缺状态不默认启用。
2. 仅启用模块按 priority 升序稳定排列，模块内 Provider 保持声明顺序；不凭 priority 重排依赖图，显式 Provider 依赖继续由唯一清单检查。
3. 全体 Provider 共用原 Container 的 register 屏障；模块配置在声明的配置 Provider boot 时点逐文件调用唯一仓储 overlay，不能统一提前，模块值胜、数字数组逐索引替换并保留尾部，前序读取不追溯。
4. 启用模块才贡献路由、配置、资源与迁移；名称/资源/路由冲突在 Ready 前失败，已登记资源走原启动失败有界回收，不新建模块服务表或第二启动器。
5. `Application` 与 `ProviderBuilder` 共用编译描述和启用快照入口；禁用只影响下一次 App 构建，当前 App 不热卸载、不回滚迁移或删除源码。
6. 将唯一现有示例完整迁至 `modules/example/`，根只保留通用编译声明/清单；示例真实 HTTP、TestApp 替换/移除、数据库迁移及资源经公开模块入口执行。
7. 验收模块排序/依赖/配置时序与冲突/失败、临时第二模块无需改框架 src、禁用后的真实 HTTP，以及 PostgreSQL 18/MySQL 8.4、生成应用和适用全门禁。
8. 模块状态写入、管理命令、生成、发布与完整 B 矩阵归 R13/后继，不把静态 Rust 编译声明称为 PHP 源码热发现。

## 当前 R1-E Spec：真实业务 CRUD 与公开能力示例

状态：已验收并提交 `bc3fa01`；本节保留该簇规格与边界。复用唯一 `modules/example`、现有 Provider/Db/Auth/HTTP 能力，不新增框架业务内核。

1. 新增有所有者的 Article 业务实体、模块迁移及 SeaORM model；仅在数据库和认证能力就绪时挂载 `/api/example/articles`。
2. 通过公开 `CurrentUser`、`AuthLayer`、`Validated`/`Json`、`Db` 和 `Routes` 实现列表、详情、新增、更新、删除，业务数据仍只属于 `modules/example`。
3. 请求验证标题/正文和有界页码、每页数量；查询和写入按当前账户所有者约束，别人的记录按未找到处理，写操作走主库事务。
4. 返回真实持久化模型的资源 DTO；列表提供稳定排序、分页总数，详情/新增/更新/删除和错误响应保持现有 API envelope 语义。
5. PostgreSQL 18/MySQL 8.4 分别用隔离库和真实 TCP HTTP 覆盖完整 CRUD、无效输入、跨账户访问、分页、删除后 404 与事务失败不留部分状态。
6. 盘点当前已实现公开能力在示例的实际调用/扩展覆盖；缺项按非重叠文件分组补齐并逐项验收，不用仅有 consumer、文档或基础 GET 冒充模块示例。
7. 同步示例 README、根/项目事实、必要模板与变更日志；fmt、默认/全特性 workspace、Rustdoc、deny、MSRV/no-default、真实服务及生成项目按受影响范围验证后限定本地提交。
8. 此簇不把 SeaORM 业务 model 称为完整 Eloquent 等价，也不领取未来 R3–R16 尚未实现的框架能力。

验收入口与结果见 `memory/testing.md`；该簇已限定本地提交，外部系统边界仍作为产品限制记录：

- Article CRUD、事务事件/通知/审计/签名/加密、附件持久清理意图、同步及异步 CSV/XLSX、命名 Redis 缓存、Session/MFA、OIDC、SSE/WebSocket、业务 telemetry 已接示例；真实双库定向已分项执行，能力边界以 `modules/example/README.md` 表为准。
- S3 既有暂存→完整校验→copy 发布和双库/版本化验收已通过；真实 MinIO 代理确认 CopyObject 成功后暂缓 SDK 响应，PostgreSQL 18/MySQL 8.4 各 1/1 验证上传任务继续持锁、DELETE 等发布收敛并清理最终 key 与 staging。请求体 EOF 与 45 秒 deadline 由共享原子状态仲裁：deadline 获胜则延迟 EOF 只能产生流错误，不会进入发布；EOF 获胜则存储收敛前保留行锁。App shutdown deadline、进程强杀、永久网络停滞，以及 CopyObject 已发送后返回 `storage_publication_uncertain` 而远端迟到成功，仍不保证 DB/S3 原子提交或无孤儿。
- 显式 `db seed` 的幂等/碰撞/不可登录隔离账号，以及数据库与 Redis 两种文章索引 worker 的真实失败、退避、终态、公开重试已双库验证；默认/全特性整组门禁与文档证据已归档。
- 出站 HTTP 超时/取消、Cache tag/SWR 生产路由、tenant 成员鉴权→查询/缓存/本地及 S3 存储/持久队列/附件清理、浏览器原生 Session WebSocket、OIDC MFA/logout/真实签名 backchannel、视图发布 TCP、i18n 复数/日期、gRPC 双端口 TLS 组合、业务 Scheduler 时钟→队列→worker 均有定向真实证据；默认/全特性 workspace、适用 consumer 与十种生成项目已按当前源码执行，精确命令、忽略项及初轮失败见 testing。
- Collector PostgreSQL runner 已确认读取活跃日志并检查增长；共享 `evidence-pg-final.log` 达 36 MiB 时不放宽 16 MiB 断言，改用隔离 Collector sink `/tmp/openrust-r1e-collector/evidence-pg-isolated.log`。delivery 与 unavailable 各 1/1，匹配 trace/metric、业务提交/不可用 warning 和无私密 sentinel；共享 Collector 与原日志未改。

### 当前 S3 copy 竞争修复 Spec

- 预期：请求体 deadline 不能与正常 EOF 并发放行已过期的 S3 发布；CopyObject 已进入真实远端但响应尚未交回时，文章删除必须等待发布收敛后再清理目标。
- 实现：共享 `AtomicU8` 通过 CAS 仲裁 pending→EOF 与 pending→deadline；deadline 先获胜时迟到 EOF 被转成流错误，storage 不会进入 copy；EOF 先获胜时请求体期限不再释放事务，App 受管上传持锁等 storage 结果。
- 验收：真实 MinIO 代理把签名 CopyObject 转发至 MinIO，检查 HTTP 200 与 `CopyObjectResult` 后才暂缓成功响应；读取真实 final key 内容，确认 HTTP waiter 超时、并发 DELETE 仍阻塞，放行后检查 DELETE 完成、final key 不存在和 staging 已清。PostgreSQL 18/MySQL 8.4 各 1/1。
- 附加验收：原子仲裁单测 3/3；PostgreSQL/MySQL `attachment_flows::` 各 15/15。
- 边界：`storage_publication_uncertain`、远端迟到成功、App 关闭期限与进程强杀未被本用例解决，不声称 DB 与对象存储原子或绝无孤儿；隔离代理控制时序，不替代真实 MinIO。

## 当前 R2-N Spec：模块硬依赖图与确定性启动顺序

状态：实现、验收完成；本簇补齐模块之间的硬依赖契约，不实现 R13 模块生成/启停命令。ADR-075 与验收证据见 [R2-N 记录](memory/testing.md#r2-n-模块硬依赖与确定性启动顺序)。

1. `ModuleDescriptor` 增加公开 `.requires(name)` 声明；依赖身份沿用模块现有大小写不敏感的名称查找规则。
2. 仅已启用模块参与准备；启用模块声明的依赖必须已编译且启用，缺失/禁用分别以安全、稳定的错误码拒绝，不隐式改写用户模块激活状态。
3. 拒绝重复依赖、自依赖与多节点循环；诊断只暴露模块名/依赖名，不暴露路径或配置秘密。
4. 依赖边优先于旧 `priority`；在所有当前可启动节点中按升序 priority、再按原始编译声明顺序稳定选择，确保同一清单结果确定。
5. `ProviderBuilder` 使用准备后的顺序注册并 boot 模块 Provider；综合示例补一个消费者模块依赖生产者 Provider 的启动场景，证明依赖顺序可观察且不要求修改框架代码。
6. 用模块单元/启动集成测试覆盖反向声明顺序、priority 冲突、依赖禁用/未编译、重复/自依赖、环与确定性排序；执行源码门禁、双库模块 HTTP/生成应用及真实根入口验收，再限定本地提交。
7. 不提供版本约束、自动依赖安装或“启用即递归改状态”；这些涉及 Cargo 编译/管理命令的能力按模块矩阵与 R13 单独验收。

## 当前 R3-A Spec：语义 HTTP 状态默认与统一 JSON 响应

状态：已完成并由 `03c8cfa`、`96f148f`、`d07e13f` 限定提交。R2-N 已由 `48be854` 限定提交；本簇仅收敛 Laravel Response/Validation 对应的 HTTP 状态默认及现有统一 JSON envelope，不领取 FormRequest、Session/CSRF、Resource 新能力。

1. 固定依据为 Laravel 13.39.0 与文档 commit `960eedf0911155ad16326ade49c0ea5d18eda0b8`；响应使用真实 HTTP 状态，validation JSON 使用 422。保持 OpenRust 自有 envelope 增强与 ADR-008 状态映射。
2. `semantic` 成为 ApiConfig、框架错误 fallback、根默认配置与生成项目默认值；框架错误遵循同一请求策略。`always_ok` 仅在应用显式配置时覆盖状态。
3. 创建成功保留 201，空成功仍按现有类型返回 200；not found、method not allowed、validation、rate limit、overload、timeout 与 5xx 返回语义状态。request ID、机器码、字段错误、脱敏和 JSON 结构保持原契约；HTML、文件、流、SSE、gRPC 保留各自协议。
4. 同步 CLI 生成模板/.env、综合示例、用户文档和 CHANGELOG；修正把 `always_ok` 当隐式默认的测试断言，覆盖省略配置时的 HTTP 行为及显式 override。
5. 生成普通/认证应用并执行 check/test/HTTP；检查根实际 HTTP 入口，执行默认/全特性 workspace 门禁、deny、MSRV/no-default。双库仅对实际依赖数据库的改变路径验收，不以旧记录代替本簇行为测试。

## 当前 R3-C Spec：命名路由与具名 URL 生成

状态：已完成适用验收并由 `801ca4e` 限定提交。对齐 Laravel 13 `Named Routes` 与 `URLs for Named Routes`；名字唯一、可跨路径调整使用，并支持路径参数和额外 query 参数。来源：[Laravel Routing](https://laravel.com/framework/docs/13.x/routing)、[Laravel URL Generation](https://laravel.com/framework/docs/13.x/urls)。

1. 在 `Routes` 注册 API 上提供紧邻路由定义的 `.name("users.show")`，记录所属 HTTP route 与完整嵌套 prefix；group 可增加 Laravel 式 name prefix，并按 Laravel 原样字符串拼接（例如 `admin.` + `users.` + `show` → `admin.users.show`）。
2. 重复名称和不受支持的路由模式（通配符、空路径段）在 router build 阶段报错；未知名称、缺失参数和无法安全编码的值返回稳定框架错误并按 Laravel URL generation exception 映射为 500，不 panic、不静默替换。
3. 提供公开 `RouteUrlGenerator` 从名字生成绝对 URL；host/scheme 以 `app.url` 为来源，不信任请求 Host。必需 route 参数进行单段百分号编码，其余参数成为有序 query；仅支持必需 `{parameter}` 段，不声称可选参数、通配参数或模型绑定已完成。具名路径仅剥离单层首尾 `/`；空段保留并于启动时报错。
4. 生成器由生产 `build_router`/应用 HTTP 请求路径公开供 handler 使用，并可在 `modules/example` 的链接/响应代码演示；生成应用应可用同一 API。
5. 黑盒测试覆盖 path 更换不影响名字调用、嵌套 name prefix 与 path prefix、参数编码/缺失、额外 query、绝对 base URL、安全 Host 边界、重复名称启动失败；更新 HTTP Rustdoc、用户 README、core/http 与完整测试记录。
6. 按 §6.3 执行 fmt、默认/all-features workspace clippy/test/doc、deny、MSRV/no-default、生成项目 check/test 与真实 HTTP。路由变更不依赖数据库，不为无关路径重复双库验收。

## 当前 R3-D Spec：Laravel middleware alias 与 group

状态：已完成并限定本地提交。依据 Laravel 13 Middleware：应用在启动时注册 middleware alias 与 named groups，路由/路由组按名称引用；group 成员按声明顺序展开。OpenRust 使用 Provider boot API 注册，通过类型安全的 Tower `Layer` 闭包保留 Rust 扩展方式。

1. `BootContext` 提供 middleware alias/group 注册入口；重复、无效名称与 alias/group 同名冲突返回稳定启动错误。定义可以由不同 Provider 声明，所有注册完成后统一用于 router 构建。
2. `RouteGroup` 与 `Routes` 提供 named middleware 使用入口；group 可嵌套，名称递归展开，按声明顺序执行且响应逆序；现有直接 Tower Layer API 保留。
3. 未知引用、循环 group 与空 group 在 `build_router` 前明确失败，不 panic、不静默忽略；middleware 不会应用于用户未选择的路由，也不改变框架内建全局 stack。
4. 在 `modules/example` 通过自定义 middleware alias/group 展示“业务 Provider 登记、业务路由引用”，至少一个真实 HTTP 断言证明只命中目标路由及顺序；生成项目可编译并通过 TestApp 行为测试。
5. 更新框架 HTTP README/Rustdoc、示例文档、core/http、CHANGELOG、context/plan/progress、testing；执行默认/all-features workspace Clippy/tests/docs、fmt、deny、MSRV/no-default、生成项目检查与实际 HTTP，无数据库路径改变时不虚构双库必要性。无注册表引用会 fail closed，`Routes::into_router()` 检查路由/middleware错误；全功能已按以上要求验证。

上游依据：[Laravel Middleware](https://laravel.com/framework/docs/13.x/middleware#middleware-groups)、[Middleware Aliases](https://laravel.com/framework/docs/13.x/middleware#middleware-aliases)。

## 当前 R3-E Spec：Laravel fallback routes

1. `Routes::fallback(handler)` 注册应用级未匹配请求处理器；它只处理没有匹配路由的请求，既有路径上的方法不匹配仍返回 405。
2. `RouteGroup` 可限定 fallback 的静态路径前缀与 middleware；同一完整 scope 不接受多个 fallback，启动/公开 router 构建须返回稳定错误，不 panic 或静默覆盖。Axum 对参数化/通配符前缀 fallback 会产生 catch-all 冲突，当前返回稳定 `http_fallback_scope_unsupported` 并在文档公开边界。
3. 若请求为 GET/HEAD 且没有业务路由匹配，先尝试现有 public 目录的静态资源服务；资源命中保持原响应，缺失后才进入业务 fallback。自定义 fallback 允许应用明确接管 `/api/*` 与 `/ws/*` 未匹配请求；未登记时继续保持框架 JSON 404。
4. 静态检查使用独立空 body 请求，仅用于 ServeDir；原始请求、headers、query、body 必须完整交给 fallback handler。非 GET/HEAD 不尝试静态资源。
5. `modules/example` 提供一个网页 fallback，实际 HTTP 验证命中自定义响应、middleware、正常路由优先、405、静态资源优先和默认 API 404；不覆盖或替代框架默认 fallback。
6. 补公开 Rustdoc/HTTP README、示例说明、core/http、CHANGELOG、context/plan/progress、testing；生成项目能通过 TestApp fallback 行为测试。
7. 执行 §6.3 workspace fmt、default/all-features clippy/tests/docs、deny、MSRV/no-default、生成项目 check/test 与真实 HTTP；不改数据库行为，不虚构双库门禁。

状态：已实现、验收并由 `dab80d4` 限定提交；真实 HTTP、生成项目及适用 workspace 门禁已记录。参数化/通配符 fallback scope 的 Axum 限制保留为明确缺口，不宣称完全 Laravel 等价。

上游依据：[Laravel 13 Routing — Fallback Routes](https://laravel.com/framework/docs/13.x/routing#fallback-routes)。

## 当前 R3-F Spec：Laravel domain/subdomain routing

状态：实现、独立复审、整组门禁、生成应用、根真实 TCP 与资源收尾均已完成，本簇限定提交见 Git（领域 contract 见 [ADR-077](decisions/adr-077-domain-routing.md)）。依据 Laravel 13.39.0 固定源码与 Routing 文档的 `Route::domain(...)->group(...)` 和 domain-first 匹配；仅使用有效请求 authority，不信任代理转发 Host。后继领取 C04 Request/Response，不将本簇完成外推为全部 routing 或 R3 完成。

1. 在 `RouteGroup` 增加 `domain(pattern)`，嵌套组继承域约束；支持精确域和完整标签占位符（如 `{account}.example.com`），拒绝空标签、部分标签占位、端口、路径、非法/重复参数以及同一规范化 domain pattern 的路由冲突，错误在 router build 稳定返回。
   子组未声明 domain 时继承父组；显式声明新 domain 时覆盖父约束，不拼接两个域模板。
2. 对每个请求严格解析唯一有效的 `Host`/URI authority，拒绝多值、逗号、非法端口、畸形 authority 与互相冲突的 authority；大小写规范化、去端口后按当前 HTTP method 优先查找匹配域路由，找不到才进入不限定域的普通路由。域路由 host 命中但 path/method 未命中时必须继续检查后继域和普通路由；仅当当前 method 均无匹配、但某路由 path 对其他 method 有效时才产生 405。业务 handler 主动返回 404 不能触发二次路由。
3. 提供公开 `DomainParams<T>` extractor，使用 serde 将已验证 host pattern 捕获的完整标签参数反序列化为类型化值；未命中域路由或格式错误时稳定失败。域参数与 Axum `Path<T>` 分离，避免更改路径参数语义。
4. 命名 domain route 的 `RouteUrlGenerator` 使用登记的域模板，以同一参数表同时替换 domain/path 参数；域名只接受单个 ASCII DNS 标签（含预先编码的 punycode），不做 Unicode IDNA 转换；缺参、非法标签和重复名称 fail closed。非 domain 路由仍以 `app.url` 的 host/scheme 为准；绝不以当前请求 Host 或未验证 `X-Forwarded-Host` 构造绝对 URL。
5. 明确 `Host` 只参与用户显式配置的路由分派，不自动改变 `app.url`、租户身份或可信代理判定。若需 `Forwarded`/`X-Forwarded-Host`，必须新增受可信 peer 约束的独立契约，不在此簇默认采信。
6. 黑盒测试覆盖 domain-first 与注册顺序无关、精确/占位子域、大小写/端口、重叠 pattern 声明顺序、域 host 命中 path 缺失后落普通路由、域 GET/根 POST 同路径、方法回退/405、域 fallback 不抢普通路由、handler 自身 404、非法/缺失/重复 Host 与参数、Host 注入不能影响 URL 生成、named URL 域与路径参数编码；请求 body/header/query 在分派中完整保留。生成项目与 `modules/example` 展示多租户域路由，但 host 参数不得替代租户认证授权。
7. 用户 fallback 在全部普通 domain/unrestricted route 候选之后才运行；域组 fallback 按 domain pattern 选择。业务 handler 404/405 必须是终端响应，不得回落。GET/HEAD 静态资源检查保持当前 public 路径策略，并测试域路由交互。
8. 领域 ADR 已建立并固化 dispatch 与安全边界；实现后更新 HTTP Rustdoc/README、示例、core/http、`todo/plan/progress/context`、CHANGELOG 和 testing；执行 §6.3 适用门禁、生成项目与真实 TCP HTTP 验收。数据库不变时不虚构双库要求。

实现决策与差异：域 pattern 支持 DNS ASCII label 和每标签一个 `{parameter}`，兼容 ASCII punycode，不做 Unicode IDNA 转换，不支持 IP literal 或末尾点；捕获值须为单个合法 label。域与路径参数同名启动报错。不同 pattern 同时匹配时按域注册顺序；同一规范域内 Axum 重复路由稳定失败，记录为与 Laravel 后注册覆盖行为的显式差异。方法/路径 miss 必须保持请求 body 不被候选路由消费，并接着检查普通路由；不得靠业务 404 状态猜测未匹配。

上游依据：[Laravel 13 Routing — Subdomain Routing](https://laravel.com/framework/docs/13.x/routing#subdomain-routing)；固定源码匹配规则确认 domain routes 在 unrestricted routes 之前处理，与注册先后无关。

## 当前 C04 Spec：Request/Response 契约校准

1. R3-F 已提交163bb49且工作区清洁后领取；本阶段先对照固定上游 L019/L020，定位首个可独立实现的 Request/Response 行为子簇，再立即进入实现与验收。
2. 审计现有 Json/Path/Query/Validated、ClientIp、类型化响应、统一 envelope 和原生 Axum 逃生口，不重复实现已覆盖能力，也不将类型别名或换名包装当成 Laravel 行为对应。
3. 首个子簇必须明确公开 API、请求 body 的单次所有权、metadata/headers/内容协商、错误语义，以及与现有可信代理/IP和URL来源规则的复用边界；仅在实际需要时增加类型或配置。
4. 保留后续 Cookie/Session、redirect/flash、CSRF、FormRequest 授权/验证、文件验证和 Response 扩展的完整范围，不能用首个子簇代替完整C04验收。
5. 每个落地子簇同步唯一模块业务调用、生成应用与真实HTTP；行为证据、适用门禁和资源收尾完成后限定本地提交，当前契约校准不计代码交付。

## C04/C05 分页整改 Spec（已授权，按依赖领取；代码未改）

1. 不抢占当前在途HTTP簇；分页解析/Response契约归R3 C04，API Resource归C05（C03资源路由不能替代Resource），查询/模型分页分别依赖R4 C07与R5 C08/C09；对应现矩阵[L059.02/.03/.05/.06、L068.04](plans/laravel-rebuild-matrix-2.md)。实现拆可验收子簇，不另建分页计划或先堆抽象。
2. 当前事实：`src/http/response.rs`的PageParams默认page=1/per_page=20，直接提取限制per_page=1..100并把非法值变为校验错误；Paginated只返回page/per_page/total/total_pages元数据。`modules/example/http/requests/{article,example_record}.rs`重复分页字段/校验；article controller仍手算page-1并拼PageParams，example_record controller直接操作SeaORM paginator的count/fetch_page并再包装。旧CRUD/统一meta通过不等于Laravel分页等价。
3. 固定依据为[Laravel pagination文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/pagination.md)与framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`：[Model](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Database/Eloquent/Model.php)/QueryBuilder默认15；[PaginationState](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Pagination/PaginationState.php)解析page、非法页回1；[LengthAwarePaginator](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Pagination/LengthAwarePaginator.php)与[PaginatedResourceResponse](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Http/Resources/Json/PaginatedResourceResponse.php)分别对标直接JSON与Resource links/meta；不能仅20改15、字段换名就关闭整改。
4. 黄金路径须是框架查询/模型调用paginate(n)并自动解析当前page、返回分页对象；simplePaginate/cursorPaginate按既有矩阵后续补齐，不让普通controller复制偏移/count/包装。per_page query不是Laravel默认自动消费，应用需要时显式校验并传入；现1..100、page上限/溢出保护、invalid422须分清应用策略/安全增强/Rust数值限制，不冒称上游默认。统一返回价值经Response/Resource公开扩展融合，不建独立分页协议，原生响应可用。
5. 高价值验收：缺省/非法/零负页与超界页、空集last_page=1及边界链接、末页/越末页、参数/fragment/自定pageName与同请求多分页器、直接Paginator JSON和Resource links/meta避免二次包装；simple无总数查询、cursor稳定排序/唯一性/前后游标及非法游标边界逐条按固定源码测试落实，不能假造全部422或隐式消费per_page。
6. 逐项对照固定[tests/Pagination](https://github.com/laravel/framework/tree/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/tests/Pagination)的LengthAwarePaginatorTest/PaginatorTest/CursorPaginatorTest及[ResourceTest](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/tests/Integration/Http/ResourceTest.php)，形成source/tests→RustAPI→真实controller/唯一modules/example→生成器/生成项目与README/config/tests→适用PG18/MySQL8.4的证据；授权代码整改待领取，本轮仅文档审计，无新代码或服务验收。

## C04-C：Cookie/响应契约（已验收）

C04-A/B1/B2的公开检查、输入基础与类型便捷方法已按各自范围验收，证据见[testing](memory/testing.md#c04-b2-类型便捷方法)，不重复实现。完整C04仍开放；本簇按[ADR-081](decisions/adr-081-request-response-cookies.md)实现Cookie读取/队列/加密与响应生命周期闭环，值工厂、队列与核心接线分非重叠文件并行；C04-B2已限定提交`0a92900`，不重复实现。

1. 本簇闭环限定Cookie读取、创建、请求队列与加密往返；不扩为完整Session/flash/CSRF/FormRequest重写。固定依据为Laravel `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181` 的InteractsWithInput、CookieJar、EncryptCookies及AddQueuedCookiesToResponse。
2. 不可变factory默认可共享，可变queue必须请求隔离；同名同path替换、不同path并存，返回阶段先附加queued再加密，取消/超时drop释放，不能留到下一请求。
3. RequestInfo与Request使用同一cookie视图，缺失/解密后null走default，空串仍存在；cookie独立于普通输入trim/null。真实wire解析、数组与名称绑定先核固定源，不直接套输入bracket parser。
4. 复用既有加密/key ring及Session middleware，入站旧key可读、出站当前key、篡改失败视null、except可公开配置；框架原生密文格式与PHP格式的必要适配在实现前记录，不能另造加密内核或静默跳过保护。
5. 明确外层cookie处理与现Session/auth读header的顺序及原始wire逃生口；响应组合沿原Response/Json/envelope入口，Cookie属性和错误/资源边界与固定源对应。
6. 验收覆盖并发请求队列隔离、相同name不同path、直接/queued输出、forget、重命名篡改、旧key轮换与超限；唯一模块、plain/auth生成项目、真实TCP及现Session/认证双库完整回归随簇交付。

本簇验收：最终workspace默认668/全特性1241 passed，根双库各48、生成plain3/认证双库各2、真实TLS与适用门禁通过；产物已清理，本簇限定提交见Git。完整Session/flash/CSRF/FormRequest不由此关闭，见[证据](memory/testing.md)。

## C04-D2：Session Store基础公开API（限定子范围已验收）

C04-D1的统一Session/flash与请求生命周期已完成适用验收并由d47162d限定提交，真实证据见[testing](memory/testing.md)。这只关闭该子范围，完整L025与Web栈保持开放。

1. [D2契约](decisions/adr-082-session-store-and-flash.md#d2基础store方法与公开扩展契约与验收边界)已实现并通过适用验收：remember/push/increment/decrement/remove、真实Cookie出口set_name与同后端handler；沿唯一Session状态/属性/数值工具。set_id/set_handler及请求注入的安全生命周期仍为具体后继缺口，不用无保护setter冒充对应。
2. 数值运算须实际PHP差分，错误/null/不存在/溢出等行为按固定上游与必要Rust约束分类；不能以编译或改名替代等价行为。
3. 新API必须在modules/example真实调用/公开扩展示范，更新根装配、生成项目、文档与稳定矩阵，执行适用workspace、双库、生成及真实HTTP门禁后限定提交。
4. 后继Old Input/redirect错误bag、CSRF、Session blocking/GC/previous URL、其余driver、FormRequest、文件与完整Response继续保留，不用基础flash关闭全能力。分页沿既有C04/C05→R4/R5依赖。

D2验收：修后12项门禁、根真实PG/MySQL完整模块HTTP各54、三类新生成应用4/2/2及独立审查通过；根target有效产物已清理、五旧consumer不变，限定提交见Git。PHP44条差分路径不是完整Laravel启动；不关闭整个L025。

## C04-D3：Session扩展生命周期（限定子范围已验收）

Spec已按ADR-082 D3收敛，进入实现：
- 补齐适用的ID/handler替换与请求注入能力，不复制Session内核，不绕过撤销安全增强。
- 对照固定Laravel Store、Manager与CookieSessionHandler及测试，先核实实际生产调用链和公开扩展入口。
- 明确start前/后、busy/closed/revoked、替换失败与取消，以及旧驱动资源引用的所有权语义。
- root收敛ADR与Rust公开API，互不重叠实现/测试交给现有代理，manifest与构建仍由单一集成人负责。
- 验收贯通外部driver、唯一模块、生成应用、真实双库HTTP和匹配workspace门禁；行为差异公开保留。

先审计固定Store的setId/setHandler与CookieSessionHandler请求注入生产路径，对照现有撤销、busy/closed、Provider装配与自定义驱动契约，写Spec后实现安全等价API。不得用D2只读handler逃生口替代驱动替换，也不得删除适用上游能力。统一状态机、唯一modules/example与生成调用、公开consumer、双库真实HTTP及匹配门禁随簇交付；Old Input/CSRF等按既有后继依赖保留。

D3验收（be98e3f）：最终12项门禁通过，默认668/全特性1328 passed（29/102 ignored不算实服务）；根真实双库完整target各55、三类新生成4/2/2及独立审查通过。root target已清、五旧consumer不变，限定提交以Git为准；本簇不实现完整内置Cookie driver或通用Container extend，不关闭整个L025。

## C04-D4：内置Cookie Session driver（限定子范围已验收）

Spec已按[ADR-082 D4](decisions/adr-082-session-store-and-flash.md#d4实现契约与明确差异)实现并完成适用验收；正常格式对应、PHP异常弱类型差异与客户端保证分别记录：
- 以固定CookieSessionHandler/SessionManager/StartSession及测试为依据，补齐真实双Cookie读写/过期/删除，不用请求hook替代完整driver。
- 复用D3请求实例、Cookie解密视图/队列与同一Session状态；不引入单例可变请求、第二bag或额外服务器存储冒充Laravel默认Cookie driver。
- 先核实stateless客户端存储与现Insert/Update、撤销和Auth契约的能力边界；不能虚称跨客户端原子撤销，必要适配按来源分类。
- 定义配置选择、数据与头部预算、错误/取消、密钥轮换及当前原子状态机的交互，再划分非重叠实现/测试任务。
- 唯一模块/生成调用、真实HTTP与浏览器适用证据、双库既有行为回归和匹配完整门禁随簇交付；来源行为未等价的缺口保持开放。
- 正式宿主与生成模板必须按显式非数据库driver接入唯一Session Provider；保默认database无数据库不挂Session的边界。新增原Application providers直通回归，禁止仅测试helper装store掩盖漏接线；实际无DB Cookie网络验收不得借假数据库绕过。
- 独立审查发现Cookie Auth无加密/保护排除组合可伪造身份，纳入本簇阻断修复：以真实中间件保护事实、driver公开认证能力与实际Auth入口校验闭环；受保护双库正例及plain/排除保护伪造负例未过前不关闭，不能仅cfg隐藏生产风险。

沿D3每请求driver绑定实现固定CookieSessionHandler完整双Cookie：配置名ID Cookie与ID名data/expires Cookie，解密后的输入视图、过期边界、队列写入/删除、取消与安全属性；唯一模块/生成/真实HTTP及适用门禁验收。D3自定义请求hook不关闭此项，也不以Cookie-only driver替代已验收数据库/Redis驱动。

D4验收：D4内置Cookie driver限定子范围已验收：final3完整12门禁通过（默认668/全特性1367 passed，29/102 ignored不算实服务），根真实双库加密各62/无加密安全负例各1、新生成plain5/双库认证各2、真实TLS代理链与仓内Chromium HTTP脚本4项及不同模型审查通过；root target清零、五旧consumer不变。本簇不关闭PHP异常弱类型、全局撤销或完整Session/Web能力，详细证据见testing。

## C04-D5：StartSession GC lottery与公开driver回收（限定子范围已验收）

D4已由47cd7b8限定提交，工作区核对干净后领取；默认lottery对照固定上游[2,100]，start后、业务前对实际请求绑定driver调用公开GC；同一Session内核，无第二存储。Spec须先收敛持久绝对expiry与上游last_activity期限的安全差异，不能误删仍获授权的显式长Expiry或声称已有expiry load替代未访问记录GC。数据库/Memory实际清扫，Cookie/Redis按上游noop；失败/取消/原子防复活、唯一模块/consumer/新生成、双库/Redis与匹配门禁同簇验收。previous URL/route、真实Precognition、Old Input/CSRF、blocking及Session cache继续保留，不能用本簇关闭整个L025。

D5 Spec：
1. `SessionConfig.lottery: [u32; 2]` 默认`[2,100]`，最终配置校验分母非零且分子不大于分母；`[0,n]`禁用、`[n,n]`确定触发，其他比例无偏抽签，熵错误正常返回错误不panic。
2. 唯一公开`SessionStore::gc(lifetime_seconds: i64) -> BoxFuture<Result<u64>>`默认返回0，返回实际删除数量；真实请求绑定实例在start成功后、业务前被调用，参数使用最终配置分钟转秒。
3. 追加公开`SessionGcMigration`建立物理表名摘要隔离的expiry索引，统一Migrator在Auth/Session-only建表后各登记一次；保留原建表版本，自定义Migrator显式追加，新建/已有表升级及同名异列冲突在双库验收。数据库单条条件DELETE清扫`expires_at <=`数据库当前时间（MySQL微秒精度）；Memory在同一锁内按已有绝对expiry边界删除并计数。Cookie/Redis不扫描、返回0，第三方driver可覆盖。不得先查后无条件删或将GC结果当业务锁。
4. 持久绝对expiry是既有授权上界，不以当前lifetime反推并缩短旧显式长截止或延长短截止；与Laravel按当前lifetime追溯last_activity及Array严格小于的差异明确按安全增强分类，正常固定lifetime对应过期回收，差异不隐藏。
5. start失败不GC；GC错误阻止业务/保存并清出站Cookie，取消不后台续跑；已发生外部删除不回滚。复用Close及当前错误响应，不建立后台worker或第二Session内核。
6. 覆盖0/全中、实际绑定驱动/参数、失败/取消、过期数量/活记录保留、续期竞争、防复活、资源边界及真实双库/Redis。唯一模块、公开consumer、原宿主生成项目、用户文档和匹配完整门禁同簇交付。

D5验收：D5请求级GC lottery/公开driver回收与追加expiry索引迁移限定子范围已验收：final4完整12门禁通过（默认668/29 ignored、全特性1374/103 ignored），根真实双库各62、Store/GC各2、Redis Store1/Auth10、新生成plain5/双库认证各2、独立审查及资源收尾通过；忽略项不算实服务，不关闭完整Session；root target已清、五旧consumer不变，限定提交见Git。后继先审Request完整URL/路由匹配上下文与previous URL/Precognition真实前置，按现C04依赖收敛下一Spec，不跳过剩余行为。

## C04-D7：真实路由匹配上下文（限定验收完成）

D7真实Route/Resolver与受管理方法声明已通过限定验收：final3完整12门禁、默认695/29 ignored、全特性1403/103 ignored、根PG18/MySQL8.4各64、fresh5/2/2、真实TCP7及不同模型复审；忽略项不算真实服务。根target清零，五旧consumer不变，限定提交以Git为准；完整Routing/Request与previous/Precognition仍开放。公开Route/参数/resolver、受管理方法与fallback/OPTIONS按[ADR-083](decisions/adr-083-matched-route-context.md)交付；[最终证据](memory/testing.md#c04-d7-真实路由上下文限定子范围已验收)保留失败修复、用户重验判断、真实服务及清理边界。

## 已交付任务的资源退役

- Q源提交850e1a4已在main；已完成owner正式归档并自然释放其cwd占用，正确workspace归档、worktree remove与branch -d均正常成功。证据 `/tmp/openrust-r2q-worktree-retirement.json`（SHA256 `18192f5ba415e0c633c2dfc98203155fc2a172adc1d3539121ab46b7f423b13c`）。X、D9与守护任务亦已退役，当前Git worktree仅main；main守护继续运行，不重复start或删除其账本。
- D9已以b056aa0集成；原执行者/workspace正式归档后自动移除worktree，tasks/c04-d9-old-input已正常branch -d。证据 `/tmp/openrust-c04d9-worktree-retirement.json`（SHA256 `2d33c1b17324d1ba1168e8670bd8684e8aa80dd9a479024c3dae55fbe92899eb`），main HEAD/WIP前后一致；不重复清理已退役路径。

### Paseo 项目归属核验

- 误project `prj_204354f586a66a10` 已通过正式 `project delete` 清理，exit 0，仅返回旧workspace `wks_e760b2e9b88fbc4c`；旧workspace退出active列表，未宣称持久记录硬删除。
- 正确OpenRust项目 `prj_34a40ee7b674b2b6` 的main `wks_0f0870e8d55cabf4` 保留；Q/D9交付后分别经正式归档安全退役任务workspace及本地worktree/分支。下方误项目清理快照仅证明当时保全，当前任务退役状态以上方新证据为准，不能把旧快照当现存workspace。
- 保全证据 `/tmp/openrust-r2q-paseo-clean-verification.json`（SHA256 `f0cc2903bff30c60b3d66fe5129247bd20b4c8640ccd17fe4bd43e29bb3d077f`），含前后快照与正式返回摘要；后续任务仍须在正确项目下登记独立workspace，不把标题前缀称为真实嵌套分组。

### 当前构建缓存与回收

- 全项目十进制50GB软/100GB硬与磁盘10%/5%保护；D9验收结束样本49.344GB、空闲约17.04%，12个去重target。下次构建前重测并按实际输入估增长，不把100GB当剩余可用量。
- main target保留复用；旧QX五生成包、本轮plain/PG生成包及四个本簇已完成测试可执行文件已精确退役，源码/锁/索引/当前二进制及共享库哨兵未变。日志明确Cargo关联test_app/auth_flows产物；不清整target，不碰归属不足的历史缓存。
- D9 My生成项目已验收，源码/锁/证据保留；如后继需要空间，先核精确package dry-run与占用后再回收其专属再生产物，不为纯Agent文档变化重跑矩阵。

## C04-D10-H 最终声明集合与真实分派同源化（已验收，限定提交见Git）

前置D10-G已提交 `c1f6a03`。root负责contract/共享文档/集成与提交；核心owner `d10c_core_continue`独占声明/分派/动作投影及集中验收；调用方owner `d10d_example_templates`的HTTP/模块/模板改动已停写。v7最终输入已通过双严格Clippy、核心all/no-default各7+5、HTTP7（含真实TCP6请求）、完整router124/120、模块5与CLI快照1；主矩阵11项均通过（默认838/34ignored、全特性1625/108ignored、双docs、4features、deny、两MSRV）。新server/CLI、fresh普通7/MySQL认证2+公开consumer1、应用真实TCP11/16及3专属package精确清理已通过，最终索引与主控复核见testing，限定提交见Git。独审/早期失败/用户§5续验决定及精确复用映射均见testing与 `/tmp/openrust-c04d10h-verification-v4`，不改写旧失败。

固定Laravel原始集合的逐method `domain+uri` view与完整methods序列view不同；G交付时的结构化(domain,path)分派曾保留PHP碰撞后应隐藏的handler；H现已统一真实声明来源，分别派生有序逐method分派与完整动作view，不建第二URL-only内核、不强求两view相等；同步真实Route元数据、Allow/OPTIONS、fallback与业务404/405终止边界。匿名无action路由亦须参与，不能仅过滤URL索引。既有PHP7+2实际探针只证明Collection，matcher/Host/path行为须真实验收。

H新增实际取证：`/tmp/openrust-c04d10h-matcher/probe.output.jsonl`35案真实RouteCollection::match/Request/Route bind及OPTIONS响应通过，统一固定Symfony8.0参考组件，无伪Request；这是匹配与合成OPTIONS证据，不是完整Laravel业务HTTP。确认仅POST存活且无GETfallback时GET/HEAD也应405 Allow:POST，H已纠正原GET/HEAD豁免404；有GETfallback仍先处理。不同模型契约/取证早审已收，root已冻结ADR-083 H契约并启动实现，Rust定向/主门禁、生成/真实服务与资源收尾均已按限定范围通过。

窄只读证据 `/tmp/openrust-c04d10g-next-collection-dispatch-evidence.md`（SHA `f4bc6cdce33ddd93d2966ca9257971300e7137628be5a513707356dafdc3b0c3`）；Spec明确Rust约束/公开API影响/受影响调用方后实施，唯一模块、生成应用、适用门禁/MySQL与真实TCP随簇交付。不以本项关闭完整Routing。signed候选窄证据 `/tmp/openrust-c04d10g-next-signed-evidence.md` 已指出key resolver、relative渲染、raw query前置；补充只读密钥审计 `/tmp/openrust-c04d10h-next-signed-key-contract-audit.md` 核实URL key使用配置原始字节，以及直接hmac0.13与sha2 0.10的digest版本不兼容，后继Spec须收敛依赖/门控，不以现JWT/AES解析器套用。另 `/tmp/openrust-signed-config-live-dependency-audit.md` 实核启动后ConfigRepository冻结，当前无公开运行期set或请求局部覆盖；每次读取不能凭空实现key轮换。后继须先收敛Laravel核心Config::set/配置可见性与启动资源快照边界，不用ADR-080未来DB动态设置冒充此核心缺口，也不偷偷解冻。均未实现/运行，后继另收敛，不夹入本簇；两者无硬技术依赖，先修已知分派缺口是明确排序，不假称签名算法依赖它。

## T01 Session纯行为测试入口整合（限定试点已验收，提交见Git）

- 前置H已提交 `3065be5` 且工作区干净；本簇只减少重复测试二进制，不改框架生产API、用例函数体、断言、fixture、feature或依赖。
- 采用已审计试点：将 `tests/http/session_extensions.rs` 作为 `session.rs` 的独立私有模块，移除Cargo对应独立test入口；预期相关26+15例仍全部执行，Session family入口8→7、源文件10个不变，须实证而非静态计数冒充通过。
- DB/Redis真实后端和环境子进程入口保持独立；禁止用整套单线程掩盖共享状态，不改历史精确filter证据。
- 执行owner `/root/session_entry_pilot` 唯一写Cargo/test装配和使用main target；root独占共享文档、索引和提交。无分支/worktree，无其他Cargo/服务owner。
- 先按同工具链/feature/target/中性env记录两入口基线no-run与实际名单/执行，再实现并记录单入口；区分首次失效编译与暖缓存，不清根target，不宣称未测加速或冷缓存可比。
- 两种feature组合的新单入口41项均已实跑通过，首次/暖缓存名单与原26+15精确映射，extensions仅增加模块前缀；集中workspace门禁均已通过，限定提交见Git。
- 集中验收覆盖全部41个用例的前后名字映射、session-only/all-features及适用workspace门禁；生产/模板/后端未变的H证据须明确输入影响后才能复用，不冒称新服务测试。
- 审计依据 `/tmp/openrust-test-entry-pilot-audit.md`；真实计时、删并理由、保留隔离、门禁及资源写入本节/测试记录后限定提交，本试点已验收，完整范围及计时限制见testing。
- 本地免费执行器只读目录查询失败：默认49374端口被占，显式连接亦无兼容V2 health（502）；未改端口/服务/系统配置，按当前可用能力回退 `gpt-6-sol` medium执行此局部已定任务。全项目缓存50GB软/100GB硬、磁盘10%/5%，共享main缓存保留。

## R2-R01 核心配置运行期原始写入（限定验收完成，本地提交见Git）

- 契约为ADR-086 R2-R01；ADR-080区分可信内存set与部署/管理员Settings边界，不启动动态平台或signed。前置H `3065be5`、T01 `b7afcfe`，本簇main冻结HEAD `f8931f5`；期间SMS限定文档提交不属于本簇源码。
- 同一仓储有序set/set_many、raw/Figment成对原子发布、Register/Boot/Frozen写入、启动schema屏障及App/Provider共享句柄已实现；App.section live，typed配置/既解析资源保持启动快照。默认/批量/稀疏混合/失败与并发回归及唯一模块、consumer、模板同步交付，无新依赖/manifest/lock变化。
- core owner `/root/config_runtime_core`（gpt-6-sol/high）四文件实现与首批39+1+39通过；caller owner `/root/config_runtime_callers`（gpt-6-sol/medium）完成模块/consumer/模板及stage2门禁，现仅后继只读Spec；services owner `/root/config_runtime_services`（gpt-6-sol/medium）完成stage3，现独占本簇精确package产物回收。root协调contract/共享文档/索引/限定提交，terra独审全部高风险改动及驱动修复。免费本地接口不可用事实沿T01记录，不重复派发或建新target。
- stage2最终14命令通过：fmt、模块exact1、CLI exact1、consumer4、双严格Clippy、双workspace845/1632（0失败、34/108 ignored）、双严格docs、no-default、MSRV1.97两组及deny。root已复核15源码SHA与14日志SHA/退出码；App布尔断言及caller两处函数长度lint均修复并保留原失败，不放宽断言。
- stage3通过：fresh plain7、公开fixture exact1、MySQL8.4.13 auth2，无skip/ignored；root双进程GET默认→POST固定业务键→GET更新，第二进程恢复默认；generated plain同样真实TCP。三进程SIGINT/join/exit0、两端口释放，MySQL同容器端点前后版本/56个TestDatabase型库集合一致，不清旧库、不将旧库归本任务。
- 证据：`/tmp/openrust-config-runtime-core/verify-stage1/result.json`、`/tmp/openrust-config-runtime-verification/stage2/STAGE2-RESULT.json`、`/tmp/openrust-config-runtime-services/STAGE3-RESULT.json`（SHA `38fbf8b6c457f8c72b5fa1829d59514f15c9bf765cb44b10dc0f5adf20109019`）。root另核565输入、13命令及2TCP日志SHA、PID不存在与端口空闲，回执 `ROOT-VERIFY.json`；PHP固定探针exit0且45097字节输出与golden逐字一致。
- 资源：15个已登记target约37.4008→38.2540GB，磁盘余10.341%，未触50GB软/100GB硬及磁盘10%/5%；现仅回收生成plain/mysql/mysql-migration精确path packageID产物，禁止全target清理。两处历史约10.6GB缓存仍缺完整退役归属，未删除；生成源码<1MB可保留作为本簇复跑证据。不得将已登记小计称为无遗漏项目总量。
- 收尾：三个生成包产物按实际归属预检/dry-run后回收526163968B，Cargo会忽略path限定，故不能仅凭pkgid声称隔离；root复核49个删除路径、2109个保护项SHA，源码及主server/CLI/框架/consumer未变，`CLEANUP.json`及`ROOT-CLEANUP-VERIFY.json`留证。未全清target，两处未知历史缓存未碰；生成源码<1MB由root保留供本簇最终复核，后继交接再次评估。全部适用门禁通过后限定本地提交；完整Config、来源初始键序、PHP对象/Closure/资源表示与signed仍开放，常规MySQL结果不关闭发布全后端矩阵。未推送/发布/tag。

## R2-R02 配置来源初始顺序（限定验收通过）

- 前置`2de050f`，契约ADR-086。不可变ConfigSources、loader单次来源捕获、仓储/Provider/TestApp入口、有序TOML及现生命周期接线已实现；root负责共享文档/清单/索引/提交，callers负责配置核心，services负责调用方及真实验收，均使用既有main工作区，不重派。
- Rust门禁已通过：首轮142项；最终默认workspace853通过/34忽略，all-feature1642通过/108忽略；双严格Clippy/docs、features、MSRV1.97两组、deny；excluded公开consumer5项及严格Clippy、最终fmt通过。来源PHP操作探针、core/integration/fixture不同模型静态审查均有证据。详见[testing](memory/testing.md#r2-r02-config-sources)。
- 原Redis租约失败在单例不复现、原三项并发再次失败；具名探针确认两个测试旧namespace碰撞，但未证实生产lease根因。最终仅修fixture时间+PID+原子序号并补固定时钟回归，原业务断言/生产租约不改，原失败与临时探针记录保留。
- 最终源码绑定566项、门禁19项，root另核25项范围输入；stage2 attempt4 RESULT SHA `c4166e03738c4e506f25c7df9617c371613c6f76551373cb21f051b45e7a267b`。服务驱动v4独审与自有子进程夹具通过，PLAN已ready；只按root逐阶段核回执启动，静态审不代替运行。
- 普通fresh生成原7项+fixture1项通过，真实TCP已验证生产reader.toml的z_label/a_label声明顺序与GET→POST→GET、exit0及43188释放。回执在`/tmp/openrust-config-source-order-services/`；plain-result SHA `1cfc318371e633428248efe00a1807024f8e7f834e22151005bde4492e0f6c06`，plain-tcp-result SHA `a29731630a2bf6e29e4d5cb486b553daf6f77d0f06d61c24cb3a2468bcc93f38`。
- 测试专属18文件已回收155316224B；postflight空列表误调用lsof的读取器失败保留，root及独立后验核96保护/42生成/566源码一致。plain包剩余产物已退役；MySQL8.4.13 fresh生成fmt/check及2auth HTTP实际通过，测试库集合前后56项一致，root核4命令/44生成输入与保存的脱敏日志SHA，回执mysql-result SHA `0ec3e72db5815ca11a7a777a16d8ce3834221ce05cad170094927a314af7bd9d`。mysql/migration专属包已退役，shared保护/源码不变；root TCP两实例已通过（初始→写后读及第二进程初始值、exit0/端口释放）。最终STAGE3 SHA `887e49d2c4610ce5adec6204ca1debf31e707a18b0424f82cb91ee8af01f08cc`，root核17日志/8清理回执/566源码/42+44生成输入、3PID退出及两端口释放。限定本地提交见Git，不关闭完整Config。
- 50GB软/100GB硬与磁盘10%/5%保持。保留活跃main缓存，不删无完整归属的root-site/m5-final历史target。一次性生成包须有同名全范围归属/dry-run/保护项证据；不凭Cargo忽略的path/version限定冒称隔离。
- 完整Config继续开放：平层TOML顺序不等于Laravel递归配置发现或文件名/目录namespace映射，PHP对象/Closure/资源等表示差异亦未关闭；不重建客户端、不以此替代ADR-080动态Settings或完整signed。

## R2-ENV 端口与缓存环境命名统一（已提交2187b6b）

- 最新contract覆盖旧端口决定：唯一中性环境入口为`SERVER_PORT`，内部保留`server.port`/`config.server.port`；删除`APP_PORT`及独立`PORT`监听兼容，通用或命名空间fallback也不得暗中映射旧变量。`.env.example`使用`SERVER_PORT=3000`与`APP_URL="http://localhost:${SERVER_PORT}"`；自定义APP命名空间不得改变中性SERVER_PORT领域键。
- 用户最新纠正以Laravel官方机制/命名为准：`CACHE_STORE`→`cache.default`，撤回先前`CACHE_DEFAULT_STORE`→`cache.default_store`自定义方案；内部配置字段/序列化/读取同样收敛到default，不只对外改名。删除REDIS_DEFAULT_STORE且不新增CACHE_DEFAULT_STORE兼容，旧/撤回键不得借fallback改变选择。该contract已落实本簇代码，R02已提交；当前ENV冻结输入待集中验收。
- REDIS_HOST/PORT/PASSWORD/DB只承载连接配置，共用REDIS_DB，不新增REDIS_CACHE_DB。框架默认具名store仍为redis、driver=redis；合法业务自定义store名（包括default）与其他driver继续可用，cache.default须准确引用已登记store；不得机械替换所有default或扩展到DB_*等其他既定键。
- 基线R02 `a875dd7`/契约 `af0f62b`；core和services完成全来源/CLI/模板/consumer/唯一模块及fixture迁移，root限定集成文档/ADR/独立锁文件。两项CLI来源独审缺陷已修，真实源码/模板而非旧命名套壳；当前规则以本段和ADR-016顶部为准。
- 最终验收`/tmp/openrust-env-verification/FINAL-RESULT.json` SHA `fa7e0dd925c99504cc50143542483fecaf0124e10e3b15ad5b104655ae4c21d7`，root复核`ROOT-FINAL-VERIFY.json`：566输入、21主日志、10回执、plain7命令/43最终输入与MySQL4命令/44输入均核SHA，5 PID不存在且6端口无监听。默认workspace858通过/34忽略，全feature默认并发1647通过/108忽略；CLI22/config73/cache20/lifecycle8及适用strict Clippy/docs/MSRV/no-default/deny/consumer通过，忽略不当实服务。
- fresh普通7+fixture1、MySQLauth2与同实例测试库前后一致、root/生成TCP读写通过；额外真实HTTP证明SERVER_PORT监听且冲突APP_PORT/PORT未监听、APP_URL由dotenv插值、默认redis/改选memory各真实读写；公开ConfigLoader/Provider探针证明缓存与queue共同REDIS_DB且真实Redis cache/dispatch/run_next。旧编译/读取器/隔离失败保留，最终不放宽原断言。
- 仅清自有一次性生成/probe包产物，未知root-site/M5 target和活跃缓存保留。已登记target约41.666GB、磁盘余99.316GB（9.985%软告警、5%硬线通过），不把已登记小计冒称未登记缓存全量。consumer锁只离线对齐当前root依赖图，未升级既有包；非host metadata缺WASM缓存不代表host失败。

- 验收覆盖旧端口/缓存键及撤回的CACHE_DEFAULT_STORE不生效、官方CACHE_STORE和SERVER_PORT新规则及自定义命名空间、dotenv URL插值、具名store/自定义driver选择与缺失引用、生成check/test、真实启动端口/URL和Redis缓存，提交前集中执行适用完整门禁。按Spec→Implement→Verify→本地Commit，不推送。
- 不热改已启动listener、不改真实.env密钥。当前仅存在性核查真实.env不存在；实施时若出现旧端口键，仅安全精确迁移并保护其他内容。普通局部变量不机械替换，历史验收证据原样保留。

## C04-SIGN URL签名家族（分支提交c77521f；main组合见本页顶部）

- 实际owner `6eae4f81-893b-4f7c-8f76-ce7cf531d16f`（codex/gpt-6-sol medium），同OpenRust项目workspace `wks_8f9a9c95fbd31026`、branch `tasks/signed-url-family`，cwd `/Users/Home/.paseo/worktrees/02joa2lr/openrust-signed-url-family`，base `af0f62b28a7a2dce65e07f02ee1026b9e30f8fb3`。限定提交 `c77521f2b58b17a47ad3e585568e31c88fd8f3ae`，45文件、提交校验通过且任务工作区干净；分支提交时待集成；现已由五父本地提交 `e105dbfc58afc330b50e53b75f0b9e1424fa387a` 纳入main。
- 同一有意义API家族实现永久/临时、绝对/相对签发、原查询验签/到期/忽略、请求与中间件/Redirector、应用级except隔离及同版本当前/旧原始key解析；唯一模块及生成模板真实接线。固定PHP27个golden ID已实际断言，SHA `bc77dc16619ed6c4cfd77834f006cfdb37207607a5ab1563c3731a245dd0a684`。空key兼容不等于安全生产配置；旧JWT SignedLinks增强不冒称LaravelURL签名。
- 回执 `/tmp/openrust-signed-final-verify-manifest.md` 与补验 `/tmp/openrust-signed-supplement-final-manifest.md`（最终SHA `daf523e9e29af8203d2fb500fbbfcd094625bab564b16bf46cf3887af1b4813e`），root核补验96项SHA及清理日志匹配。定向库7/router4（真实TCP）/模块1/CLI20/普通生成7、严格Clippy/docs/MSRV、publicTestApp1、grpc host check与freshMySQL8.4认证2无skip通过。原错误与无效0项过滤不算通过；TCP异常清理和局部RouteGroup等不同模型差异复审闭合。消费者12份锁离线解析同步，无第三方版本升级；不冒称main全workspace或真实gRPC拓扑验收。
- 两自有生成包精准清理回收316715008B，清理日志/root审计留存；随后target已交R03，未知root-site/M5未碰。该资源登记仅属分支阶段；分支阶段未提前退役；现随五父提交已正常退役、workspace已归档。普通生成helper旧回执笔误已更正，删除后的原普通项目无法当前复算，保留其证据边界，不补造哈希。
- 只读三方预检 `/tmp/openrust-signed-main-integration-precheck.md` 显示与ENV main源码基线6个文本冲突；未merge或修改索引。四簇main限定集成与组合门禁已完成，保留SERVER_PORT、CACHE_STORE/cache.default、默认redis/sharedREDIS_DB与signed新能力。Vapor实部署/代理前缀、PHP对象/数组/Enum参数与完整URLGenerator/Redirector仍开放，不以本子交付关闭全部Routing。

## R2-R03 配置递归发现与文件命名空间（分支提交6482a38；main组合见本页顶部）

- 实际owner `8b64acae-1c91-4b51-b74b-db4dcc278023`（codex/gpt-6-sol medium），同OpenRust项目workspace `wks_844aa6c8c9ac2f31`、branch `tasks/config-discovery-family`，cwd `/Users/Home/.paseo/worktrees/02joa2lr/openrust-config-discovery-family`，base `e25ec7589c7662245f9d6cc4390b1fd06a920f1e`。限定提交 `6482a38d610ff0dd9f4b629b8240f29be5f274d5`，43文件、提交校验通过且任务工作区干净；分支提交时未合并、推送或发布；现已由五父本地提交 `e105dbfc58afc330b50e53b75f0b9e1424fa387a` 纳入main，仍未推送或发布。
- 固定PHP实测为准：递归TOML/真实basename/自然序/隐藏与链接边界；同名命名空间按实际遍历后项覆盖，不保证跨平台碰撞胜者。根领域配置一次迁移相对内容；同一Sources/仓储保存点路径及来源，精确文件浅合并/白名单PHParraymerge、仅子文件时最终剩余base根覆盖再重放环境层。模块overlay独立，未引入第二配置系统。
- 最终回执 `/tmp/openrust-config-discovery-final-manifest.json`（SHA `cd6483f40f80d73767fa32995064df254b5f5cf31e15617cf4861b40add6304c`），root核 `/tmp/openrust-config-discovery-root-final-audit.json` 128项哈希全部匹配；配置76、CLI21、模块2、consumer6、普通生成7+安装fixture1、MySQL8.4认证2无skip且库集合未变、严格Clippy/docs/fmt通过。config-feature HTTP 200/enabledtrue与当前URL/TLS工具10场景分开证明；旧0项/共享旧binary/旧脚本结果不冒称当前通过。生产/工具/CLI和helper经不同模型差异审查；不声明main完整workspace组合通过。
- 当前无Cargo占用，保留暖target；已登记16路径54.48GB、磁盘8.782%为软警告，100GB/5%硬线未触。三个自有生成包clean exit0只有执行者终端回报，无独立原始stdout日志，不编造清理日志SHA；未知root-site/M5缓存未碰。后继构建按实测预计增长调度，不扩大新target，不全清活跃缓存。
- R03、typed、CC、Signed四分支已完成同一main限定组合；五父本地提交 `e105dbfc58afc330b50e53b75f0b9e1424fa387a` 已包含原main与四source HEAD；组合范围和证据见本页顶部及验收记录。保留SERVER_PORT、CACHE_STORE/cache.default及默认redis/共享REDIS_DB新contract，不复活旧别名。完整Config、动态Settings及发布全后端矩阵仍开放；详细源码/验收记录见任务分支对应core/testing与Git。

## 发布前全后端兼容性验收

日常功能簇与数据库型生成默认真实MySQL8.4；后端专属SQL/驱动/故障或明确方言/迁移/锁风险仍当场必要定向，规则见[当前数据库运行政策](memory/testing.md#当前数据库运行政策)。已在途PG自然结束清理，未开始的常规PG阶段转本节，不删除支持或历史证据。

- 发布前按实际已实现/承诺范围统一执行MySQL、PostgreSQL、SQLite等后端及适用版本、最低版本/大版本风险、迁移/事务/数据安全、队列/集群与生成项目完整矩阵；先列明各后端已实现范围与仍缺能力，不能以custom SQLite consumer冒称框架全支持。
- D10-B尚未执行的常规PG Session真实TCP及PG生成认证回归归入发布待验；若后续变更暴露PG专属风险则提前定向。MySQL日常通过可以按新政策提交，但不得关闭本节或宣称全部数据库兼容。
- CI手动release-backends保留原PG18子矩阵；完整发布的其他真实服务/后端/版本证据仍须补齐。逐项登记源码输入、命令、环境版本、结果与资源清理；未执行/失败/外部缺口明确保持待验或阻塞，fake/compile不得替代。

## 完整实现后的发布仓库整理（后置，未启动）

完成1:1 Laravel及laravel-modules的既定交付范围和最终门禁后，按[发布仓库整理计划](plans/release-repository-consolidation.md)领取：冻结/备份验收快照→文档与ADR合并去重→按最终功能簇重组本地历史→最终树与完整门禁复验→核对远程状态和授权后推送。不得抢占当前开发、不提前改写历史、不隐藏有效限制或丢失来源归属。

## 阶段二：API 正文加密扩展（仅规划）

按[API正文加密与Vue对接计划](plans/api-payload-encryption.md)，R7安全contract→R11复用现HTTP中间件/SSE事件/WS消息链→R14真实浏览器/双库/多节点及流重连/背压验收；交付全站API组与局部策略、可直接引用的Vue适配和Apifox支持范围内的脚本/示例，不启动可选快速开发模块。标准协议、响应密钥、认证绑定、防重放、轮换/错误/重试边界先Spec，未验收不声称API加密已提供；不抢占当前功能簇。

## 阶段二：Laravel 官方 AI SDK 可选扩展（已授权开发，待前置）

依[AI SDK扩展计划](plans/ai-capabilities.md)实施，不能继续视为仅规划；当前核心簇按唯一队列继续，阶段一核心与对应Provider/config/HTTP/安全契约具备后领取AI-0。队列/上传及MCP工具子项各按实际依赖推进，不等待独立AI模型平台或可选后台。

1. AI-0固定Laravel官方AI SDK源码/文档/测试基线，建立全部适用能力对应矩阵与Rust必要适配Spec；官方为唯一主要标准，Prism只补充成熟体验，不引入冲突或第二SDK。
2. AI-1至AI-4依次实现统一SDK与在线驱动、官方适用能力及审核/翻译示例、自建模型服务HTTP接入、生成项目/文档/真实provider及2核4GB和4核8GB部署验收；按功能子簇Spec→Implement→Verify→Commit，不只做同名接口。
3. 不启用扩展不引入专属依赖；在线/自建服务模式不要求本地权重或推理环境。内嵌推理仅AI-5后置质量/资源/总成本评估达标才提供，不阻塞首批或V1完成。
4. 未取得服务商凭据/授权测试内容或目标环境时保留具体缺口，继续独立项；不自行付费调用、不虚报1:1或生产性能。业务产品、后台及AI模型平台仍未开工。

## 官方能力全量交付（已授权，两阶段必交）

以[官方全量交付规则](plans/laravel-official-coverage.md#全量交付授权与两阶段)和带阶段列的749条Laravel矩阵/35个模块入口为范围，不再将官方可选包留作无期限候选。完全按照固定官方概念、默认、配置、生命周期、扩展与开发流程，仅Rust必要差异举证适配；代码/示例/生成应用/文档/真实验收闭环前不能关闭行为。

1. **范围与基线闭合**：在各簇领取前固定对应官方包独立版本/源码/文档/测试，核对所有子行为、前端/命令/错误及可观察默认；旧资产按新contract重验。SDK、MCP、Boost等不能仅列名字，Wayfinder/Stream、Echo/Vite、Passkeys/通知渠道与开发工具也要给明确对应。维护内部工具/商业平台/语言工具差异按规则分类，不静默删适用功能。
2. **阶段一**：保全当前在途核心簇，独立簇可按冻结contract与既定R2–R13依赖并行，完成Laravel核心与模块内核/生成管理/资源命令；官方可选包或生态集成不反向阻塞阶段一门禁，核心必需配置、HTTP、数据库/ORM、安全、缓存/存储、队列/调度等不挪走。M031/M033/M034等依赖生态的集成在阶段二闭环，保留范围和ID。
3. **阶段二A：官方基础包与体验**：共享前置成熟后，按包实现Fortify/Sanctum/Socialite/Passport、Folio/Head/Precognition、Octane对应及额外数据库driver；先Spec默认/安全/扩展与实际生产链路，模块生态集成、官方starter-kit及JS配套同步对应，不新增第二示例。
4. **阶段二B：数据、异步、观测与工具**：按DB/Model/Queue/HTTP前置对应Scout/Pennant、Cashier Stripe/Paddle、Reverb/Echo、Horizon/Pulse/Telescope及Dusk/Envoy/Prompts/Pail/Tinker/Pint/Sail/Valet/Homestead等适用体验，Wayfinder/Stream和编辑器/分析元数据依赖随相应簇完成。Horizon/观测工具自身必要UI与鉴权随包交付，按需引入不默认公开；这不授权组织/菜单等业务后台或App。
5. **阶段二C：AI与已授权增强**：AI-0至AI-4、官方MCP Server/Client、Boost按真实前置组合，SDK不能改成另一套机制；MQTT、gRPC、DB动态设置及既定增强继续。AI-5内嵌推理后置评测，不因全量SDK强制模型运行环境。A/B/C是职责批次，可按真实依赖交错，不把列表顺序变成循环前置，独立簇可由总控按依赖图批准并行，main集成与最终验收串行。
6. **最终收口**：每个官方包逐项行为、公开扩展、唯一模块、生成项目与真实服务/浏览器/协议证据均齐备，Rust差异/外部条件缺口公开；两阶段/C33证据汇入R14，不能只关闭核心便称完全体。若缺账号/许可/服务继续独立项，禁止fake冒充实测；不推送/发布、不改写历史，业务后台与AI产品仍未授权。
