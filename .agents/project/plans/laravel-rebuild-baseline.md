# Laravel 重构固定基线

> 受众：设计、实现和验收 Agent
> 更新：2026-10-28
> 摘要：R0 冻结来源、完整文档导航和现有资产；实现状态由 todo.md 管理。

## Spec

以 Laravel 13.x 导航和源码为行为基线，Goravel 为跨语言参照。
逐章拆出二级节及其三级行为，覆盖核心、测试、AI 与官方包，不以 C01–C33 代替。
现有源码、公共 consumer 与真实服务记录作为资产保留，不继承为新 contract 通过。
目录、默认启动和分发先在 R1 改；Container 等公共语义留后继，避免搬迁时混入改义。
验收为导航章节/节数量一致、源码路径存在、相对链接与 diff 检查；不运行代码门禁。

## 固定来源

以下版本由 GitHub 官方 release/commit API 于 2026-10-28 核对；日期为上游 UTC 日期。

| 来源 | 版本、固定 commit | 日期与用途 |
| --- | --- | --- |
| [Laravel 文档](https://laravel.com/docs/13.x) | `13.x` / [`960eedf0911155ad16326ade49c0ea5d18eda0b8`](https://github.com/laravel/docs/tree/960eedf0911155ad16326ade49c0ea5d18eda0b8) | 2026-10-27；完整 documentation.md 导航 |
| [Laravel 应用](https://github.com/laravel/laravel/releases/tag/v13.11.0) | `v13.11.0` / `06d016a364a37430eec9cbc52209adce3d7667ce` | 2026-10-06；app/bootstrap/config/database/routes/resources/public/storage/tests |
| [Laravel 框架](https://github.com/laravel/framework/releases/tag/v13.39.0) | `v13.39.0` / `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181` | 2026-10-27；src/Illuminate 与 tests 为各领域实现/回归锚点 |
| [Goravel 框架](https://github.com/goravel/framework/releases/tag/v1.18.0) | `v1.18.0` / `a554ced468cada50a269780692a0491e5f9e3a11` | 2026-07-05；contracts、foundation、route、database、auth、queue 等跨语言实现 |
| [Goravel 目录说明](https://www.goravel.dev/getting-started/directory-structure.html) | 2026-10-28 参考入口；不作为浮动新增范围来源 | Rust 编译期模块与独立工具边界优先 |

Laravel 与 Goravel 仅参考机制；复制代码需逐文件核对 MIT/原许可。矩阵保留简短章节标识和自写决策，不复制文档正文。

## 模块化正式范围

最新定位为Laravel + laravel-modules的Rust 1:1实现。Laravel原固定版本保持，上游模块包另冻v13.2.0/commit `59714840708a2aece19e810764473f53227c35fe`，官方v13文档导航于2026-10-28读取，来源与35个入口见[模块矩阵](laravel-modules-matrix.md)。目前仅登记导航/源码入口，子行为尚待逐项盘点，不声称模块包已经实现或全量取证。

## 对应关系、差异分类与证据

749个Laravel稳定ID保留现状，不因定位更新批量改状态或假填证据。每ID后续补齐：固定上游章节/源码/测试→Rust公开API/配置与操作→等价行为测试→modules/example实际业务/扩展→文档→适用真实服务。必要细项可附同ID子表，不能用旧泛化“适配/不适用”文字直接豁免核心能力。

差异统一按[总计划](laravel-rebuild.md#逐项对应与差异分类)分类；分类不等于已实现/已验收。语义等价、必要Rust差异、用户约定、授权增强、范围外与未闭合缺口分别说明。无法等价或无真实验收继续保持缺口，不能声称严格1:1已完成。

## 逐章行为矩阵

当前纳入范围为 **102 个唯一章节、749 个二级节**；每条涵盖其纳入的三级节，ID 沿用固定上游章节序号/节序号，不重新编号。源码来源链接指向固定文档 commit。

- [矩阵一](laravel-rebuild-matrix-1.md)：导航前 34 章。
- [矩阵二](laravel-rebuild-matrix-2.md)：第 35–68 章。
- [矩阵三](laravel-rebuild-matrix-3.md)：第 69–102 章。
- [现有源码/测试/调用方资产](laravel-rebuild-assets.md)：精确路径清单，引用原真实服务记录。

所有行为初始为“待真实验收”：表示已盘点而未在新 contract 验收，不能解读为已有实现通过。R1–R16 各功能簇须把适用条目改成 contract 已定、实现中、已验收或外部阻塞并写测试名/命令。矩阵的领域资产入口指向当前事实；现有行为的具体测试名/路径见资产表与 core，目标测试在 Spec 定稿。不能为关闭盘点而虚构未写的测试名。

## 文档导航外的官方生态

| 能力 | 归属、选择与依赖 | 验收与外部条件 | 状态 |
| --- | --- | --- | --- |
| Nova 资源管理/授权/筛选/指标 | R16 独立管理包，复用 R15 UI；不复制闭源实现 | 授权、CRUD、过滤/指标真实 API 与浏览器 | 待真实验收 |
| Cloud/Forge/Vapor 部署、域名、进程/环境、扩缩容 | R14 部署/遥测接口和自托管替代；商业运营平台本身不适用 | 隔离部署/回滚/共享状态；外部云账户须授权 | 待真实验收 |
| Nightwatch 追踪/异常/请求分析 | R12 通用遥测、R15 视图；托管运营平台本身不适用 | 真实 Collector、脱敏、采样、资源上限 | 待真实验收 |
| SQL Server/MariaDB/MongoDB 驱动 | R16 可选 driver；R4/R5 contract 后逐驱动实现；不以 SQL 方言兼容声称支持 | 每后端 ORM/auth/queue/事务完整测试；服务条件未准备 | 待真实验收 |
| Livewire/Inertia/Vue 前后端体验 | R9 视图状态/组件/资产，R15 Vue；PHP 解释器/字节兼容不适用 | 表单、验证、授权、历史/导航与浏览器流程 | 待真实验收 |
| AI SDK/MCP/Boost | R16 独立通用适配包纳入范围，基于 R11 HTTP/process/context；不启动产品平台 | 结构化输出/流/工具调用/MCP 生命周期；Provider 凭据缺口分开 | 待真实验收 |
| Cashier Stripe/Paddle、Passport | R16 独立支付订阅/OAuth2 授权服务器；领域不进核心 | sandbox webhook 验签、重放、撤销、幂等；生产扣款无授权 | 待真实验收 |

## 破坏性共享 contract 与旧规则

[ADR-070](../decisions/adr-070-laravel-first-rebuild.md) 是替代清单的权威入口：001 参考顺序，006/010/016 命名与 CLI，007/021/039/062–065 目录/分发，008 HTTP 包装，018 JWT-first，023 扩展，030/068/069 配置/路由/模板。R1 仅定目录/分发；R2 定 Container/Provider、scope、配置与停机；R3 定 HTTP 默认/错误，R7 定 guard。旧 ADR 的目标约束被替代，行为证据和 WIP 保护不失效。

依赖顺序保持总计划 R0→R1→R2；数据在 R4、Model R5、SQLite R6、安全 R7 后，再收敛共享 store、异步事务、网络和集群。默认 store/Session/认证/响应不能在目录迁移时偷偷重写。每项领域测试必须覆盖正常、错误、取消/停机、竞争、资源上限与适用服务故障/恢复。

## 盘点验证边界

R0 只做文档与路径检查。原 evidence 的双库、Redis、SMTP、OIDC、S3/gRPC/实时记录保持原文；本轮未执行，不表述为新架构通过。完整复制、负载均衡、多 API/worker/scheduler、最低版本、SQLite、供应商和真人 Laravel 走查仍是后继门禁。
