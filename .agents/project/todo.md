# 活跃待办

> 当前代码与目标：业务示例已迁至 `modules/example/` 并经模块清单接入；R1-E CRUD/能力门禁已由 bc3fa01 提交；附件 PG/MySQL 全量各 15/15，真实 MinIO CopyObject 延迟响应时的双库删除竞争各 1/1；deadline/EOF 原子仲裁后，远端结果不确定的迟到发布仍属未保证边界。OIDC 确认页已完成 Chromium/mock 分支验收，真实 Keycloak/双库 `AlwaysOk` 错误密码、已消费 state 与成功夹证各 1/1；Collector PostgreSQL 隔离 sink 的 delivery/unavailable 各 1/1 且保留 16 MiB 限制，runner 路径和 fixture run ID 已修正。R2-N 已由 48be854 提交；R3-A/B/C/D/E/F 已按适用范围验收，C04-A/B1/B2/C已按子簇验收，后继C04-D Session/flash与Web栈契约待领取。

> 受众：AI Agent
> 更新：2026-10-29
> 摘要：Laravel + laravel-modules 的 Rust 1:1 实现唯一活跃队列；R0旧基线已记录，模块包逐项盘点待补；R1-S、R2-A至R2-N及 R1-E 真实 CRUD/当前公开能力示例已分别交付；R1-E 提交 bc3fa01。R2-N 由 48be854 提交；R3-A 语义 HTTP 状态默认已按子簇交付，继续 R3 HTTP/Web 路由、请求、资源与异常行为矩阵。远端结果不确定后的迟到发布边界保留。

## 领取规则

