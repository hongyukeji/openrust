# OpenRust — Laravel 的 Rust 实现

OpenRust 面向熟悉 Laravel 的 Rust 开发者，以 Laravel 的概念、默认行为与开发流程为目标，保留 Rust 类型、所有权、异步和 Cargo 工具链。Laravel Modules 是默认集成的第三方扩展，单独介绍模块组织与状态管理。

本指南先介绍运行应用、配置、职责目录、启动与 API 路由，再介绍模块扩展。主导航沿用固定 Laravel 13 文档的章节分组和阅读顺序，仅列已有实质内容；其余能力将随实现补充。当前框架仍在重构，不能据此视为完整 Laravel 等价实现。

从[安装与运行](getting-started/installation.md)开始。API 类型和方法的细节查阅 Rustdoc；各框架模块 README 保留更细的当前接口说明。
