# 当前计划

> 当前代码与目标：R1-E、R2-N、R3-A/B/C/D/E/F 均已按范围交付（R3-E：dab80d4；R3-F 本簇限定提交见 Git）。R3-F 已完成独立复审、适用门禁、生成应用、根真实 TCP 与资源收尾；C04-A RequestInfo/内容协商与staging并发回收修复已通过适用验收，随后继续todo中的完整C04剩余行为。完整模块管理仍按 [ADR-074](decisions/adr-074-laravel-modules-directory.md) 推进。

> 受众：AI Agent
> 更新：2026-10-29
> 摘要：执行 Laravel + laravel-modules 的 Rust 1:1 实现；R0 上游和逐章资产基线已记录，R1-S、R2-A/A2/A3、R2-B/C/D/M/N、R1-E 与 R3-A/B/C/D/E 已按簇完成适用验收；后续继续 R3 HTTP/Web 与模块行为矩阵。R0–R16 全体及严格 1:1 仍未完成。

## 当前里程碑

[ADR-070](decisions/adr-070-laravel-first-rebuild.md) 已接受全面重写方向；[总计划](plans/laravel-rebuild.md) 定义能力矩阵、Rust 适配、目标目录、R0–R16、验收和模型分工。[执行任务](plans/laravel-rebuild-execution.md) 可直接交给执行 Agent。当前领取项仅在 [todo.md](todo.md)。

R2-T ConfigRepository严格类型读取已由独立分支 `b1f4bf7` 限定验收，并纳入R03/CC/Signed同一main限定组合交付；原分支生成与TCP证据保留，主线结果须引用组合门禁和最终五父提交 `e105dbfc58afc330b50e53b75f0b9e1424fa387a`。完整Config仍开放。

## 运行时配置与动态调度依赖

