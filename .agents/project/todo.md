# 活跃待办

> 当前代码与目标：业务示例已迁至 `modules/example/` 并经模块清单接入；R1-E CRUD/能力门禁已由 bc3fa01 提交；附件 PG/MySQL 全量各 15/15，真实 MinIO CopyObject 延迟响应时的双库删除竞争各 1/1；deadline/EOF 原子仲裁后，远端结果不确定的迟到发布仍属未保证边界。OIDC 确认页已完成 Chromium/mock 分支验收，真实 Keycloak/双库 `AlwaysOk` 错误密码、已消费 state 与成功夹证各 1/1；Collector PostgreSQL 隔离 sink 的 delivery/unavailable 各 1/1 且保留 16 MiB 限制，runner 路径和 fixture run ID 已修正。R2-N 已由 48be854 提交；R3-A/B/C/D/E/F 已按适用范围验收，C04-A/B1/B2/C已按子簇验收，后继C04-D Session/flash与Web栈契约待领取。

> 受众：AI Agent
> 更新：2026-11-02
> 摘要：Laravel + laravel-modules 的 Rust 1:1 实现唯一活跃队列；R0旧基线已记录，模块包逐项盘点待补；R1-S、R2-A至R2-N及 R1-E 真实 CRUD/当前公开能力示例已分别交付；R1-E 提交 bc3fa01。R2-N 由 48be854 提交；R3-A 语义 HTTP 状态默认已按子簇交付，继续 R3 HTTP/Web 路由、请求、资源与异常行为矩阵。远端结果不确定后的迟到发布边界保留。

## 领取规则

收到需求、审计或回执先判定补充/纠正/明确替代；未明确取消的原任务继续有效，重复内容合并、过期结论标明替代关系。每次处理消息后恢复原owner停点，记录范围、实际状态、下一动作与未闭合验证/提交/清理；切簇、压缩恢复和最终提交前核唯一todo，不用最新聊天或已派发代替完成证据。仅源码、匹配验证、本地提交与必要资源交接齐备才关闭对应范围；已编码待验、已提交未集成、外部缺口分别保留。全量审计归并本页/既有矩阵，不覆盖已确认后继、不新建平行队列。

本轮最新顺序：先收齐当前已在途 main 组合源码、调用方、模板、唯一示例和说明，冻结后一次集中适用验收，修复失败并通过后本地提交；不再追加新簇延长本批，也不以全计划最后测试无限推迟提交。随后处理今日尚未吸收的整改，再按 ADR-001 持续推进。未验证源码保持 WIP，旧证据不冒充本批通过；默认20%额度保留；构建产物累计达到 100 GB 或磁盘可用空间低于 5% 时，回收本项目全部可重建的编译产物，在途构建释放占用后也须清理，复测后继续。

后续默认同一 OpenRust 项目、同一 main 物理工作目录，多个对话框或 workspace 标签页按不重叠文件分工；不再默认创建任务分支/worktree。产品不支持共享目录标签页时复用既有 main workspace 的多个对话框，不新建源码副本或修改内部数据库。同文件串行交接，公共 API 先收敛；manifest/lock、共享文档、Git索引/MERGE/提交、target/Cargo和真实服务由总控唯一调度。冻结受验文件禁止并发修改。

已授权框架范围内本地 main 集成沿用用户持续授权，不重复询问；先组合本批源码再集中验证，通过后限定提交及安全退役。当前环境身份`8d5e635`与 Request/MIME`5e2ec6b`已获本批明确合入授权，组合前保全所有WIP/证据，本次集成owner独占main索引。

