# 开发者文档边界整理：docs/zh_CN/digging-deeper/contracts.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_CN/digging-deeper/contracts.md (迁移前)
+++ docs/zh_CN/digging-deeper/contracts.md (开发者正文)
@@ -11,4 +11,4 @@

 通过 `use …::Container as _;` 导入契约方法，避免遮蔽 `openrust::container::Container`。在 Provider 中注册实现，调用方解析 trait，而不是依赖具体类型。参阅[容器绑定](../architecture-concepts/container.md#绑定)和 [Provider](../architecture-concepts/providers.md)。

-Rust trait 实现替代 PHP 接口声明。没有自动运行时反射，需要在工厂中声明接口绑定及构造依赖。本页覆盖启动和容器基线使用的契约，其他契约文件可能仍是骨架，随所属模块验收。
+Rust trait 实现替代 PHP 接口声明。没有自动运行时反射，需要在工厂中声明接口绑定及构造依赖。各服务的公开契约和注册要求见对应功能指南。
````
