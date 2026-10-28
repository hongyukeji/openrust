# 活跃待办

> 当前代码与目标：业务示例已迁至 `modules/example/` 并经模块清单接入；完整模块管理和真实 CRUD 仍未完成，按 [ADR-074](decisions/adr-074-laravel-modules-directory.md) 继续推进。

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：Laravel + laravel-modules 的 Rust 1:1 实现唯一活跃队列；R0旧基线已记录，模块包逐项盘点待补；R1-S与R2-A至R2-M各已交付对应子簇，后继 R1-E 真实 CRUD/能力示例未完成。

## 领取规则

先核对 Git/WIP、代理/进程、[执行校准](plan.md#执行校准) 与 [ADR-070](decisions/adr-070-laravel-first-rebuild.md)。每次一个主功能簇，完成 Spec → Implement → Verify → Commit 后再领取后继簇。整体依赖见 [R0–R16](plans/laravel-rebuild.md#5-按依赖实施)，不按旧 M5/M6 顺序执行。

## 从零重建与并行执行授权

用户再次明确按 Laravel + laravel-modules 的完整 Rust 实现从零设计、重写核心；旧 API、目录和实现不构成兼容约束。新内核以固定上游行为为依据，不继续堆叠旧 Plugin 装配；旧源码仅作事实与回归输入，由对应功能簇替换后通过 git rm 清除失效实现。此授权不删除他人 WIP、共享服务、数据或原始测试证据，也不要求重造成熟密码学和数据库驱动。

R2-M 模块闭环的源码与适用门禁已通过，随本簇限定本地提交收口；下一个可领取功能簇为 R1-E 真实 CRUD 与当前能力示例。全部工作留在 main；共享 Cargo 清单、锁文件、验证服务和提交串行协调；子代理不得递归派发。

## 当前可领取

| 功能簇 | 状态 | 交付与验收 |
| --- | --- | --- |
| R1-S 根结构与普通模块挂载 | 已完成结构子交付34dfb5e | 根Laravel职责、同package lib/server、资源内聚、模块公开挂载；按范围实际门禁与独立审查通过后可限定提交，仅结构子交付 |
| R2 基础：Container/Provider/模块配置/TestApp | R2-A/A2/A3、R2-B、R2-C、R2-D、R2-M 各子簇已验收 | 全体register后boot、统一Container与typed配置、最小TestApp及模块闭环；先形成真实CRUD/扩展接入的稳定API，不依赖完整R1-E |
| R2-M 模块最小闭环与目录迁移 | 真实服务与生成项目已验收，本簇提交 | modules/ 声明注册、模块配置/资源/统一迁移、依赖与冲突错误；示例迁至 modules/example，真实HTTP及双库验证；完整管理命令归R13 |
| R1-E 真实CRUD与当前能力示例 | 下一可领取；未交付 | 真实实体列表/详情/新增/更新/删除，校验/分页/404/授权/事务；PG18/MySQL8.4真实HTTP闭环；当前公开能力实际业务/扩展/测试逐项覆盖，GET基础信息响应不算CRUD |
| R2 后继内核与测试底座 | 待R2基础，按依赖分簇 | scope/Facade/DI/停机与模块依赖/顺序等完整对应；每簇同步modules/example，不建双内核；不把全部R2能力塞入首批 |
| 模块包固定范围细化 | 待逐项盘点 | 对正式laravel-modules固定源码/文档的导航、源码与测试拆行为矩阵，分类必要Rust差异/用户约定/缺口；不把目录规范或显式Plugin等同完整模块管理 |

## 后继路由

R1-S → R2基础 → R2-M模块闭环 → R1-E当前CRUD/能力贯通；R2后继及R3 HTTP → R4 数据 → R5 Model → R6 Factory/SQLite → R7 安全 → R8 Cache/Storage → R9 视图/通信 → R10 异步/事务 → R11 网络/组合 → R12 搜索/特性/观测 → R13 开发体验 → R14 底座门禁 → R15 V2 → R16 已纳入生态包。每个后继能力同簇补示例；R1-E不等待未来全框架完成，R2基础不等待完整示例，消除循环依赖。阶段内部按总计划拆可验证提交。

## 必须带入新矩阵的现有缺口

- 统一返回格式等已有优秀机制 → R0 登记对应 Laravel 职责与保留价值，R3/R5 在 Response / Resource / 异常渲染融合并回归，见[融合计划](plans/laravel-rebuild.md#已有优秀机制的融合)；重写不等于删除能力。
- 原 M5 可观测性/资源独立审查与完整生产门禁 → R12/R14。
- 原 DX1–DX8、F9/F10：默认 cargo run 与 Cargo 正常兼容设计、TestApp/assertions、factory、命令/生成/stubs、只读 doctor、dev 编排、文档和更新体验 → R1/R2/R6/R13。
- 双库负载均衡、多 API/worker/scheduler、共享状态/文件/实时、滚动升级/依赖故障、最低版本与性能 → R14。
- [SQLite 完整支持](plans/v1-sqlite-backend.md) → R6 及各能力适用验收；不能只接通连接。
- [V2 后台/App](plans/v2-admin-platform.md) → R15；PC 仅规划；独立 AI 产品未启动。

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
