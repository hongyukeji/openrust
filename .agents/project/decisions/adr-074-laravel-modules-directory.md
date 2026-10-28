# ADR-074：Laravel 与 laravel-modules 的统一框架及模块目录

- 状态：Accepted；目标已确认，目录迁移与模块能力尚未验收
- 日期：2026-10-28
- 前置：[ADR-070](adr-070-laravel-first-rebuild.md)
- 替代：ADR-073 中 `app/<模块>`、`app/example/` 的目标位置；其根布局、公共接入、资源隔离及测试保护继续有效

## 决策

**OpenRust = Laravel + laravel-modules**。固定上游的框架功能、可观察行为及模块化开发机制共同构成实现目标，使用一个 Container/ServiceProvider 内核。Goravel 提供 Rust 跨语言设计的参考，不替代 Laravel 行为标准。1:1 的范围与差异分类以[总计划](../plans/laravel-rebuild.md)为准。

框架、应用宿主与业务模块只在本仓库 `main` 开发。根保留 Laravel 的 app/bootstrap/config/database/public/resources/routes/storage/tests 职责及 Rust 的 src/tools/Cargo；不创建另一仓库、开发分支、worktree、独立 demo 应用或根 examples 目录。根 package 提供框架 library 和唯一默认 server，根 `cargo run` 启动应用。

业务统一在 `modules/<模块名>/`，唯一综合示例为 `modules/example/`。根 `app/` 保留通用应用级装配，不再承载示例业务。模块不是完整应用骨架，无自己的 server 或一套容器；模块中的 controllers/models/routes/providers/database/resources/tests 是业务职责，并非复制框架实现。仅创建有实际代码的子目录。

模块专有配置、迁移、种子、视图、静态源资源、协议、生成脚本、测试 fixture 和自定义 driver 实现留在模块中；根只提供通用机制和公开注册入口。不得为示例增加根 build.rs、构建依赖或业务特判。框架核心不得依赖 Example 类型。

开发者新增模块只需编写模块代码、声明编译入口并更新应用 Provider/模块清单；这些是应用配置，不是修改框架核心。公开契约须覆盖模块路由、配置、资源、迁移、命令/任务等接入。ServiceProvider 按全体 register → 配置定稿 → boot 收敛，当前 Plugin 不能冒充已具备完整生命周期。

Rust 模块编译发现与运行时启用是不同概念。必须明确静态声明/生成注册、依赖与顺序、错误与停机行为；不得宣称无需编译即可加载任意 Rust 源码。启停的作用时点、资源释放及禁用后路由不可见由后继合同和测试确定。

第三方依赖由 Cargo 管理。普通业务模块没有独立依赖解析，Provider 也不隔离版本或 features；教程须展示清单声明、最小 features、锁图审查与框架回归。需要 package 隔离时先证明依赖单向且仍同仓，不通过空 Cargo.toml 或循环依赖伪装隔离。

## 综合示例验收

1. 一个真实实体的完整 CRUD，含输入校验、分页、404、授权、事务与 PostgreSQL/MySQL HTTP 闭环。
2. 当前框架每项公开能力都有可定位的实际调用或扩展代码、配置入口和测试；后继能力同簇补入示例。
3. 开发者参考示例创建临时第二业务模块，无需修改框架 src，即可注册路由/使用服务；测试 fixture 不作为第二长期示例。
4. 模块注册/未注册、配置合并与冲突、资源命名空间、依赖错误等均有测试，不能用移动目录替代。
5. 外部供应商、复制和集群要求真实证据；未实现或缺外部条件保持缺口。

## 当前事实与实施顺序

本决策写入时源码仍在 `app/example/`，存在未提交迁移 WIP，不能宣称 `modules/example/` 已运行。先保护并核对现有根结构与证据，再按[活跃任务](../todo.md)完成 R2 基础、R2-M 模块最小闭环与目录迁移、R1-E CRUD/已有能力示例，后继框架能力与示例同步扩展。旧路径的历史证据保留原含义，不改写 Git 历史，不直接计作新布局验收。
