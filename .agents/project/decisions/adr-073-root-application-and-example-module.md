# ADR-073：根应用与唯一综合示例模块

- 状态：Partially Superseded；模块目标位置由 [ADR-074](adr-074-laravel-modules-directory.md) 改为 modules/，其他有效边界保留；下文 app/example 为旧决定
- 日期：2026-10-28
- 前置：[ADR-070](adr-070-laravel-first-rebuild.md)
- 替代：[ADR-072](adr-072-framework-and-application-projects.md) 的 demo 成员、默认成员与双应用目录决定

## 决策

OpenRust 用 Rust 语言实现 Laravel 框架，与其机制、概念、默认行为、生命周期、工作流和扩展方式高度吻合。框架与应用始终只在本仓库 main 开发，不新增源码仓库、开发分支或隔离 worktree。

根目录采用 Laravel 应用职责：app、bootstrap、config、database、public、resources、routes、storage、tests。根 Cargo package 为 openrust，同时保留框架 src/lib.rs 与薄 server 入口 src/main.rs；default-members=["."]、default-run="server"，根无参数 cargo run 启动唯一应用。

唯一贯穿框架功能的综合示例放在 app/example，作为普通业务模块展示开发者如何在 app/<业务>/ 调用公开框架 API。根唯一 server 经 app/mod.rs、bootstrap/app.rs 与 routes 显式装配；示例不是独立应用、package、bin 或额外运行模式，不新增 example Cargo feature。有效路由、任务、模型、认证、迁移和测试迁移后，删除原双应用及根 examples 目录；不维护第二份可运行示例应用或平行骨架。app/example 只保存实际需要的业务/公开扩展文件及其专属资源，可包含实际需要的资源目录，不复制完整框架职责树。根 bootstrap/config/routes/database/resources/storage 均为通用框架结构与加载/公开挂载，不存示例业务实现或样例特判。根 database 仅保留框架通用迁移结构与加载/公开挂载入口；示例业务专有迁移、种子、资源、协议、生成脚本与测试 fixture 归 app/example，通用验收测试按根 package targets 挂载模块。

工具保留独立编译边界，tests/consumers 保留公开 API 与真实拓扑验收职责。CLI new 的用户应用生成功能保留，模板是其唯一维护来源；本轮生成验证只使用 /tmp，不转化为额外长期仓库。

## Rust 适配与边界

除 app/example 外均为框架结构与代码。示例专有 migration/seed、模板、proto、代码生成脚本和测试 fixture 全部留在模块内；根 build.rs 不得因示例存在，根 Cargo 不因示例协议增加 build 依赖。示例通过公开 API 接入通用挂载点，不在核心加入样例名称、专属配置分支或测试回门。此前将示例 Migrator/模板放根的方案已由本条覆盖，实际归位以源码与验证为准。

数据库、授权、队列等沿用已有 feature 与应用配置启用真实服务；默认基础示例可启动，缺必需依赖时明确失败，不静默回退。未实现能力按后继计划交付，不为每项功能创建独立 demo。

框架核心不依赖示例业务；框架库、薄二进制和显式应用模块满足 Cargo 编译模型，不依赖运行时源码扫描。必要静态模块声明与 feature 保留 Rust 类型/所有权/async 优势，不改变 Laravel 的职责划分。根工作目录直接提供配置与资源，不再为示例切换到子应用目录。

本簇只收敛目录、应用装配与依赖路径，底层既有机制在对应职责保留并回归；目录完成不等于整个框架已与 Laravel 等价。旧 ADR 的真实验证是历史证据，不自动成为本布局通过。

## 模块封装与注册参考