先核对 Git/WIP、代理/进程、[执行校准](plan.md#执行校准) 与 [ADR-001](decisions/adr-001-framework-principles.md)。按总控依赖图领取；契约明确、前置满足的独立功能簇可按当前共享main文件归属并行，各自完成 Spec → Implement → Verify → Commit，main集成与组合门禁串行。整体依赖见 [R0–R16](plans/laravel-rebuild.md#5-按依赖实施)，不按旧 M5/M6 顺序执行。

## 从零重建与并行执行授权

用户再次明确按 Laravel + laravel-modules 的完整 Rust 实现从零设计、重写核心；旧 API、目录和实现不构成兼容约束。新内核以固定上游行为为依据，不继续堆叠旧 Plugin 装配；旧源码仅作事实与回归输入，由对应功能簇替换后通过 git rm 清除失效实现。此授权不删除他人 WIP、共享服务、数据或原始测试证据，也不要求重造成熟密码学和数据库驱动。

R2-M 模块闭环、R1-E 综合示例与 R2-N 模块依赖顺序均已在 main 限定提交；当前在途主线为R3 HTTP/Web后继，现有WIP由原负责人闭环；总控可从唯一队列另选前置已满足的独立簇，按当前共享main文件归属并行。共享Cargo清单、锁文件、验证服务和main集成串行协调；各任务登记负责人/依赖/分支/目录/验收/清理状态，细则见[并行开发与集成](conventions.md#并行开发与集成)；子代理不得递归派发。

## 当前可领取

### 本批组合交付与清理停点

- 存量收尾已对齐 Session/gRPC consumer、共享导入、示例协议与现行 API 文档。下一步回到依赖已满足的 AR-N01、Container 与 Model 后继。其他数据库实库与最低版本验收仍在发布矩阵。

- 本对话为唯一框架集成总控；旧总控与旧守护保持停止，不迁移其 Goal 或额度授权。main 组合已收齐最终适用门禁、真实服务与独审，本地集成提交 `9035a00`；基线 `317e2f6`、环境来源 `8d5e635`，MERGE 已结束。完整证据与开放边界见 [本批验收](memory/testing.md#当前-main-组合限定验收)。
- 九份真实来源、六份 WIP、11 个 source refs、完整历史 bundle 全部保全；所有非 main worktree 与两个缺失 guard 登记已正常移除，八个来源 workspace 已正式归档，当前仅 main。证据 `tmp/controller-handoff-20261102/`，不重复 merge 或删除。六个任务分支活动别名已退役：两条按祖先正常删除，其余在核实同对象保护引用、完整bundle、无worktree后以带旧值的compare-and-swap移除别名；所有原提交与WIP仍可达，回执 `tmp/takeover-20261102/archived-task-branch-retirement.json`。危险 `cd487bb` 只归档保全，绝不合入。
- 源码/调用方/模板/唯一模块本批冻结，shared docs/index/Cargo/service 均由新总控独占。旧失败不回填通过，真实 MySQL 与 TCP/TLS 已闭合限定范围；完整 Query/ORM、Provider包发现、配置默认依赖、模块管理、全后端与集群等仍开放，不因本批通过关闭。最终记录 `tmp/takeover-20261102/`。
- skill 本批仅接纳旧142/61测试及不同模型最终独审九SHA完全匹配的版本，精确快照与索引整合证据在同目录。后续 ended-grant/Codex 原生适配由另一个获用户授权的 owner 在同 skill 收口，未部署，完整工作文件保持；本轮最小恢复判定补丁及并发漂移报告已保全 `/tmp/openrust-guard-recovery-consistency.*`。不再并改，不重启旧守护，不把开发中适配当已生效。
- 默认80%预算政策和旧账号历史、问答/发送账本保持，未rebase/fullgrant或伪复用旧授权；本对话已可正式读取共享账号窗口，数值按当次回执汇报；不重算基线。成功验收的自有服务进程与端口已收尾，数据库集合无增删；完成簇根target已正常清零：48,729,780,224B→0，磁盘可用202,384,363,520B；源码/生成fixture/证据保留，下一配置簇按需复用同一路径。

### R13 文档正式对齐：固定 Laravel 13.x 主导航（真人授权）

1. owner=root，去重既有R13/docs；当前命令簇先提交，不以全量文档延迟当前簇或SQLite。首批mdBook骨架/规范整改与下一模块/API宿主簇同步，已实现章节迁移和后续各功能同步交付分别取证。
2. 固定docs13.x commit960eedf的documentation.md实际章节及阅读顺序为主导航，Modules按固定包来源独立章节组；OpenRust—Laravel的Rust实现。内部102章/749节矩阵和Modules35入口不是用户指南，不复制上游整章或以空页/矩阵数量报完成。
3. 首批修规范旧口径：conventions的M6建立/Diataxis主导航、README旧M6建站承诺、style强制TOML及同类当前规范；保留有效历史M6验收记录，不机械全替换。默认config/*.rs＋.env，TOML仅显式可选。
4. 各已实现章连接稳定ID、固定上游源码/测试、当前Rust API/配置/CLI、正常/错误/安全边界与必要适配/用户特约/未实现缺口；指南讲完整流程，Rustdoc作API参考，README作入口，Agent计划不混入用户教程。
5. 建站沿用mdBook；不全局安装、不部署/推送/发布。适用构建、链接/锚点/旧引用、Rustdoc/真实编译示例、CLI失败退出/生成check+test及相关真实服务验收；复用绑定当前源码的有效证据，不逐句全矩阵。
6. 状态：首批8页指南、代码配套、mdBook构建与189链接已随宿主簇限定验收，规范已改Laravel主导航/默认Rust配置；不代表全量迁移。未迁移的已实现章节及后续官方生态指南仍开放，SQLite优先并随簇补章，不以全量文档延后SQLite。
7. 五项负责人/依赖/状态/下一步吸收回执tmp/docs-laravel-alignment-20261102/handoff.json。业务后台/独立AI未启动边界保持。

### R13 明确补充：Laravel CLI 使用方式全对应（真人授权，按依赖穿插）

1. owner=root，沿原Goal/main/预算。当前冻结命令所有权簇先提交，不追加本项；随后优先模块入口/API小闭环，SQLite尽快，CLI独立子项可穿插，不以全量CLI拖延SQLite。
2. 固定Laravel framework/docs及nWidart为准，逐命令核名称/参数/选项/默认/错误/退出码/交互、产物/自动登记与应用生命周期；不是机械别名。R13已有项去重保留，缺能力按原依赖补齐，不伪造支持。
3. 已确认待对齐：queue work→queue:work、queue retry→queue:retry、schedule run→schedule:run、db seed→db:seed、config show→config:show、key generate→key:generate；generate迁移/mail/policy/job等对应真实make:*，mail不是机械make:mailer。make:controller/model/provider现名已存在，行为/选项仍核。
4. 安装器new与应用命令职责明确，Rust入口openrust可保留。静态CLI与应用Command解析复用唯一规范；Provider/Command/Container扩展、发现/help/list、未知选项/有效参数及退出码/自动登记须一致，无历史兼容义务不等于吞参数。
5. 每个可验簇同改消费者/模板/快照/文档/脚本及唯一example；真实CLI/生成check+test，核路径/内容/登记/重复冲突/失败不破坏文件、错误退出码，独审和适用集中门禁后限定本地提交。完整CLI开放范围继续保留，不只help快照宣称完成。
6. R13 的嵌套 `make:controller` 已在 `3b4c4a524a7b` 限定交付，支持相对命名空间、逐层登记与失败补偿。应用命令目录和用法按 Laravel Artisan 介绍接入 `list`、`help <命令>`。命令签名、统一命名、额外列表格式、模型与 Provider 生成器继续按依赖领取。

## 其他旧兼容整改（已授权，当前Fortify优先）

| 稳定项 | owner／依赖与范围 | 验收与排期 |
|---|---|---|
| OLD-VALIDATION-02 | root；固定Laravel规则核对后收敛`Rule::length/range`及runner旧分流 | 保留空白/空串/安全回归；有价值增强经官方custom扩展；Fortify后按依赖实施 |
| OLD-QUEUE-03 | root；定义driver上下文contract后处理`run_next_with_state`默认丢AppState | 内/外driver与worker、consumer、唯一示例显式满足能力，适用真实队列/并发回归；不能静默忽略上下文 |
| OLD-GENERATOR-04 | root；待Fortify释放`tools/cli/src/main.rs`，去旧vec迁移骨架回填 | 当前公开迁移扩展保留，真实new/check/test与冲突保护；独立簇不延长本批 |

待判定：`db/database`双路径按正式API目标核对；Fortify配置官方key与Rust字段名按必要适配核对，未判定不删。
排除误报：固定Laravel的VerifyCsrfToken兼容本身属于官方对应；APP_PREVIOUS_KEYS轮换、S3兼容、当前queue无metadata成对payload协议、非空seaql_migrations拒绝保护、正常模块activator、显式可选TOML均不按旧OpenRust兼容删除。
只读来源：`tmp/fortify-single-implementation/other-compatibility-audit.json`，非全仓穷尽；行号按当前源码重核。持续Goal中实际分簇实现/验证/本地提交，不能以本登记冒充完成。

## Query 剩余对应

- DB-COMPAT：owner=root协调，目标MariaDB 10.3+、MySQL 5.7+、PostgreSQL 10.0+、SQLite 3.26.0+、SQL Server 2017+，详见[兼容基线](plans/database-compatibility-baseline.md)。当前vendor/openrust/framework/src/database/mod.rs仍PG14/MySQL8拒绝阈值，MariaDB/SQL Server及旧版SQL/驱动差异未闭合；先逐项对应生产链，不只降阈值。日常仅MySQL8.4，其他类型/最低版完整运行转发布前，不能扩当前C07或推迟AR-P01。
- SQLite 写目标发布验收：`9fb58bb3` 已将联表和限量 UPDATE/DELETE 接入固定 SQLiteGrammar 的 rowid 子查询语义，保留 rowid 遮蔽与 WITHOUT ROWID 原生边界。默认和日常 MySQL 组合、严格 Clippy、Rustdoc、依赖门禁、Web/JWT 生成回归与 SQLite 生成测试目标编译已通过；发布前须对当前 SQLite 和 3.26.0 各运行框架、示例、生成项目实库回归，含实际迁移。 发布矩阵命令见 `tmp/sqlite-write-v4-v8-review/review.md`，对应当前提交的日常证据见 `tmp/local-closeout-20261107/sqlite-write-after-j01-v9/COMMIT-RESULT.json`。

- 完整FROM/跨连接、Query/ORM及官方五后端最终验收仍开放，不由before-query子簇关闭。冷PREPARE失败淘汰的限定事实及Cargo必要构建约束见[testing](memory/testing.md#mysql-冷预处理取消与连接淘汰限定验收)。升级/发布前核对应上游修复及patch实际生效，未证明任意网络阶段取消后同连接可复用。
- 当前有限簇完成后按下一已授权Passport/OIDC排期与依赖领取，不重置原Goal/main/逐账号预算历史，默认保留20%，本次明确普通额度覆盖见下方回执；credits=false。

## 总控核心与并行分工（最新真人排期）


数据库运行策略以[当前数据库运行政策](memory/testing.md#当前数据库运行政策)为唯一权威：日常仅真实MySQL8.4；其他数据库与最低版本运行统一发布前验收，不增加当前Query或AR-P01的非MySQL运行门槛。

最新真人顺序：MB-A01、C07-W01/W02及AR-P01均已限定验收提交推送，AR-P01提交4463238且远端SHA已核 → AR-M01已由6c21bab0限定验收提交推送 → AR-N01命名空间与开发流程对应。J01、SC-N01及Model日期准备保全WIP，不作为迁移的额外前置；SQLite、数据库国际化和Passport按互斥文件并行编码，公共contract、包路径、manifest和集成由root串行收敛。工作交接与验收按本节当前职责执行。


- AR-N01（完整官方职责与开发流程后继）：owner=root；职责命名空间限定迁移已由`b3f6117be1fc`提交，范围与[验收证据](memory/testing.md#命名空间与开发流程限定验收)分别记录，不能以搬迁关闭完整Laravel语义。唯一Container/Support/Console/Foundation/Database/Routing/Http/Translation/Filesystem/Encryption实体、调用方、模板、示例和文档入口已收敛；继续补齐[命名空间计划](plans/laravel-namespace-alignment.md)中Contracts及Container绑定/生命周期、Console命令语义、Routing/controller/middleware、Translation默认FileLoader/choice/deferred、Filesystem完整契约/默认及Encryption官方cipher/key/payload等差距，按依赖领取独立有限簇，不建空门面或第二体系。Source、SQLite、Passport、J01、SC和Model后继WIP不计入本次验收。
- CT-B01（AR-N01子簇，绑定目录与条件登记）：owner=root；统一ServiceKey/编译类身份/投影与目录已隔离编码，尚未集成或运行。当前资源关闭前置v6由container_state_v3_review修订：同源controlled聚合逐项析构、OwnerDrop/Scope close/reset、真实Tokio直接Drop Pending观察任务、普通任务panic在进入JoinError前受控处理，保实际owner失败与完成栅栏，不新建调度表。v5原机制修订通过静审，但两关闭聚合旁路及直接Drop覆盖不足仍阻断，原780断言和失败输入保全；v6已冻结（receipt SHA 9d0ac1adef969eb8c032ea6e2c46d44d79b8f6669401c436e4e23dc0ebad6104），Astra独审已发现seal认领child.close到实际retire间另一shutdown可先缓存Ok的竞争窗口，全审已收齐两项阻断（另有真实用户future poll+Drop双panic的外包层次），v7已冻结（receipt SHA 7a3af02e08712d830ca4b7a4a67a31973a7522ea2acd72811dd0a2be4d3e37d1），v6两反例静态闭合；Astra全审另发现未交付raw结果（末检/发送槽/等待completion取消）及调用future最后FactoryHandle绕隔离析构，v8已冻结（receipt87a7743d…）但独审三阻断：reservation过早完成位、并发retire活跃caller漏unknown、嵌套未poll raw捕获旧Resolver参数；v9已冻结（receipt04302d3f1ee20f3068a77a41bfe8b1917686ffb2261f4d6ef3e68f784d350695），原三路径已静态修正；完整独审又确认克隆旧Resolver参数共享状态双计/误拒，以及未交付Resolver结果析构吞panic漏报的真实反例，v9不得交运行。Root已完成CT10精准末次析构/实际delivery责任合同784c2de4…并经Astra设计准入；v10已冻结（receipt `3dc78b6a…`），独审`ed0442a4…`确认普通ResultFuture清理路径的CT10-1阻断：绑定/扩展擦除可在框架析构Future捕获的末次Resolver时吞掉已完成原owner的参数失败，当前实际owner账本漏报。v10保全不交验；v11 已冻结合同 `6af6052b1d4c0131ca8677cd65a866d66856524dc62cf5ab33ac13d83907886d`。普通回调、取消与捕获析构已隔离实现 v11（回执 `9e11ff02f61b006687f30a4b219b4e04c6607906b9574472d48efc7d1ea909d3`）。保留旧1098断言，新增8测试58断言，旧live对照在 v11 校准到实际末次引用，格式及补丁重放通过；独审 `17e29adc93423cc3bca39d09192277912e3ccbb3f43631a936690c9a896b4b5d` 静态关闭 CT10-1，组合与运行待验，不盲授票据、无全局owner表、不改已完成历史。绑定替换/Rebound聚合的真实owner归属另收Spec，不能关闭整个CT-B01；descriptor/constructor/metadata与完整Factory/示例/consumer/生成继续开放。来源与逐版证据在tmp/container-bindings-20261107/，不混当前Core受验输入。descriptor/constructor内部候选 v1 已冻结（receipt `c1f3fc7ac0ba4d41793f4942a8962c27b89ee50ebd79ddb466a87836a19ddbc9`），37 Rust 叶，保留 v11 全1156断言，新增12元数据测试53断言及已审投影3测试8断言；仅私有cfg(test)基础，独审 `fb247c7fd61a0ea7fbf6482140ddca7f48b24beec9cf476c4e19ebb65d616e20` 无静态阻断，真实Resolver/PHP转换与生产接线仍开放；真实 Laravel makeWith→build→new 构造标量参考已写 `tmp/container-bindings-20261107/constructor-scalar-reference-v1/oracle.php`（244场景双profile、script SHA `71094c05a4bc467fb272a3b71b66cc2aead853db55a9e4c97a66af026c9384c9`），v1独审定位误用warning-control和helpers摘要漏字，原件保全。v2 script `4fc8c4c78573000c0f3df7b5a4c79e492e1775006a5de024a840b76850057923` 使用已批准正常环境并加启动严格边界，独审 `c571b23b77f0f3dc9d8b59a8eaee91f3c329014cebc9fe8fa63c520e49ec8b8e` 通过，handoff `5f4d4576a215f25c1cb19e7103d7a2e3962ed29375da6c1f7a26540c9da4dfae` 已完成独立PHP实际244场景/488观测，RESULT SHA `7a6b220fbc128a5af1a245f1f875c479c995df1fc51df33ec4528ddbe45ec0e0`、raw `e14b7e7be22800a4325f4c21df7f936fd8c87bb2ea41cf946d13601b7e78bed6`，Root复核 `7f7b51a27caceaa09cf9fc5b999355beed45915350ff63a8028c51f26c115f01`。record181返回/63抛，实际未bootstrapped HandleError178返回/66抛，3个NaN E_WARNING转ErrorException；默认/显式null/contextual/variadic细项原值保留，非244通过。已交实现参考。Root生产接线草案 `a3dcc51511cbac8b41f513d92b59c79cac1a4957420358ea1d1000e0246c7f91` 正在独审，已识别descriptor缺binding时原generation准入/末检与history不适用、以及解析chain不能充当构造依赖buildStack的设计阻断；独审 `f700e25809f0ff7ad4e2ab9464fde3649701affa9ebf8fb00ac7d8264a6d7efb` 收齐三项阻断。Root v3合同 `53a9e704b9009527b1a55f0db8fd26cb3d69817d00a13c58d6adb25eb128e410` 经 `043ac368f1a356a6a568d990befc2f9abeb2eeae9de1edcb70db19e90c7b8a92` 仅准入唯一Registry安装/原State私有选择revision与alias路径见证阶段，已交container_state_v3_review隔离实施；完整fallback/public仍不准入，原binding/instance/pending-extender retry保持，descriptor new已开始后的过期策略另收合同。实施核出旧forget_instance在无实例时也丢pending/失效进行中构造；固定Container.php:1739仅unset instances，Root补充 `3feb8670f1446a404289516278b5d9f5358624bb642bba285545a485fc05f793` 明确无显式实例/Ready缓存为no-op、原晚回填断言改真实instance(999)替换后forget、另加no-op/pending回归；无实际forget-binding/flush入口仍列后继。Registry基础v1已冻结 `03d34f09fda2b18cd92e19b622390033ef31b21877b3135f3e810b778b853710`，六路径保1217旧断言并加7测试/93断言，独审 `1c4863238d6e731403e70a099848fe8d6c18646ab8221b8de1a5a67922722058` 通过有限静态边界；非空Registry仍仅测试装配，真实构造接线及严格门禁待续。Root标量合同 `0e187672f3e8489cce74a28e7e5d90b9411c34f75e98f15b322a943a1eecb510` 按244/488观测落实选值先后、数值/字节/union身份与原错误处理，待独审。 私有弱标量候选 v1 已通过静审（`07a5b42b…`），231×2 Rust 对照仍待运行。补充 PHP 顺序 oracle 15×3 已静审；实际 Resolver 接线逐文件审计 `0615aaeb…` 确认现 TransientGuard 依赖真实 binding generation、现投影参数缺目标 typed key。唯一 Resolver 私有输出元数据已隔离实现（`246e2b9e…`），普通 typed 查找、named/alias 与旧预检保原语义，raw/Rebound 路径保留。输出元数据独审通过（`61909238…`）。描述符 admission v1 独审发现真实清理/成功 history 顺序及 extender 等待风险。修订 v2（`19eaa117…`）冻结 new 后读取已发布链、旧构造等待仅匹配 Binding 来源、锁外真实观察退役；待独审。生产选参、owner 退役、换源错误与诊断接线仍开放。
- CT-B01 当前执行停点：已接入 Config、Give、Tag 和普通默认/具名 Database、DB、Storage 属性。数据库路由名 ::read、::write、::direct 继续随管理器对应；不从普通连接推断路由。验收见 `tmp/lead-manager-attributes-Oe0rY1wG/delivery-receipt.json`。Cache 默认/具名及作用域 memo 已完成限定验收，见 `tmp/lead-cache-log-attributes-EO5et1q6/delivery-receipt.json`。Log 属性及 L028 的默认/具名频道、重命名、single/stack/stderr/null 和应急回退已通过限定验收，见 `tmp/lead-log-channels-3DXNdx4Z/delivery-receipt.json`。L028 后继保留轮转、网络 driver、共享上下文、tap、事件、弃用频道接线与 Pail；RequestAttribute/RouteParameter 及可信类型化 Route 绑定已通过限定验收，见 `tmp/lead-request-attributes-YNrhEqpl/delivery-receipt.json`；通用隐式模型绑定沿路由独立项推进。CT-B01 已接入 Auth、Authenticated、CurrentUser 和请求级用户解析器。Laravel AuthServiceProvider 的可空 Authenticatable 默认绑定仍需单独对应。已接入作用域日志 Context 仓储、构造属性与日志 extra。完整仓储方法、队列和命令传播、生命周期事件仍在 L028 Context 后继。CT-B01 已接入环境 Bind，验证首次解析按环境匹配并登记普通工厂。BindWhen 已接入顺序、未命中重试、条件错误、同键重入和并发首次选择。固定 PHP 的条件内同键显式重绑会被本次声明覆盖，该副作用次序仍待对应。后继为 Singleton、Scoped 与容器完整 flush。




当前持续交付授权：真人已明确要求已验收的完整簇及时限定规范提交并推送origin/main，不等无关WIP或全部计划结束。Lead唯一负责索引/commit/push，Architect不并发push；先fetch核远端祖先，以验收绑定的准确commit SHA安全快进推送，ls-remote核远端hash。主目录有其他WIP时保护原索引/源码，使用临时clean detached树核准确提交后显式SHA推送，再正常移除；不stash/reset/夹带未验源码。此前1a7473ba及祖先已远端，禁止按旧全本地历史重组。旧仅本地/不推送口径由本次专门授权覆盖；force、tag、release、发布与改写已推历史仍不授权。

当前开发、验收和交付统一归 Lead 总控对话。互不重叠的已授权任务可在该对话内由子智能体协作。Lead 在项目内调度 Cargo、真实服务和适用 main 门禁，核对精确源码树后提交并推送。既有独立任务只收取在途结果，保全独有源码、索引与回执后退役验收副本。

簇层级固定：能力族≠可独立交付的完整行为簇≠单方法/修复。开工固定簇ID、边界、上游与退出清单；源码/配套、Laravel逐项对照、适用运行验收、限定本地提交及资源交接全部满足后，主控才领取下一主线簇。已完成簇保持稳定事实，后续新缺陷/需求另建关联整改，不用同名反复完成/开发；上层能力族继续列未完成项。报告固定列簇ID/边界、编码、对照缺口、验收、commit、下一簇。并行owner可在互斥文件推进独立ready簇，不扩大当前冻结输入。

最新开发节奏：有限功能簇先Spec固定上游contract→集中补齐实现、调用方、配置、生成、唯一示例、测试代码与文档→逐项Laravel映射及独立静态审查、批量闭合编码缺口→冻结并集中适用验收→失败按影响修复复验→通过后本地提交。状态区分编码中/对照缺口/冻结待验/验收失败修复/已验收已提交；最小编译或歧义诊断探针允许，静态对照不等于行为通过，当前受验批次正常收口不重来或加簇。

- MB-A01（模型属性与认证前置）：owner=root；独立验收已闭合，准确树`f8e5800fe8721cbbc83030f754ee799481981a04`由`06b4d50c6c89001a2db0adee2ff54f13d78bffb8`限定提交推送并核远端，其他WIP原内容不变。原始行、metadata/getter/基础cast/异步setter、可逆回执与认证committed状态、根/模板/唯一示例配套通过；完整生成/适用consumer及资源收尾见[验收](memory/testing.md#模型属性与认证前置限定验收)。
  后继仍保留完整Model、日期及值载体、PG显示与未知DB提交/跨连接顺序缺口；通用scalar大指数后继另列。当前先绑定实际提交后的C07-W01/W02有限组合验收，不夹J01、Passport或Source WIP。
- Model 日期转换与类型访问器已由 `7129b5c9` 限定交付。完整 Eloquent 持久生命周期、日期行为及调用方差异继续按 Model 计划跟踪。
- NumericDate 纯 Core 与生产示例路径已收齐[数值日期限定验收](memory/testing.md#数值日期格式化与解析限定验收)。保留完整 Calendar、Model 日期、Passport/JWT 消费与发布矩阵待办。
- C07-W01/W02 有限Query组合：比较、六个多列入口/Closure、必要空FROM与受控raw投影及时间线search+mode、两个生成query测试与文档已编码。唯一交验入口tmp/independent-testing-20261106/c07-main-ready.json，实际main基线b122457含MB-A01；最终v8树e37eb27bffbc3a9e539869893dd39710cad2238a、22文件/1546输入，已由ed3f608b924ba5cded2ed6f5d238011077dd0a04提交推送且远端核同SHA。首轮5处E0624与3处E0505已修：query内transform可见、保原错误断言后释放Result再回滚；v3 fmt/all-target/all-feature check通过且不同模型限定静审0阻断。v3严格Clippy的17项已集中修复为v4（精确imports/match、移除零调用私有方法、NULL构造提helper、等价断言等，无宽泛豁免），9文件差分独立静审又发现compare长度与五测试叶传递导入缺口，v5已在原求值点抽Bound RHS并在测试叶显式补原名字（不回加生产imports），6文件差分独立续审已关闭两项静态阻断（函数体97行），v5实际严格Clippy仅余multi_column未使用ExprTrait一项，v6已删除这一导入，实际全target/all-feature严格Clippy通过且root核日志SHA；v6完整all-feature workspace test通过、12 consumer locked metadata通过；双库比较/分组/factory/时间线实际通过，MySQL Raw算术投影仅i64解码失败，PREPARE已确认DOUBLE值6，v7仅将该fixture匹配为f64==6.0并保原SQL/绑定/负例。v7该MySQL失败项实际exact复验1 passed/0 ignored且catalog恢复；随后新lib二进制双库comparison/multi/factory共6命令9项实际通过无skip，root核日志SHA与catalog，夹具读取问题已闭合，时间线沿v6未变输入映射；v7默认严格/默认workspace tests、双Rustdoc、精简features、MSRV两配置及deny实际通过，root已核日志SHA。最低3.26实际comparison15/16、multi16/factory6/subquery1，唯一失败为fixture手写不支持的箭头SQL；原生oracle复现后v8仅改json_extract并将原箭头留SQL注释，保bindings/unbound true/count并加arrow文本断言，已导出定向复验，不能先报39全过。fresh三类与consumer两项继续独立验收，原失败保留。最终C07-RESULT正式回执及root核20份收据/46日志已收齐；限定日常适用验收通过并提交推送，不跑两套同内核完整矩阵，不夹J01/Passport/Source；完整where/Query仍开放，PGjson_contains实际复现留J01后继。
- 并行扩展：insert_id_adapter_review已在tmp/passport-code-pkce-20261105/repository-candidate实际编码Repository/Bridge持久读取与测试；不伪装尚缺save/events/global-scopes完整写链，已交Client/协议WIP保留；前置为统一Model/Hash/Query，下一验收为真实Client持久链与授权码/PKCE，然后OIDC OP与双RP SSO。不得另建核心替代体系。
- C07-J01 简单列 JSON 包含修正已提交推送 `e0b9a873`，远端已核。冻结树 `14e09c10a75d` 保留 7 个重叠 WIP 前镜像及原复验失败，覆盖 PostgreSQL SQL 编译、绑定顺序、MySQL 8.4.13 示例与 Web/JWT 生成应用、PostgreSQL 生成应用测试目标编译及适用 workspace 门禁。PostgreSQL 实库在发布矩阵执行。路径、限定列、OR/NOT、SQLite 包含及 raw 会话后继继续开放。原 Spec 和候选证据见 `tmp/query-multi-column-20261106/json-contains-canonical-v3/receipt.json`，本批推送与门禁见 `tmp/local-closeout-20261107/c07-j01-after-pivot-all-lints/PUSH-RESULT.json`。
- C07 raw编译后继：普通PG字符串的非标准会话模式、MySQL转义mode与版本门控执行注释的参数编译仍需真实连接能力/会话元数据闭环；现受控API明确拒绝歧义或未知版本参数，不将完整raw支持划不适用。日常MySQL8.4不等于框架最低支持版本。该后继不追加进当前已冻结C07组合。
- root唯一调度公共contract、manifest/lock、迁移、共享文档、Cargo/服务和main索引；冻结验收期间不并入新源码。各owner当前实际产出和后继证据位于tmp/full-speed-20261106，ready任务持续复核。

- 2026-11-06真人要求全速并发并取消本任务额度保留线，root已用正式read_thread核原始消息；仅本Goal普通额度覆盖，credits=false、平台硬限与原80%历史不变。Desktop正式覆盖入口适配由query_import_timeline_example负责，正式入口已由root激活，checkpoint=continue且full_budget_once=active，原account_budgets逐字值相等、credits=false；root负责来源/激活/共享集成，query_between_columns_impl修默认feature Model测试，insert_id_adapter_review复审冻结Query/唯一示例。原insert_get_id有限簇不扩大；Cargo/服务/索引串行由root负责。证据tmp/full-speed-20261106。

## 已授权后继：L090 Passport 与 OIDC SSO 扩展



- EV-R01 Events身份与响应公共前置（Model save依赖）：owner=root；1621项v5及24路径源码树47b1f9dd62ca7583f53117bef7e60a65bc150553已完成[日常适用限定验收](memory/testing.md#事件身份与响应限定验收)，正式报告SHA2d5371853fe5b9b39df3aa905a29b061447eea23441896ebd4d25a4f37bd0a6c；有限提交/推送以Git为准。具名/类型化同一选择器、保身份响应/until、跨包/Guard/Fortify/真实MySQL、workspace/fresh/文档/MSRV/离线依赖已验；原失败、ignored及缓存advisory边界保留。不含Hashing/Schema/CT/Passport，完整接口/多参/defer-aftercommit、可变Model/全mixed、官方事务connection载荷与Model save仍开放。

- EN-L01 加密 Provider 默认生命周期：按固定 Laravel 的延迟解析契约收敛首次使用，核根应用、CLI、生成、Cookie 与 Session 的安全失败和退出边界。以当前生产接线核对剩余差距。

- Passport OAuth2 已由 `8bd2e20c` 限定交付。下一项先核对 Passport 剩余行为及授权服务器前置，OIDC Provider 与双 RP SSO/退出仍开放。
- MB-S01 已收齐[调用级默认属性 setter 限定验收](memory/testing.md#调用级默认属性-setter-限定验收)。Passport Client 依赖其调用级默认 setter，下一步从准确已提交父树接入有限组合诊断。
- PHP8.5框架异常边界后继审计：原始float越界/INF/NaN(int)会E_WARNING，默认HandleExceptions抛ErrorException；本次Query插入ID已覆盖，既有Request/scalars整数入口需按其真实生产调用链复核Result/错误边界，不把裸PHP吞warning后的modulo返回当已等价。见本次18case真实oracle与独审，不重写未受影响数值路径或丢失原证据。
- HA-X01 已收齐[增强认证限定验收](memory/testing.md#增强认证单次叶选择限定验收)。按现有边界接续首用安全配置和认证后继。
- Hashing后继缺口：Model binary/非UTF8 encoded与历史缓存所有权仍待完整验收。固定Manager的getDrivers/forgetDrivers/getContainer/setContainer与HashManager.getDefaultDriver尚无公开等价；须结合统一Container/Provider生命周期补contract与行为证据，不能因Rust所有权直接判不适用。Model hashed cast仍随Model依赖对应。本前置只关闭生成/校验/配置与认证共用接线，Client不依赖的Manager表面不追加到冻结批次。
- 对应既有阶段二A Passport / 矩阵L090及R7 SSO，以下为同一条目的优先实施细化，不建立第二队列。
- Lead 按依赖领取 Passport 剩余行为、OIDC Provider 与双 RP SSO/退出，公共契约和 main 验收统一收敛。
- [协议与分期退出](plans/oidc-authorization-server.md)；现 auth-oidc RP、Keycloak fixture、普通 JWT 均不算自建授权服务器。
- 中心默认沿对应官方Web机制；显式JWT中心模式才执行全程无Cookie契约，Token持有/续期/授权事务及双RP浏览器限制先设计取证。此前一律禁止或等待Cookie确认的口径已由最终决定覆盖；实际实现和验收尚未完成。

## 网络测试库剩余平台与拓扑边界

- Unix生命周期/持久回收已收齐[限定验收](memory/testing.md#网络测试数据库生命周期与持久恢复限定验收)，本地提交以Git为准；不重复旧22/公开20场。root只提交测试库所需源码、Cargo/lock与文档，Passport/Hashing WIP保持。
- 后继仍需非Unix安全私有ACL/真实Windows回归、真实只读副本与无应答服务的专项验证；不得将明确unsupported或静态fail-closed标成这些平台/拓扑已交付。MySQL活动快照依赖关闭独立池并停止新借用，不合作新连接/外部DDL与同名含marker备份不具原子CAS保证，不按前缀/PID/年龄清理。不扩当前限定提交；依赖允许时按唯一队列安排，当前回Passport/统一Hashing前置。

## 当前 Fortify 单体系与显式 JWT API profile（最终生效契约）

最新真人决定：框架、根默认应用与未显式选择JWT的生成路径，以对应固定Laravel官方默认、生命周期和公开机制为准；Fortify原生Web/Session是其对应模式默认，不将所有场景一律Session或一律JWT。API+Bearer JWT是本批完整必交的显式选配，开发者选择后全程无Session/Cookie/CSRF-cookie/remember/隐藏Session/Set-Cookie或Cookie身份兜底。两种传输共用唯一UserProvider/Guard/Fortify三字段/Actions；旧auth-mfa兼容仍删除。此决定覆盖此前JWT为根默认、一律禁中心Cookie等中间口径，不改写历史验收。

**本批必须实际交付 JWT access/refresh API 与无 Cookie 管理闭环。** 显式 profile 只区分开发者选择及来源分类，不降低交付优先级、不推给应用自行实现；Fortify不内置此流程，按Laravel Guard/UserProvider/Actions公开机制扩展。Laravel官方生态已有Passport OAuth2与Sanctum，不能笼统称Laravel不支持Token，也不以本API代替这些包的全量对应。

术语统一：MFA为总能力，2FA为本批Fortify形式、TOTP为技术；用户按需开启，当前应用confirm=true时待确认不视为已开启，未绑定可正常密码登录，确认后挑战、必要身份确认后可关闭。删除旧MFA实现不删除MFA能力，不恢复旧feature；其他应用confirm=false仍遵官方。

真人已确认：旧 OpenRust MFA 代码和数据均不兼容；不建设旧数据迁移、重绑、接管或长期fallback。共享数据库、其他WIP与原始证据不删除。原Goal/main/20%保留与credits=false不变。

1. **框架官方基线**：固定Fortify v1.41.0 / `c456642584c102f6a6aafee9932299542da637b5` 的StatefulGuard+Session、三字段、Actions/Provider、login.id/login.remember及八份加密可重读恢复码原样对应；JSON不等于无Session。Sanctum Bearer PAT与SPA Cookie模式分开，Passport是OAuth2；自有JWT不冒称二者已完成。
2. **用户选中的应用profile**：根应用显式选择JWT及生成JWT API profile后全程Bearer，无Session middleware、临时Web Session、Cookie/CSRF-cookie/remember-cookie及隐式Set-Cookie。框架默认与其他生成模式不被该profile覆盖；不得擅自把JWT换成PAT。
3. **唯一二因素**：API密码验证后，仅在需要时返回短时/限次/一次消费且绑定原始ID、Guard、Provider与用途的challenge；通过同一Fortify状态/算法/Actions验证TOTP或恢复码后才颁发access/refresh。DB/缓存挑战不等于Web Session；此为明确API扩展，不是官方默认或Rust被迫适配，不复活另一MfaService/状态体系。
4. **管理与跨入口安全**：Bearer管理经受控近期密码/必要二次证明；复用同一Actions。JWT/PAT/refresh、Session/remember、OIDC/attach与资源逐项防绕过；必要版本/nonce/事务/取消/审计/限流能力按实际不变量经官方扩展保留，不按authority/proof字样一刀切。
5. **实际删除**：auth-mfa feature、MfaService、旧独立管理/挑战/表定义、旧生成profile、legacy hash/seed/adoption/floor/fallback及专用marker；不换名隐藏旧机制。旧攻击/并发/回滚/审计/期限/隔离测试迁到唯一新机制；非官方旧协议本身不再是兼容义务。
6. **有限退出**：源码/所有调用方/CLI与显式profile/唯一示例/文档收齐→冻结→main/default/all-feature/严格文档/MSRV/适用consumer、SQLite当前/3.26及受影响MySQL/PG、实际new/check/test、API完全无Cookie流程与官方Web分别实测、独立安全复审→脚本限定本地提交。Google真实客户端仍外部未验，不push/tag/release。

当前源码已合并在main WIP；root接入唯一现代authority/challenge、JWT API与管理adapter、根默认Web/显式JWT选择及审计，删除旧feature/Provider/迁移注册与兼容路径，CLI、模板、唯一example及OIDC调用已同步。不同模型发现的事务末检、实际Session父存活、fresh存储能力/同Db及外部StatefulGuard扩展缺口已修；限定静态复审未再发现明确P1/P2，完整批次仍未交付。

最新定向验收：`focused-attempt-3`同一输入fmt/diff、库测试1379通过/22忽略、根API4通过/1真实MySQL忽略；覆盖默认Web、显式JWT无Session/Cookie完整流程、唯一example具名Guard、自定义api组短路及动态prefix真实超时的禁止缓存。`main-clippy-all-fifth.log`全workspace/全目标/全特性严格Clippy通过。真实MySQL与PostgreSQL各6项选定因子门控/队列事务回滚/OIDC关联/根API通过，数据库与角色清单未变化；MySQL原runner在转PG清单查询时缺显式db的错误已保留，PG只重跑未执行部分。以上实测不代表Redis/Keycloak、最低SQLite、外部consumer、生成应用及完整main矩阵已通过。

当前停点：本簇限定源码、调用方、唯一示例、配置/模板/文档与适用验收已收齐，所有本簇实现owner停写；main12门禁、五类实际new/check/test、受影响consumer、最低SQLite3.26的60项probe、真实Keycloak双RP/独立API/Redis故障恢复，以及MySQL/PG更新后的8条CI认证命令均通过，匹配输入与失败/清理证据见[当前验收](memory/testing.md#fortify-官方默认与显式-jwt-组合验收)。本地限定提交以Git为准，不把完整R7/Google实际客户端/UUID字符串JWT适配等开放项关闭。原失败及原始证据保留，所有自有服务已回收，库与角色清单无差异；完成的SQLiteprobe target已正常清零，源码/候选哈希保护通过，只保留下一实际Query接线/适用门禁所需的一份共享缓存。下一业务停点转上方独立Query候选的Q2执行期来源校验复审→认证提交后串行接线/验收与独立提交；不追加Passport或其它旧兼容延长本簇。

生成退出补充Spec（同一CLI/profile簇，不展开后继）：真实`new`全部源码写入后先对新应用显式manifest执行`cargo fmt`并只以`-p`选择其自身及存在的迁移package（不使用会递归本地依赖的`--all`），再做已有SQLite默认迁移；这是Rust脚手架产物的工具约束适配，不改框架运行语义。环境须已有rustfmt，失败返回非零并保留文件，不自动安装组件或改全局。纯生成helper单元不嵌套Cargo，真实new的输出必须独立通过fmt check。JWT+gRPC需给独立test_app同一模块声明，当前允许组合不静默删feature；实际fresh矩阵包含受影响组合。首个fresh-web-sqlite已new/迁移成功但fmt失败，check/test未运行，原目录和日志保留。修复后使用新目录重验，不以手工修临时产物回填。

API持久adapter仍限定正整数users.id，UUID/字符串的完整JWT持久链为明确未达成项；通用Guard/Fortify原始ID能力不能关闭此缺口。其它旧兼容四项与Passport/OP/SSO仍在既有依赖队列，不塞入当前冻结输入。主认证指南、模块与CLI指南已删除失效MFA使用方式，保留新组合未验收边界；最终链接与旧引用核查随提交输入执行。

owner=root统一contract、Cargo/lock、迁移、共享文档、索引、构建和服务；worker按互斥文件交接后编码。当前组合已具备限定本地提交条件，状态以Git为准；删除前558文件归档及真人授权回执在`tmp/fortify-single-system-20261105/`，旧候选与失败/清理事实在`tmp/fortify-totp-alignment-20261104/`，不作为新体系已验。本簇不再追加旧兼容矩阵，后继按上方独立条目领取。


当前并行与文件归属（所有writer交接冻结，root独占共享文件/索引/Cargo）：
- Example · OpenRust：可见对话框 `01a1a63a-2d8f-7471-8729-9ce2a9abad19`，gpt-6.1-sol；账户服务/路由/两组账户Cookie测试、Cookie慢save回归已交接；首业务工具exec-407f8323-0ce2-498d-9463-1626eafbd68f，慢save交接exec-2a442f44-fe55-4276-9b80-635750a69321；Cookie负例只读建议已由root按实测边界吸收。
- Docs · OpenRust：可见对话框 `01a1a63a-2ebb-7782-8f5b-dd49bea8b321`，gpt-6.1-sol；认证/Session指南及导航四文件和配置父文件/子section初始修复已交接；首业务工具exec-df9bb381-0934-43b1-b01d-82836ca8aa08，最新文档48链接核查通过。
- session_guard_impl：manager.rs及manager/tests.rs修复已停写交接，root定向10通过；guard_database_provider修后只读复审无剩余blocker/should-fix。fortify_contract_audit已独审旧最终证据及不可变来源映射，manager新输入须单独验收。Docs可见对话框9处一致性整改由root已吸收，171相对文件链接0缺失。

当前验证与证据（目录统一 `tmp/fortify-totp-alignment-20261104/`）：
- 最新认证边界：具名HTTP9、原生Session45、HTTP Session23（实际阻塞save跨真实UTC秒）、配置142均通过；guest的Guard.id fallback不准用于授权；真实双SQLite+TCP的同数字ID跨表/Db来源负例在全feature与无MFA组合均通过。普通Cookie保PHP折叠，原始wire重复Session/dot别名/所选recaller在账户查询前400。各自日志/输入分层，不以定向替代组合。
- 真实服务：MySQL8.4/PostgreSQL18各33项，账户3、Cookie4、认证DB8、迁移1、浏览器4、OIDC主8/lifecycle5，全部实际执行且0ignored；Cookie4为2Router构建拒绝+2真实TCP流程，不称4TCP。Chrome153实跑、真正IdP签名backchannel/重放、各后端库差异0，索引见real-services-summary.json。按未变生产路径复用，后续calamine升级仅影响数据解析，不重称新Auth实跑。
- 安全与失败证据：Cookie先完整response预检→save→按最终ID/时间生成；原非法Cookie不先写库、remember自动recall与慢save断言保留。Frozen父/子文件双向浅边界、runtime set和缓存晚注册不补默认；OIDC普通GET用cookies/session，JWT绑定用共享cookies+JWT组，不降CSRF、不混身份。全部失败输入、0测试误过滤、panic残留库精确归属/0连接清理和IdP旧测试会话清理留在tmp，不覆盖原始日志。
- 组合：新calamine0.38输入833项的final-gates-2十项全部通过：严格全特性Clippy、全特性workspace2272/0/124、默认/全特性warnings-denied文档、deny、CLI构建、三精简feature和MSRV1.97。默认1339/0/40证据按未变路径限定复用；最新模板修复在此输入之后，须补验CLI与fresh，不宣称旧快照覆盖新模板。
- 最低SQLite：真实3.26.0动态库执行remember迁移标准up/down/up通过30断言（sqlite326.log），含数据/索引/FK/完整性与自有库/锁清理；当前3.51.3旧28断言按输入单列。
- fresh：新输入r7-freeze-template-source-3.json在fresh-r7-5四类sqlite-auth/sqlite-mfa/mysql-mfa/postgres-auth实际new/check/test十二命令全部0，每类3测试、0跳过、双库差异0。生成测试漏Application::config、装饰器漏Guard元数据转发、Global profile导致fixture lottery覆盖失效均仅修测试模板；旧失败副本/日志保留，未降生产failclosed。CLI39测试、严格Clippy、fmt及更新后的正式快照均通过；输入/日志SHA见template-source-final-evidence.json，尚待consumer与最终独审。
- 资源/服务：旧受验target经dry-run及Cargo原生锁正常clean；当前受控使用根target、fresh共享target及最低SQLite probe target；总量按实际du核算。专属Keycloak容器、管理trigger进程及管理员凭据已清理；共享MySQL/PG/Redis、源/证据/依赖源码保留。该阶段估算采用 50 GB / 100 GB 和 10% / 5%，后续新 target 按现行预算预检，源码不随缓存清理。

R7上述门禁为已提交5ad34b9的限定历史基线；不再重启旧R7冻结/生成批次。Fortify当前新源码、测试与安全优先停点以本页「当前批次与Fortify优先例外」为准，旧Guard证据不覆盖新行为。

本批之后，固定 Fortify 官方双因素与 Google Authenticator TOTP 是经真人批准的阶段二优先例外；复用本认证内核，官方默认/安全增强分开，真实Google客户端未导入保留外部验收缺口。其他Fortify功能全量范围不删除，其他官方生态不随本例外全面提前。

优先闭环后回核心依赖主线：复核 Container/Provider、HTTP/Session实际剩余前置，再按 Query/Schema/迁移 → Model/分页 → Factory 的实际依赖逐簇领取；不重做已验收范围，不把R2–R6绑成巨型提交。SQLite适用链、CLI逐命令/参数/默认/错误/生成、唯一示例与固定Laravel13.x导航文档随所属簇交付；S3及全部核心/Modules/官方生态缺口保留。仍沿原Goal/main/预算/历史，不启动冻结业务UI或AI产品，不重启旧守护。

### 当前：SQLite 迁移、配置边界与既有能力适配

1. 用户明确要求“SQLite 支持请尽快安排开发好”；owner=root，沿用原完整Goal/预算/main。当前R2命令所有权簇先闭环；按最新真人明确顺序接范围明确的模块入口整改，再立即SQLite。SQLite可并行只读盘点，不加入当前受验源码，不等待完整R5/Factory。
2. 固定docs 960eedf database章节的SQLite 3.26.0+、默认外键开启与显式DB_FOREIGN_KEYS=false，以及framework d52c3b3 SQLiteConnector/SQLiteConnection、Query/Schema Grammar、迁移与官方测试为contract；复用同一Db/ConfigRepository/Provider，新增db-sqlite及内置backend，保默认MySQL＋Redis。现custom consumer只证扩展/事务子范围，不算标准支持。
3. 首簇收敛内存/文件/URI、普通文件存在与错误脱敏、每连接PRAGMA/池初始化/隔离、事务/savepoint及DDL类型；官方installer选择SQLite时创建database/database.sqlite并执行默认迁移，与Connector对普通缺失文件拒绝连接分别对应，不能靠连接器静默建库代替生成流程；同步标准根配置/CLI new/测试库，记录依赖feature与版本，不要求开发者自写Provider。锁与迁移单机互斥须按SQLite实际行为证明，不借用双库SQL。
4. 按依赖分簇打通现有已实现Query/模型CRUD、认证/MFA/Session、DB cache/锁、DB queue/worker/scheduler及框架迁移。每个未适配生产链明确保持缺口，不能以连接SELECT 1或SqliteQueryBuilder分支关闭全部能力。
5. 同步modules/example真实调用、生成/测试/文档；验收内存与真实临时文件、migration fresh/rollback/status、事务/失败/并发隔离、文件跨进程持久化、根HTTP/CLI及独立worker和生成check/test。SQLite仅单机；不能为方便使用RETURNING无据提高固定官方最低版本，所需能力/编译选项与最低版本须实际取证。未来通用功能继续补SQLite矩阵，不宣称共享多机集群或未开发Laravel能力已对应。
6. root独占公共contract/Cargo/lock、共享文档/索引/构建/实服务；deferred_contract_review仅只读盘点上游和现链，报告/tmp/openrust-sqlite-builtin-contract-audit.md。当前生产阻点已核：DatabaseBackend拒绝sqlite、标准连接/版本match仅双库、dialect UTC/JSON拒绝、TestDatabase无SQLite、migration_lock仅PG/MySQL；默认audit、Session/auth/MFA、coordination/限流、queue SKIP LOCKED与schedule也有双库限定，不能只移除SQL锁语句。只读报告已保全tmp/sqlite-builtin-20261102/contract-audit.md。
7. 下一动作：当前R2验收期间收齐只读contract/文件清单；R2本地提交及本次范围明确模块入口整改后，立即按必要依赖收敛短Spec并实际编码SQLite，不只留计划。Factory维持R5前置，SQLite内置适配用现有R4基础优先交付，完整后续矩阵随各簇保持。

S2已交付限定范围，编码基线20cea6c（交付提交f9e831a），无新worktree。S2配置/连接/既有迁移与安装子范围的13主门禁、SQLite定向/所有者12、最低3.26库46/Root3、真实生成check/test与9配置子进程、根CLI/配置/HTTP、生成HTTP及12consumer check已通过；最后cfg(not db-sqlite)能力错误前置的默认严格Clippy/完整workspace测试与最新全feature文档4项也通过。外部consumer保全部事务/查询断言，run-2成功；不同模型final-review按直接source diff批准已排除代码的SQLite-enabled证据作用域复用，双库各5亦仅限定复用。当前依据见[验收](memory/testing.md#sqlite-配置与迁移限定验收)，所有失败与补丁来源保全，不把子范围记为全量SQLite完成。

收尾已实证：61份生成文件SHA核对归档后移除；main target50.28GB与后期5.57GB、minimum1.77GB均清零，二进制/HTML/自有cache回收，SQLite文件证据归档；磁盘可用约159GB，依赖源码/native oracle和全部服务/回执保留。本地提交f9e831a已存在；不得重跑已关闭矩阵或重复领取S2，继续下面同一SQLite项的S3业务适配及剩余缺口，不建第二队列。

S3a已完成限定验收：基线 `f9e831a`，root独占main/索引/Cargo/公共contract/文档/服务，所有writer停写交接。原生DatabaseSessionHandler/Store、canonical表/官方索引/exists/活动期限、connection/table/IP/UA及绑定Session身份投影；显式Guarded/SQLite微秒事务/GC与调用方/模板/唯一示例/原强断言同步。冻结20主全feature四门禁均0（2193/0/122 ignored），默认18经等价差异独审限定复用（1333/0/40 ignored）；三精简feature、MSRV1.97、deny、最低3.26当前原生common/真实动态库身份、双库23项及fresh四类9/11/3/3均按分输入实证通过。不同模型独审核700输入零漂移、日志SHA/真实执行/数量与复用边界，无限定blocker。229份生成源归档SHA核对，main/最低/生成target全部正常clean至0，磁盘可用约157GB；依赖/native源码、服务及全部失败/证据保留。详见[限定验收](memory/testing.md#sqlite-数据库-session-对应限定验收)；本地提交以Git为准，不重跑已关闭S2/S3a矩阵，不扩大本批。S3b的限定闭环与验收见当前测试基线，后继只在本项剩余行为及阶段二Fortify领取。完整Guard.id/named guard仍归R7，当前身份投影不冒充完整Guard。


整项SQLite继续in_progress：完整Guard解析及Session其余语义、DB缓存锁/队列调度业务SQL、完整Migrator/fresh/Schema/native JSON、Root+Auth、构造/Drop/驱动fault全矩阵、Windows原生、mount/bind别名与替换路径TOCTOU仍开放。after_release/驱动故障与已在途acquire没有原子禁重连保证；不降低断言、不说全面防丢库，也不把该窗口归为必要Rust适配。原Goal active、新周期99/19.8，旧27/5.4历史保留，default80%不变，不rebase/credits，不重启stopped coop/旧守护。

### 标准 MySQL＋Redis 与队列开箱（已限定提交3703939）

1. owner=root，main共享目录，基线91b1d9e；只领取下表既有标准依赖/队列项，不新增队列。原生接入负责人已撤销同线程只等待heartbeat的临时约束；本主线恢复正常开发，主动helper检查不冒充真实定时触发。
2. 固定Laravel d52c3b3的config/queue.php、QueueManager、QueueServiceProvider及对应tests/docs为来源；默认database具名连接、QUEUE_CONNECTION选择、连接按需解析和公开creator扩展统一到一套配置/管理器。
3. 核清并移除顶层旧driver与空connections回退路径；根/new可编辑配置同机制提供database与redis，未选driver不连接、不要求其编译依赖，未知/未编译的实际选择明确失败。所有旧调用方与测试一起迁移，保留其有效断言。
4. 标准根/new预整合MySQL、Redis缓存及Redis队列依赖；保留PG/外部driver与明确精简opt-out，编译具备不代表运行已连接。DB队列仍默认，迁移和worker显式运行，serve不启动worker且不静默fallback。
5. 唯一模块真实Jobs贡献与DB/Redis选择解耦；DB业务任务/事务和数据库调度的必要限制留在实际使用边界，不能让可并存的调度目录阻断Redis队列派发/worker。
6. root独占公共contract、src/config/mod.rs、DefaultProviders、根config/模块/调用方、Cargo/lock、共享文档/索引/构建/实服务。config_dependency_kernel已获队列内核src/queue.rs、src/config/queue.rs、必要src/queue/redis.rs与tests/queue/driver_contract.rs的writer；config_editable_defaults独占tools/cli/src/main.rs、templates及其快照。两者不跑Cargo/Git/服务、不递归派发，skill独立owner范围不碰。
7. 源码、全部调用方、配套测试、模板、modules/example和文档收齐后冻结；集中适用主门禁、精简/no-driver、实际MySQL与Redis派发→worker→执行、fresh生成及缓存验收。未验证保持WIP，通过后本地提交和资源收尾。
8. root/内核/CLI源码、全部调用方、模板和唯一模块已收齐，writer均停写。tmp/standard-queue-20261102/attempt5完整12门禁实际通过；其后只改tests/server/queue/redis.rs的四个资源后断言为fallible检查，独审确认清理闭合，默认/全特性queue_redis与严格Clippy定向通过，其他输入按SHA影响复用。真实MySQL/PG调度事务拒绝各1、无默认库Redis模块HTTP→handler→ACK各1已实际通过，数据库集合无增删。
9. 根标准构建TCP/独立worker的数据库与Redis两条链、serve不暗启worker、缓存固定database选择及clear后切换Redis均通过；四个进程正常exit0、端口释放、无任务库/Redis键遗留。首轮缓存后另选端口的fixture失败原样保留，根因是缓存确实冻结server.port，生产源码未改。并行3eae8df仅改品牌标题/包简介/说明，Cargo除description完全相同、队列WIP未夹带，差异与文档补验在branding-only-delta.json/branding-doc.log。
10. fresh验收已收齐：minimal check/test/build通过，12项为原模板9项＋三个公开consumer各1；standard与PostgreSQL生成应用各check/test通过（迁移目录1＋test_app 11），其中真实数据库派发→handler→ACK及无默认库Redis同链各1；MFA默认MySQL生成应用check/test通过（认证2＋迁移目录1）。全部无ignored，服务环境显式设置且runner检查未跳过。fixture曾缺app.env=testing的失败与诊断原样保留，修复只涉及临时验收fixture，不改生产源码/断言。
11. SQLite外部consumer已显式default-features=false并保原cache-redis闭包，locked check与真实SQLite事务run通过，lock未改；生成minimal的真实配置缓存/清缓存重启及坏dotenv缓存隔离通过。所有真实验收库集合无增删，证据tmp/standard-queue-20261102/。源码归档、文档/矩阵与不同模型最终独审已收齐；全部适用验证通过，仅限定本地提交，不追加新簇延长本批。完整Laravel Queue仍开放：当前启动检查default名称/存在且Queue Provider非完整deferred，默认连接错误时点与固定QueueManager/DeferrableProvider未等价；并入既有R2/Queue R10，不作为Rust必要适配。生成fixture仅验证公开单次worker入口，独立CLI/SIGTERM证据来自根标准应用，两者分别计证。
12. 本线程合作式checkpoint沿原Goal/default80%账本已实际continue，当前专属automation为openrust，旧openrust-agent不可恢复；主动回执与真正定时触发分别取证。尚未收到同框native输入，不伪写native成功。自然批次边界正常final、Goal保持active；下一轮回本节原停点，不暂停/空转等调度。

13. 本标准队列簇资源已确认归属/无进程及文件占用，正常cargo clean：11,963,707,392B→0，磁盘可用161,699,864,576B；生成/运行源码341份与lock已归档，SHA核对无漂移，32份命令回执日志及12门禁SHA匹配。最终不同模型独审无阻断，文档两次全特性-Dwarnings补验通过；源码/证据/服务与独立skill WIP保持。下一依赖领取先核本簇Git提交及唯一R2/R10缺口，不把完整ADR-001标完成。

### R10 应用同进程启动（限定验收）

用户批准小型应用默认用一个进程运行 HTTP、队列和适用调度。WebSocket 随已启用的 HTTP 路由提供。根应用、新项目和 `openrust dev` 接线已完成，`serve`、`queue work` 和 `schedule run` 可独立运行。真实链路、生成项目和工作区验收见[同进程启动证据](memory/testing.md#应用同进程启动)。提交见 Git。

### DB 通用能力与驱动门控（限定验收）

1. 固定Laravel d52c3b3 DefaultProviders/DatabaseServiceProvider/MigrationServiceProvider与ModelMakeCommand；通用DB/ORM/迁移注册不取决于某个连接driver，Rust编译驱动与运行配置分别判断。
2. 当前db层和SeaORM本已常开，不为名称统一新增虚假db总feature；解除facade/prelude、根迁移与唯一模块普通业务对MySQL/PG并列cfg的耦合，方言/连接器及真实后端专测保专属cfg。
3. DatabaseConfig增加单一has_default_connection语义，只指默认url已配置；具名库不冒充默认库。DefaultProviders自动选择、业务模块与路由复用此语义，未知/未编译/replica非法仍走现驱动预检拒绝且脱敏。
4. 根及活跃模块迁移/种子注册独立于连接可用；实际执行再沿同一Db与现有锁/错误。生产模块在真实register读当前配置，静态候选仅声明可达性；显式测试descriptor保其明确覆写能力。自动默认清单选择后register/boot改变默认URL有无拒绝启动；显式模块register未选业务而boot反向启用也拒绝，防不完整装配暴露。此为当前生命周期缺口下的明确失败，不冒称完整上游动态/懒连接。
5. make:model与普通实体生成不靠Cargo文本包含driver；generate job保真实migration/queue结构预检，去driver字符串假能力判断。workspace继承、自定义driver与普通无连接生成均加回归，不将新queue默认体验混入本簇。
6. 保原自定义SQLite Provider通用Db/事务/应用迁移扩展；框架互斥及默认auth/队列未承诺SQLite，不因解除编译门控宣称新后端支持。现eager启动连接与完整Laravel懒连接仍保明确缺口。无默认URL时显式database Session仍被现自动选择省略，不能当已启用/已报错，严格上游语义保Session既有后继。
7. owner=root独占公共contract/config/defaultProviders/bootstrap、Cargo/index/docs/服务；example worker仅modules/example源码且不改README；CLI worker仅tools/cli/src/main.rs及必要模板/快照且不改README/共享AGENTS。worker不跑Cargo/Git/服务/skill、不递归派发。
8. 源码/调用方/测试/唯一模块/模板/docs完整后冻结，一次集中门禁、无driver/单driver/双driver、外部SQLite、fresh普通与MySQL及PG定向真实服务；验收通过正常清产物、本地提交，完整框架Goal继续。

owner=root，main共享目录，基线710dd55、规则提交231b0b8；752输入已完成主门禁/作用域复用、无DB/queue-only、真实两库认证与named-only各1、SQLite与fresh12/3/2、四TCP及最终独审，源码归档与target清零收齐，本地提交91b1d9e。前三次及后续失败均保留，用户已改自主最优处理规则，不再按次数问授权。准确范围、旧负例排除及全部边界见[限定验收](memory/testing.md#db-通用能力与驱动分层限定验收)。后继标准MySQL+Redis依赖与队列开箱已在上节领取；原生守护实际触发另行取证，不重启旧守护或重建Goal/预算。

### B003/B004 FileActivator 实例快照（限定验收）

1. 固定5971484 FileActivator构造readJson、hasStatus/set/delete/reset及Laravel Filesystem错误行为；默认file实例不观察外部改盘，重新构造才读取。自身set/delete的内存修改先于持久写，失败不伪回滚；reset清自身快照。
2. FileModuleActivator::new改为Result<Self>，构造时捕获绝对路径并读取；ModuleManifest保存文件激活器工厂，在实际包register/legacy准备处构造，不在配置装配中提前读盘，保自定义共享driver扩展。
3. 默认缓存clone快照，多个独立实例互不更新；显式with_live_reads及file-live激活器保留已授权fresh读取增强，不冒称官方file默认，沿同一ModuleActivator/配置机制，不新增第二状态仓储。
4. 保协作锁/锁内重读/原子替换/符号链接与未知值保护增强；默认实例内存视图不吸收其他writer。失败的自身变更保pending，后续成功写包含这些变更，不能因磁盘合并丢掉失败后内存语义。
5. 按PHP关联数组规则拒绝非空numeric-consecutive对象等array_is_list输入，写空表/连续数字键按列表编码；仅本实例已提交且字节精确匹配的列表允许后续写恢复，不用此例外覆盖外部坏文件。非有限值缺口继续公开。
6. fallible构造失败时通过显式reset_file(path)保旧手工恢复能力；不提供绕过完整bootstrap的管理CLI，不重建/删除业务目录。写失败与缓存状态、持久化结果分开说明及验收。
7. root独占activator.rs/manifest加载/配置/共享docs/index/Cargo/服务；codec worker仅新state_json.rs，测试worker仅新activator_cache_tests.rs及唯一example/publicconsumer对应测试，不运行Cargo或服务；不同模型只读审查。
8. 根/生成/唯一模块同步默认缓存与显式live用法，真实四进程验证旧App不热读/新App重载、显式live刷新和状态文件保护；完整编码与配套后冻结集中适用门禁，正常清理并本地提交。不改独立guard skill/live/Goal/账本。

owner=root，main共享目录，基线d32162d；751输入最终attempt3十门禁、PHP真实固定类probe、fresh12/MySQL3/MFA2/root1及四TCP均通过，独审与生成源码归档、target清零已收齐，本地提交以Git为准。旧失败和首次占用清理回执保留，见[限定验收](memory/testing.md#fileactivator-实例缓存限定验收)。该后继DB通用能力门控已按上节限定验收完成，下一标准MySQL+Redis队列项；ProviderRepository/aliases等原缺口保留，不关闭完整B003/B004或Modules，skill独立writer仍独占。

### module.json 运行发现与编译绑定（限定验收）

1. 固定nWidart5971484正常ModulesServiceProvider::registerModules：aliases→同内核Provider加载→files；不使用deprecated Module::register/boot，也不重新执行Application::modules。
2. 同一次应用启动扫描配置路径的module.json，主路径与额外scan路径独立于statuses-file；Repository get_path从同一ConfigRepository动态读当前路径，Manifest捕获扫描快照，不承诺热卸载。
3. 原始JSON字段保留，disabled坏JSON仍先解析；启动Manifest保重复来源和provider声明，lookup按lowercase最后项覆盖。providers递归flatten/PHP falsy过滤；priority比较按固定PHP语义取证，不以i32限制冒称上游默认。
4. Rust静态代码须显式编译：公开命名Provider工厂表供metadata引用，真实register仍走39086cf统一执行器。缺失编译绑定在启用模块实际加载时失败，禁用模块仍可被发现并持久启用。
5. files采用显式编译初始化绑定并公开PHP execution与Rust App生存期适配，不能执行未编译.rs；复用同Container/配置，保先前副作用与include_once解析路径去重，不伪称任意源码执行等价。
6. PHP class aliases不能偷换为Container aliases；未有可证明等价的Rust机制须保明确缺口，不能静默忽略或宣布B011完成。deferred ProviderRepository完整机制亦保原队列，不以eager子注册覆盖。
7. 根公共contract、module.rs/repository/provider/config类型、Cargo/lock、共享docs/index/服务由root独占；parser worker仅新discovery.rs，readonly审查不写源码。skill独立writer仍独占，不live/Goal/账本操作。
8. 根bootstrap、可编辑config/生成模板、唯一example module.json与实际Provider/files调用、公开fresh consumer及原断言同步完成后冻结；集中适用门禁、真实服务/重启及独立审查，通过后本地提交并正常清理。未闭合保持WIP，不追加后继簇延迟提交。

当前停点：最终749输入及attempt6十门禁、fresh普通12/MySQL3/MFA2、根MySQL1/三进程/缓存复启、PHP2068向量与独审均通过，源码与证据归档、完成缓存及独占PHP镜像正常清理，本地提交以Git为准，见[限定验收](memory/testing.md#模块运行发现与编译文件限定验收)。两项Mac nonUTF8 FS分支只证能力不足、不计scanner已验；aliases、完整ProviderRepository eager编译/构造缓存与deferred、非有限JSON、完整POSIX glob/非UTF8通配及完整Modules仍开放。三个旧绿/失败阶段与官方双路径纠正均保留，未删断言降门禁。skill继续独立owner独占，不部署/Goal/账本操作；嵌套前置39086cf。

B003/T03后继实例快照整改已按上节限定验收；旧7f60560热读证据仅证明历史Rust增强，不能替代固定上游缓存语义。严格IO/锁内合并/live明确保留增强，非有限数及完整B003/B004继续开放，下一DB门控。

### 模块运行发现前置：同内核嵌套 Provider 注册（限定验收）

1. 固定framework Application::register 与 nWidart ModulesServiceProvider::registerModules；真实加载是parent register中注册child及随后files，不能以direct register/boot转调构造第二生命周期。下一metadata发现必须依赖此入口；本前置不称发现已完成。
2. RegisterContext新增register_provider/force_register_provider/boxed_register_provider，返回ProviderId，与App对应入口同义；前者必须await后才执行父回调下一语句，不缓冲成boot或延后队列。
3. 唯一内部ProviderRegistry保存有效实例、首次槽位与正在register身份，启动Builder和已启动App复用同一注册执行器；相同类型非force返回原身份，force register成功替换原槽位，失败保旧实例及已发布贡献。
4. 正常启动子register在父register返回前完成；成功子先进入有效顺序，父随后入列，统一register屏障后按实际有效顺序boot。已boot App动态注册时，子register/boot在父下一语句前完成，父也正常即时boot，不另建Container。
5. 父register失败不伪称回滚已经成功的子及绑定；父boot失败保新实例，启动整体失败与取消仍按原资源owner/deadline关闭。动态调用未来被取消时保全已有Registry，不能mem::take后遗失原App Provider。
6. 重入注册同一进行中类型返回稳定cycle错误；取消必须释放进行中标记，不持任何Registry锁执行用户callback。显式Provider依赖和原平面预检断言保留，嵌套child在调用点核已注册依赖，不捏造上游自动拓扑。外部依赖引用的子ID由ServiceProvider::nested_providers纯声明补入原预检位置，parent返回前校验真实已登记；Rust不能预先反射异步回调，未声明的未来类型不冒称可自动发现。原Session late-overlay依赖及断言保持，不以改依赖目标求绿。
7. TestApp移除的ProviderId对嵌套注册同样生效，不能由parent悄悄重新装回；静态/嵌套/运行期注册复用同一去重来源。同一次启动中的显式force不能绕过该mask；已启动App的显式运行期注册沿原App接口。
8. 所选module启用仍仅生产一份描述。唯一example改为一个普通ExampleModuleServiceProvider在register中按原顺序注册原有child，原child保独立身份和真正生命周期；不改root默认DB/队列/HTTP语义，不把本前置当DB能力门控已完成。
9. 同步RegisterContext取得配置仓储句柄的公开入口，复用已有同一ConfigRepository；不添加另一配置或模块注册中心。现模块Events fallback不得吞nested真实监听，按现默认行为保管理服务可用并验证强制重绑。
10. 根公共contract、provider/app/相关core文件、共享docs/index/Cargo由root独占；example worker仅模块装配适配与其测试，独审只读。禁止改独立skill WIP；不新建worktree、不push。
11. 收齐源码、配套测试、唯一模块、必要生成公开fixture/文档后冻结，集中验嵌套顺序/相同类型/force失败/取消/cycle/mask/动态已boot顺序、原门禁与fresh/MySQL实际文章事件队列、正常清理和本地提交；之后继续module.json/Provider/files绑定，不以此前置关闭B001/B007/B012。

当前停点：771输入冻结、最终10门禁和独立只读审查通过，默认1099/35 ignored、全特性1940/112；fresh普通12/MySQL3/MFA2、根MySQL精确1及三进程启停/缓存复启通过。原Session晚覆盖依赖/断言保留，先修子身份声明再重跑完整门禁；生成源码归档、target正常清零，本地提交以Git为准，见[限定证据](memory/testing.md#同内核嵌套-provider-注册限定验收)。下一步module.json运行发现/编译Provider及files绑定，不关闭完整B001/B007/B012；独立skill owner仍独占其WIP。

### 模块持久启停簇（161bfcf 后继，限定验收）

1. 固定nWidart5971484的FileActivator/Module/BaseCommand/Enable/Disable/List及对应测试；先闭环已编译模块的持久管理，随后实现module.json运行发现/Provider绑定，不把本簇catalog称发现或关闭B001/B007/B012。
2. 同一ModuleActivator扩展snapshot/set_active_by_name/delete/reset，旧只读driver默认明确拒写；Snapshot保留未知键/非布尔值，exact键has_status(false)区分缺失/null与Other，is_disabled=!is_enabled。程序化enable仍走原find规则，文件状态只查canonical精确键，孤立旧键忽略。
3. File写以稳定sidecar std try_lock、锁内重读、同目录临时文件同步后原子替换，锁不跨事件/用户回调；busy明确失败可重试，set/delete损坏不覆写，显式reset可按官方清除坏状态，其他键/原值保留；并发/原子性是质量增强而非上游已有保证；普通文件及配置符号链接目标纳入本簇，硬链接别名等价与Windows原生门禁仍开放；rename为提交点，后事件失败不得假称未写。
4. 新ModuleRepository/Module只保存同一次manifest的全量metadata与所选activator，不复制Provider或重调Application::modules；同Container懒构造，Module对象保可共享管理句柄及既有Weak Resolver（每次操作从同一容器解析当前Events），当前loaded快照与持久状态分别可见，不热挂卸资源。
5. 同一Events增加typed named/wildcard监听及dispatch_named、Continue/Stop结果：Stop仅停本次传播而动作继续，Err异常传播；原typed/queued顺序与错误契约保持。Module enabling/enabled/disabling/disabled名称为modules.<lower>.<phase>，直接set_active无四事件，不建第二bus。
6. 模块清单显式接入时提供同Container管理服务；全量Provider收齐后仅无EventsServiceProvider且未显式移除时补空fallback，不吞原模块/应用监听器。普通无模块ProviderBuilder保持原行为，模块服务factory不额外启动App。
7. 核心命令module:enable/disable支持多名、-a/--all及显式非交互；名称全部先验，再逐项执行，重复不去重，--all覆盖显式名单、空all成功，异常停止后项并保前项。module:list支持only/direction官方选项。TTY搜索选择及通用批命令false聚合单列后继，不冒充完整CLI。
8. 命令仍走唯一完整bootstrap，坏JSON/模块或服务故障不会另造绕过启动器；手工恢复坏配置与官方启动失败边界明确。活跃App原Provider/路由/迁移资源保持，下一App按新状态选择。
9. 可编辑config/modules.rs只展开本批真正实现的activator与activators.file.statuses-file；默认应用根modules_statuses.json，允许独立相对/绝对路径，不自动迁移旧文件。应用根输入modules_root纠正为application_root，资源根与状态文件不再混用；模块源码根/扫描配置随后发现簇交付。
10. 同步根/生成bootstrap、唯一示例的事件与持久管理真实调用、consumer/生成测试、README与changelog；所有旧modules_root活跃调用按归属迁移，历史证据不机械替换。无新依赖/Cargo写，守护WIP不触碰。
11. root独占core公共contract/provider/events/commands/config类型、docs/index/Cargo/服务；file worker仅新activator文件及内置测试；app worker仅根/模板接线、配置与受影响宿主fixture路径。完整配套冻结后集中门禁、真实进程启停重启/状态/资源/错误/并发验收，验证通过才本地提交。

当前停点：767运行输入冻结、完整适用门禁及fresh普通12/MySQL3/MFA2、根MySQL1和真实三进程通过；模块定向34含跨进程锁通过，独审资源回收与锁释放整改已闭合。生成源码/证据保全、两个完成缓存均正常清零，所有实现owner停写，本地提交7f60560；事实见Git与[限定验收](memory/testing.md#模块持久启停与同容器管理限定验收)。旧失败/无效cache回执不计通过。下一步只领取本行后继module.json运行发现/Provider/files绑定；TTY/硬链接/Windows等明确开放，不关闭完整B001/B007/B012。skill由guard_native_codex_impl独占，不并改或启用。

### 当前应用职责子簇（限定验收，本次提交以Git为准）

根/生成的Controller拥有固定API响应，bootstrap负责API前缀与中间件组，routes/api.rs只声明相对路径，Rust mod仅导出；bootstrap两项真实测试主体已迁tests并保留private挂载，新增禁用模块仍保宿主API与通用HTTP无应用API的回归。完整门禁、fresh普通11/MySQL3/MFA2、根综合MySQL1、真实API/模块/TCP/TLS通过；源/证据独审无blocker，owner均停写，target已正常清零，生成源码与证据已归档。详见[应用职责验收](memory/testing.md#应用启动与固定api宿主职责限定验收)。下一模块状态/持久启停/发现先Spec，不追加输入延迟本次提交。

### 当前组合提交后的首批：应用配置可编辑默认值整改

本整改与既有应用/模块布局项共同闭环，执行映射只维护本页：

#### 当前配置簇 Spec（c52ba8f 后继，主控已收敛）

1. 固定来源为 Laravel `LoadConfiguration` 顺序加载、Repository 读取及官方 configuration 章节；JWT/签名 issuer 的默认继承为用户授权增强，不冒充 Laravel 核心默认。沿唯一 ConfigManifest/ConfigSources/Repository，不另建内核。
2. 保留 manifest 声明顺序，每个 section/typed 函数同步且仅调用一次；全部名称/重复/typed-typed冲突先验，raw/typed动态冲突在候选中拒绝。失败不发布半成品；原有 section 兄弟值与 typed 子路径可并存。
3. 增加 `ConfigContext::get<T>(key) -> Result<Option<T>, ConfigError>` 与 `require<T>(key) -> Result<T, ConfigError>`，仅读可序列化配置；缺失、显式 null、类型错误区分。读取已完成区段与最终同次来源；自读/未完成相交声明明确失败，不用 Default/高层覆盖掩盖前向或循环错误，不自动执行未来回调。
4. 同步读取作用域以 Weak 绑定并核执行线程，退出作用域/跨线程返回稳定脱敏错误；环境快照读取仍可克隆。用户函数不在不可重入锁内执行；内部安全错误用不可由应用伪造的私有字段载体保留类别，任意用户错误继续脱敏。
5. TOML/环境层只捕获解析一次，在独立读取候选中用原合并方法重放；不把 tail 写入累计代码底层。最终发布与依赖视图同一算法，保键序/profile/metadata、浅合并/mergeableOptions/typed冲突。
6. 新增 `application_code_sources_with_overrides(..., entries)` 求值前全局原始覆盖，最终高于环境；迁移实际示例/生成桥接中需要该语义的调用。晚 `merge_ordered_entries`/Config::set 保持快照，不自动重建资源/重签token。非空前置覆盖遇缓存明确拒绝，不静默忽略。
7. 保留现有两阶段缓存路径：外部环境早命中不读dotenv，dotenv选路径晚命中已读所选dotenv；两种命中均不执行配置函数。typed/工厂缓存拒绝不变。
8. 模块求值读取同次 boot 仓储快照，按原递归覆盖构建内部依赖视图，保原 schema/CAS 一次发布；不伪称根配置可读未来模块，也不把应用文件浅合并套给模块。
9. 根/生成 manifest 声明 server→app→auth/security；APP_NAME/APP_URL只由app读取，缺URL继承最终server.port；JWT/签名默认继承最终app值，独立域覆写保持。运行期字段/工厂不因默认继承自动热重算。
10. 显式展开已实现的根/模板/唯一模块可编辑字段及Option/feature分支；不预建未实现生态配置或借本簇启新driver。View常量仅控制默认是否声明（根false/new true），显式来源仍可启用，不能称强制关闭。
11. 所有源码/配套测试/示例/模板/文档收齐后集中验证：依赖顺序/空值/失败原子性/单次副作用/覆盖/模块/CAS/cache，修改单一默认后的传播，独立覆写，fresh普通/MySQL、缓存重启及main完整适用门禁；未验保持WIP。
12. owner：新总控收敛公共contract、调用方与共享docs/index/Cargo/服务；kernel worker只实现已定内核接口，defaults worker只展开根/模块/模板与生成清单，按文件归属同main并行，禁碰独立native guard WIP。

当前停点：本配置簇适用验收全部通过，限定产物已正常清理，本地提交以Git为准；运行时653输入冻结一致，完整记录`tmp/config-defaults-20261102/`。新总控独占索引，两个实现owner与独审均已停写。普通fresh最终installer11项、MySQL3项、真实缓存/根服务及8类修改默认探针通过；早期fixture注入/探针别名/runner参数失败独立保留。之后按用户顺序领取bootstrap/app与固定API职责，不夹带新driver或模块命令延迟本次提交。


| 整类范围 | owner与批次 | 代码/差异落点 | 验证与提交状态 |
| --- | --- | --- | --- |
| 所有应用可编辑默认项、嵌套/feature配置、模块实际需要及生成模板 | 新总控协调的单一配置owner，当前组合提交后首批 | 根config、CLI模板、同一ConfigManifest/Repository | 已编码并限定验收；完整对应仍开放，见本页Spec及testing配置证据 |
| APP_NAME/APP_URL等跨领域单一来源、有序依赖与显式独立身份 | 同owner，与配置同批 | 所属配置+必要公开依赖读取contract，consumer/缓存/生成同步 | 已编码并限定验收：代码默认传播、SERVER_PORT/APP_URL、独立source覆盖、空值非法与缓存/fresh通过；既有token/链接不自动重签，完整配置仍开放 |
| bootstrap及同类正式入口测试/fixture职责 | 新配置owner，既有应用布局后继 | 真实测试主体迁tests，不复制宿主/扩大API | 已迁移原两测试并新增宿主/模块边界断言；完整门禁和真实服务限定验收通过，提交以Git为准 |
| 固定/api宿主内容及同类业务反向依赖 | layout应用/路由owner，与既有app职责整改合批 | app/controllers处理内容、routes/api.rs声明、AppServiceProvider公开挂载；src仅通用路由机制 | 已移除库专属端点，根/生成Controller与bootstrap真实接线；consumer、fresh普通/MySQL/MFA、真实TCP/TLS通过，完整路由仍开放 |
| 模块状态路径/发现注册/持久启停/统一Provider | 新配置owner与root contract，既有模块整改 | 现ModuleManifest/FileActivator扩展，不造新内核 | 已完成持久启停/独立状态路径/非交互命令事件限定验收与清理；提交事实见Git，运行发现/metadata Provider/files绑定随后，TTY/硬链接/Windows及完整模块管理仍开放 |
| 数据库通用能力门控与驱动选择/运行时启用分层 | root统筹公共contract/应用装配，example与CLI worker按文件交接；当前710dd55后继 | bootstrap/app/模块/auth/通用层/模板/consumer，专属方言保留后端cfg | 通用门控/注册/生成已限定验收，no-driver/queue-only、真实MySQL/PG/named-only与SQLite扩展均有证据；完整DB生命周期及标准依赖开箱继续开放 |
| 标准应用队列开箱体验与精简构建分层 | layout配置/模板owner + root队列contract/Cargo单一调度；关联R10既有队列能力 | 根/new标准能力、config/queue.rs、具名连接/env/Provider/worker统一接线 | 已确认待实现；默认DB和可选Redis真实派发→worker、精简错误/无副作用及生成待验 |

设计与复审同时遵循conventions现有单一来源/默认继承/能力边界规则：同一根因主动查完整传播链，验证修改正确归属的一处后实际跟随，独立语义保持覆写。不是新ADR或扩大当前批次。

本轮全域审计的3个具体子行为归既有执行项（当前组合提交后处理，不新增平行任务）：

- R13生成测试包装器泛化边界：本批只验空编译模块清单、默认bootstrap开启、无package/additional覆盖的普通/MySQL固定生成宿主。后继支持自定义宿主时，测试Provider不应被with_bootstrap_providers=false吞掉，显式Session替换须同时考虑package/additional/module家族；唯一注册证明须绑定实际启动前来源及统一最终注册结果，不能拿boot后重建清单代替所有宿主。来源为本批独立增量审查；不以此关闭完整生成器或扩本轮冻结输入。
- R13生成器嵌套命名：owner为应用/CLI整改负责人，触发例 `Admin/UserController` 被现ASCII单名校验拒绝；按固定官方命名/目录/自动登记及Rust模块导出收敛，验controller/model/provider嵌套路径、安全拒绝/覆盖保护、生成编译运行。Provider自动登记已有，不能重做或误报缺失。
- R8安全/R13 key生成默认工作流：同CLI owner，现只输出随机值而未默认写目标应用.env；先核固定官方命令、只显示模式/生产保护/覆盖与加密key格式，验自有fresh应用默认写入/显式显示/失败不破坏/重复与权限边界。不得本轮改用户真实.env密钥或以改命令名当完成。
- R3 C05响应NoContent：HTTP owner承接既有Response整改，核当前no_content/ApiResponse::empty链，交付真正204空体及状态/头/HEAD边界；独立200成功envelope仍保公开机制，不机械将所有empty成功响应改204。真实controller/生成/consumer对照验收，非静态改名关闭。

实际代码完成后将此表“待”状态改为精确差异与运行证据、本地commit；未闭合项保留，不以消息已交或局部截图已改标完成。生成或证据副本先核归属，从真实源头更新；不另建竞争计划。

- 用户已确认，待当前组合集中验收并本地提交后立即领取；不扩大当前冻结输入。单一owner 本对话后续指定的单一配置owner 承接全部根 `config/*.rs` 与 CLI new 对应配置模板，按共享main文件归属串行；root收敛必要跨配置公开contract及索引/门禁，模块沿同一机制核实际需要，不建第二配置体系。
- 同类数据库门控补充：bootstrap/providers.rs及模块通用装配不得长期逐处枚举db-postgres/db-mysql。先核固定Laravel Database/Migration Provider与现Rust依赖，分清通用能力编译门控、backend实现feature、运行时连接配置、驱动迁移/锁/方言能力。可选统一db能力feature须由backend显式启用且宿主映射同步，不只是将枚举挪成别名；也可沿现统一Provider按配置/实际能力校验，本轮contract已选现有db常开与统一Provider路径并限定验收，不新增虚假db总feature；完整连接管理仍开放。
- 标准队列体验补充（同配置/生成整改，核心语义映射既有R10，不新建计划）：默认连接按固定Laravel与既定DB默认收敛，标准根/new应用预置常用DB及Redis队列实现/Provider/worker/config，选择Redis主要改具名连接配置，不再临时编辑Cargo才能用。内部feature/no-default仍支持精简，不将可编译等同运行连接，更不因Redis可选实现使DB队列依赖Redis队列服务。
- 用户明确标准开发路径为MySQL+Redis：根宿主/new项目预整合常用MySQL数据库、Redis缓存与队列实现所需依赖/feature、Provider、可编辑配置/env映射，填连接信息后无需再手补内置driver Cargo/Provider。仅激活所选连接，不无条件连所有driver；默认DB队列不变，Redis队列配置可选，部署服务/迁移/显式worker仍必需，不静默降级。PG/其他承诺后端/外部driver保留，精简opt-out单列；标准体验不是全体开发者环境统计。后继验证真实MySQL/Redis及两队列链、fresh生成、精简构建/其他后端影响；当前只已授权待实施。
- 重核QUEUE_DRIVER与官方QUEUE_CONNECTION、default_connection/connections/legacy driver及配置覆盖，统一同一机制；新增可编辑config/queue.rs并纳根/生成manifest，不能只删注释/重命名键掩盖两套入口。迁移/连接/显式worker是部署步骤，不serve暗启、不静默fallback sync或其他后端；公开外部driver扩展保留。库feature与宿主feature分别核映射，不机械将全生态all-features视为标准默认。
- 该同项验证包含根与fresh生成默认DB派发→worker→执行、显式Redis连接同链路、未知连接/精简未编译driver明确错误、no-default与无无关连接副作用、配置缓存与文档模板。主动核同类内置能力使用摩擦，但只扩既有授权范围，当前冻结组合不改Cargo/queue源码；现阶段未编码/未验收。

- 扫描bootstrap、app、模块、auth、通用框架、CLI模板及consumer；真实专属SQL/锁/迁移/测试继续保留backend条件，不能全仓替换。配置了未编译或未知驱动须明确失败，不无条件注入缺前置服务、不平行造Db/Provider、不承诺任意连接trait自带完整能力。现有两个后端条件不是天然错误，问题是通用扩展遗漏；未实现SQLite等仍保留缺口。本轮通用能力/注册/调用方及真实测试边界已完成限定验收；真实专属backend条件仍保留，完整后端与连接管理继续开放。

- 每个领域（含feature门控）显式呈现应用可编辑选项、嵌套配置、可选项、默认策略及必要中文说明；不能用单独 `XxxConfig::default()` 或大面积结构体默认更新隐藏应用选项。框架类型Default可保留供serde/程序化使用；内部运行状态、随机/运行时生成数据不机械暴露为配置。
- APP_URL存在时保留用户值（包括外部域名、HTTPS、反代及显式空值现有语义）；缺失时本地URL取同一有效server.port，无两项配置时仍localhost:3000。开发者仅修改config/server.rs默认端口也要生效；实现先收敛同次求值/依赖顺序，避免两处3000、重复工厂求值副作用或循环依赖。不得把Rust字符串`${SERVER_PORT}`当插值或另造env解析/全局状态。
- 单一来源与依赖补充：APP_NAME/APP_URL的环境读取及应用默认只在所属可编辑app配置定义；依赖领域复用已解析有效app.name/app.url，缺APP_URL依同次有效server.port。不能仅抽常量、重复调用配置函数、另读进程环境或建平行仓储。当前ConfigContext仅环境快照，尚不具备最终配置读取；须先收敛一次求值/有序依赖/覆盖与循环诊断的真实公开contract，不假称已有能力。
- JWT audience/issuer及签名issuer分别区分“缺省继承”与“显式独立值”；仅缺省继承，不强制耦合、不随请求Host变化。这些JWT配置为现有扩展，不能冒称Laravel核心同名项。核显式source、受权运行期修改与cache边界，不暗示所有下游资源/已签身份会自动热重建。
- 全部根配置/模板按真实语义核同类跨领域依赖，不把语义独立的同文字面量强合并。应用可编辑项仍完整显式展示；截图application/resolved/config、sources/application/config等副本先识别归属，历史证据/冻结来源不机械改写。补仅改代码默认name/url/port且无env、env覆盖、独立audience/issuer覆盖与cache/fresh生成的继承一致性回归，保留空值/非法端口契约。
- 公开地址补充覆盖根及模板app/auth/security：app.url、auth.jwt.issuer、security.signed_links.issuer复用同一已解析应用地址默认策略，不复制三份拼接。显式APP_URL及各领域独立issuer覆写保留，issuer不得按每请求Host或动态listener推导；公开地址/issuer变化对既有token和签名链接的验证影响必须明确。
- 分类核查生产源码、应用配置、模板、文档、测试fixture中的localhost:3000及127.0.0.1:3000；固定测试输入和明确默认断言可保留，不能为rg清零损坏测试。框架AppConfig::default安全回退、程序化使用、缓存命中与未配置路径按职责检查，不盲替换或删除无环境回退。增加同端口/显式URL三域一致、独立issuer覆写、缓存复启及生成应用对应验证。
- 复用dotenv快照、ConfigManifest及唯一ConfigRepository/Provider；核环境覆盖、代码默认、框架安全回退、typed/factory与cache责任，不复制校验，不降低端口/安全校验，不把显式空值随意当缺失，不隐改所有下游类型Default。逐领域按固定Laravel文档/源码对照，字段展开不是完整1:1验收。
- 同批同步根/生成模板及必要调用方说明，保留root_target等有意差异；写高价值回归：无env默认、仅SERVER_PORT、直接修改应用server默认、显式APP_URL优先、非法端口/空值、嵌套选项、配置缓存、fresh生成check/test。本簇源码/配套已收齐并通过集中适用验收；证据见[配置验收](memory/testing.md#配置可编辑默认值与有序依赖限定验收)，本次本地提交以Git为准，完整配置与其余整改继续开放。

- 本批配置/应用、Web/CSRF、Handler、方向文档与来源三方整合已由 `9035a00` 收口，原 owner 均交接停写、工作区已退役；旧过程和失败见本批验收及来源归档，不再按旧 Implement/冻结状态派发。
- CLI dev代码配置依赖预热缺口：任意Rust配置不能由工具静态解析，应用端有效配置/依赖准备桥接尚未实现；保留显式TOML/环境可判定旧路径，code-config不伪造预热成功，后继R13继续交付真实公开桥接。不得输出带secret完整config以实现预热。
- 默认Rust代码配置入口已由9035a00整合；本次新总控与kernel/defaults worker完成可编辑默认值、有序依赖、模板与调用方限定验收，worker均交接停写；旧来源分支已归档，不按旧owner重复派发。TOML仍仅显式source，完整配置能力不关闭。
- Exceptions/Handler整改已启动owner `/root/trust_proxies_resume`，branch`tasks/exception-handler-family`、base`317e2f6`、workspace`wks_262bd3f349a55c4e`、cwd `/Users/Home/.paseo/worktrees/02joa2lr/openrust-exception-handler-family`；按固定C06实现逐类型report/render与HTML/JSON/响应定制，先Spec收敛再完整编码。原TrustedHosts 23文件main WIP已交接，仅静态格式检查，未行为验证。
- 长期WIP恢复：neutral 17文件与dotenv 9文件已从任务HEAD生成binary diff并保全逐文件SHA、来源base/branch，位于ignored `guards/6b56ea51-0bb7-44a7-8547-7073f8f2c7e4/wip-source-bundles/`；仅恢复/集成输入，不代表提交或验收。dotenv临时Spec也已保全，不依赖/tmp存活。
- 根app职责与生成器整改的原来源已由9035a00整合并安全归档；配置默认依赖由d952199限定提交。本次新总控与root-app/generator worker按同main文件归属完成应用API、bootstrap测试组织及调用方/模板验收，owner均交接停写，旧worktree不再作为活跃来源。完整应用/CLI及其余矩阵继续开放。
  - bootstrap测试组织本次已限定验收：原两测试主体迁tests/server/bootstrap.rs，真实Server/Provider/HTTP与模块运行根断言保留；仅私有宿主访问所需cfg/path，不扩大生产API、不复制启动内核。模板/调用方同步；不将所有框架unit tests判违规。
  - 同项补模块启停布局决策：root倾向“编译manifest + 可配置持久activator + 统一Provider”互补，不能以注释mod/删Provider替代enable/disable/发现/依赖/资源生命周期。固定nWidart v13.2.0 commit5971484实际取证已完成，回执 `tmp/module-activation-layout-review.md`（SHA `974447b79208d145e51b1c0c4b907db8ba4ee3d4cc2182861827008c6c3e5216`）。旧root保根JSON默认建议已被最新真人授权替代：默认Provider注册为唯一启用来源，集中模块入口fallback获准；file/file-live与可配置statuses-file仅显式可选，不隐式迁移文件。既有持久管理已按限定子范围提交，不重复标未实现；完整交付仍按顶部新整改及既有矩阵。
  - 后继实施同owner布局/模块范围，覆盖根/生成模板、路径配置/状态持久命令、依赖与Provider启动、缺失状态行为及旧文件显式迁移；不自动移动用户已有状态文件，不删未合WIP。当前只方案核对，保持冻结组合先验收提交，不新Goal/守护。
  - 同类入口排查范围包含bootstrap/app/routes/config及CLI生成正式入口：仅搬应用装配/HTTP测试主体、fixture和示例特判，保留正常框架unit tests与普通app业务能力；必要薄cfg挂载明确Rust原因。模块同时核路径/清单硬编码、持久状态对Provider/路由/资源真实控制及依赖错误，不以截图路径移动关闭整类问题。
  - 固定/api职责本次已限定验收：原库内置内容归根app/http/controllers/application_info.rs，routes/api.rs相对声明，AppServiceProvider经bootstrap前缀/组公开挂载。根/生成及consumer同步，库不反向引用宿主；通用API分组/异常协商、health/readiness/metrics和模块业务保留。
  - 同类核查内核宿主/示例端点、路径业务特判和反向依赖；health/readiness/metrics按官方来源及既有授权语义判断，不能路径相似即删除。同步根装配/CLI模板/consumer/唯一示例/README与测试，走真实应用路由验证，不仅测试旧framework_routes；bootstrap测试迁移同owner协调。当前不扩大冻结组合，先现批提交再本项代码闭环。



- 环境dotenv/console后继代码ready、未验收：owner `/root/environment_dotenv_console`，branch `tasks/environment-dotenv-console`，base `8d5e635`，同OpenRust项目workspace `wks_6301baa5cbae383a`，cwd `/Users/Home/.paseo/worktrees/02joa2lr/openrust-environment-dotenv-console`，已核common-dir同仓且起点干净。范围为固定LoadEnvironmentVariables的console `--env`/外部APP_ENV专属文件选择、缓存屏障及公开调用/模板/测试代码；不改main TrustedHosts的server/http/App文件，必要共享接口先交接；已交9文件WIP（含run_with_args显式console选择及config:cache fresh传递），未改src/main.rs/CLI模板；常规验收后置，无新target。
- 编码顺序：环境/Request/MIME组合候选保全 → 中性命名代码收口（独立worktree `/root/neutral_runtime_names`）→ TrustedHosts完整家族（复用 `/root/trust_proxies_resume`，基于当前未提交环境/代理API，main单一源码owner；总控独占索引/共享规范）。TrustedHosts来源契约与PCRE边界已备，前置源码已组合但未验收，故只记Implement，不将前置冒称已交付。
- 本批main组合候选：环境身份`8d5e635`与Request文件`2f90127`/MIME`5e2ec6b`源码已叠入同一未提交索引，源码组合已就绪，常规组合验证按最新授权后置，不同模型窄范围复审独立进行；原分支门禁不能代替组合门禁。
- 中性运行命名整改由`/root/neutral_runtime_names`在`tasks/neutral-runtime-names`（base`317e2f6`）独立code-first实施，范围为storage/cache/dataexchange/工具运行路径命名；17文件代码已修B1/S1：未知既有`.staging`拒绝接管，新建目录写归属标记；旧持久清理payload缺省版本拒绝自动删除，新派发版本2及独立去重key；原owner交SHA清单`4b17a144…`，不同模型delta静态复审已关闭B1/S1；17文件已无冲突三方并入main工作区候选并保全source-mapping，未改索引/提交，行为验收仍后置；不运行常规矩阵，待最终统一验收。


当前执行优先级（恢复完整计划）：①R03/typed/CC/Signed四簇五父main限定组合已交付 `e105dbf`，来源与分支已安全退役，证据见[四簇记录](memory/testing.md#r2-四簇-main-组合限定验收)；②R2-P `4ac8099` 与S-DV `6ffb5c8` 已由三父main提交 `9e5a94198b421987851b3580b27fa90370feeb77` 完成限定组合门禁：默认943/0/34忽略、全特性1748/0/108忽略，双严格Clippy/docs、MSRV、no-default、deny、consumer/生成/MySQL/TCP按[最终记录](memory/testing.md#r2-p-s-dv-main-limited)通过。S-DV与P工作区/分支已安全退役；P两份root文档事实已迁入main，原稿与迁移核对回执保全于 `/tmp/openrust-provider-root-docs-wip-preserved/`，正常退役回执 `/tmp/openrust-provider-registration-safe-retirement.json`。③ST-V来源`2262df9`已由两父提交`8d18021b630ea10652316e025ef6019c7772bbb0`完成main限定组合验收，来源工作区/分支已安全退役；真实S3 ACL、迟到CopyObject、`throw=false`仍开放，见[本簇记录](memory/testing.md#st-v-main-限定组合验收)。本轮用户已明确恢复完整计划，覆盖此前仅收尾限制；下方C04-F Request文件家族与MIME已由 `9035a00` 完成main限定组合验收，来源及guard worktree登记均已安全退役。完整Config、Provider发现/Facade、Session/Web、签名URL复杂参数/代理、动态Settings和发布全后端矩阵仍开放。

<a id="c04-f-request-files"></a>
### C04-F Request文件家族（main限定子范围已交付9035a00）

当前main状态以 `9035a00` 及[本批验收](memory/testing.md#当前-main-组合限定验收)为准；以下保留来源分支验收事实，旧owner/Goal/守护运行态与“未集成”描述均已被本次交付和退役替代，不据此唤醒旧任务。

- 来源分支收口：MIME 家族21文件冻结v7，608构建输入零漂移；15代码门禁通过，默认workspace955/34忽略、全特性1770/111忽略，双严格Clippy/docs/MSRV/no-default/deny通过。services九阶段fresh生成/真实TCP/MySQL链全部exit0，回执`/tmp/openrust-request-mime-services-prepare/run-20261101T080437Z/receipt.json`；生成test_app8/TCP1、Request fixture1、真实MySQL模块1均通过，无新增库残留。原owner `/root/config_runtime_callers`正汇总最终证据，services owner `/root/request_files_services_prepare`已完成本轮target清理：30,115,557,376B→0，磁盘16.968%→19.971%，未知外置65,982,464B保留，回执`/tmp/openrust-request-mime-final-cleanup-retry/receipt.json`；源码保持冻结，已限定提交`5e2ec6b`（28文件，含7份Agent事实文档）；不冒称已合main或完整Request对应完成。受控gate已修复Duration::ZERO测试竞争；原失败保留。首Request历史默认数纠正为945/34（原922漏计CLI父23），旧全特性1760/111不变。守护独立修复已合main `31a6d38`，plain重载PID71755，原Goal active、working/task_running及三项存活/心跳均true；默认0.8/baseline1793413935.123801、账号历史、已结束grant保留，38问答ID无丢失，四行因正常复核变化。

- 已创建限定任务分支本地提交`2f901272ec6fcc8cdd7d3a2b4ec6624a7445aaaf`（43文件），提交信息校验与工作区干净检查通过。源码未合入main；保留任务工作区等待main组合验收，完成集成后安全退役，不再等待逐分支许可。

- 沿用原完整Goal、默认80%可用/20%保留和原baseline；不新建Goal/守护，不启动冻结产品，不push/tag/发布。
- 同仓任务位置：OpenRust项目`prj_34a40ee7b674b2b6`，workspace `wks_748133ad70032a5e`，branch `tasks/request-files-family`，base `a9b057e78f35c42955dd99401526a5d1eda60ba1`，cwd `/Users/Home/.paseo/worktrees/02joa2lr/openrust-request-files-family`。原核心owner `/root/config_runtime_callers`，当前生成/真实服务owner `/root/request_files_services_prepare`；root独占索引、依赖/锁、共享文档和提交。
- 范围：唯一Request管线的multipart、递归文件树、DataValue身份、UploadedFile独立reader/暂存与取消清理、store字符串路径、闪存/Redirect公开链路，贯通唯一模块、consumer、生成模板和文档。Spec `/tmp/openrust-request-files-family-spec.md`；固定Laravel/Symfony实际行为优先，普通选择自主，不造独立上传内核。
- 流式契约：网络正文读一次，沿原Axum默认/覆盖/禁用限额；小正文内存、越阈值受管暂存，multipart逐chunk写入。异步`content()`受管reader与`content_bytes(max)`显式投影，原文32MiB/结构化8MiB额外预算属可配置Rust资源策略，不能冒称Laravel默认。writer/在途open/read均持文件与路径owner，取消后小ReadBuf续读保持缓存/offset。
- 已核代码门禁：Request lib41、http_input38、公开consumer Session2、CLI23均通过；默认workspace945通过/34忽略，全特性1760通过/111忽略，双严格Clippy/docs、no-default、Rust1.97和deny均通过。CLI子进程摘要已去重；无服务环境的忽略/跳过不算真实后端。root日志SHA审计`/tmp/openrust-request-files-code-gates-root-audit.json`、`/tmp/openrust-request-files-all-features-root-audit.json`；原失败记录保留，测试数量不是整体完成率。
- 最终808冻结`/tmp/openrust-request-files-generated-json-freeze.json`，tree `e8ff0cac0e66bfa04a92ea5aaa165fce307904cf`，36候选文件；全部SHA无漂移。相对全特性已过输入仅模块multipart用例失败清理与模板/快照三路径变化，生产`src/`、manifest、locks不变；受影响server测试严格Clippy、CLI/生成/MySQL已补验。证据索引`/tmp/openrust-request-files-final-evidence-index.json`。
- 最终真实链已通过：CLI主23、全新生成应用check/test_app8+TCP1、Request精确1/TCP精确1、唯一模块附件真实MySQL精确1且无skip；同MySQL server UUID和schema集合前后相等，进程组无残留。最终wrapper`/tmp/openrust-request-files-services-third-20261101T042452Z/receipt.json`、driver`/tmp/openrust-request-files-services-prepare/run-20261101T042456Z/receipt.json`；旧模板JSON失败和首次临时fixture编译失败原证据保留，不能改写为原命令通过。生成源码已由成功driver清理，不能现时重算原字节。
- MIME续作已完成任务分支限定验收，本次任务分支限定交付、main待组合：固定Symfony v8.0.0有序正反表、Unix `FileBinary`默认检测器及同App公开guesser注册/晚重绑/隔离已接入。最终608输入/21候选无漂移，v7代码15阶段通过（默认workspace955/0/34忽略、全特性1770/0/111忽略，双严格Clippy、文档、no-default、Rust1.97和deny）；真实链九阶段通过，生成`test_app`8、multipart/真实TCP精确各1、模块真实MySQL精确1无skip，同一server UUID及隔离库集合前后相等。证据索引`/tmp/openrust-request-mime-final-evidence-index.json`；v3–v6失败日志保留，旧Request默认945/34的纠正记录不被本次955/34覆盖。
- MIME续作集中产物已按归属和占用核查后正常`cargo clean`（exit0）：根target实测30,115,557,376B→0，磁盘可用168,778,330,112B（16.968%）→198,646,296,576B（19.971%）；未知外置65,982,464B保留。回执`/tmp/openrust-request-mime-final-cleanup-retry/receipt.json`；首轮lsof解码失败未清理的原证据仍保留，不把Cargo逻辑字节当磁盘实际回收。
- 依赖root已统一：Axum官方multipart、基础tempfile仅新增multer3.1.0；http-body-util提升基础direct。12 consumer实际native metadata校准，11锁仅补该direct边，全部既有包版本集合不变；证据`/tmp/openrust-request-files-consumer-direct-lock-sync/receipt.json`。最初离线wasm解析与未缓存平台依赖失败均保留，不能冒称成功。
- 资源：本轮已完成归属/进程/打开句柄核查与正常Cargo清理，根target实测29,937,872,896B→0，磁盘可用167,465,619,456B→197,155,180,544B（19.821%）；回执`/tmp/openrust-request-files-final-cleanup-retry/receipt.json`。首轮因缺缓存标记安全中止的回执保留；确认归属后exclusive create标准CACHEDIR.TAG，dry-run及正常clean均exit0；全项目50GB软/100GB硬与磁盘10%软/5%硬保持。后续仅经root核归属/占用后清可再生产物，保未知外置缓存、guard工作区、服务和数据卷。
- 开放边界：已完成main限定组合交付；Symfony原生Fileinfo后端、Windows默认检测与原生取消清理、完整MIME与文件验证、真实S3 ACL/迟到CopyObject、Storage `throw=false`、Laravel完整Request/Filesystem能力继续未完成，不因本簇限定验证关闭。

<a id="c04-g-trustproxies-请求上下文verify"></a>
### C04-G TrustProxies 请求上下文（main限定子范围已验收）

- 本轮源码、唯一模块/consumer/模板与文档、不同模型复审及集中门禁已由本地提交`772eae4`收口，提交校验通过；该提交后main工作区干净。完整Request/TrustedHosts/云部署不关闭，MIME任务分支未合main。后继当前领取环境身份簇，见下节。
- 最终452构建输入聚合SHA `792d4b783cc6d55423cf98255f8c5d052e280f892c3ef0b6b30cba9709bc125c`，36份日志哈希和原失败核对完整；持久回执`guards/6b56ea51-0bb7-44a7-8547-7073f8f2c7e4/proxy-evidence-v3/`。默认/全特性workspace、双严格Clippy/docs、精确MSRV/no-default/deny、公开调用、fresh普通8/MySQL认证2与新增真实TCP3案通过；旧URL/TLS工具10项只证明其声明范围。
- quoted-pair解码、IPv6原文本冲突及显式port不急切计算scheme三项修后独审关闭。新PHP40案两轮结果相同，但PHP8.2低于Symfony8官方最低8.4；旧/tmp59案与旧freeze缺失，不当作恢复或当前验收。mask0等逐案Rust映射缺口保留，见[测试事实与限制](memory/testing.md#c04-g-trustproxies-main-限定验收)。
- 原Docker未运行导致的Redis失败、错误test target及测试类型推断错误保留；现有Docker服务恢复后精确/完整workspace通过。误用`+1.97`别名触发自动下载已中止，未报成功、不擅清全局状态；改用已装精确`1.97.0`且禁自动安装后通过。
- MySQL事前schema基线未采，不宣称前后集合相等；事后没有本轮命名时间残留。生成/consumer专属target已clean归零；登记产物采样44.659GB→38.929GB，未知外置65,982,464B单列保留，不能从该登记数假报完整总量。根target仅给已实际启动环境身份簇复用，下一统一构建目录仍根target。
- 原Goal沿用，默认百分比80%/保留20%；用户明确批准当前账号基准48%/保留9.6个百分点，旧账号历史与38问答保全，不fullgrant/rebase重放、不推送/发布。运行PID非长期契约，按实际guard状态核验，不沿旧句柄重派任务。

### 环境身份保真（分支已交付，main组合待验）

- 本地提交`8d5e635`已完成16文件限定交付（15源码/模板输入及CHANGELOG），提交校验通过、任务工作区干净；owner `/root/environment_identity`已交接，不再运行Cargo。分支`tasks/environment-identity`，base`266e746`，OpenRust项目workspace`wks_e62dea2c5a1a719f`，cwd `/Users/Home/.paseo/worktrees/02joa2lr/openrust-environment-identity`。
- 精确保留环境名、大小写/空值，官方精确谓词与独立生产保护分开；Loader以`environment_override`标识显式覆盖，最终身份来自加载配置；同名值Eq/Hash/serde一致。三项修复和模板快照增量经不同模型复审通过。
- 最终15输入diff SHA `24266a44489fbd567d2e2a86f6baeb009ddf071b7289b02f5dc49356f429d9af`；配置97/core136/示例33/consumer8、严格Clippy/docs、受影响feature编译、普通生成8/MySQL认证2及CLI快照/认证模板通过，真实数据库65→65且集合SHA不变，无skip。27次命令/25份日志和158份证据输入已保全到主工作区ignored `guards/6b56ea51-0bb7-44a7-8547-7073f8f2c7e4/environment-identity-evidence/`。
- 用户已明确批准与Request/MIME`5e2ec6b`同批main集成，并持续授权后续已批准框架任务直接本地集成；先整合所有源码、解决冲突再集中组合验证，不逐分支重复矩阵，不创建未验证中间合并提交。root独占共享索引；主target仅保留给本批组合。CLI `--env`/专属dotenv、骨架显式development等差异继续开放，见[分支证据](memory/testing.md#environment-identity-branch)。
- TrustedHosts源码/PCRE取证及短契约已备，`/root/trust_proxies_resume`基于未提交环境/代理组合源码实施；root只维护索引及共享规范，无第二main源码写入者。常规验证后置，不能把Spec或源码存在当main通过。

### 上轮收尾执行窗口（已完成）

- main Provider/S-DV源码与集中验证已收口，本轮仅同步Agent事实文档；ST-V main门禁、真实尾链和确认归属的产物清理均已结束；旧owner与分支已安全退役，后续新阶段仍须独立预检资源。
- ST-V默认945/34 ignored与全特性1756/111 ignored、双严格Clippy/docs、MSRV、deny、公开consumer及真实MySQL/生成/TCP已有本次main证据；真实S3 ACL、迟到CopyObject与`throw=false`继续开放。当时阈值为 50 GB / 10% 软、100 GB / 5% 硬，后续按现行规范并保护未知缓存。
- 本次清理回执`/tmp/openrust-stv-main-cleanup/receipt.json`：根target已正常`cargo clean` exit0，登记3个现存target合计67,485,491,200B→65,982,464B，磁盘可用126,622,199,808B→193,684,373,504B。小生成源码与166个文件哈希保留，外置65,982,464B归属不足未删；两guard工作区和服务/数据库卷未碰。

### 本轮组合产物回收（已完成）

- 四簇main验收与本地合并后，资源owner `04cca925-e59f-42f1-800b-424e7b4cd1b8` 已核占用并正常执行 `cargo clean`（exit 0，8.89秒）。本轮清理前后根target实测55,298,695,168B→0B，已登记target合计55,494,791,168B→65,982,464B，磁盘可用139,102,576,640B→193,909,436,416B；不同时间采样不与上方验收收口样本混算。外置db-plugin-consumer的65,982,464B归属不足且无CACHEDIR.TAG，未删；源码/WIP/证据哈希前后一致。回执 `/tmp/openrust-four-family-cache-cleanup/RESULT.json` 与 `clean.log`；当时S-DV owner `7611b92b` 获独占根target重建窗口，此为历史采样，当前资源归属见上方收尾执行窗口。

### 集中产物回收（已完成，保留原始日志缺口）

- 资源owner `04cca925-e59f-42f1-800b-424e7b4cd1b8` 独占清理窗口，driver先保存；root已通知S-DV/ST-V/driver禁止新增构建，源码修复与caller迁移继续。root当次核cargo/rustc/rustdoc均无进程；main与两历史target顶层为标准Cargo结构、非symlink。owner已按锁/占用/归属复核后集中清理，非仅几百MB精准删。
- 清单基于既有resource_paths、main/worktree/consumer与外置目标去重；共享main target获准完整清后按需重建，未知root-site/M5集中核一次来源后明确清/留理由，不盲删、不无限搁置。保护未合分支源码/WIP、依赖源码、PHP来源、数据库/卷与证据。现已完整清13个确认OpenRust target（含主target、root-site/M5及consumer），逐项实测共93,855,621,120B产物归零，登记剩余Cargo缓存65,982,464B；磁盘可用约101.17→193.60GB。PHP参考源码/证据244.76MB单列保留，不算Cargo cache。回执 `/tmp/openrust-concentrated-cache-retirement/RESULT.json`；root实核13路径已无、caller冻结35文件无漂移。auth-pg一项第二轮同basename覆盖两份原始日志，原预期SHA/退出码/字节记录保留且明确不可恢复；其余24份日志SHA核对一致。原失败ROOT-AUDIT及RESULT.log_integrity_audit保留，不改expected hash或重跑伪造历史。
- 下一唯一构建目录 `/Users/Home/Codes/Rust/openrust/target`；该S-DV重建窗口属历史阶段，现S-DV已完成main组合并安全退役；当前target归属以本页收尾窗口和ST-V owner实际交接为准。跨簇只留下一已安排步骤明确需要的一份缓存，不为历史feature持续堆积；当时采用 50 GB / 100 GB，后续按现行规范。

执行节奏：先按依赖完成全部已授权源码及配套调用方、示例、模板和测试代码，ready_for_verify仅登记待最终统一验收，不逐簇启动常规验证；最终通过前不提交交付。不得继续按每个方法/小改动重复跑测试或Clippy；当前在途命令自然结束，已有有效证据按实际影响复用，规则见[集中验证](conventions.md#功能簇提交前集中验证)。

持续提速约束：每次worker交接、门禁结束或阻塞变化时，总控按e25ec75核对关键路径、下一独立簇是否实际编码、重复审查/构建和逐阶段停派、资源归属及释放条件；发现可避免等待立即调整，用现有日志记录前后事实，并向架构协调会话发送一次含瓶颈、调整、下一动作/owner及证据位置的简短事件回执。无变化不发、不高频轮询，不新建计划/Goal/守护；默认20%额度保留及真实授权边界不变。

| 功能簇 | 状态 | 交付与验收 |
| --- | --- | --- |
| R1-S 根结构与普通模块挂载 | 已完成结构子交付34dfb5e | 根Laravel职责、同package lib/server、资源内聚、模块公开挂载；按范围实际门禁与独立审查通过后可限定提交，仅结构子交付 |
| R2 基础：Container/Provider/模块配置/TestApp | R2-A/A2/A3、R2-B、R2-C、R2-D、R2-M 各子簇已验收 | 全体register后boot、统一Container与typed配置、最小TestApp及模块闭环；先形成真实CRUD/扩展接入的稳定API，不依赖完整R1-E |
| R2-M 模块最小闭环与目录迁移 | 真实服务与生成项目已验收，本簇提交 | modules/ 声明注册、模块配置/资源/统一迁移、依赖与冲突错误；示例迁至 modules/example，真实HTTP及双库验证；完整管理命令归R13 |
| R1-E 真实CRUD与当前能力示例 | 已验收并提交 bc3fa01 | 真实 Article CRUD；当前公开能力进入唯一模块示例；PostgreSQL 18/MySQL 8.4、生成项目、真实服务与整组门禁已验收。Collector PostgreSQL 隔离 sink delivery/unavailable 已复验通过，详见 testing |
| R2 后继内核与测试底座 | R2-N 模块依赖图与顺序已完成 | `.requires` 硬依赖、稳定拓扑顺序、register/boot 一致；真实 PostgreSQL/MySQL HTTP、生成项目、根无参数 TCP、工作区门禁均有本簇证据 |
| R3 HTTP 与 Web | R3-F 已提交163bb49；C04-A 已提交08be69d；C04-B1输入基础已验收；C04-B2类型便捷方法已通过适用验收；C04-C Cookie/响应子簇已验收；C04-D1子簇适用验收通过；C04-D2基础Store API子范围已验收，扩展生命周期后继 | 按历史决策 078实现无正文请求检查与固定上游协商；C03其余及完整C04范围继续保留 |
| 模块包固定范围细化 | 待逐项盘点 | 对正式laravel-modules固定源码/文档的导航、源码与测试拆行为矩阵，分类必要Rust差异/用户约定/缺口；不把目录规范或显式Plugin等同完整模块管理 |

## 后继路由

- **测试源码目录隔离（已授权，待分批迁移）**：执行[测试源码与生产源码分离](conventions.md#测试源码与生产源码分离)。新增和修改的第一方测试直接按新规范归位。历史内联用例、源目录的独立测试文件和测试专用夹具按所属 package 或模块盘点后逐批迁到其 `tests/`，私有单元测试放对应 `tests/unit/`，保留薄挂载。先核对每批原用例和 Cargo 入口，迁移后验证仍实际收集与执行，保留 feature、环境和真实服务隔离，不扩大公开可见性，不改在途冻结输入。现行目录规则优先于旧验收记录的布局描述。

- **高价值测试精简与入口整合（已授权，T01已验收）**：T01已由 `b7afcfe` 提交，extensions15例纳入既有http_session26例、全部41项逐名保全，显式入口46→45、Session family8→7，环境子进程/DB/Redis等隔离入口保留。当前 `tests/http/session.rs` 已公开挂载私有扩展模块，不能再按旧只读审计重复实施；实际双feature、完整门禁及计时限制见[验收记录](memory/testing.md#t01-session纯行为测试入口整合限定试点已验收提交见git)，单样本未证明整体开发显著提速。后继只按[测试分层与入口整合](conventions.md#测试分层与入口整合)核独有缺陷、真实编译入口、feature及全局状态后领取；不按数量配额删、不预报倍数、不重排在途冻结测试。

- **宏观主线**：[两阶段框架主线](plans/laravel-rebuild.md#两阶段框架主线)；先验阶段一Laravel+modules核心基线，再做已纳入阶段二通用增强，均是框架main而非产品可选快速开发模块。R0–R16仅技术分解；现增强保留复用，不搬走核心Config/Queue/Scheduler/安全，当前B2不变，发布整理须两阶段既定范围验收。

- **可选模块边界**：原V2版本阶段取消，后台/App需求转入[可选快速开发模块](plans/optional-rapid-development-modules.md#开发与使用边界)，暂不开发，待用户明确开工。R14、两个框架阶段或持续Goal“全部完成”不能自动启动；main只推进框架/唯一示例，不清理现有WIP，不为暂不开发业务模块预建分支或默认合并；已授权框架任务分支按并行规范执行。框架配置管理、Queue/Scheduler及官方扩展继续；官方工具自身界面属于对应可选包，不受业务后台冻结限制。

- 阶段一R2公共声明/覆盖/错误契约→R13包发现与生成工具：[Provider编译期发现Spec](plans/laravel-rebuild.md#r2r13-provider-编译期发现-spec)（L044.02/模块M016），复用R2-M/N清单、拓扑和register→boot；仅规划未实现，不抢占当前HTTP。
  - 当前前置只读Spec：复用已交付Q会话 `ea706795-768f-43c9-9a56-33b38c0abfac`，只核公开声明/退出/覆盖/错误与现ModuleManifest接线，无源码/构建/新分支变更；候选Spec已交，auto/explicit同Provider重复是否应去重需补核固定Application::register/force与RegisterProviders顺序，不能把上游正常重复误判冲突；root冻结后才分配实施，不能把显式清单冒称自动发现已完成。 后续重复注册契约与真实PHP证据已收：`/tmp/openrust-provider-discovery-registration-contract.md`、`/tmp/openrust-provider-registration-oracle/receipt.json`；普通重复首次实例、force启动前/后与dont-discover不阻止显式均由真方法实跑并重放一致，configured三段顺序仍仅源码证据。自动发现仍未实现；普通/force生命周期已由R2-P `4ac8099`及main组合`9e5a941`限定交付，见[当前core事实](core/core.md)，不能继续把该历史探针结论当作force待实现。

- 阶段一R2容器Binding历史缺口（矩阵L012.02）：保留原factory的公开`extend`及作用域/实例装饰已由R2-X `d299ca4`与Q/X main组合限定验收，并在当前core事实记录；不再按旧D3结论重复领取。完整Binding/Container其余上游对应仍按矩阵逐项闭合，不由已完成子范围外推。

- 阶段二R11 MQTT按[总计划可选扩展契约](plans/laravel-rebuild.md#r11-明确待办mqtt-可选服务端与客户端)排期：client/Broker按需独立引入，沿既有Provider加载，client操作/订阅CLI/Broker CLI才产生网络副作用；仅规划，不抢占当前HTTP簇。

R1-S → R2基础 → R2-M模块闭环 → R1-E当前CRUD/能力贯通；R2后继及R3 HTTP → R4 数据 → SQLite内置适配优先（现R4基础）/R5 Model → R6 Factory → R7 安全 → R8 Cache/Storage → R9 视图/通信 → R10 异步/事务 → R11 网络/组合 → R12 搜索/特性/观测 → R13 开发体验 ；阶段一核心门禁后，R16官方框架扩展及工具自身界面按各自前置在阶段二分批领取，最终证据汇入R14；原R15版本阶段取消，业务后台/App转为可选模块并暂不开发，官方工具自身UI随包阶段二交付，待用户明确开工。每个后继能力同簇补示例；R1-E不等待未来全框架完成，R2基础不等待完整示例，消除循环依赖。阶段内部按总计划拆可验证提交。

## 23项静态纠偏的唯一执行映射

来源[固定行为整改对照](plans/laravel-rebuild.md#执行偏离的纠正)。2026-11-02 已将三域46项审计逐项去重归入本表、既有C04-F/G和阶段二，不增第二队列。报告与manifest SHA核一致；取证来源仍为固定framework `d52c3b3`、docs `960eedf`，原草稿priority-five和旧行号不改变用户首批配置→bootstrap/app→固定API→模块启停/发现→DB门控顺序。

“确认缺口”不否定既有资产；“已编码待验”仅指未具当前contract证据的子行为；“已验子范围”不关闭全家族。必要Rust适配须写具体编译/类型/所有权/async约束与等价证据，不能据其省略功能。用户特约/授权增强与完成状态分别记录。原报告中“无运行期Provider注册”和本批“仅WIP”已纠正，历史真实验收保持。

| 项 | 既有阶段 | owner与当前边界 | 审计归并与稳定矩阵 |
| --- | --- | --- | --- |
| 1 app与modules | R1/R2/R13 | 新总控协调；9035a00交付应用/模块同内核与生成子范围；bootstrap测试归属、固定API与模块完整管理仍按首批表整改 | 根app/modules并存为用户特约，不等于Modules全量完成 |
| 2 Rust代码配置 | R2/R13 | 新总控协调；生产Rust manifest/dotenv/typed/cache与可编辑默认值、最终配置依赖已由d952199限定验收；标准依赖/具名队列3703939已提交；其余配置完整对应与开发预检继续核 | foundation:F6【确认缺口】L005.02/03；services:redis_default【用户特约，官方应用默认另核】L005/C14；显式编译manifest/typed工厂拒绝缓存为必要Rust适配 |
| 3 Exceptions/Handler | R3 C06/R12 | 新总控协调；9035a00已验应用/模块report/render/respond限定链；完整异常行为继续逐项核 | http:assets:handler【已验子范围】L027.02/03/04 |
| 4 web组/CSRF/Session | R3 C04/R7 | 新总控协调；9035a00已验显式组/419/默认边界；分布式Session blocking等仍开放 | http:assets:web【已验子范围】L017.02/03/04；http:assets:session【已有已验资产＋blocking确认缺口】L025.04 |
| 5 分页/API Resource | R3 C04/C05→R4 C07→R5 C09 | 新总控协调；沿既有分页Spec补Resource分页链接/meta与真正NoContent，不重造Query底座 | http:2【确认缺口，Query子范围已验】L059.02/03/05/06 L068.03/04/06；http:3【确认缺口】L020.01/03 |
| 6 逐迁移连接/事务/事件 | R4–R6 C10 | 新总控协调；现Db/迁移已验资产保留；逐迁移连接、嵌套事务及回调按真实DB前置补齐 | services:db_tx【既有资产保留，未闭合子行为待核/待验】C07/C08/C19；services:migration【确认缺口】C10 |
| 7 Queue afterCommit | R10 C19 | 新总控协调；按R4事务前置领取，事务内持久写入不替代提交回调 | services:after_commit【确认缺口】C19 |
| 8 每任务调度锁策略 | R10 C20 | 新总控协调；统一slot资产保留，补每任务withoutOverlapping/onOneServer语义 | services:schedule【确认缺口】C20 |
| 9 Storage throw/ACL/迟到发布 | R8 C15 | 新总控协调；已有visibility/presign限定验收保留；ACL/迟到发布/throw=false与copy/move仍开放 | services:storage_throw【确认缺口，Result仅为Rust形式】C15；services:storage_visibility【已有资产＋未验子行为/确认缺口】C15 |
| 10 named guard/provider、Gate/Policy | R7 C12/C13 | 新总控协调；依HTTP/Session前置补named manager/Gate，包括数据库Session元数据从绑定身份投影补为真实Guard.id解析/失效生命周期；现JWT/Session/RBAC资产不抹除 | http:5【确认缺口】L051.07/08 L052.02/04/05 |
| 11 URL复杂参数/路由绑定 | R3 C03/C04 | 新总控协调；简单golden不算全量；Request已有已验范围见C04-F/G，完整路由/绑定仍开放；matchit普通参数25上限已安全拒绝，但仍是需解除的后端差距，不是Rust语言必要适配 | http:1【resource/apiResource七动作确认缺口】L018.04 L015.03/05；http:9【复杂参数/隐式绑定确认缺口】L015.05 L024.03/04/06；http:assets:request【已验子范围＋明确Rust资源/async适配】L019.02–07 |
| 12 Encrypt/Hash drivers | R7 C13 | 新总控协调；补官方算法/选择与key默认工作流，保AAD/key-ring/容量增强，不改用户真实.env | foundation:F5【确认缺口】L054.02；http:6【确认缺口＋授权增强保留】L054.02/03 L055.02/03/04 |
| 13 Query/Eloquent | R4/R5 C07–C09 | 新总控协调；9035a00默认SELECT *、真实count/分页/隔离已验；cursor、完整Query/ORM仍开放，不以SeaORM抵扣 | http:assets:query【已验子范围＋cursor缺口】L059.02/03/05；services:query【同一子范围，cursor确认缺口】C07/L059；services:eloquent【确认缺口】C08/C09 |
| 14 Factory/SQLite | R6/R4 C10/C11 | root；SQLite内置单机支持按用户要求在当前冻结R2提交后优先，现R4基础可提前；Factory仍依R5。seed/custom driver资产保留，不冒充完整支持 | services:factory【确认缺口】C10/C11 |
| 15 Container/Facade/发现 | R2/R13 C02 | 新总控协调；运行期register/force、extend/rebinding/scoped已验；contextual/tag/call、deferred与包自动发现仍缺；同内核嵌套注册39086cf及模块运行发现d32162d已限定验收，不重复领取 | foundation:F1【确认缺口】L012.02/03/04；foundation:F2【已验注册子范围＋deferred/发现缺口】L013.02/03/04 L044.02；foundation:facade【完整对应待核】L014.03/04 L079.03 |
| 16 FormRequest生命周期 | R3 C04/R7 | 新总控协调；沿Handler/Web/权限补prepare/authorize/validated与完整规则 | http:4【确认缺口】L026.03/05/07/10/12 |
| 17 Cache Repository契约 | R8 C14 | 新总控协调；现tags/driver资产保留，补原子add/increment/flexible等契约 | services:cache【确认缺口】C14 |
| 18 Events/Bus完整派发 | R10 C18/C19 | 新总控协调；typed/queued及chain/batch既有资产保留；until/defer/subscribe/afterResponse等未闭合 | services:bus【已编码资产/未闭合子行为待验及缺口】C18/C19；services:events【同类分类，确认缺口保留】C18/C19 |
| 19 Mail/Mailable/Notifications | R9/R10 C17 | 新总控协调；补Mailable/Notifiable/channel完整语义；SMS直发增强保留，真实外部条件单列 | services:mailable【确认缺口】C17；services:notification【确认缺口】C17；services:sms【授权增强＋未验供应商范围】C17 |
| 20 Views/I18n | R9 C16 | 新总控协调；保请求错误包与ICU资产，补composer/creator/share及Laravel替换/choice | http:7【确认缺口】L021.03/04；http:8【确认缺口＋ICU授权增强】L041.02/03；http:assets:view_errors【已验子范围，完整三领域待核】L021/L026/L027 |
| 21 CLI/模块管理/独立安装 | R1/R2/R6/R13 C26/C28 | 新总控协调；app生成/Provider自动登记已验；嵌套命名、持久模块命令/事件/发现、测试部署工作流按既有项补齐 | foundation:F3【确认缺口】M018/M020 B005/B006；foundation:F4【嵌套命名缺口，自动登记已存在】L007.03 L013.02；foundation:testing【分项缺口/待核，TestApp隔离库资产保留】L074.03/04；foundation:deployment【入口缺口/编译等价待核，config cache已验】L010.04/05 |
| 22 Process/Context/HTTP client | R11/R12 C21/C25 | 新总控协调；HTTP/context入口资产不等于逐方法契约；按既有网络/进程前置补齐 | services:http_client【已有资产＋未验子行为/缺口】C21/C25；services:http_fake【确认缺口】C21/C25；services:process【确认缺口】C21/C25；services:context【确认缺口，task-local仅Rust形式】C21/C25 |
| 23 动态命名限流 | R3/R7 C13 | 新总控协调；固定窗口不冒全契约，按既有计划后继 | 本次46项无新增独立发现，不删除原范围 |

`services:optional_ecosystem`【分包待核】直接归本页既有“官方能力全量交付”及阶段二，稳定ID L071–073、L080/081、L087/089/090/091/095/099/101与覆盖计划；官方工具自身UI仍必交，核心资产不抵扣。`services:enhancements`【用户特约/授权增强】归既有R11/R12及各领域计划，MQTT/gRPC/Settings/正文加密/SMS保留并融入统一机制；不启动冻结业务模块或独立AI产品。

## 必须带入新矩阵的现有缺口

- 统一返回格式等已有优秀机制 → R0 登记对应 Laravel 职责与保留价值，R3/R5 在 Response / Resource / 异常渲染融合并回归，见[融合计划](plans/laravel-rebuild.md#已有优秀机制的融合)；重写不等于删除能力。
- 原 M5 可观测性/资源独立审查与完整生产门禁 → R12/R14。
- 原 DX1–DX8、F9/F10：默认 cargo run 与 Cargo 正常兼容设计、TestApp/assertions、factory、命令/生成/stubs、只读 doctor、dev 编排、文档和更新体验 → R1/R2/R6/R13。
- 双库负载均衡、多 API/worker/scheduler、共享状态/文件/实时、滚动升级/依赖故障、最低版本与性能 → R14。
- SQLite 完整支持 → R6 及各能力适用验收；不能只接通连接。
- [可选快速开发模块 后台/App](plans/optional-rapid-development-modules.md) → R15；PC 仅规划；独立 AI 产品未启动。

## 外部条件与证据缺口

Twilio 实发缺授权账号/凭据/目标；其他供应商与生态包实测条件在 R0 盘点。遇到外部缺口继续无依赖工作，但不能标记真实发送、支付、完整生态或全部验收完成。

## R1-S 结构子簇历史 Spec

1. 框架与应用保持同仓、main集成；后续默认共享main目录按文件分工，既有worktree安全交接退役；根 package 为 openrust，src/lib.rs 提供框架，薄 src/main.rs 提供默认 server。
2. default-members=["."]、default-run="server"；根无参数 cargo run 启动唯一应用，不需要选择 package/bin。
3. app/bootstrap/config/database/public/resources/routes/storage/tests 直接位于根，根工作目录自然加载配置和资源。
4. 当前结构子簇保全 app/example 代码和测试；最终唯一示例为 modules/example，随 R2-M 模块契约迁移并由根 server 统一装配，不再次独立做一轮仅改名的搬迁。
5. 根app支持Laravel常规应用开发，modules可选并存，不强制业务模块化；框架src保持通用实现；示例 migration/seed、模板、proto/生成脚本及测试 fixture 留模块内，根只提供通用结构和公开挂载；根不得有示例 build.rs、build 依赖或样例业务特判。
6. 工具与公开 consumers 保持其编译/验收职责；CLI new 是框架能力，仅在 /tmp 验证用户应用，不新建长期源码仓库。
7. ADR-001 替代 历史决策 073 的模块位置目标，其他有效根结构边界保留；保留旧布局非界面真实证据，不改 Git 历史，不当作本次通过。
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
- 资源：只在已清理的 `/Users/Home/Codes/Rust/openrust/target` 按需重建，jobs2；不占端口/库。构建前重测所有登记缓存和预计增量。当时采用 50 GB / 100 GB 和 10% / 5%，后续按现行规范；独立执行者受总控暂停/停止协调，原默认80%额度不变。完成后按真实归属清理，不动未合入分支。

## R2-P Provider 完整注册生命周期（main限定组合已交付）

以下96/97源、生成、资源与Clippy计数保存分支执行过程；当前main结果见顶部组合记录，P源分支的文档保全核对与安全退役其后已完成，见本页顶部main组合记录及正常退役回执；下文只保留分支历史，不再领取退役操作。

- 已冻结普通/force与boot前后完整家族；固定Laravel真实正常/失败探针、Rust接口候选及独审在 `/tmp/openrust-provider-registration-{frozen-spec,rust-contract,rust-contract-review}.md`。不以启动前去重代替晚注册；异步register/原位置rebinding、绑定generation与缓存槽同锁准入为明确Rust实现约束；失败已发生贡献按PHP保留，不造Provider全局事务。
- 同项目workspace `wks_f7e111d2b503053b`，branch `tasks/provider-registration-family`，base `3aba0a3`，cwd `/Users/Home/.paseo/worktrees/02joa2lr/openrust-provider-registration-family`。`04cca925-e59f-42f1-800b-424e7b4cd1b8`负责provider/app/resources与配置owner/schema；`0b1cd54b-3828-4eff-bf97-5c519973a41e`仅负责container.rs及container子目录；既有callers负责其余Provider/模块/consumer/模板迁移，禁止重叠编辑。
- 共享API已收敛：ServiceProvider::register改BoxFuture；Context涉及绑定/回调的方法原位置await；rebinding在已绑定时立即解析并返回Option<Arc<T>>。configure_after_register改FnOnce(Container)→BoxFuture，原register后覆盖时点不变；启动前建立唯一Container/StartupResources，配置登记当场可读但保留boot前校验，默认合并不得重复应用。
- 当前冻结96个变更文件，生成验收另外绑定实际运行输入；core125、仓储47、公开Container契约24，以及独立container-extension2、test-app1、config读取consumer6均实际通过。Register重放覆盖后续set的缺陷已修并经不同模型静态复审。默认严格Clippy三次历史失败保留；用户授权单次重验已通过，源码限定分支提交 `4ac8099`；main三父组合提交 `9e5a94198b421987851b3580b27fa90370feeb77` 已通过限定门禁。root精确刷新3个consumer锁，新增包匹配根锁，无第三方升级。
- 生成首轮view遗漏await已修；第二轮确认共享target旧CLI复用，未误改正确异步模板。attempt3 `/tmp/openrust-r2p-generated-acceptance-attempt3/` 实际强制重编当前CLI并绑定97输入，13阶段通过：普通生成/Provider/TCP/公开fixture、MySQL认证2项无skip；root独立SQL审计前后61库集合摘要一致。前两轮失败保留。其后Cookie生产修复已用保留生成项目补验3项plain及2项真实MySQL auth具名测试，均通过/无skip，97源与生成锁无漂移、前后61库集合一致。`/tmp/openrust-r2p-cookie-generated-supplement/result.json` 与root五日志核验留证，不重跑未受影响生成/CLI/Marker TCP。
- Cookie当前节读取改has(session)，保留已解析singleton快照；固定PHP真实CookieServiceProvider探针确认早解析不热建，新增身份回归后cookie9、默认workspace lib/tests862、全feature1650/68ignored、doctest22/34ignored及24/40ignored、双warnings-denied docs、no-default/MSRV/deny均有新回执。`/tmp/openrust-r2p-cookie-matrix-handoff.json` 关联10日志，root逐哈希核对全匹配；ignored不计真实服务，严格Clippy按用户授权单次重验通过；本行数字仍只属分支证据，main新结果见顶部组合记录。
- 资源补验中的unregistered_count79已确证是对unknown字符串按字符计数，不是79target，原result保留且resource-audit.md纠正；限定main/worktree/tmp扫描无新增target，登记15路径约94.30GB仍为覆盖有界的小计。P补验已收尾8个确证自有测试二进制，释放379,628,016B；root逐项核缺失状态、来源/.d/日志SHA，97源与224保护项由owner前后核不变，生成源码/锁/证据/共享依赖保留，回执 `/tmp/openrust-r2p-generated-cleanup/result.json` 和root-audit.json。
- 分支当时owner回执主target76,380,396KiB、磁盘可用101.3GB；其他登记target仍需合计，不能冒称全量已核。后续构建按现行规范预检合计与预计增长。当时采用 50 GB / 10% 软、100 GB / 5% 硬；未知缓存不清。派发/停止ID入口 `/tmp/openrust-provider-registration-dispatch.json`，总控用正式Paseo stop逐任务协调；不另建Goal/guard、不推送、不提前退役未合分支。

## R2-CC 配置缓存家族（分支提交7be446d；main组合见本页顶部）

本条为唯一CC家族条目；来源已通过main限定组合并退役，以下owner/分支/窗口为历史证据，当前代码配置后继见顶部首批整改。

<a id="r2-cc-配置缓存家族分支限定验收历史"></a>

### 分支限定验收历史

- 本分支复用唯一Application/Provider/ConfigSources/ConfigRepository实现config:cache与config:clear。首次命令引导保持正常缓存屏障，实际派发才清选定缓存；fresh启动、资源关闭、原子写/回读、失败无半写、双阶段env与模块屏障同簇验证，不热改在途App。
- 最终18文件 `/tmp/openrust-config-cache-command-bootstrap-script-delta2.json` 已独立复审。修后默认851、全feature1638/68ignored、双严格Clippy、配置77/生命周期18/模块1/CLI1、no-default/MSRV及普通生成8/两类真实进程脚本通过；ignored不算服务。首链驱动忽略规则断言失败及旧benchmark失败均保留；强制重编CLI的续跑与原失败分开索引。
- 修后双doctest/docs均通过，离线deny仅复用相同依赖图/策略输入且注明2026-10-30 advisory快照；根fmt/diff通过，限定本地提交见Git。root核61项证据及42生成文件。两任务temp已退役，未实际执行范围过宽的cargo clean，约1GiB同名包缓存留待root下次资源交接按精确归属复核；不误删其他产物。以上为CC分支阶段证据；CC已与R03/typed/Signed完成四簇main限定组合，P另与S-DV完成三父main限定组合，发布全后端矩阵保持开放。


- 稳定main `9581a53` 起点，不依赖未提交R2-P；固定Laravel ConfigCache/Clear、LoadConfiguration/LoadEnvironmentVariables 与现来源/命令机制，完成官方 `config:cache/config:clear`、同Application fresh配置、缓存命中与两阶段env路径、原子文件/失败清除/资源关闭、模块/生成/真实复启验收；Spec `/tmp/openrust-config-cache-frozen-spec.md`，候选与来源 `/tmp/openrust-config-cache-contract.md`，未实现/验收项不得提前关闭L005.04。
- owner `cbc80e79-d801-4d4e-aef0-c4e7f4551346`，同项目workspace `wks_d752446d3d277b26`，branch `tasks/config-cache-family`，cwd `/Users/Home/.paseo/worktrees/02joa2lr/openrust-config-cache-family`；本lane独占自身源码/模块/模板/说明，manifest/lock/共享Agentdocs与main索引root协调。不复制其他worktree WIP，后继R2-P异步API/R03文件来源在实际集成时组合验证。
- 已完成多轮生产批修及历史分支验收：配置77、生命周期16、默认workspace849、双严格Clippy/docs、MSRV/no-default/deny、生成与真实缓存复启具证据；全特性all-targets的额外复制benchmark曾失败，测试目标补验与原失败分别保留，不把整条命令写成通过。后续独审发现维护命令应先正常bootstrap再执行清缓存，原owner已修该生产顺序，18文件新冻结 `/tmp/openrust-config-cache-command-bootstrap-delta.json`；原结果须按输入影响复用，命令/缓存真实验收需新输入重验。
- 限定分支提交 `7be446d81c6c1aa0269c4f2aa36b0ce5a4e27aa1`，26文件（18实现/用户文档+8共享记录），worktree干净。固定命令引导/fresh/原子缓存/env与模块屏障、生成器和两类真实复启由不同模型审查及实际门禁验证。默认workspace lib/tests851、全feature1638/68ignored、doctest22/34ignored与24/40ignored、双严格Clippy/docs、no-default/MSRV、普通生成8通过；旧offline deny仅复用相同依赖图/策略与2026-10-30 advisory快照。
- 最终证据 `/tmp/openrust-config-cache-final-manifest.json`（SHA `73da1b7712e35176dff0bd30b63004d07e7f35f5e6e73603c99b6005c3508119`）；root核61项和42生成文件。原driver断言失败/旧benchmark失败保留；共享target后续更换server，原真实脚本首尾SHA与expected一致，不声称现在仍可重算旧binary。
- 两自有temp目录已退役，磁盘实测回收13,570,048B，共享target变化0；Cargo忽略包URL/版本限定，会清全部同名sample-app，未执行实际clean，约1GiB混合归属缓存由root下次资源交接精确核对，未知产物保护。分支提交时未main集成；现已纳入R03/typed/Signed四簇main组合；P后续与S-DV另经main限定组合交付；原工作区/分支已正常退役、workspace已归档，发布全后端矩阵保持开放。

## R2/R10 运行时配置与动态调度（仅规划，按依赖领取）

- 依据 历史决策 080 与 [运行时配置与动态调度计划](plans/runtime-configuration-scheduling.md)；当前 HTTP 主功能簇不抢占，本轮没有代码实现或真实验收。
- R2：V1必须交付Laravel简洁typed分组DB业务设置与权限保护管理API/CLI（宿主启用可选）；开发者注册字段、管理员后台保存，白名单/权限/完整校验/CAS审计/快照/通知轮询/故障期限封装内部，持久实现依赖R4事务、R7权限审计及适用R8，基础设施启动项不热换；邮件/SMS/支付/存储业务secret加密、driver受控激活或pending_restart，区分DB保存/节点应用。
- R9/R10 Notification/SMS细化沿[短信渠道与多服务商交付计划](plans/sms-delivery.md)：先SMS-0来源/SDK审计与Notification公开渠道契约，再按前置做具名连接、官方Vonage对应及阶段二国内/其他服务商增强；队列、动态设置和回执复用R10/历史决策 080。候选SDK未选型，跨attempt配置版本策略待Spec与既定运行时contract统一；真实发送需获准凭据/号码，当前不抢占D10-G或启动供应商实现。
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
3. `Scope::run` 仅在当前 future 安装与所属 Container 匹配的 task-local scope；`ApplicationState::resolve` 只在匹配时取该 scope，普通 `tokio::spawn` 不继承它。
4. `Scope::spawn` 使用同一 ResourceOwner 管理任务，显式传递允许的 ExecutionMetadata、预算和取消；关闭时取消/join，禁止跨入口继承身份或已关闭 scope。
5. Queue 的 `JobContext::service` 优先本次 attempt scope，worker 与自定义 driver 都经实际 handler 入口建 scope；retry 必须得到新实例，租约失败和超时也清理。
6. Scheduler 每次触发才进入 scope，不把任务服务缓存进 Scheduler singleton；命令自身 scope 与任务触发 scope 分离。
7. 受管回调逆序且仅一次；入口 future 取消后由 App 持有的子资源关闭队列接管，App.shutdown 等待并聚合失败，不以 `Drop` 冒充异步关闭。
8. 用真实 HTTP stream/drop、连续 console 命令、queue 成败/超时/取消/重试与 scheduler 多次触发验证隔离和清理；适用 PostgreSQL 18/MySQL 8.4、生成项目及完整门禁后限定提交。
9. 本簇只处理入口生命周期；R2-D TestApp、R2-M 模块目录及 R1-E CRUD 不计入完成，保留既有 Figment/第三方 driver 差异。

## 当前 R2-D Spec：TestApp 最小路径

状态：生产与测试共用 Provider 构建路径、示例和生成应用已接入；默认/全特性、真实双库与 Redis 回归及 MSRV 门禁见 [R2-D 测试证据](memory/testing.md#r2-d-testapp-验收证据)。本簇完成后继续 R2-M，不把 TestApp 当模块系统或 CRUD 已交付。all-features workspace 一次 fixture 启动 `internal_error` 的原因仍未证实，保留日志及安全阶段诊断。

1. 复用生产 `ApplicationDefinition::providers` 清单、CoreCommands、应用配置绑定及唯一 `ProviderBuilder::start`；测试显式传 Figment，不修改全局环境或工作目录。
2. 在清单排序前允许按具体 ProviderId 移除；仍执行依赖检查，被移除模块不得注册、启动或贡献路由。
3. 全体 `register` 完成后、容器封口前执行一次类型化绑定覆盖，支持 instance/singleton/scoped；任意 `boot` 都看到最终绑定，不建第二服务表。
4. `TestApp` 持有真实 App 与 Router，`request` 走生产 build_router/oneshot；响应正文有字节上限和截止时间，断言错误不回显敏感值。
5. 流式响应可由调用方持有并显式 Drop；App 关闭等待请求 scope 清理，失败向测试传播；重置先关闭旧 App 再重建，不能仅 reset 根 scope。
6. 验收 fake Mailer/业务接口在 boot 与真实 HTTP 可见、同进程两个 App 隔离、模块移除 404/health 保留、取消与资源回收、status/header/JSON 断言。
7. 同步当前示例、公开 consumer、CLI 生成应用和用户文档；默认/全特性门禁、适用双库真实测试、MSRV/no-default/deny 与限定提交后关闭本簇。
8. 本簇只提供测试宿主与覆盖/移除契约；R2-M 目录和 R1-E CRUD 仍未交付。

## 当前 R2-M Spec：模块最小闭环与目录迁移

状态：源码、真实服务与适用门禁通过，随本簇限定本地提交收口；固定 laravel-modules v13.2.0 的 B007–B010、B016–B020 与 ADR-001 为当前实现依据，其余矩阵条目按实际能力保留缺口。

1. 编译期描述 `modules/<name>` 的身份、来源、priority、按序 Provider 与资源；启动时读取独立启用快照，未编译名称、同名/规范化碰撞均拒绝，缺状态不默认启用。
2. 仅启用模块按 priority 升序稳定排列，模块内 Provider 保持声明顺序；不凭 priority 重排依赖图，显式 Provider 依赖继续由唯一清单检查。
3. 全体 Provider 共用原 Container 的 register 屏障；模块配置在声明的配置 Provider boot 时点逐文件调用唯一仓储 overlay，不能统一提前，模块值胜、数字数组逐索引替换并保留尾部，前序读取不追溯。
4. 启用模块才贡献路由、配置、资源与迁移；名称/资源/路由冲突在 Ready 前失败，已登记资源走原启动失败有界回收，不新建模块服务表或第二启动器。
5. `Application` 与 `ProviderBuilder` 共用编译描述和启用快照入口；禁用只影响下一次应用构建，当前 App 不热卸载、不回滚迁移或删除源码。
6. 将唯一现有示例完整迁至 `modules/example/`，根只保留通用编译声明/清单；示例真实 HTTP、TestApp 替换/移除、数据库迁移及资源经公开模块入口执行。
7. 验收模块排序/依赖/配置时序与冲突/失败、临时第二模块无需改框架 src、禁用后的真实 HTTP，以及 PostgreSQL 18/MySQL 8.4、生成应用和适用全门禁。
8. 模块状态写入、管理命令、生成、发布与完整 B 矩阵归 R13/后继，不把静态 Rust 编译声明称为 PHP 源码热发现。

## 当前 R1-E Spec：真实业务 CRUD 与公开能力示例

状态：已验收并提交 `bc3fa01`；本节保留该簇规格与边界。复用唯一 `modules/example`、现有 Provider/Db/Auth/HTTP 能力，不新增框架业务内核。

1. 新增有所有者的 Article 业务实体、模块迁移及 SeaORM model；仅在数据库和认证能力就绪时挂载 `/api/example/articles`。
2. 通过公开 `CurrentUser`、`AuthLayer`、`Validated`/`Json`、`Db` 和 `Router` 实现列表、详情、新增、更新、删除，业务数据仍只属于 `modules/example`。
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

状态：实现、验收完成；本簇补齐模块之间的硬依赖契约，不实现 R13 模块生成/启停命令。历史决策 075 与验收证据见 [R2-N 记录](memory/testing.md#r2-n-模块硬依赖与确定性启动顺序)。

1. `ModuleDescriptor` 增加公开 `.requires(name)` 声明；依赖身份沿用模块现有大小写不敏感的名称查找规则。
2. 仅已启用模块参与准备；启用模块声明的依赖必须已编译且启用，缺失/禁用分别以安全、稳定的错误码拒绝，不隐式改写用户模块激活状态。
3. 拒绝重复依赖、自依赖与多节点循环；诊断只暴露模块名/依赖名，不暴露路径或配置秘密。
4. 依赖边优先于旧 `priority`；在所有当前可启动节点中按升序 priority、再按原始编译声明顺序稳定选择，确保同一清单结果确定。
5. `ProviderBuilder` 使用准备后的顺序注册并 boot 模块 Provider；综合示例补一个消费者模块依赖生产者 Provider 的启动场景，证明依赖顺序可观察且不要求修改框架代码。
6. 用模块单元/启动集成测试覆盖反向声明顺序、priority 冲突、依赖禁用/未编译、重复/自依赖、环与确定性排序；执行源码门禁、双库模块 HTTP/生成应用及真实根入口验收，再限定本地提交。
7. 不提供版本约束、自动依赖安装或“启用即递归改状态”；这些涉及 Cargo 编译/管理命令的能力按模块矩阵与 R13 单独验收。

## 当前 R3-A Spec：语义 HTTP 状态默认与统一 JSON 响应

状态：已完成并由 `03c8cfa`、`96f148f`、`d07e13f` 限定提交。R2-N 已由 `48be854` 限定提交；本簇仅收敛 Laravel Response/Validation 对应的 HTTP 状态默认及现有统一 JSON envelope，不领取 FormRequest、Session/CSRF、Resource 新能力。

1. 固定依据为 Laravel 13.39.0 与文档 commit `960eedf0911155ad16326ade49c0ea5d18eda0b8`；响应使用真实 HTTP 状态，validation JSON 使用 422。保持 OpenRust 自有 envelope 增强与 历史决策 008 状态映射。
2. `semantic` 成为 ApiConfig、框架错误 fallback、根默认配置与生成项目默认值；框架错误遵循同一请求策略。`always_ok` 仅在应用显式配置时覆盖状态。
3. 创建成功保留 201，空成功仍按现有类型返回 200；not found、method not allowed、validation、rate limit、overload、timeout 与 5xx 返回语义状态。request ID、机器码、字段错误、脱敏和 JSON 结构保持原契约；HTML、文件、流、SSE、gRPC 保留各自协议。
4. 同步 CLI 生成模板/.env、综合示例、用户文档和 CHANGELOG；修正把 `always_ok` 当隐式默认的测试断言，覆盖省略配置时的 HTTP 行为及显式 override。
5. 生成普通/认证应用并执行 check/test/HTTP；检查根实际 HTTP 入口，执行默认/全特性 workspace 门禁、deny、MSRV/no-default。双库仅对实际依赖数据库的改变路径验收，不以旧记录代替本簇行为测试。

## 当前 R3-C Spec：命名路由与具名 URL 生成

状态：已完成适用验收并由 `801ca4e` 限定提交。对齐 Laravel 13 `Named Routes` 与 `URLs for Named Routes`；名字唯一、可跨路径调整使用，并支持路径参数和额外 query 参数。来源：[Laravel Routing](https://laravel.com/framework/docs/13.x/routing)、[Laravel URL Generation](https://laravel.com/framework/docs/13.x/urls)。

1. 在 `Router` 注册 API 上提供紧邻路由定义的 `.name("users.show")`，记录所属 HTTP route 与完整嵌套 prefix；group 可增加 Laravel 式 name prefix，并按 Laravel 原样字符串拼接（例如 `admin.` + `users.` + `show` → `admin.users.show`）。
2. 重复名称和不受支持的路由模式（通配符、空路径段）在 router build 阶段报错；未知名称、缺失参数和无法安全编码的值返回稳定框架错误并按 Laravel URL generation exception 映射为 500，不 panic、不静默替换。
3. 提供公开 `UrlGenerator` 从名字生成绝对 URL；host/scheme 以 `app.url` 为来源，不信任请求 Host。必需 route 参数进行单段百分号编码，其余参数成为有序 query；仅支持必需 `{parameter}` 段，不声称可选参数、通配参数或模型绑定已完成。具名路径仅剥离单层首尾 `/`；空段保留并于启动时报错。
4. 生成器由生产 `build_router`/应用 HTTP 请求路径公开供 handler 使用，并可在 `modules/example` 的链接/响应代码演示；生成应用应可用同一 API。
5. 黑盒测试覆盖 path 更换不影响名字调用、嵌套 name prefix 与 path prefix、参数编码/缺失、额外 query、绝对 base URL、安全 Host 边界、重复名称启动失败；更新 HTTP Rustdoc、用户 README、core/http 与完整测试记录。
6. 按 §6.3 执行 fmt、默认/all-features workspace clippy/test/doc、deny、MSRV/no-default、生成项目 check/test 与真实 HTTP。路由变更不依赖数据库，不为无关路径重复双库验收。

## 当前 R3-D Spec：Laravel middleware alias 与 group

状态：已完成并限定本地提交。依据 Laravel 13 Middleware：应用在启动时注册 middleware alias 与 named groups，路由/路由组按名称引用；group 成员按声明顺序展开。OpenRust 使用 Provider boot API 注册，通过类型安全的 Tower `Layer` 闭包保留 Rust 扩展方式。

1. `BootContext` 提供 middleware alias/group 注册入口；重复、无效名称与 alias/group 同名冲突返回稳定启动错误。定义可以由不同 Provider 声明，所有注册完成后统一用于 router 构建。
2. `RouteGroup` 与 `Router` 提供 named middleware 使用入口；group 可嵌套，名称递归展开，按声明顺序执行且响应逆序；现有直接 Tower Layer API 保留。
3. 未知引用、循环 group 与空 group 在 `build_router` 前明确失败，不 panic、不静默忽略；middleware 不会应用于用户未选择的路由，也不改变框架内建全局 stack。
4. 在 `modules/example` 通过自定义 middleware alias/group 展示“业务 Provider 登记、业务路由引用”，至少一个真实 HTTP 断言证明只命中目标路由及顺序；生成项目可编译并通过 TestApp 行为测试。
5. 更新框架 HTTP README/Rustdoc、示例文档、core/http、CHANGELOG、context/plan/progress、testing；执行默认/all-features workspace Clippy/tests/docs、fmt、deny、MSRV/no-default、生成项目检查与实际 HTTP，无数据库路径改变时不虚构双库必要性。无注册表引用会 fail closed，`Router::into_router()` 检查路由/middleware错误；全功能已按以上要求验证。

上游依据：[Laravel Middleware](https://laravel.com/framework/docs/13.x/middleware#middleware-groups)、[Middleware Aliases](https://laravel.com/framework/docs/13.x/middleware#middleware-aliases)。

## 当前 R3-E Spec：Laravel fallback routes

1. `Router::fallback(handler)` 注册应用级未匹配请求处理器；它只处理没有匹配路由的请求，既有路径上的方法不匹配仍返回 405。
2. `RouteGroup` 可限定 fallback 的静态路径前缀与 middleware；同一完整 scope 不接受多个 fallback，启动/公开 router 构建须返回稳定错误，不 panic 或静默覆盖。Axum 对参数化/通配符前缀 fallback 会产生 catch-all 冲突，当前返回稳定 `http_fallback_scope_unsupported` 并在文档公开边界。
3. 若请求为 GET/HEAD 且没有业务路由匹配，先尝试现有 public 目录的静态资源服务；资源命中保持原响应，缺失后才进入业务 fallback。自定义 fallback 允许应用明确接管 `/api/*` 与 `/ws/*` 未匹配请求；未登记时继续保持框架 JSON 404。
4. 静态检查使用独立空 body 请求，仅用于 ServeDir；原始请求、headers、query、body 必须完整交给 fallback handler。非 GET/HEAD 不尝试静态资源。
5. `modules/example` 提供一个网页 fallback，实际 HTTP 验证命中自定义响应、middleware、正常路由优先、405、静态资源优先和默认 API 404；不覆盖或替代框架默认 fallback。
6. 补公开 Rustdoc/HTTP README、示例说明、core/http、CHANGELOG、context/plan/progress、testing；生成项目能通过 TestApp fallback 行为测试。
7. 执行 §6.3 workspace fmt、default/all-features clippy/tests/docs、deny、MSRV/no-default、生成项目 check/test 与真实 HTTP；不改数据库行为，不虚构双库门禁。

状态：已实现、验收并由 `dab80d4` 限定提交；真实 HTTP、生成项目及适用 workspace 门禁已记录。参数化/通配符 fallback scope 的 Axum 限制保留为明确缺口，不宣称完全 Laravel 等价。

上游依据：[Laravel 13 Routing — Fallback Routes](https://laravel.com/framework/docs/13.x/routing#fallback-routes)。

## 当前 R3-F Spec：Laravel domain/subdomain routing

状态：实现、独立复审、整组门禁、生成应用、根真实 TCP 与资源收尾均已完成，本簇限定提交见 Git（领域 contract 见 历史决策 077）。依据 Laravel 13.39.0 固定源码与 Routing 文档的 `Route::domain(...)->group(...)` 和 domain-first 匹配；仅使用有效请求 authority，不信任代理转发 Host。后继领取 C04 Request/Response，不将本簇完成外推为全部 routing 或 R3 完成。

1. 在 `RouteGroup` 增加 `domain(pattern)`，嵌套组继承域约束；支持精确域和完整标签占位符（如 `{account}.example.com`），拒绝空标签、部分标签占位、端口、路径、非法/重复参数以及同一规范化 domain pattern 的路由冲突，错误在 router build 稳定返回。
   子组未声明 domain 时继承父组；显式声明新 domain 时覆盖父约束，不拼接两个域模板。
2. 对每个请求严格解析唯一有效的 `Host`/URI authority，拒绝多值、逗号、非法端口、畸形 authority 与互相冲突的 authority；大小写规范化、去端口后按当前 HTTP method 优先查找匹配域路由，找不到才进入不限定域的普通路由。域路由 host 命中但 path/method 未命中时必须继续检查后继域和普通路由；仅当当前 method 均无匹配、但某路由 path 对其他 method 有效时才产生 405。业务 handler 主动返回 404 不能触发二次路由。
3. 提供公开 `DomainParams<T>` extractor，使用 serde 将已验证 host pattern 捕获的完整标签参数反序列化为类型化值；未命中域路由或格式错误时稳定失败。域参数与 Axum `Path<T>` 分离，避免更改路径参数语义。
4. 命名 domain route 的 `UrlGenerator` 使用登记的域模板，以同一参数表同时替换 domain/path 参数；域名只接受单个 ASCII DNS 标签（含预先编码的 punycode），不做 Unicode IDNA 转换；缺参、非法标签和重复名称 fail closed。非 domain 路由仍以 `app.url` 的 host/scheme 为准；绝不以当前请求 Host 或未验证 `X-Forwarded-Host` 构造绝对 URL。
5. 明确 `Host` 只参与用户显式配置的路由分派，不自动改变 `app.url`、租户身份或可信代理判定。若需 `Forwarded`/`X-Forwarded-Host`，必须新增受可信 peer 约束的独立契约，不在此簇默认采信。
6. 黑盒测试覆盖 domain-first 与注册顺序无关、精确/占位子域、大小写/端口、重叠 pattern 声明顺序、域 host 命中 path 缺失后落普通路由、域 GET/根 POST 同路径、方法回退/405、域 fallback 不抢普通路由、handler 自身 404、非法/缺失/重复 Host 与参数、Host 注入不能影响 URL 生成、named URL 域与路径参数编码；请求 body/header/query 在分派中完整保留。生成项目与 `modules/example` 展示多租户域路由，但 host 参数不得替代租户认证授权。
7. 用户 fallback 在全部普通 domain/unrestricted route 候选之后才运行；域组 fallback 按 domain pattern 选择。业务 handler 404/405 必须是终端响应，不得回落。GET/HEAD 静态资源检查保持当前 public 路径策略，并测试域路由交互。
8. 领域 ADR 已建立并固化 dispatch 与安全边界；实现后更新 HTTP Rustdoc/README、示例、core/http、`todo/plan/progress/context`、CHANGELOG 和 testing；执行 §6.3 适用门禁、生成项目与真实 TCP HTTP 验收。数据库不变时不虚构双库要求。

实现决策与差异：域 pattern 支持 DNS ASCII label 和每标签一个 `{parameter}`，兼容 ASCII punycode，不做 Unicode IDNA 转换，不支持 IP literal 或末尾点；捕获值须为单个合法 label。域与路径参数同名启动报错。不同 pattern 同时匹配时按域注册顺序；同一规范域内 Axum 重复路由稳定失败，记录为与 Laravel 后注册覆盖行为的显式差异。方法/路径 miss 必须保持请求 body 不被候选路由消费，并接着检查普通路由；不得靠业务 404 状态猜测未匹配。

上游依据：[Laravel 13 Routing — Subdomain Routing](https://laravel.com/framework/docs/13.x/routing#subdomain-routing)；固定源码匹配规则确认 domain routes 在 unrestricted routes 之前处理，与注册先后无关。

## 当前 C04 Spec：Request/Response 契约校准

- 标量 JSON 后继（源自 SC-N01）：`59ddcd72` 已将 Request、Session 数值字符串、InputNumber 与 Model 接入共享显式指数规则，PHP 8.5.12 差分、公开入口、示例、生成 Web/JWT 和日常适用门禁均已验证。原始 JSON 数字字面量在入站解析后仍可能丢失，须按独立 parser 契约处理。已交付范围与证据见 `tmp/local-closeout-20261107/sc-n01-after-sqlite-snapshot/COMMIT-RESULT.json`。

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

C04-A/B1/B2的公开检查、输入基础与类型便捷方法已按各自范围验收，证据见[testing](memory/testing.md#c04-b2-类型便捷方法)，不重复实现。完整C04仍开放；本簇按历史决策 081实现Cookie读取/队列/加密与响应生命周期闭环，值工厂、队列与核心接线分非重叠文件并行；C04-B2已限定提交`0a92900`，不重复实现。

1. 本簇闭环限定Cookie读取、创建、请求队列与加密往返；不扩为完整Session/flash/CSRF/FormRequest重写。固定依据为Laravel `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181` 的InteractsWithInput、CookieJar、EncryptCookies及AddQueuedCookiesToResponse。
2. 不可变factory默认可共享，可变queue必须请求隔离；同名同path替换、不同path并存，返回阶段先附加queued再加密，取消/超时drop释放，不能留到下一请求。
3. RequestInfo与Request使用同一cookie视图，缺失/解密后null走default，空串仍存在；cookie独立于普通输入trim/null。真实wire解析、数组与名称绑定先核固定源，不直接套输入bracket parser。
4. 复用既有加密/key ring及Session middleware，入站旧key可读、出站当前key、篡改失败视null、except可公开配置；框架原生密文格式与PHP格式的必要适配在实现前记录，不能另造加密内核或静默跳过保护。
5. 明确外层cookie处理与现Session/auth读header的顺序及原始wire逃生口；响应组合沿原Response/Json/envelope入口，Cookie属性和错误/资源边界与固定源对应。
6. 验收覆盖并发请求队列隔离、相同name不同path、直接/queued输出、forget、重命名篡改、旧key轮换与超限；唯一模块、plain/auth生成项目、真实TCP及现Session/认证双库完整回归随簇交付。

本簇验收：最终workspace默认668/全特性1241 passed，根双库各48、生成plain3/认证双库各2、真实TLS与适用门禁通过；产物已清理，本簇限定提交见Git。完整Session/flash/CSRF/FormRequest不由此关闭，见[证据](memory/testing.md)。

## C04-D2：Session Store基础公开API（限定子范围已验收）

C04-D1的统一Session/flash与请求生命周期已完成适用验收并由d47162d限定提交，真实证据见[testing](memory/testing.md)。这只关闭该子范围，完整L025与Web栈保持开放。

1. D2契约已实现并通过适用验收：remember/push/increment/decrement/remove、真实Cookie出口set_name与同后端handler；沿唯一Session状态/属性/数值工具。set_id/set_handler及请求注入的安全生命周期仍为具体后继缺口，不用无保护setter冒充对应。
2. 数值运算须实际PHP差分，错误/null/不存在/溢出等行为按固定上游与必要Rust约束分类；不能以编译或改名替代等价行为。
3. 新API必须在modules/example真实调用/公开扩展示范，更新根装配、生成项目、文档与稳定矩阵，执行适用workspace、双库、生成及真实HTTP门禁后限定提交。
4. 后继Old Input/redirect错误bag、CSRF、Session blocking/GC/previous URL、其余driver、FormRequest、文件与完整Response继续保留，不用基础flash关闭全能力。分页沿既有C04/C05→R4/R5依赖。

D2验收：修后12项门禁、根真实PG/MySQL完整模块HTTP各54、三类新生成应用4/2/2及独立审查通过；根target有效产物已清理、五旧consumer不变，限定提交见Git。PHP44条差分路径不是完整Laravel启动；不关闭整个L025。

## C04-D3：Session扩展生命周期（限定子范围已验收）

Spec已按历史决策 082 D3收敛，进入实现：
- 补齐适用的ID/handler替换与请求注入能力，不复制Session内核，不绕过撤销安全增强。
- 对照固定Laravel Store、Manager与CookieSessionHandler及测试，先核实实际生产调用链和公开扩展入口。
- 明确start前/后、busy/closed/revoked、替换失败与取消，以及旧驱动资源引用的所有权语义。
- root收敛ADR与Rust公开API，互不重叠实现/测试交给现有代理，manifest与构建仍由单一集成人负责。
- 验收贯通外部driver、唯一模块、生成应用、真实双库HTTP和匹配workspace门禁；行为差异公开保留。

先审计固定Store的setId/setHandler与CookieSessionHandler请求注入生产路径，对照现有撤销、busy/closed、Provider装配与自定义驱动契约，写Spec后实现安全等价API。不得用D2只读handler逃生口替代驱动替换，也不得删除适用上游能力。统一状态机、唯一modules/example与生成调用、公开consumer、双库真实HTTP及匹配门禁随簇交付；Old Input/CSRF等按既有后继依赖保留。

D3验收（be98e3f）：最终12项门禁通过，默认668/全特性1328 passed（29/102 ignored不算实服务）；根真实双库完整target各55、三类新生成4/2/2及独立审查通过。root target已清、五旧consumer不变，限定提交以Git为准；本簇不实现完整内置Cookie driver或通用Container extend，不关闭整个L025。

## C04-D4：内置Cookie Session driver（限定子范围已验收）

Spec已按历史决策 082 D4实现并完成适用验收；正常格式对应、PHP异常弱类型差异与客户端保证分别记录：
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

D7真实Route/Resolver与受管理方法声明已通过限定验收：final3完整12门禁、默认695/29 ignored、全特性1403/103 ignored、根PG18/MySQL8.4各64、fresh5/2/2、真实TCP7及不同模型复审；忽略项不算真实服务。根target清零，五旧consumer不变，限定提交以Git为准；完整Routing/Request与previous/Precognition仍开放。公开Route/参数/resolver、受管理方法与fallback/OPTIONS按历史决策 083交付；[最终证据](memory/testing.md#c04-d7-真实路由上下文限定子范围已验收)保留失败修复、用户重验判断、真实服务及清理边界。

## 已交付任务的资源退役

- Q源提交850e1a4已在main；已完成owner正式归档并自然释放其cwd占用，正确workspace归档、worktree remove与branch -d均正常成功。证据 `/tmp/openrust-r2q-worktree-retirement.json`（SHA256 `18192f5ba415e0c633c2dfc98203155fc2a172adc1d3539121ab46b7f423b13c`）。X、D9与守护任务当时亦已退役；“仅main”是该次退役时刻的历史样本，不代表现有登记。当前worktree按Git现场及本批交接核对，不重复start或删除守护账本。
- D9已以b056aa0集成；原执行者/workspace正式归档后自动移除worktree，tasks/c04-d9-old-input已正常branch -d。证据 `/tmp/openrust-c04d9-worktree-retirement.json`（SHA256 `2d33c1b17324d1ba1168e8670bd8684e8aa80dd9a479024c3dae55fbe92899eb`），main HEAD/WIP前后一致；不重复清理已退役路径。

### Paseo 项目归属核验

- 误project `prj_204354f586a66a10` 已通过正式 `project delete` 清理，exit 0，仅返回旧workspace `wks_e760b2e9b88fbc4c`；旧workspace退出active列表，未宣称持久记录硬删除。
- 正确OpenRust项目 `prj_34a40ee7b674b2b6` 的main `wks_0f0870e8d55cabf4` 保留；Q/D9交付后分别经正式归档安全退役任务workspace及本地worktree/分支。下方误项目清理快照仅证明当时保全，当前任务退役状态以上方新证据为准，不能把旧快照当现存workspace。
- 保全证据 `/tmp/openrust-r2q-paseo-clean-verification.json`（SHA256 `f0cc2903bff30c60b3d66fe5129247bd20b4c8640ccd17fe4bd43e29bb3d077f`），含前后快照与正式返回摘要；后续任务仍须在正确项目下登记独立workspace，不把标题前缀称为真实嵌套分组。

### 当前构建缓存与回收

- D9 当时采用十进制 50 GB / 100 GB 和磁盘 10% / 5%，后续按现行规范；D9验收结束样本49.344GB、空闲约17.04%，12个去重target。下次构建前重测并按实际输入估增长，不把100GB当剩余可用量。
- main target保留复用；旧QX五生成包、本轮plain/PG生成包及四个本簇已完成测试可执行文件已精确退役，源码/锁/索引/当前二进制及共享库哨兵未变。日志明确Cargo关联test_app/auth_flows产物；不清整target，不碰归属不足的历史缓存。
- D9 My生成项目已验收，源码/锁/证据保留；如后继需要空间，先核精确package dry-run与占用后再回收其专属再生产物，不为纯Agent文档变化重跑矩阵。

## C04-D10-H 最终声明集合与真实分派同源化（已验收，限定提交见Git）

前置D10-G已提交 `c1f6a03`。root负责contract/共享文档/集成与提交；核心owner `d10c_core_continue`独占声明/分派/动作投影及集中验收；调用方owner `d10d_example_templates`的HTTP/模块/模板改动已停写。v7最终输入已通过双严格Clippy、核心all/no-default各7+5、HTTP7（含真实TCP6请求）、完整router124/120、模块5与CLI快照1；主矩阵11项均通过（默认838/34ignored、全特性1625/108ignored、双docs、4features、deny、两MSRV）。新server/CLI、fresh普通7/MySQL认证2+公开consumer1、应用真实TCP11/16及3专属package精确清理已通过，最终索引与主控复核见testing，限定提交见Git。独审/早期失败/用户§5续验决定及精确复用映射均见testing与 `/tmp/openrust-c04d10h-verification-v4`，不改写旧失败。

固定Laravel原始集合的逐method `domain+uri` view与完整methods序列view不同；G交付时的结构化(domain,path)分派曾保留PHP碰撞后应隐藏的handler；H现已统一真实声明来源，分别派生有序逐method分派与完整动作view，不建第二URL-only内核、不强求两view相等；同步真实Route元数据、Allow/OPTIONS、fallback与业务404/405终止边界。匿名无action路由亦须参与，不能仅过滤URL索引。既有PHP7+2实际探针只证明Collection，matcher/Host/path行为须真实验收。

H新增实际取证：`/tmp/openrust-c04d10h-matcher/probe.output.jsonl`35案真实RouteCollection::match/Request/Route bind及OPTIONS响应通过，统一固定Symfony8.0参考组件，无伪Request；这是匹配与合成OPTIONS证据，不是完整Laravel业务HTTP。确认仅POST存活且无GETfallback时GET/HEAD也应405 Allow:POST，H已纠正原GET/HEAD豁免404；有GETfallback仍先处理。不同模型契约/取证早审已收，root已冻结历史决策 083 H契约并启动实现，Rust定向/主门禁、生成/真实服务与资源收尾均已按限定范围通过。

窄只读证据 `/tmp/openrust-c04d10g-next-collection-dispatch-evidence.md`（SHA `f4bc6cdce33ddd93d2966ca9257971300e7137628be5a513707356dafdc3b0c3`）；Spec明确Rust约束/公开API影响/受影响调用方后实施，唯一模块、生成应用、适用门禁/MySQL与真实TCP随簇交付。不以本项关闭完整Routing。signed候选窄证据 `/tmp/openrust-c04d10g-next-signed-evidence.md` 已指出key resolver、relative渲染、raw query前置；补充只读密钥审计 `/tmp/openrust-c04d10h-next-signed-key-contract-audit.md` 核实URL key使用配置原始字节，以及直接hmac0.13与sha2 0.10的digest版本不兼容，后继Spec须收敛依赖/门控，不以现JWT/AES解析器套用。另 `/tmp/openrust-signed-config-live-dependency-audit.md` 实核启动后ConfigRepository冻结，当前无公开运行期set或请求局部覆盖；每次读取不能凭空实现key轮换。后继须先收敛Laravel核心Config::set/配置可见性与启动资源快照边界，不用历史决策 080未来DB动态设置冒充此核心缺口，也不偷偷解冻。均未实现/运行，后继另收敛，不夹入本簇；两者无硬技术依赖，先修已知分派缺口是明确排序，不假称签名算法依赖它。

## T01 Session纯行为测试入口整合（限定试点已验收，提交见Git）

- 前置H已提交 `3065be5` 且工作区干净；本簇只减少重复测试二进制，不改框架生产API、用例函数体、断言、fixture、feature或依赖。
- 采用已审计试点：将 `tests/http/session_extensions.rs` 作为 `session.rs` 的独立私有模块，移除Cargo对应独立test入口；预期相关26+15例仍全部执行，Session family入口8→7、源文件10个不变，须实证而非静态计数冒充通过。
- DB/Redis真实后端和环境子进程入口保持独立；禁止用整套单线程掩盖共享状态，不改历史精确filter证据。
- 执行owner `/root/session_entry_pilot` 唯一写Cargo/test装配和使用main target；root独占共享文档、索引和提交。无分支/worktree，无其他Cargo/服务owner。
- 先按同工具链/feature/target/中性env记录两入口基线no-run与实际名单/执行，再实现并记录单入口；区分首次失效编译与暖缓存，不清根target，不宣称未测加速或冷缓存可比。
- 两种feature组合的新单入口41项均已实跑通过，首次/暖缓存名单与原26+15精确映射，extensions仅增加模块前缀；集中workspace门禁均已通过，限定提交见Git。
- 集中验收覆盖全部41个用例的前后名字映射、session-only/all-features及适用workspace门禁；生产/模板/后端未变的H证据须明确输入影响后才能复用，不冒称新服务测试。
- 审计依据 `/tmp/openrust-test-entry-pilot-audit.md`；真实计时、删并理由、保留隔离、门禁及资源写入本节/测试记录后限定提交，本试点已验收，完整范围及计时限制见testing。
- 本地免费执行器只读目录查询失败：默认49374端口被占，显式连接亦无兼容V2 health（502）；未改端口/服务/系统配置，按当前可用能力回退 `gpt-6-sol` medium执行此局部已定任务。当时采用 50 GB / 100 GB 和磁盘 10% / 5%，共享 main 缓存保留。

## R2-R01 核心配置运行期原始写入（限定验收完成，本地提交见Git）

- 契约为历史决策 086 R2-R01；历史决策 080区分可信内存set与部署/管理员Settings边界，不启动动态平台或signed。前置H `3065be5`、T01 `b7afcfe`，本簇main冻结HEAD `f8931f5`；期间SMS限定文档提交不属于本簇源码。
- 同一仓储有序set/set_many、raw/Figment成对原子发布、Register/Boot/Frozen写入、启动schema屏障及App/Provider共享句柄已实现；App.section live，typed配置/既解析资源保持启动快照。默认/批量/稀疏混合/失败与并发回归及唯一模块、consumer、模板同步交付，无新依赖/manifest/lock变化。
- core owner `/root/config_runtime_core`（gpt-6-sol/high）四文件实现与首批39+1+39通过；caller owner `/root/config_runtime_callers`（gpt-6-sol/medium）完成模块/consumer/模板及stage2门禁，现仅后继只读Spec；services owner `/root/config_runtime_services`（gpt-6-sol/medium）完成stage3，现独占本簇精确package产物回收。root协调contract/共享文档/索引/限定提交，terra独审全部高风险改动及驱动修复。免费本地接口不可用事实沿T01记录，不重复派发或建新target。
- stage2最终14命令通过：fmt、模块exact1、CLI exact1、consumer4、双严格Clippy、双workspace845/1632（0失败、34/108 ignored）、双严格docs、no-default、MSRV1.97两组及deny。root已复核15源码SHA与14日志SHA/退出码；App布尔断言及caller两处函数长度lint均修复并保留原失败，不放宽断言。
- stage3通过：fresh plain7、公开fixture exact1、MySQL8.4.13 auth2，无skip/ignored；root双进程GET默认→POST固定业务键→GET更新，第二进程恢复默认；generated plain同样真实TCP。三进程SIGINT/join/exit0、两端口释放，MySQL同容器端点前后版本/56个TestDatabase型库集合一致，不清旧库、不将旧库归本任务。
- 证据：`/tmp/openrust-config-runtime-core/verify-stage1/result.json`、`/tmp/openrust-config-runtime-verification/stage2/STAGE2-RESULT.json`、`/tmp/openrust-config-runtime-services/STAGE3-RESULT.json`（SHA `38fbf8b6c457f8c72b5fa1829d59514f15c9bf765cb44b10dc0f5adf20109019`）。root另核565输入、13命令及2TCP日志SHA、PID不存在与端口空闲，回执 `ROOT-VERIFY.json`；PHP固定探针exit0且45097字节输出与golden逐字一致。
- 资源：15个已登记target约37.4008→38.2540GB，磁盘余10.341%，未触50GB软/100GB硬及磁盘10%/5%；现仅回收生成plain/mysql/mysql-migration精确path packageID产物，禁止全target清理。两处历史约10.6GB缓存仍缺完整退役归属，未删除；生成源码<1MB可保留作为本簇复跑证据。不得将已登记小计称为无遗漏项目总量。
- 收尾：三个生成包产物按实际归属预检/dry-run后回收526163968B，Cargo会忽略path限定，故不能仅凭pkgid声称隔离；root复核49个删除路径、2109个保护项SHA，源码及主server/CLI/框架/consumer未变，`CLEANUP.json`及`ROOT-CLEANUP-VERIFY.json`留证。未全清target，两处未知历史缓存未碰；生成源码<1MB由root保留供本簇最终复核，后继交接再次评估。全部适用门禁通过后限定本地提交；完整Config、来源初始键序、PHP对象/Closure/资源表示与signed仍开放，常规MySQL结果不关闭发布全后端矩阵。未推送/发布/tag。

## R2-R02 配置来源初始顺序（限定验收通过）

- 前置`2de050f`，契约历史决策 086。不可变ConfigSources、loader单次来源捕获、仓储/Provider/TestApp入口、有序TOML及现生命周期接线已实现；root负责共享文档/清单/索引/提交，callers负责配置核心，services负责调用方及真实验收，均使用既有main工作区，不重派。
- Rust门禁已通过：首轮142项；最终默认workspace853通过/34忽略，all-feature1642通过/108忽略；双严格Clippy/docs、features、MSRV1.97两组、deny；excluded公开consumer5项及严格Clippy、最终fmt通过。来源PHP操作探针、core/integration/fixture不同模型静态审查均有证据。详见[testing](memory/testing.md#r2-r02-config-sources)。
- 原Redis租约失败在单例不复现、原三项并发再次失败；具名探针确认两个测试旧namespace碰撞，但未证实生产lease根因。最终仅修fixture时间+PID+原子序号并补固定时钟回归，原业务断言/生产租约不改，原失败与临时探针记录保留。
- 最终源码绑定566项、门禁19项，root另核25项范围输入；stage2 attempt4 RESULT SHA `c4166e03738c4e506f25c7df9617c371613c6f76551373cb21f051b45e7a267b`。服务驱动v4独审与自有子进程夹具通过，PLAN已ready；只按root逐阶段核回执启动，静态审不代替运行。
- 普通fresh生成原7项+fixture1项通过，真实TCP已验证生产reader.toml的z_label/a_label声明顺序与GET→POST→GET、exit0及43188释放。回执在`/tmp/openrust-config-source-order-services/`；plain-result SHA `1cfc318371e633428248efe00a1807024f8e7f834e22151005bde4492e0f6c06`，plain-tcp-result SHA `a29731630a2bf6e29e4d5cb486b553daf6f77d0f06d61c24cb3a2468bcc93f38`。
- 测试专属18文件已回收155316224B；postflight空列表误调用lsof的读取器失败保留，root及独立后验核96保护/42生成/566源码一致。plain包剩余产物已退役；MySQL8.4.13 fresh生成fmt/check及2auth HTTP实际通过，测试库集合前后56项一致，root核4命令/44生成输入与保存的脱敏日志SHA，回执mysql-result SHA `0ec3e72db5815ca11a7a777a16d8ce3834221ce05cad170094927a314af7bd9d`。mysql/migration专属包已退役，shared保护/源码不变；root TCP两实例已通过（初始→写后读及第二进程初始值、exit0/端口释放）。最终STAGE3 SHA `887e49d2c4610ce5adec6204ca1debf31e707a18b0424f82cb91ee8af01f08cc`，root核17日志/8清理回执/566源码/42+44生成输入、3PID退出及两端口释放。限定本地提交见Git，不关闭完整Config。
- 该阶段采用 50 GB / 100 GB 和磁盘 10% / 5%，后续按现行规范。保留活跃main缓存，不删无完整归属的root-site/m5-final历史target。一次性生成包须有同名全范围归属/dry-run/保护项证据；不凭Cargo忽略的path/version限定冒称隔离。
- 完整Config继续开放：平层TOML顺序不等于Laravel递归配置发现或文件名/目录namespace映射，PHP对象/Closure/资源等表示差异亦未关闭；不重建客户端、不以此替代历史决策 080动态Settings或完整signed。

## R2-ENV 端口与缓存环境命名统一（已提交2187b6b）

- 最新contract覆盖旧端口决定：唯一中性环境入口为`SERVER_PORT`，内部保留`server.port`/`config.server.port`；删除`APP_PORT`及独立`PORT`监听兼容，通用或命名空间fallback也不得暗中映射旧变量。`.env.example`使用`SERVER_PORT=3000`与`APP_URL="http://localhost:${SERVER_PORT}"`；自定义APP命名空间不得改变中性SERVER_PORT领域键。
- 用户最新纠正以Laravel官方机制/命名为准：`CACHE_STORE`→`cache.default`，撤回先前`CACHE_DEFAULT_STORE`→`cache.default_store`自定义方案；内部配置字段/序列化/读取同样收敛到default，不只对外改名。删除REDIS_DEFAULT_STORE且不新增CACHE_DEFAULT_STORE兼容，旧/撤回键不得借fallback改变选择。该contract已落实本簇代码，R02已提交；当前ENV冻结输入待集中验收。
- REDIS_HOST/PORT/PASSWORD/DB只承载连接配置，共用REDIS_DB，不新增REDIS_CACHE_DB。框架默认具名store仍为redis、driver=redis；合法业务自定义store名（包括default）与其他driver继续可用，cache.default须准确引用已登记store；不得机械替换所有default或扩展到DB_*等其他既定键。
- 基线R02 `a875dd7`/契约 `af0f62b`；core和services完成全来源/CLI/模板/consumer/唯一模块及fixture迁移，root限定集成文档/ADR/独立锁文件。两项CLI来源独审缺陷已修，真实源码/模板而非旧命名套壳；当前规则以本段和历史决策 016顶部为准。
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
3. **阶段二A：官方基础包与体验**：共享前置成熟后，按包实现Fortify/Sanctum/Socialite/Passport、Folio/Head/Precognition、Octane对应及额外数据库driver；先Spec默认/安全/扩展与实际生产链路，模块生态集成、官方starter-kit及JS配套同步对应，不新增第二示例。 Fortify/TOTP已于2026-11-04获真人明确实施授权（owner=root；既有Guard/Provider/Session前置已按R7限定交付，当前Fortify单体系按本页唯一停点收口，不能把本簇并入历史S3b或当作全部Auth完成）：仅复用同一Fortify状态和公开扩展、删除旧auth-mfa兼容，先固定兼容Laravel13.x的正式release/tag/commit、依赖与官方测试，RFC6238和Google官方Key-Uri-Format只作互操作来源。必须支持Google Authenticator本地otpauth二维码/手动密钥、issuer/account转义、SHA1六位30秒/前导零/窗口；真实Google客户端未实际导入验证须保留外部验收缺口。对齐近期密码确认、待确认绑定、第二因素前无正式Session/token、所有入口防绕过和Guard/账号Provider身份隔离；官方Disable清secret/recovery/confirmed及8份加密可重看恢复码须提供官方默认，现哈希一次展示/关闭保留绑定等仅作显式增强，不降安全。跨实例并发重放/一次消费、期限限速/failclosed、关闭换绑重置挑战/会话及脱敏审计，MySQL/PG/SQLite最低版、generated-project、唯一modules/example、文档及不同模型独审均为交付条件。2026-11-04真人进一步明确一切以前述固定Laravel/Fortify官方机制为前提；跨项目TTL/独立开关/仅哈希恢复码/关闭保留绑定/额外撤销均不得覆盖官方默认，增强必须经同一公开扩展可选提供且不增加强制交互。当前固定Fortify v1.41.0 / c456642584c102f6a6aafee9932299542da637b5及21份同commit官方测试，兼容google2fa v9.1.0独立参考已固定；[认证Spec](plans/fortify-authentication.md)区分包回退/安装模板默认。用户补充已纳入同一认证契约：用户ID必有且支持整数/UUID等；用户名一般存在但不全局强制，email/phone均允许缺失。按应用配置支持用户名/邮箱/手机号或组合密码入口，常规界面单一“用户名／邮箱／手机号”账号框加密码框、只填任一种，共用同一Provider/密码/限流/MFA，配套唯一示例/生成/验证；明确规范化、大小写与跨字段歧义拒绝，不冒充官方默认。R7前置已由5ad34b9按限定范围验收提交，当前官方Fortify/TOTP已完成源码和应用接线，限定验收与未闭合项见本页同名停点，不由历史增强定义官方机制；SQLite生产链、CLI选项/生成、Laravel导航文档及S3缺口持续保留。
4. **阶段二B：数据、异步、观测与工具**：按DB/Model/Queue/HTTP前置对应Scout/Pennant、Cashier Stripe/Paddle、Reverb/Echo、Horizon/Pulse/Telescope及Dusk/Envoy/Prompts/Pail/Tinker/Pint/Sail/Valet/Homestead等适用体验，Wayfinder/Stream和编辑器/分析元数据依赖随相应簇完成。Horizon/观测工具自身必要UI与鉴权随包交付，按需引入不默认公开；这不授权组织/菜单等业务后台或App。
5. **阶段二C：AI与已授权增强**：AI-0至AI-4、官方MCP Server/Client、Boost按真实前置组合，SDK不能改成另一套机制；MQTT、gRPC、DB动态设置及既定增强继续。AI-5内嵌推理后置评测，不因全量SDK强制模型运行环境。A/B/C是职责批次，可按真实依赖交错，不把列表顺序变成循环前置，独立簇可由总控按依赖图批准并行，main集成与最终验收串行。
6. **最终收口**：每个官方包逐项行为、公开扩展、唯一模块、生成项目与真实服务/浏览器/协议证据均齐备，Rust差异/外部条件缺口公开；两阶段/C33证据汇入R14，不能只关闭核心便称完全体。若缺账号/许可/服务继续独立项，禁止fake冒充实测；不推送/发布、不改写历史，业务后台与AI产品仍未授权。
