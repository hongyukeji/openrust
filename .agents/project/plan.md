# 当前计划

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
