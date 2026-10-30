# 项目上下文

> 当前代码与目标：旧 `app/example/` 过渡结构已迁至 `modules/example/`；R2-M 模块闭环与 R1-E 当前综合示例均已有真实验收及提交。R2-N 模块硬依赖图和确定性启动顺序已由 48be854 提交；R3-A/B/C/D 已按范围验收并提交；R3-E fallback 已验收并由 dab80d4 提交。完整模块管理和 Laravel 1:1 范围仍按 [ADR-074](decisions/adr-074-laravel-modules-directory.md) 与活跃队列推进。

> 受众：AI Agent
> 更新：2026-10-31
> 摘要：正式目标为 Laravel + laravel-modules 的 Rust 1:1 实现。R1-E、R2-N 与 R3-A/B/C/D/E/F 已按适用范围交付并保留验收证据。R3 HTTP/Web 和完整 Laravel/laravel-modules 能力矩阵仍未完成。

## 当前状态

| 项 | 当前事实与目标 |
| --- | --- |
| 当前代码基线 | 规划开始 HEAD 为 `7f964b8`；根 openrust 同 package 的框架 lib 与默认 server 已装配；应用职责回根，当前示例位于 modules/example，按编译清单和运行激活状态接入；tools 与 consumers 保留；版本和依赖以 Cargo.toml 为准 |
| 当前阶段 | [全面重构计划](plans/laravel-rebuild.md) 已记录；R1-S、R2-A/A2/A3、R2-B统一Provider、R2-C入口scope、R2-D TestApp、R2-M模块闭环、R2-N模块依赖顺序与R1-E综合示例各子簇按范围完成；R1-E 提交 bc3fa01。R3-A/B/C/D/E 已提交，R3-E 为 dab80d4；R3-F domain routing 的独立复审、workspace/生成项目/根TCP验收与资源收尾完成，本簇限定提交见 Git。`storage_publication_uncertain` 后远端迟到完成、强杀与 shutdown deadline 仍不保证无孤儿；C04-A RequestInfo/内容协商与staging并发回收修复已通过适用门禁、生成应用、根TCP及不同模型独审；C04-B1统一请求输入与默认规范化已通过定向、最新workspace、根真实TCP、生成项目及双库适用验收；C04-B2类型便捷方法已通过当前workspace/feature/MSRV/docs/deny、真实双库、生成应用与TCP及独立复审；C04-C Cookie读取/队列/加密/响应子簇已通过最终workspace、根双库各48、生成应用与真实TLS及不同模型审查；C04-D1统一Session Store/flash与请求生命周期已通过修后适用门禁、真实双库/Redis/Keycloak/浏览器、生成项目和根TLS，限定提交以Git为准；C04-D2 Store基础公开API已通过修后12项门禁、真实双库HTTP各54、三类生成4/2/2及资源收尾，D2提交3cabae2；D3扩展生命周期已完成最终12项门禁、真实双库各55、三类生成4/2/2与资源收尾，限定提交见Git；D4内置Cookie driver限定子范围已验收：final3完整12门禁通过（默认668/全特性1367 passed，29/102 ignored不算实服务），根真实双库加密各62/无加密安全负例各1、新生成plain5/双库认证各2、真实TLS代理链与仓内Chromium HTTP脚本4项及不同模型审查通过；root target清零、五旧consumer不变。本簇提交47cd7b8，不关闭PHP异常弱类型、全局撤销或完整Session/Web能力，详细证据见testing；D5请求级GC lottery/公开driver回收与追加expiry索引迁移限定子范围已验收：final4完整12门禁通过（默认668/29 ignored、全特性1374/103 ignored），根真实双库各62、Store/GC各2、Redis Store1/Auth10、新生成plain5/双库认证各2、独立审查及资源收尾通过；忽略项不算实服务，不关闭完整Session；D6请求URL限定子范围已验收：12项门禁通过（默认686/29 ignored、全特性1393/103 ignored），根PG18/MySQL8.4各63、fresh普通5/双库认证各2、真实TCP/TLS终止代理10场景及不同模型独立审查通过；两线真实PHP8.5差分逐案通过，不关闭可信Host/Port/Prefix、真实路由匹配、previous或完整Session。完整Store/HTTP/Session、RedirectResponse、文件输入、CSRF/FormRequest/Response后继继续开放；D9仅关闭旧输入限定子范围，见 [todo.md](todo.md) |
| 当前功能簇 | C04-D10-E scheme/secure与guest/intended适用门禁、生成应用及MySQL/真实TCP已通过，资源收尾已完成，限定提交见Git；previous原值与back生产链纠偏已修复并独审。D10-D已提交018b778；完整Redirector、HTTP/Session及发布全后端矩阵仍开放，见[testing](memory/testing.md)。 |
| 目标授权 | [ADR-070](decisions/adr-070-laravel-first-rebuild.md)：Laravel固定基线为1:1行为标准，laravel-modules为正式模块化范围，Goravel仅跨语言实现参考；允许全面重写，无历史兼容义务 |
| 当前完成边界 | 原认证/数据/队列/缓存/存储/实时/gRPC/视图等有专项证据；完整生产/集群/最低版本仍有缺口，新目标不能继承为已验收 |
| 协作 | 已定 contract 优先 gpt-6.1-sol 子智能体；主控负责设计/协调；独立功能簇可按依赖在本地任务分支/worktree并行；main集成及共享manifest/lock/迁移/文档由总控串行协调，target/服务资源预先分配，审查并行只读；高风险改码不同模型或 Provider 审查；不推送 |

## 固定约束

本机相关参考仓库共同父目录为 `/Users/Home/Codes/Git/`：`laravel-framework/` 是官方框架实现，`laravel-docs/` 是官方文档，`laravel/` 仅指官方应用骨架；固定验收 checkout 分别为 `laravel-framework-baseline/`、`laravel-docs-baseline/`。平级迁移及 linked worktree repair 已核完成，五个仓库工作区均干净；应用骨架本次 HEAD 为 `9ffef3bc4241e3cc333ef267258a404d0d95f194`，未安装 Composer 依赖。执行者不重复迁移或创建旧路径。本次拉取 framework 官方 `13.x` HEAD 为 `5a787ee008553a35206de2918d5242626ddeaed2`，docs HEAD 为 `960eedf0911155ad16326ade49c0ea5d18eda0b8`；固定验收 commit 仍为 framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`、docs `960eedf0911155ad16326ade49c0ea5d18eda0b8`，不随最新源码拉取静默升级。新取证/执行脚本读取固定 baseline，不能用应用骨架替代框架，也不再读取 `/tmp` 旧 Laravel 源码；冻结历史输出与 provenance 保留原路径事实。旧副本先核 owner/占用、无WIP后再安全回收，证据不当缓存删除。

- 开发优先完成源码/测试代码/示例/模板，功能簇提交前集中验证；不默认TDD或逐小改动重复跑门禁。分支定向门禁与main完整组合验收分层，未通过不提交。正常空间预算内缓存可跨簇复用，阈值/资源归属与临时worktree清理规则保持；详见[集中验证](conventions.md#功能簇提交前集中验证)。

- 当前框架保持同仓 main 集成；总控已获准创建本地任务分支与临时 worktree，按稳定契约/依赖并行独立功能簇。现有 main WIP由原负责人保全，不强行迁移；分支验收后串行合入并重验组合行为，再清理任务专属资源，细则见[并行开发与集成](conventions.md#并行开发与集成)。不新建源码仓库、不推送。

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