先核对 Git/WIP、代理/进程、[执行校准](plan.md#执行校准) 与 [ADR-070](decisions/adr-070-laravel-first-rebuild.md)。每次一个主功能簇，完成 Spec → Implement → Verify → Commit 后再领取后继簇。整体依赖见 [R0–R16](plans/laravel-rebuild.md#5-按依赖实施)，不按旧 M5/M6 顺序执行。

## 从零重建与并行执行授权

用户再次明确按 Laravel + laravel-modules 的完整 Rust 实现从零设计、重写核心；旧 API、目录和实现不构成兼容约束。新内核以固定上游行为为依据，不继续堆叠旧 Plugin 装配；旧源码仅作事实与回归输入，由对应功能簇替换后通过 git rm 清除失效实现。此授权不删除他人 WIP、共享服务、数据或原始测试证据，也不要求重造成熟密码学和数据库驱动。

R2-M 模块闭环、R1-E 综合示例与 R2-N 模块依赖顺序均已在 main 限定提交；当前唯一主功能簇转入 R3 HTTP/Web 后继。全部工作留在 main；共享 Cargo 清单、锁文件、验证服务和提交串行协调；子代理不得递归派发。

## 当前可领取

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

- **宏观主线**：[两阶段框架主线](plans/laravel-rebuild.md#两阶段框架主线)；先验阶段一Laravel+modules核心基线，再做已纳入阶段二通用增强，均是框架main而非产品可选快速开发模块。R0–R16仅技术分解；现增强保留复用，不搬走核心Config/Queue/Scheduler/安全，当前B2不变，发布整理须两阶段既定范围验收。

- **可选模块边界**：原V2版本阶段取消，后台/App需求转入[可选快速开发模块](plans/optional-rapid-development-modules.md#开发与使用边界)，暂不开发，待用户明确开工。R14、两个框架阶段或持续Goal“全部完成”不能自动启动；main只推进框架/唯一示例，不清理现有WIP，不预建分支或默认合并。框架配置管理、Queue/Scheduler及官方扩展继续；官方工具自身界面属于对应可选包，不受业务后台冻结限制。

- 阶段一R2公共声明/覆盖/错误契约→R13包发现与生成工具：[Provider编译期发现Spec](plans/laravel-rebuild.md#r2r13-provider-编译期发现-spec)（L044.02/模块M016），复用R2-M/N清单、拓扑和register→boot；仅规划未实现，不抢占当前HTTP。

- 阶段一R2容器完整Binding后继（矩阵L012.02）：D3接线实证现API只有后注册覆盖，缺保留任意原factory的Laravel extend。按固定上游补通用公开装饰、生命周期/已解析singleton/多次顺序/错误与资源归属，贯通显式custom Session driver、模块及生成应用；D3已知Db/Memory fixture不关闭此项，不新建专属装饰内核，不抢占当前HTTP。

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

1. 框架与应用只在本仓 main 开发；根 package 为 openrust，src/lib.rs 提供框架，薄 src/main.rs 提供默认 server。
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

## R2 配置能力审计输入（已只读完成，未实现）

- 已对照 Goravel 官方源码和现有 ConfigLoader/section<T> 完成只读审计；具名连接/store与类型化读取是已有能力，不能重复实现。
- 审计范围包括完整领域配置说明、env 映射、typed 读取/启动校验、模块配置的公开注册与覆盖；先说明现有能力、真实缺口和依赖，不凭空叠加第二配置层或改变 env 语义。
- 本项归 R2，依赖只读审计交付与统一 Container/ServiceProvider contract；R1 不混入配置实现改动，当前配置源、默认运行值和 Redis 默认无静默回退保持。

## R2/R10 运行时配置与动态调度（仅规划，按依赖领取）

- 依据 [ADR-080](decisions/adr-080-runtime-configuration-scheduling.md) 与 [运行时配置与动态调度计划](plans/runtime-configuration-scheduling.md)；当前 HTTP 主功能簇不抢占，本轮没有代码实现或真实验收。
- R2：V1必须交付Laravel简洁typed分组DB业务设置与权限保护管理API/CLI（宿主启用可选）；开发者注册字段、管理员后台保存，白名单/权限/完整校验/CAS审计/快照/通知轮询/故障期限封装内部，持久实现依赖R4事务、R7权限审计及适用R8，基础设施启动项不热换；邮件/SMS/支付/存储业务secret加密、driver受控激活或pending_restart，区分DB保存/节点应用。
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

## 后继 C04-D5：StartSession GC lottery与公开driver回收

D4限定提交后领取；默认lottery对照固定上游[2,100]，start后、业务前对实际请求绑定driver调用公开GC；同一Session内核，无第二存储。Spec须先收敛持久绝对expiry与上游last_activity期限的安全差异，不能误删仍获授权的显式长Expiry或声称已有expiry load替代未访问记录GC。数据库/Memory实际清扫，Cookie/Redis按上游noop；失败/取消/原子防复活、唯一模块/consumer/新生成、双库/Redis与匹配门禁同簇验收。previous URL/route、真实Precognition、Old Input/CSRF、blocking及Session cache继续保留，不能用本簇关闭整个L025。

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
2. **阶段一**：继续当前唯一核心簇，按既定R2–R13依赖完成Laravel核心与模块内核/生成管理/资源命令；官方可选包或生态集成不反向阻塞阶段一门禁，核心必需配置、HTTP、数据库/ORM、安全、缓存/存储、队列/调度等不挪走。M031/M033/M034等依赖生态的集成在阶段二闭环，保留范围和ID。
3. **阶段二A：官方基础包与体验**：共享前置成熟后，按包实现Fortify/Sanctum/Socialite/Passport、Folio/Head/Precognition、Octane对应及额外数据库driver；先Spec默认/安全/扩展与实际生产链路，模块生态集成、官方starter-kit及JS配套同步对应，不新增第二示例。
4. **阶段二B：数据、异步、观测与工具**：按DB/Model/Queue/HTTP前置对应Scout/Pennant、Cashier Stripe/Paddle、Reverb/Echo、Horizon/Pulse/Telescope及Dusk/Envoy/Prompts/Pail/Tinker/Pint/Sail/Valet/Homestead等适用体验，Wayfinder/Stream和编辑器/分析元数据依赖随相应簇完成。Horizon/观测工具自身必要UI与鉴权随包交付，按需引入不默认公开；这不授权组织/菜单等业务后台或App。
5. **阶段二C：AI与已授权增强**：AI-0至AI-4、官方MCP Server/Client、Boost按真实前置组合，SDK不能改成另一套机制；MQTT、gRPC、DB动态设置及既定增强继续。AI-5内嵌推理后置评测，不因全量SDK强制模型运行环境。A/B/C是职责批次，可按真实依赖交错，不把列表顺序变成循环前置，不额外启动并行主功能簇。
6. **最终收口**：每个官方包逐项行为、公开扩展、唯一模块、生成项目与真实服务/浏览器/协议证据均齐备，Rust差异/外部条件缺口公开；两阶段/C33证据汇入R14，不能只关闭核心便称完全体。若缺账号/许可/服务继续独立项，禁止fake冒充实测；不推送/发布、不改写历史，业务后台与AI产品仍未授权。
