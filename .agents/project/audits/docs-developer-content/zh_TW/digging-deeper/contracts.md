# 开发者文档边界整理：docs/zh_TW/digging-deeper/contracts.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_TW/digging-deeper/contracts.md (迁移前)
+++ docs/zh_TW/digging-deeper/contracts.md (开发者正文)
@@ -11,4 +11,4 @@

 透過 `use …::Container as _;` 匯入契約方法，避免遮蔽 `openrust::container::Container`。在 Provider 中註冊實作，呼叫方解析 trait，而不是依賴具體類型。參閱[容器綁定](../architecture-concepts/container.md#綁定)和 [Provider](../architecture-concepts/providers.md)。

-Rust trait 實作替代 PHP 介面宣告。沒有自動執行時反射，需要在工廠中宣告介面綁定及建構依賴。本頁涵蓋啟動和容器基線使用的契約，其他契約檔案可能仍是骨架，隨所屬模組驗收。
+Rust trait 實作替代 PHP 介面宣告。沒有自動執行時反射，需要在工廠中宣告介面綁定及建構依賴。各服務的公開契約和註冊要求見對應功能指南。
````
