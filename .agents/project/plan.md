# 当前计划


Hashing字节与单次叶选择已完成[Hashing字节与单次叶选择限定验收](memory/testing.md#hashing字节与单次叶选择限定验收)；后继先交验MB-S01，再按唯一todo推进增强认证、Model与Manager剩余契约。隔离候选静审不等于运行，本批不扩张到HA、NumericDate、CT或Passport。

EV-R01已完成[事件身份与响应限定验收](memory/testing.md#事件身份与响应限定验收)，提供Model保存后继需要的事件身份与响应机制。后续串行交验Hashing字节、Schema等独立冻结候选；其静审不算运行，不追加到本Events有限提交，完整Model事件/官方事务载荷仍未完成。

事务来源与文章末检前置已完成[限定验收](memory/testing.md#事务来源与提交前检查限定验收)，后继来源借用可依赖公开`TransactionContext`，认证权威仍须独立实现。后续按唯一todo串行交验Events/Schema等冻结候选，不把其静审当运行通过，不追加到已闭合CTX有限簇。
模型属性状态 MB-P01-A 已收齐[限定验收](memory/testing.md#模型属性状态与无持久预览限定验收)：同一属性存储的 original/dirty/changes/previous、公开单值 cast、账户记录无持久化预览及根/生成 User/外包接线；完整转换后 getOriginal、fill/save、事件/关系和完整值域仍开放。有限提交以 Git 为准，下一步沿唯一 todo 处理 Events、Schema 与模型持久生命周期。

当前有限交付：MB-A01模型属性与认证前置已提交推送`06b4d50`，限定验收见[测试基线](memory/testing.md#模型属性与认证前置限定验收)。C07-W01/W02已按限定日常范围验收并提交推送ed3f608；AR-P01已由4463238完成独立包迁移、日常适用验收及远端核对，AR-M01已由6c21bab0限定验收提交推送，AR-N01职责命名空间限定迁移已由`b3f6117be1fc`提交并收齐[限定验收](memory/testing.md#命名空间与开发流程限定验收)；I18N-S01来源增强已收齐[翻译来源限定验收](memory/testing.md#翻译来源限定验收)；完整官方contract及后继核心仍在唯一todo。J01、SC-N01与日期cast准备保全WIP，不插队扩大当前组合；SQLite、数据库国际化和Passport并行推进实际实现与接线。

Unix网络测试库生命周期与持久归属回收已收齐[限定验收](memory/testing.md#网络测试数据库生命周期与持久恢复限定验收)，本地提交以Git为准；非Unix ACL与专项拓扑边界保留。统一Hashing前置、认证与生成接线已收齐[限定验收](memory/testing.md#统一-hashing-与认证接线限定验收)；通用Manager表面与Model hashed cast仍开放。后续以执行校准的最新核心/并行分工为准：root继续通用核心，Passport Client/签发/持久接线由并行owner推进，不重跑已关闭范围或扩张冻结簇。

> 当前代码与目标：R1-E、R2-N、R3-A/B/C/D/E/F 均已按范围交付（R3-E：dab80d4；R3-F 本簇限定提交见 Git）。R3-F 已完成独立复审、适用门禁、生成应用、根真实 TCP 与资源收尾；C04-A RequestInfo/内容协商与staging并发回收修复已通过适用验收，随后继续todo中的完整C04剩余行为。完整模块管理仍按 [ADR-001](decisions/adr-001-framework-principles.md) 推进。

> 受众：AI Agent
> 更新：2026-11-02
> 摘要：执行 Laravel + laravel-modules 的 Rust 1:1 实现；R0 上游和逐章资产基线已记录，R1-S、R2-A/A2/A3、R2-B/C/D/M/N、R1-E 与 R3-A/B/C/D/E 已按簇完成适用验收；后续继续 R3 HTTP/Web 与模块行为矩阵。R0–R16 全体及严格 1:1 仍未完成。

## 当前里程碑

[ADR-001](decisions/adr-001-framework-principles.md) 已接受全面重写方向；[总计划](plans/laravel-rebuild.md) 定义能力矩阵、Rust 适配、目标目录、R0–R16、验收和模型分工。执行按唯一待办领取。当前领取项仅在 [todo.md](todo.md)。

R2-T ConfigRepository严格类型读取已由独立分支 `b1f4bf7` 限定验收，并纳入R03/CC/Signed同一main限定组合交付；原分支生成与TCP证据保留，主线结果须引用组合门禁和最终五父提交 `e105dbfc58afc330b50e53b75f0b9e1424fa387a`。完整Config仍开放。

## 运行时配置与动态调度依赖

用户授权目标见 历史决策 080 与 [运行时配置与动态调度计划](plans/runtime-configuration-scheduling.md)：Laravel配置体验、V1必交typed分组DB业务设置与受权管理API（宿主启用可选）、默认DB队列与动态计划；业务secret加密及driver受控激活/待重启，DB保存和节点应用分别可见。当前仅规划，不切换当前HTTP簇；R2契约/持久基础（依赖R4/R7及适用R8）→R10队列事务/每任务策略/动态管理→R14真实多节点与迁移验收；[可选管理界面暂不开发](plans/optional-rapid-development-modules.md#开发与使用边界)，不得自动进入。公共服务/CLI和示例随各簇交付，内部版本/快照不增加普通读取负担；唯一执行状态仍见todo。

## 执行校准

日常实际数据库验收仅MySQL8.4；其他类型与最低版本运行统一发布前完整验证，唯一规则见[数据库运行政策](memory/testing.md#当前数据库运行政策)。不因此追加当前Query或AR-P01非MySQL运行矩阵。

短闭环优先以依赖已满足、范围稳定、实现及验收可有限结束为判据：已验收有限簇及时提交推送 → 唯一todo中的通用Model、Container与官方对应后继；其余主线和SQLite、Passport、Model后继按唯一todo与互斥文件继续，不以测试代码数量报完成。Lead与独立测试对话已获真人双向直接协作授权，交验后立即继续互斥开发，不经Architect转发、不重复派发或中断working测试。

最终批准的包布局为根应用→独立框架包 `vendor/openrust/framework/`，框架使用标准 `src/lib.rs`，扩展位于 `vendor/openrust/<扩展>/`，不采用平铺 lib.rs。当前独立包布局已由4463238限定验收提交推送；模块扩展拆包已由6c21bab0限定验收提交推送，root按唯一todo接续AR-N01，不把搬迁验收当作命名空间或完整Laravel能力验收。旧冻结测试输入保持原路径。依赖方向、公开导入与生成验收见[包布局](plans/extension-package-layout.md)。

最新职责分离：Lead与开发worker负责生产/配套测试代码和失败修复；独立验收对话01a1ad87-8f71-7b42-9738-59dd7c27ec42负责官方行为映射验收、运行门禁/实库/生成及证据。当前在途命令自然结束后交资源，不中止重跑。稳定输入SHA、边界、features、命令及资源窗口经tmp/independent-testing-20261106交接；Lead可继续不影响受验输入的互斥编码，源码依赖有变时结果不得沿用。真人随后明确授权独立验收目录作为测试隔离例外：/Users/Home/Codes/Rust/openrust-dev使用detached worktree及自有target（验收方已迁移并核验1528项快照SHA）；短窗口导出准确HEAD+tracked binary patch+未追踪allowlist与SHA，不含.env/guard账本/target。测试path依赖须解析隔离副本，导出核验后解除主目录长冻结，失败由Lead修后同步新快照。共享服务/端口/磁盘及多target总预算仍预约；有效输入绑定回执由Lead核候选树后唯一索引提交。编码及配套收齐、官方契约静态自查后立即导出交验，不先由Lead跑完整测试；交接记编码完成/验收中，Lead继续下一互斥核心编码，失败由Lead修复后导出新快照复验。下一簇若依赖候选接口须显式关联输入变化。未经验收不得交付；不新增业务Goal或Architect守护。

最新开发节奏：有限功能簇先Spec固定上游contract→集中补齐实现、调用方、配置、生成、唯一示例、测试代码与文档→逐项Laravel映射及独立静态审查、批量闭合编码缺口→冻结并集中适用验收→失败按影响修复复验→通过后本地提交。状态区分编码中/对照缺口/冻结待验/验收失败修复/已验收已提交；最小编译或歧义诊断探针允许，静态对照不等于行为通过，当前受验批次正常收口不重来或加簇。

2026-11-06真人纠正：总控主线持续实现Laravel核心依赖；Passport/OIDC/SSO等官方可选生态由并行owner交付，不能长期阻塞核心。当前insert_get_id有限核心簇先验收提交，随后通用Model及核心矩阵；SQLite后端/最低版/专项开发交专属并行owner，支持目标与受影响交付门禁不降低，公共API/vendor仍统一收敛。CLI/生成与官方章节配套并行但仍属必交核心。所有状态、owner和前置仅在唯一todo维护，公共contract/资源/集成仍root串行。当前Goal完整普通额度覆盖按本次明确授权单独登记，不修改全局80%默认或历史账本，credits=false。

列边界Query子簇已完成固定上游对应、组合12门禁、唯一示例双库和fresh生成验收，限定事实见[testing](memory/testing.md#query-列边界与布尔条件链限定验收)。exists/doesnt_exist子簇也已完成相同组合门禁、真实双库和fresh生成验收；存在性回调叶也已限定验收；before-query共享子节点/冻结绑定与原查询生命周期也已收齐限定验收，见[testing](memory/testing.md#query-查询前回调与共享子查询限定验收)。冷prepare取消采用已验单连接淘汰边界，临时Cargo补丁约束见[testing](memory/testing.md#mysql-冷预处理取消与连接淘汰限定验收)；后续按唯一todo及最新分工领取：总控继续通用核心，Passport/OIDC并行推进，不关闭完整Query/ORM。

2026-11-04真人追加并行授权：同main启用多个可见开发对话框结合子智能体，按唯一todo互斥文件/依赖分工；总控继续独占公共contract、共享文档、manifest/lock、迁移、Git、Cargo/服务及最终验收。当前Guard核心先独立验收提交→Fortify官方TOTP优先→返回核心依赖顺序不变；本批冻结范围不扩大，但后续每簇持续按依赖图并行。每次领取/闭环/worker完成/依赖解除主动复核ready队列，前置满足、互斥文件和资源许可就复用可见对话框/子智能体接续派发，无需再次批准；无ready项、接口未定或最终验收须串行时记录原因，不强求固定人数/空转。及时复用或释放worker，不递归派发，不扩大本批。创建/queued不算开发，首个真实业务工具及交付回执登记唯一todo。

真人最新排期：当前在途R7具名认证核心簇先按源码/生产接线/配置/调用方/模板/唯一示例/文档/适用验收闭环，冻结修复后独立本地提交，不追加整个Fortify。随后固定Fortify双因素与Google TOTP按明确批准的阶段二优先例外推进；其他Fortify与全部官方生态范围保留，但其他包不因此全面提前。该优先闭环后回Container/Provider、HTTP/Session剩余前置核对，再沿Query/Schema/迁移→Model/分页→Factory实际依赖逐簇领取；SQLite/CLI/导航文档同步，不重做已验范围或做R2–R6巨型提交。真实Google客户端外部缺口单列，原Goal/main/预算与冻结业务边界保持。

## 当前 Fortify 单体系与显式 JWT API profile（最终生效契约）

最新真人决定：框架、根默认应用与未显式选择JWT的生成路径，以对应固定Laravel官方默认、生命周期和公开机制为准；Fortify原生Web/Session是其对应模式默认，不将所有场景一律Session或一律JWT。API+Bearer JWT是本批完整必交的显式选配，开发者选择后全程无Session/Cookie/CSRF-cookie/remember/隐藏Session/Set-Cookie或Cookie身份兜底。两种传输共用唯一UserProvider/Guard/Fortify三字段/Actions；旧auth-mfa兼容仍删除。此决定覆盖此前JWT为根默认、一律禁中心Cookie等中间口径，不改写历史验收。

**本批必须实际交付 JWT access/refresh API 与无 Cookie 管理闭环。** 显式 profile 只区分开发者选择及来源分类，不降低交付优先级、不推给应用自行实现；Fortify不内置此流程，按Laravel Guard/UserProvider/Actions公开机制扩展。Laravel官方生态已有Passport OAuth2与Sanctum，不能笼统称Laravel不支持Token，也不以本API代替这些包的全量对应。

真人已确认：旧 OpenRust MFA 代码和数据均不兼容；不建设旧数据迁移、重绑、接管或长期fallback。共享数据库、其他WIP与原始证据不删除。原Goal/main/20%保留与credits=false不变。

1. **框架官方基线**：固定Fortify v1.41.0 / `c456642584c102f6a6aafee9932299542da637b5` 的StatefulGuard+Session、三字段、Actions/Provider、login.id/login.remember及八份加密可重读恢复码原样对应；JSON不等于无Session。Sanctum Bearer PAT与SPA Cookie模式分开，Passport是OAuth2；自有JWT不冒称二者已完成。
2. **用户选中的应用profile**：根应用显式选择JWT及生成JWT API profile后全程Bearer，无Session middleware、临时Web Session、Cookie/CSRF-cookie/remember-cookie及隐式Set-Cookie。框架默认与其他生成模式不被该profile覆盖；不得擅自把JWT换成PAT。
3. **唯一二因素**：API密码验证后，仅在需要时返回短时/限次/一次消费且绑定原始ID、Guard、Provider与用途的challenge；通过同一Fortify状态/算法/Actions验证TOTP或恢复码后才颁发access/refresh。DB/缓存挑战不等于Web Session；此为明确API扩展，不是官方默认或Rust被迫适配，不复活另一MfaService/状态体系。
4. **管理与跨入口安全**：Bearer管理经受控近期密码/必要二次证明；复用同一Actions。JWT/PAT/refresh、Session/remember、OIDC/attach与资源逐项防绕过；必要版本/nonce/事务/取消/审计/限流能力按实际不变量经官方扩展保留，不按authority/proof字样一刀切。
5. **实际删除**：auth-mfa feature、MfaService、旧独立管理/挑战/表定义、旧生成profile、legacy hash/seed/adoption/floor/fallback及专用marker；不换名隐藏旧机制。旧攻击/并发/回滚/审计/期限/隔离测试迁到唯一新机制；非官方旧协议本身不再是兼容义务。
6. **有限退出**：源码/所有调用方/CLI与显式profile/唯一示例/文档收齐→冻结→main/default/all-feature/严格文档/MSRV/适用consumer、SQLite当前/3.26及受影响MySQL/PG、实际new/check/test、API完全无Cookie流程与官方Web分别实测、独立安全复审→脚本限定本地提交。Google真实客户端仍外部未验，不push/tag/release。

owner=root统一contract、Cargo/lock、迁移、共享文档、索引、构建和服务；worker按互斥文件交接后编码。本簇限定验收已收齐、本地提交以Git为准；删除前558文件归档及真人授权回执在`tmp/fortify-single-system-20261105/`，旧候选与失败/清理事实在`tmp/fortify-totp-alignment-20261104/`，不作为新体系已验。唯一停点：完成删除/现代化内核与显式API profile接线，不继续旧兼容矩阵。


## 路由

| 任务 | 入口 |
| --- | --- |
| 新方向与替代关系 | [ADR-001](decisions/adr-001-framework-principles.md)、[ADR 索引](decisions/README.md) |
| 宏观执行顺序 | [两阶段框架主线](plans/laravel-rebuild.md#两阶段框架主线)：阶段一固定Laravel+modules基线→阶段二已纳入通用增强，均属框架main；R0–R16为技术分解，原R15版本阶段取消，可选模块不自动开工 |
| 整体范围、阶段、验收 | [全面重构计划](plans/laravel-rebuild.md)、[模块正式矩阵](plans/laravel-modules-matrix.md) |
| Provider包发现后继 | [Provider编译期发现Spec](plans/laravel-rebuild.md#r2r13-provider-编译期发现-spec)；R2契约→R13工具，依赖R2-M/N；仅规划不切换当前HTTP |
| 当前实现资产 | [architecture.md](architecture.md)、core、Cargo.toml、源码和 Git |
| 原能力/生产验收输入 | 原 v1 计划、底层补齐、集群、原 DX |
| 编码、提交、证据 | [conventions.md](conventions.md)、[testing.md](memory/testing.md)、[verification.md](memory/verification.md) |

## 验证入口

每代码簇按 AGENTS.md 跑 fmt、workspace clippy/test/doc、cargo deny，及适用feature/MSRV、日常MySQL及后端专属定向、生成项目、真实服务门禁；发布前全后端矩阵另验；记录命令与实际结果。文档-only 只做 diff、链接/锚点和旧口径检查。不能把本轮规划检查写成代码或重构已验收。

当前重构来源与 102 章/749 子节当前纳入范围盘点见 [固定基线](plans/laravel-rebuild-baseline.md)；未将旧证据继承为新 contract 通过。
