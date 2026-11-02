# 项目上下文

> 当前代码与目标：旧 `app/example/` 过渡结构已迁至 `modules/example/`；R2-M 模块闭环与 R1-E 当前综合示例均已有真实验收及提交。R2-N 模块硬依赖图和确定性启动顺序已由 48be854 提交；R3-A/B/C/D 已按范围验收并提交；R3-E fallback 已验收并由 dab80d4 提交。完整模块管理和 Laravel 1:1 范围仍按 [ADR-001](decisions/adr-001-framework-principles.md) 与活跃队列推进。

> 受众：AI Agent
> 更新：2026-11-02
> 摘要：正式目标为 Laravel + laravel-modules 的 Rust 1:1 实现。R1-E、R2-N 与 R3-A/B/C/D/E/F 已按适用范围交付并保留验收证据。R3 HTTP/Web 和完整 Laravel/laravel-modules 能力矩阵仍未完成。

## 当前状态

SQLite连接基础已完成[限定验收](memory/testing.md#sqlite-连接基础限定验收)：主10门禁/deny、SQLite-only严格Clippy/文档、fresh11、真实MySQL3与根进程HTTP通过，固定输入补丁影响经独审映射。实际引擎3.51.3；最低3.26、复杂PRAGMA/nullable配置及完整取消清理仍未验。迁移/CLI安装/现有业务SQL继续按唯一todo推进，本地提交以Git为准；不称全量SQLite完成。

显式模块注册与宿主API前缀已完成[限定验收](memory/testing.md#显式模块注册与宿主-api-前缀限定验收)：10项主门禁、6个真实server进程、fresh minimal9/MFA真实MySQL3、生成模块持久启停1及只读空目录负例通过；首批8页mdBook构建/链接、112生成源码归档、不同模型独审和target清零完成，本地提交以Git为准。registered为应用约定，库file默认保留；完整Modules/Routing与全量文档未关闭，下一SQLite及CLI后继按唯一todo。

默认production＋log通信启动与安全诊断已完成[限定验收](memory/testing.md#默认生产日志驱动启动与安全诊断限定验收)：原始cargo run三场景、10项主门禁、通信consumer、fresh minimal9/MFA真实MySQL3与实际help通过，113生成源码及失败证据保全，最终target正常清零。完整MailManager与ADR-001未闭合，本地提交以Git为准；随后按唯一todo依次完成模块默认注册/API前缀及首批正式文档，再立即SQLite，CLI后继保留。

2026-11-02 接管校准：当前对话为唯一框架集成总控，旧总控/旧守护保持停止。九份来源及未提交/未追踪代码已归档保全，来源 worktree 与 workspace 已正常退役，当前仅 main；六个已归档任务分支活动别名已安全退役，source/WIP保护引用与完整bundle保留。本批限定源码、生成与真实服务验收已收齐，本地集成提交 `9035a00`，清理后优先配置整改，详见唯一 todo 顶部的接管状态与 `tmp/takeover-20261102/`。另一个已获用户授权的 skill owner 正收口 Codex 原生守护适配，本总控不并改该范围，不把开发中适配当已启用。默认80%政策与旧账本保持；本对话已可通过正式额度工具读取共享账号窗口，数值以当次回执为准；不rebase/fullgrant，不改变默认80%及旧账本。

配置d952199与应用职责161bfcf已限定提交。模块持久启停、独立状态路径、同容器管理/事件已完成限定验收及产物清理，提交事实见Git；默认1081/35 ignored、全特性1923/112，fresh普通12/MySQL3/MFA2、根真实MySQL1与三进程启停/缓存通过，见[模块管理证据](memory/testing.md#模块持久启停与同容器管理限定验收)。同内核嵌套Provider注册前置已通过限定验收和资源收尾，本地提交39086cf，见[嵌套注册证据](memory/testing.md#同内核嵌套-provider-注册限定验收)；module.json运行发现与Provider/files编译绑定已通过[限定验收](memory/testing.md#模块运行发现与编译文件限定验收)及资源收尾，本地提交d32162d；B003/B004默认实例缓存、失败内存语义与PHP顶层列表形状已通过[限定验收](memory/testing.md#fileactivator-实例缓存限定验收)并完成资源收尾，本地提交以Git为准；完整Modules/ProviderRepository/aliases、DB能力门控及ADR-001继续开放。DB通用能力/驱动/默认连接选择已完成[限定验收](memory/testing.md#db-通用能力与驱动分层限定验收)、独审与资源收尾，本地提交以Git为准；标准MySQL+Redis依赖与队列开箱已通过主门禁、根真实DB/Redis独立worker、四种fresh、外部SQLite及缓存重启的限定验收；不同模型独审、源码归档及资源收尾完成，本地提交以Git为准，详见[队列开箱证据](memory/testing.md#标准依赖与队列开箱限定验收)。默认连接错误时点与完整deferred Provider仍为R2/R10明确缺口。完整懒连接/默认名切换、五后端与ADR-001仍开放。

当前R2共享生命周期的命令所有权前置已完成[限定验收](memory/testing.md#共享命令所有权前置限定验收)：12项主门禁、真实MySQL模块维护1项、标准server/CLI续作构建、fresh精简9/MFA3及实际help通过；用户换号暂停/恢复与原始中断证据保留，原Goal active、原预算沿用。独审、113份生成源码归档及最终target清零完成，本地提交以Git为准；不关闭deferred/manifest/when或完整ProviderRepository。具体owner、配套和下一步以唯一todo顶部为准。 用户已明确后继顺序：当前簇提交后先完成本次模块Provider默认入口整改，再立即内置SQLite，不扩Modules全量或等待R5/Factory；默认MySQL＋Redis不变，custom consumer不等于内置支持。

用户已纠正应用边界：根app按Laravel支持常规Controller/Model/Provider业务开发，modules可选并存且共用同一内核；bootstrap承担启动，app/mod.rs仅必要模块导出。原“业务只能modules、根app只装配”限制失效，源码/生成器整改按唯一todo推进，历史验收不改写。

收到需求、审计或回执先判定补充/纠正/明确替代；未明确取消的原任务继续有效，重复内容合并、过期结论标明替代关系。每次处理消息后恢复原owner停点，记录范围、实际状态、下一动作与未闭合验证/提交/清理；切簇、压缩恢复和最终提交前核唯一todo，不用最新聊天或已派发代替完成证据。仅源码、匹配验证、本地提交与必要资源交接齐备才关闭对应范围；已编码待验、已提交未集成、外部缺口分别保留。全量审计归并本页/既有矩阵，不覆盖已确认后继、不新建平行队列。

本轮最新顺序：先收齐当前已在途 main 组合源码、调用方、模板、唯一示例和说明，冻结后一次集中适用验收，修复失败并通过后本地提交；不再追加新簇延长本批，也不以全计划最后测试无限推迟提交。随后处理今日尚未吸收的整改，再按 ADR-001 持续推进。未验证源码保持 WIP，旧证据不冒充本批通过；默认20%额度保留及50GB软/100GB硬、磁盘保护不变。

后续默认同一 OpenRust 项目、同一 main 物理工作目录，多个对话框或 workspace 标签页按不重叠文件分工；不再默认创建任务分支/worktree。产品不支持共享目录标签页时复用既有 main workspace 的多个对话框，不新建源码副本或修改内部数据库。同文件串行交接，公共 API 先收敛；manifest/lock、共享文档、Git索引/MERGE/提交、target/Cargo和真实服务由总控唯一调度。冻结受验文件禁止并发修改。

| 项 | 当前事实与目标 |
| --- | --- |
| 当前代码基线 | 规划开始 HEAD 为 `7f964b8`；根 openrust 同 package 的框架 lib 与默认 server 已装配；应用职责回根，当前示例位于 modules/example，按编译清单和运行激活状态接入；tools 与 consumers 保留；版本和依赖以 Cargo.toml 为准 |
| 当前阶段 | [全面重构计划](plans/laravel-rebuild.md) 已记录；R1-S、R2-A/A2/A3、R2-B统一Provider、R2-C入口scope、R2-D TestApp、R2-M模块闭环、R2-N模块依赖顺序与R1-E综合示例各子簇按范围完成；R1-E 提交 bc3fa01。R3-A/B/C/D/E 已提交，R3-E 为 dab80d4；R3-F domain routing 的独立复审、workspace/生成项目/根TCP验收与资源收尾完成，本簇限定提交见 Git。`storage_publication_uncertain` 后远端迟到完成、强杀与 shutdown deadline 仍不保证无孤儿；C04-A RequestInfo/内容协商与staging并发回收修复已通过适用门禁、生成应用、根TCP及不同模型独审；C04-B1统一请求输入与默认规范化已通过定向、最新workspace、根真实TCP、生成项目及双库适用验收；C04-B2类型便捷方法已通过当前workspace/feature/MSRV/docs/deny、真实双库、生成应用与TCP及独立复审；C04-C Cookie读取/队列/加密/响应子簇已通过最终workspace、根双库各48、生成应用与真实TLS及不同模型审查；C04-D1统一Session Store/flash与请求生命周期已通过修后适用门禁、真实双库/Redis/Keycloak/浏览器、生成项目和根TLS，限定提交以Git为准；C04-D2 Store基础公开API已通过修后12项门禁、真实双库HTTP各54、三类生成4/2/2及资源收尾，D2提交3cabae2；D3扩展生命周期已完成最终12项门禁、真实双库各55、三类生成4/2/2与资源收尾，限定提交见Git；D4内置Cookie driver限定子范围已验收：final3完整12门禁通过（默认668/全特性1367 passed，29/102 ignored不算实服务），根真实双库加密各62/无加密安全负例各1、新生成plain5/双库认证各2、真实TLS代理链与仓内Chromium HTTP脚本4项及不同模型审查通过；root target清零、五旧consumer不变。本簇提交47cd7b8，不关闭PHP异常弱类型、全局撤销或完整Session/Web能力，详细证据见testing；D5请求级GC lottery/公开driver回收与追加expiry索引迁移限定子范围已验收：final4完整12门禁通过（默认668/29 ignored、全特性1374/103 ignored），根真实双库各62、Store/GC各2、Redis Store1/Auth10、新生成plain5/双库认证各2、独立审查及资源收尾通过；忽略项不算实服务，不关闭完整Session；D6请求URL限定子范围已验收：12项门禁通过（默认686/29 ignored、全特性1393/103 ignored），根PG18/MySQL8.4各63、fresh普通5/双库认证各2、真实TCP/TLS终止代理10场景及不同模型独立审查通过；两线真实PHP8.5差分逐案通过，不关闭可信Host/Port/Prefix、真实路由匹配、previous或完整Session。完整Store/HTTP/Session、RedirectResponse、文件输入、CSRF/FormRequest/Response后继继续开放；D9仅关闭旧输入限定子范围，见 [todo.md](todo.md) |
| R2-CC 分支历史 | `7be446d` 的限定分支证据保留；已纳入四簇main限定组合交付，最终五父提交 `e105dbfc58afc330b50e53b75f0b9e1424fa387a`，主线结果以组合验收记录为准。 |
| R2-P / S-DV main限定组合 | Provider `4ac8099` 与 Session对象值 `6ffb5c8` 已由三父本地提交 `9e5a94198b421987851b3580b27fa90370feeb77` 合入main。默认/全特性workspace分别943/1748通过、34/108忽略；双严格Clippy/docs、MSRV、no-default、deny、公开consumer、fresh生成、真实MySQL与根TCP六场景按[最终记录](memory/testing.md#r2-p-s-dv-main-limited)通过。旧分支失败和用户授权的单次Clippy重验仍按原证据保留；S-DV与P均已安全退役，P两份root文档事实已迁入main并保全原稿，不关闭完整Provider/Container/Session。 |
| 独立实现 | S-DV来源 `6ffb5c8` 已与R2-P在main限定组合交付，原分支子范围证据仍保留；S-DV工作区/本地分支已安全退役。ST-V来源 `2262df9` 已由两父提交 `8d18021b630ea10652316e025ef6019c7772bbb0` 合入main并通过限定组合门禁；真实S3 ACL、迟到CopyObject和`throw=false`失败值语义仍开放，来源工作区与分支已按退役回执安全清理。见[本簇记录](memory/testing.md#st-v-main-限定组合验收)。 |
| 当前功能簇 | Env/Request-MIME、TrustedHosts、应用/代码配置/Provider、异常、Web/CSRF与部分Query已收齐main限定组合验收；提交以Git为准，来源保全与实际门禁见[本批证据](memory/testing.md#当前-main-组合限定验收)。下一批优先配置可编辑默认值、依赖单一来源及应用职责整改，唯一todo保持完整核心/Modules/官方生态范围。 |
| R2-T 分支历史 | `b1f4bf7` 的仓储、Collection、模块、consumer、生成与TCP限定证据保留；已纳入四簇main限定组合交付，完整L005.03仍开放。 |
| 目标授权 | [ADR-001](decisions/adr-001-framework-principles.md)：Laravel固定基线为1:1行为标准，laravel-modules为正式模块化范围，Goravel仅跨语言实现参考；允许全面重写，无历史兼容义务 |
| 当前完成边界 | 原认证/数据/队列/缓存/存储/实时/gRPC/视图等有专项证据；完整生产/集群/最低版本仍有缺口，新目标不能继承为已验收 |
| 协作 | 已定 contract 优先 gpt-6.1-sol 子智能体；主控负责设计/协调；默认同main物理目录多对话框按不重叠文件并行；main集成及共享manifest/lock/迁移/文档由总控串行协调，target/服务资源预先分配，审查并行只读；高风险改码不同模型或 Provider 审查；不推送 |

本轮新增配置整改已确认：当前组合验收提交后，先由单一配置/模板owner显式展开全部应用config/*.rs可编辑默认策略，并让缺失APP_URL取同次有效server.port（显式APP_URL优先）。APP_NAME/APP_URL由所属应用配置一次求值，下游缺省audience/issuer复用最终应用值并保留显式独立覆写；ConfigContext最终配置依赖读取尚待实现，不能以常量去重/重复求值替代。公开地址策略覆盖auth.jwt.issuer/security.signed_links.issuer默认，不取请求Host；框架Default回退与固定测试输入分类处理。不扩大在途冻结输入；具体范围与空值/缓存/生成验收在唯一todo首批整改项。

用户已明确MySQL+Redis为标准宿主/生成应用便捷路径，常用能力依赖/Provider/config/env预整合；默认DB队列、Redis队列配置可选，精简opt-out和其他承诺后端仍保留。不自动连接所有driver/启动worker，不静默降级；归当前组合提交后的同一配置/生成整改；源码已收齐，尚未验收/提交。

## 固定约束

本对话接管完整已授权框架计划，本批环境身份与Request/MIME等组合已经收齐限定验收，详见[本批证据](memory/testing.md#当前-main-组合限定验收)。旧总控Goal及守护保持停止；本对话独立Goal按默认80%政策执行，不跨Goal复用旧授权，不重算旧baseline或清账本。Codex原生守护适配保持独立WIP，未验收部署前不称已启用。

本机相关参考仓库共同父目录为 `/Users/Home/Codes/Git/`：`laravel-framework/` 是官方框架实现，`laravel-docs/` 是官方文档，`laravel/` 仅指官方应用骨架；固定验收 checkout 分别为 `laravel-framework-baseline/`、`laravel-docs-baseline/`。平级迁移及 linked worktree repair 已核完成，五个仓库工作区均干净；应用骨架本次 HEAD 为 `9ffef3bc4241e3cc333ef267258a404d0d95f194`，未安装 Composer 依赖。执行者不重复迁移或创建旧路径。本次拉取 framework 官方 `13.x` HEAD 为 `5a787ee008553a35206de2918d5242626ddeaed2`，docs HEAD 为 `960eedf0911155ad16326ade49c0ea5d18eda0b8`；固定验收 commit 仍为 framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`、docs `960eedf0911155ad16326ade49c0ea5d18eda0b8`，不随最新源码拉取静默升级。新取证/执行脚本读取固定 baseline，不能用应用骨架替代框架，也不再读取 `/tmp` 旧 Laravel 源码；冻结历史输出与 provenance 保留原路径事实。旧副本先核 owner/占用、无WIP后再安全回收，证据不当缓存删除。

- 开发优先完成源码/测试代码/示例/模板，功能簇提交前集中验证；不默认TDD或逐小改动重复跑门禁。分支定向门禁与main完整组合验收分层，未通过不提交。正常空间预算内缓存可跨簇复用，阈值/资源归属与临时worktree清理规则保持；详见[集中验证](conventions.md#功能簇提交前集中验证)。

- 当前框架保持同仓 main 集成；用户已持续授权总控在既定框架范围内直接串行整合本地任务，无需逐次询问合入许可；先组合源码再集中适用验证，通过后本地提交并安全清理，保留来源祖先或等价映射。后续默认共享main目录分工，不再默认创建worktree；既有worktree先保全交接再退役。现有 main WIP由原负责人保全，不强行迁移；分支验收后串行合入并重验组合行为，再清理任务专属资源，细则见[并行开发与集成](conventions.md#并行开发与集成)。不新建源码仓库、不推送。

- 正式定位为 Laravel + laravel-modules = OpenRust，以固定基线1:1实现功能与可观察语义，持续跟进其框架功能与行为演进；Laravel的目录、概念、默认值、错误/安全边界、Container/Provider register→boot与开发/CLI/生成/测试/部署流程是验收标准。逐项上游源码/测试→Rust API/配置→等价测试→modules/example→文档→真实服务证据，未等价/未实测保留缺口。版本基线、差异矩阵与必要 Rust 适配遵循[长期上游跟进](plans/laravel-rebuild.md#长期上游跟进)。
- 已有优秀机制保留并融合进 Laravel 对应职责；统一返回格式明确归 HTTP Response / API Resource 与异常渲染，提供便捷接入与公开定制，避免重复内核和协议破坏。规则见[融合计划](plans/laravel-rebuild.md#已有优秀机制的融合)。
- 用户明确要求从零设计并重写 Laravel 式框架核心，旧实现不构成设计或兼容约束；旧源码按功能簇替换、验证后清除，不批量抹除 WIP 和证据。
- 旧目录、API 名称、CLI 风格、默认值、交付方式和“已验证不可重写”等约束可被新 contract 替代。先校准领域 ADR 与调用方；当前实现事实不等于未来设计约束。
- 保留 Rust 类型、所有权、trait、async、Cargo 的便利性。采用熟悉的 Laravel 术语，语法用 Rust 惯用形式；不做 PHP 运行环境，也不引入不可测试的全局可变状态。
- 允许重构全部框架，但保护他人 WIP、进程、服务、数据和已存在证据；底层成熟组件按能力复用，不为全面重写重造密码算法/协议驱动。
- 对应新能力按 Spec → Implement → Verify → Commit 连续推进；独立且前置满足的功能簇可按总控依赖图并行，main集成串行。旧 M0–M6/DX/F/G 范围作为矩阵输入，顺序由新计划重排，不能丢失原未完成验收。
- 第一方说明使用简体中文，技术标识符/原许可保持原文，见 [style.md](style.md)。代码安全、依赖记录和测试/提交规则见 [conventions.md](conventions.md)。

- 除 modules/example 外均为框架通用结构与代码；示例专有迁移/种子/资源/协议/生成脚本/测试 fixture 均归模块内，根只保留通用加载与公开挂载，不因示例新增 build.rs/build 依赖。

- 根 openrust 同 package 提供框架库与默认 server；default-members=["."]、default-run="server"，根默认启动，modules/example 是唯一综合示例；生成应用与真实 HTTP 验证继续执行。

必要Rust差异、用户约定和授权增强按[差异分类](plans/laravel-rebuild.md#逐项对应与差异分类)公开；单仓main、根结构、modules/<模块>、模块专属资源、Vue/uni-app与Redis策略不被字面1:1覆盖。核心全量，模块包独立来源但同内核，生态/商业/外部授权边界保持。

## 服务与验收

- 日常真实数据库验证默认MySQL8.4 LTS、Redis8；PostgreSQL18服务保留可用，专属SQL/驱动/故障及方言/迁移/锁风险按影响当场定向，其他常规PG验证留发布全后端矩阵；先检查复用合适现有服务，测试数据隔离。不得停止/删除/重建他人容器、卷和配置。临时资源有明确 owner，只清理本任务资源。
- 发布前统一验收全部已实现/承诺后端及适用版本、迁移/事务/数据安全/队列/集群/生成项目，不用日常MySQL通过关闭PG/其他后端缺口。SQLite 单机完整支持仍是必交规划；额外后端按新能力矩阵分阶段。最低版本须在最终门禁确定并实测；历史 PG16 记录不充当当前日常基线。
- 读写分离须真实复制；双库须真实数据库；集群须真实负载均衡、多 API/worker/scheduler 与共享状态。已具备 Docker 服务、fake 或编译通过均不能替代实际验收。
- 真实邮件/SMS/供应商发送只用已授权凭据与目标；当前 Twilio 实发条件未具备。外部缺口保留为未验收，继续其他独立工作。
- 加密在框架进程内完成，新密文用当前密钥，历史密钥仅解密；不强制第三方 KMS/Vault。
- 单机为普通应用默认部署形态，集群同属必交；共享能力不得在依赖失败时静默降级到本机权威状态。具体默认 store/guard/目录在对应新 contract 定稿。

## 版本与外部边界

- 原V2独立版本计划已取消，需求转为[可选快速开发模块](plans/optional-rapid-development-modules.md#开发与使用边界)，用于快速组合应用，暂不开发。R14完成不自动开工；main继续通用框架与唯一示例，未来模块范围和交付组织由用户明确指定。
- [AI模型平台](plans/ai-model-platform.md) 仍为独立产品，本次不启动；Laravel 生态中的 AI 开发辅助不自动授权该产品。
- 发布、推送、tag、修改凭据/系统配置、项目外持久写操作不从重构授权推断。所有阶段仅创建限定范围的本地 commit。

当前重构来源与 102 章/749 子节当前纳入范围盘点见 [固定基线](plans/laravel-rebuild-baseline.md)；未将旧证据继承为新 contract 通过。
