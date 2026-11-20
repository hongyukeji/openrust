# 开发者文档边界整理：docs/en/digging-deeper/contracts.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/en/digging-deeper/contracts.md (迁移前)
+++ docs/en/digging-deeper/contracts.md (开发者正文)
@@ -11,4 +11,4 @@

 Import a contract as `use …::Container as _;` to call its methods without shadowing `openrust::container::Container`. Register an implementation through a provider and resolve the trait rather than coupling callers to a concrete type. See [container binding](../architecture-concepts/container.md#binding) and [providers](../architecture-concepts/providers.md).

-Rust trait implementations replace PHP interface declarations. There is no automatic runtime reflection: declare the trait binding and constructor dependencies in a factory. This page covers the contracts used by the startup and container baseline. Other contract files may still be skeletons and are accepted with their respective modules.
+Rust trait implementations replace PHP interface declarations. There is no automatic runtime reflection: declare the trait binding and constructor dependencies in a factory. Consult each service guide for its public contracts and registration requirements.
````