用户授权目标见 [ADR-080](decisions/adr-080-runtime-configuration-scheduling.md) 与 [运行时配置与动态调度计划](plans/runtime-configuration-scheduling.md)：Laravel配置体验、V1必交typed分组DB业务设置与受权管理API（宿主启用可选）、默认DB队列与动态计划；业务secret加密及driver受控激活/待重启，DB保存和节点应用分别可见。当前仅规划，不切换当前HTTP簇；R2契约/持久基础（依赖R4/R7及适用R8）→R10队列事务/每任务策略/动态管理→R14真实多节点与迁移验收；[可选管理界面暂不开发](plans/optional-rapid-development-modules.md#开发与使用边界)，不得自动进入。公共服务/CLI和示例随各簇交付，内部版本/快照不增加普通读取负担；唯一执行状态仍见todo。

## 执行校准

用户本轮已明确恢复原完整Laravel+laravel-modules计划，解除仅收尾/不领取新簇限制；当前C04-F Request文件家族进入Spec→Implement，owner/稳定基线/工作区和资源见唯一[todo](todo.md#c04-f-request-files)。沿用原Goal、默认80%预算及20%保留，不新建守护、不重算基线，不启动冻结产品。

R2-P `4ac8099` 与 S-DV `6ffb5c8` 已由三父提交 `9e5a94198b421987851b3580b27fa90370feeb77` 完成main限定组合门禁；ST-V `2262df9` 后由两父提交 `8d18021b630ea10652316e025ef6019c7772bbb0` 完成main限定组合门禁；真实S3 ACL、迟到CopyObject及`throw=false`仍开放，见[本簇记录](memory/testing.md#st-v-main-限定组合验收)。S-DV与P均已完成工作区/分支安全退役，P文档事实已迁入main并保全原稿，完整Provider/Container/Session仍开放。四簇main组合默认/全特性严格Clippy与workspace tests已通过（907/0/34 ignored、1698/0/108 ignored）；双docs/MSRV、no-default、deny和生成/MySQL限定证据见组合记录；五父本地提交 `e105dbfc58afc330b50e53b75f0b9e1424fa387a` 仅关闭本轮限定集成。唯一队列见[todo](todo.md)。

C04-D10-E scheme/secure与guest/intended已通过适用集中门禁、fresh生成/MySQL及真实TCP4/9，资源收尾完成，限定提交见Git；D10-D为018b778。E限定提交6b143ca。C04-D10-F refresh/away已通过当前门禁、生成/MySQL与真实TCP7/12，资源收尾完成、限定提交见Git；F已提交ba538ea；动作路由身份与action URL按ADR-083 D10-G已限定验收，主门禁、新构建/fresh生成/MySQL及真实TCP10/15与资源收尾通过，含单实例namespace setter与clone隔离；当前H按ADR-083收口真实声明集合/分派同源化，已完成限定验收：定向/双严格Clippy/完整router及主矩阵11项、新构建/生成/MySQL/真实TCP与资源收尾通过，限定提交见Git；T01已完成限定入口整合并实测名单/计时，未证明整体显著提速；R2-R01按ADR-086已完成同仓储运行期set/set_many、App读取及启动快照限定交付，含工作区/fresh生成/MySQL/真实TCP与资源收尾；当前R2-R02按ADR-086已完成有序来源实现、适用Rust门禁、普通/MySQL生成、真实TCP与资源收尾限定验证；已提交a875dd7，R2-ENV已完成工作区/consumer、fresh普通/MySQL、真实HTTP/Redis与资源收尾限定验收；signed完整家族基于af0f62b隔离实现/修复，R2-R03基于e25ec75隔离编码递归配置发现，main集成/组合验证串行；唯一队列见[todo](todo.md)，action/signed前置与完整Redirector、发布全后端矩阵继续开放，不以薄封装替代缺失底层契约。

C04-D9旧输入限定子范围已完成main组合验收，[证据](memory/testing.md#c04-d9-main-组合限定子范围验收通过)保留；R2-Q/X与守护补丁已在main收口。C04-D10-A纯MessageBag/ViewErrorBag已由`27c006a`限定提交。C04-D10-B请求绑定重定向、Session命名错误包及空库迁移启动修复已通过当前源码门禁、无Session与MySQL8.4真实TCP、普通生成7项及MySQL生成认证2项，已由 `a2befef` 限定提交；证据和未覆盖边界见[testing](memory/testing.md#c04-d10-b-请求绑定重定向与-session-错误包限定子范围已验收)。[测试入口整合小试点](memory/testing.md#session-测试入口整合小试点限定验收通过)已通过适用验收并由 `4023e05` 限定提交：三个纯 Session 测试 target 聚为一个，26 项与独立环境/服务入口保留；联合 gRPC 测试的预绑定 listener 修复及旧配置入口也经真实测试；C04-D10-C请求局部视图错误共享、Session对象身份及模板参数桥接已按限定子范围验收并提交 `784045c`；根目标/fallback、内部forward与HEAD站点边界修复已通过不同模型审查及当前输入门禁、fresh生成与真实TCP。默认workspace798/31 ignored、全特性1571/105 ignored，严格双Clippy/docs、features/deny/MSRV、新server/CLI、普通生成7项、MySQL认证2项与独立公开consumer1项通过；无Session/MySQL8.4.13真实TCP通过并清理自有库/端口。ignored不计真实服务，完整Redirector、Validation/CSRF、HTTP/Session及PG/全部后端发布矩阵仍开放。[最终测试记录](memory/testing.md#c04-d10-c-请求局部视图错误共享验收中未提交)。C04-D10-D命名路由重定向限定子范围已验收，提交见Git：`route/route_with` 复用同应用URL生成器及既有响应/闪存链；v2定向、双严格Clippy及main11项通过，默认workspace802 passed/32 ignored、全特性1576 passed/106 ignored。最终server/CLI、fresh普通7项、MySQL8.4.13认证2项及独立公开helper1项通过；无Session/MySQL真实TCP分别3/4个场景通过，正常停止并释放监听、清理敏感文件及自有数据库。ignored不算真实服务；完整Redirector及L020.02仍开放。见[当前验收](memory/testing.md#c04-d10-d-命名路由重定向当前验收中未提交)。固定参考路径与最新源码区分见context。

R2-R03 `6482a38` 的配置76、CLI21、模块2、consumer6、生成/MySQL与真实HTTP分支证据保留；typed `b1f4bf7`、CC `7be446d`、Signed `c77521f` 已与其完成同一main限定组合交付。主线限定验收见[组合记录](memory/testing.md#r2-四簇-main-组合限定验收)，旧分支结果不替代主线组合记录。

当前验证节奏：先完成代码与配套测试代码，功能簇稳定后在提交前集中验收；分支限定验证、main完整组合门禁。实现期不反复跑测试/Clippy矩阵，无关计划文档不使代码证据失效；按[集中验证](conventions.md#功能簇提交前集中验证)调度。正常空间阈值内受控缓存可跨簇复用，50/100 GB（十进制）及磁盘10%/5%政策不变；不再每簇强制清空target。

全量范围已按用户最新授权收敛：固定Laravel核心、laravel-modules及官方可选生态必须按官方功能/默认/生命周期/扩展体验完整对应；[749条/35入口分期与工具UI边界](plans/laravel-official-coverage.md#全量交付授权与两阶段)为本轮范围，唯一todo已分两阶段排期。官方工具自身UI随阶段二包交付，业务后台/App及AI产品仍未启动；不因语言、可选或阶段二而省略适用功能，当前核心簇按依赖继续。

1. 当前代码的根 lib/server 和旧 app/example 过渡结构由 R1-S 以34dfb5e收口；R2-M 已将业务代码迁入 modules/example，根模块边界依 ADR-074。模块清单、运行激活、资源与统一迁移的真实验收见本轮 testing；完整模块管理和 R1-E CRUD 不因目录迁移自动完成。
2. 正式定位 Laravel + laravel-modules = OpenRust，以固定上游源码/测试和文档逐项1:1实现功能/可观察语义、默认/错误安全边界、Container/Provider与开发流程，并持续跟进；模块化不是只有示例目录规范。目录、概念、命名、默认行为和扩展流程以 Laravel 为准；Goravel 为实现参考。Rust必要适配、用户约定和授权增强须按[差异分类](plans/laravel-rebuild.md#逐项对应与差异分类)记录原行为/限制/选定行为/证据，未等价保持缺口；版本基线和后继更新见[长期上游跟进](plans/laravel-rebuild.md#长期上游跟进)。既有目录和 API 可以重写，无历史兼容义务。
3. 旧 ADR 中的已实现行为和验收记录作为迁移输入；统一返回格式等优秀机制须融合进 Laravel 对应职责，不能为追求上游外形而删除。新方向不自动使旧记录变成新代码已通过。冲突约束依 ADR-070 替代，功能簇落地时同步领域 ADR/事实。
4. 原 M5/M6、DX/F/G 顺序让位于 R0–R16；未完成生产、集群、SQLite、供应商条件等映射到新矩阵，不丢弃。总控可按依赖图批准独立功能簇并行 in_progress，main集成与最终门禁串行。
5. 对已定 contract 的实现/测试优先 gpt-6.1-sol；主控定跨模块接口，高风险改码不同模型或 Provider 审查。worker可在总控分配的本地任务分支/worktree并行，前置和contract须明确；manifest/lock/迁移/共享文档与main集成由负责人串行协调，构建target/服务预先分配。按[并行开发与集成](conventions.md#并行开发与集成)验收、合入并清理，不覆盖main WIP，不创建额外源码仓库、不递归派发、不推送。
6. 根无参数 cargo run、唯一综合示例、生成应用和真实 HTTP 是验收项；根 package 同时包含框架库和默认 server。
7. 日常真实数据库默认MySQL8.4 LTS（Redis8不变）；PG18服务保留，后端专属SQL/驱动/故障或方言/迁移/锁风险当场必要定向。发布前统一执行全部承诺后端、版本与适用迁移/事务/数据安全/队列/集群/生成矩阵，最低版本和真实复制等证据按总计划，不以单库日常通过宣称全后端兼容。
8. [R15/C32 可选快速开发模块暂不开发](plans/optional-rapid-development-modules.md#开发与使用边界)，R14通过仅技术前置；须用户明确开工，当前框架main与官方可选包（含工具自身UI）按依赖继续。独立 AI模型平台仍未获启动授权。
9. WIP、他人资源、依赖安全、真实证据及对外授权边界继续有效。常规问题自主决策并记录；外部缺口继续独立工作，不能虚报完成。

当前交付依赖是 R1-S结构/挂载 → R2基础Container/Provider/配置/TestApp（A–D 已验收）→ R2-M模块契约/目录迁移 → R1-E真实CRUD与当前能力演示（已由 bc3fa01 提交）→ R2-N模块硬依赖与启动顺序（本簇已验收）→ R3–R16逐簇实现并同步示例。R2-D 测试应用由生产 Provider/Router 同路径构建，但不代表完整模块管理已完成；R1-E 的 Article、租户、附件、通信、观测等生产路径与 R2-N 证据分别记录于 [testing.md](memory/testing.md)。模块完整管理与 Laravel 严格 1:1 范围继续保留。

C04-A/B1/B2/C与C04-D1均按各自子簇验收。D1以唯一公开Session领域替换旧tower运行时，最终默认668/29ignored、全特性1300/102ignored，双库完整模块HTTP各53、Auth各8、浏览器各4、模块OIDC各8、真实Keycloak主流程各1、三类生成与Session真实TLS均有实际证据；忽略项和PHP候选不冒充完成。根网络/完整12项门禁通过，限定提交见Git；资源收尾见testing。C04-D2基础Store公开API已通过helpers9/HTTP5、独立consumer、模块定向3项、修后12项门禁、根真实双库各54和三类生成4/2/2及资源收尾；D2提交3cabae2；D3扩展生命周期亦已通过最终12项门禁、真实双库各55、生成4/2/2及资源收尾；D3提交be98e3f；D4内置Cookie driver限定子范围已验收：final3完整12门禁通过（默认668/全特性1367 passed，29/102 ignored不算实服务），根真实双库加密各62/无加密安全负例各1、新生成plain5/双库认证各2、真实TLS代理链与仓内Chromium HTTP脚本4项及不同模型审查通过；root target清零、五旧consumer不变。本簇不关闭PHP异常弱类型、全局撤销或完整Session/Web能力，详细证据见testing；D4提交47cd7b8；D5请求级GC lottery/公开driver回收与追加expiry索引迁移限定子范围已验收：final4完整12门禁通过（默认668/29 ignored、全特性1374/103 ignored），根真实双库各62、Store/GC各2、Redis Store1/Auth10、新生成plain5/双库认证各2、独立审查及资源收尾通过；忽略项不算实服务，不关闭完整Session；D5提交e4adc40；D6请求URL限定子范围已验收：12项门禁通过（默认686/29 ignored、全特性1393/103 ignored），根PG18/MySQL8.4各63、fresh普通5/双库认证各2、真实TCP/TLS终止代理10场景及不同模型独立审查通过；两线真实PHP8.5差分逐案通过，不关闭可信Host/Port/Prefix、真实路由匹配、previous或完整Session。D6提交86f231a；D7真实Route/Resolver与受管理方法声明已通过限定验收：final3完整12门禁、默认695/29 ignored、全特性1403/103 ignored、根PG18/MySQL8.4各64、fresh5/2/2、真实TCP7及不同模型复审；忽略项不算真实服务。根target清零，五旧consumer不变，限定提交16b208e；完整Routing/Request与previous/Precognition仍开放。C04-D8 previous与显式Precognition限定子范围已完成适用验收：稳定主线门禁、全特性1439 passed/104 ignored、Rust1.97 check、真实PG18/MySQL8.4各67项模块HTTP及各1项previous、所有权PG3/My2、新生成应用5/2/2、真实TCP6项与最终CLI快照1项均有证据。默认workspace以709项有效结果与修后20项doctest按影响复用，非虚报原失败命令exit0。完整Old Input/CSRF/FormRequest、规则集与整个HTTP/Session继续开放。 D9旧输入限定子范围已交付；后继按唯一todo收敛错误包/RedirectResponse输入与请求局部视图共享，CSRF/FormRequest依赖后继。

## 路由

| 任务 | 入口 |
| --- | --- |
| 新方向与替代关系 | [ADR-070](decisions/adr-070-laravel-first-rebuild.md)、[ADR 索引](decisions/README.md) |
| 宏观执行顺序 | [两阶段框架主线](plans/laravel-rebuild.md#两阶段框架主线)：阶段一固定Laravel+modules基线→阶段二已纳入通用增强，均属框架main；R0–R16为技术分解，原R15版本阶段取消，可选模块不自动开工 |
| 整体范围、阶段、验收 | [全面重构计划](plans/laravel-rebuild.md)、[模块正式矩阵](plans/laravel-modules-matrix.md) |
| Provider包发现后继 | [Provider编译期发现Spec](plans/laravel-rebuild.md#r2r13-provider-编译期发现-spec)；R2契约→R13工具，依赖R2-M/N；仅规划不切换当前HTTP |
| 当前实现资产 | [architecture.md](architecture.md)、core、Cargo.toml、源码和 Git |
| 原能力/生产验收输入 | [原 v1 计划](plans/gen1-plan.md)、[底层补齐](plans/v1-foundation-completion.md)、[集群](plans/v1-cluster-deployment.md)、[原 DX](plans/v1-developer-experience.md) |
| 编码、提交、证据 | [conventions.md](conventions.md)、[testing.md](memory/testing.md)、[verification.md](memory/verification.md) |
| 执行任务文本 | [执行说明](plans/laravel-rebuild-execution.md) |

## 验证入口

每代码簇按 AGENTS.md 跑 fmt、workspace clippy/test/doc、cargo deny，及适用feature/MSRV、日常MySQL及后端专属定向、生成项目、真实服务门禁；发布前全后端矩阵另验；记录命令与实际结果。文档-only 只做 diff、链接/锚点和旧口径检查。不能把本轮规划检查写成代码或重构已验收。

当前重构来源与 102 章/749 子节当前纳入范围盘点见 [固定基线](plans/laravel-rebuild-baseline.md)；未将旧证据继承为新 contract 通过。