最新正式定位为Laravel + laravel-modules = OpenRust；[nWidart/laravel-modules](https://github.com/nWidart/laravel-modules)不再只作目录习惯参考。模块包仍是独立第三方来源，固定版本/对应矩阵见[正式模块矩阵](../plans/laravel-modules-matrix.md)，其适用功能按统一Container/ServiceProvider实现，未盘点/未实现/未验收均保持缺口。该范围不自动增加其他生态产品。同仓 main 与资源内聚约定继续有效；本 ADR 原定 app/<模块> 位置已由 ADR-074 的 modules/<模块> 目标取代。

采用模块自有 controller/model/resource、路由/API、迁移与测试，并以业务 provider 公开装配。本 ADR 当时约定 app/<模块>；该历史位置已由 ADR-074 取代。snake_case、Rust 编译期模块声明与静态注册是必要语言/编译模型适配。当前 Plugin 只证明显式挂载，不能当成模块自动发现、启停、依赖/顺序等功能已实现。模块依赖/顺序与统一 ServiceProvider 归 R2，模块生成/管理和配套开发体验归 R13，未实现项保留待办。

开发者可修改 app/mod.rs 中的编译期业务模块声明与应用注册清单，通过既有 Plugin/Routes 契约挂载模块自有路由、API、资源和迁移；这属于业务接入。不得为了增加业务修改 src/ 框架核心、加入全局样例特判或复制框架内核。允许薄业务 Plugin/provider 正常调用框架，不依赖运行时扫描 Rust 源码。具体公开注册方法以实际代码为准，未定 API 不写为已存在。

## 模块内职责约定

mod.rs 仅声明与导出接入；providers 承担业务公开注册，routes/api.rs 与 web.rs 定义路由，http/controllers 处理请求，http/requests 做输入/校验，http/resources 做输出，models 保存实体，services 仅用于实际需要的业务编排。database/migrations 与 seeders、resources/views、jobs/events/listeners/notifications、extensions、tests 按实际实现创建；proto/generated 仅实际 gRPC 示例需要时保留。

此为获准的整理约定，不等于所有目录当前已有实现；具体树与命令以源码确认后的模块 README 为准。不造空层，不复制 app/bootstrap/main/Cargo 完整骨架。当前 provider 使用真实 Plugin 接口；R2 再统一 Container/ServiceProvider 的 register/boot，不伪称已实现。

## 业务依赖边界

普通 app/example 模块没有独立 Cargo 依赖图。根同 package 的 lib/server 共享依赖解析，业务新增包可修改根应用清单；优先继承 workspace 已登记包，不擅改框架版本/default feature 或批量更新锁图。仅测试使用 dev-dependencies，package alias 不隔离 feature 或消除冲突；清单/锁图与 feature 来源须实际检查。不因教学新增生产依赖、根示例 build.rs/build 依赖或 patch 核心包。

同仓独立 package 的更强隔离仅可在后续明确依赖方向后设计，不能让 root lib 与示例互相依赖形成环；本簇不实施第二套架构，也不把“无需改框架源码”解释成不能改应用依赖/注册清单。

## 综合示例覆盖

综合示例首先交付一个真实业务实体的完整 CRUD（列表/详情/新增/更新/删除），含校验、分页、未找到、授权与事务，并在 PostgreSQL/MySQL 各验证真实 HTTP 闭环；常量基础路由不算 CRUD。综合示例须贯穿当前已实现的公开能力，不能缩减为基础路由。逐项列出 app/example 的实际业务调用或公开扩展代码、通用根挂载及模块专属配置/迁移/资源、测试及外部条件；只给文档链接或 tests/consumers 不构成示例覆盖。公开 Plugin/driver 扩展按现有 contract 展示，不复制框架内核，也不新增平行机制。

当前代码与验收仍在迁移，缺项如实保留。后继新增能力同时补入该综合模块和测试，作为每功能簇完成定义；尚未实现能力按总计划推进，不承诺现已全框架等价。

## 验收

完整 workspace 默认/all-features fmt、clippy、test、doc、deny，适用 MSRV/no-default、公开 consumers、PG18/MySQL8.4 真实集成均重新执行。根无参数 cargo run 须验证配置/公开资源、真实 HTTP 和正常退出；唯一综合示例及 /tmp 三类生成应用按支持行为验证。结构子闭环可先验证并本地提交，尚缺的综合 CRUD 与能力覆盖继续保留活跃任务，不因此宣称整个功能簇完成。实际结果只写 [测试基线](../memory/testing.md) 与 [验收口径](../memory/verification.md)，不同模型只读审查后由集成负责人本地提交。
