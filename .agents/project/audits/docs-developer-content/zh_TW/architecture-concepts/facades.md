# 开发者文档边界整理：docs/zh_TW/architecture-concepts/facades.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_TW/architecture-concepts/facades.md (迁移前)
+++ docs/zh_TW/architecture-concepts/facades.md (开发者正文)
@@ -29,7 +29,7 @@

 框架為應用引導、每個 HTTP 請求和每次 Artisan 呼叫設定門面作用域。同一應用中的巢狀執行共用作用域，不同應用彼此隔離。新請求或命令使用新的門面快取；註冊為單例的服務仍屬於該應用的容器。應用程式碼自行建立的 Tokio 任務不會自動繼承門面作用域，應向這些任務傳入已解析服務。

-`Route::get` 與 `Artisan::command` 在路由模組的 `register()` 執行期間使用專用註冊作用域，屬於已交付的註冊門面。其他門面類型宣告存在，不代表對應服務已經實作。Mocking 輔助介面和解析回呼由後續單元交付。
+`Route::get` 與 `Artisan::command` 在路由模組的 `register()` 執行期間使用專用註冊作用域。服務門面須在應用程式生命週期內使用，並先註冊對應服務。

 未載入的延遲 Provider 或非同步工廠使用 `getFacadeRootAsync().await`。它直接走一次非同步容器解析，等待就緒並共用目前應用的門面快取；期間出現較新的 `swap` 或其他已快取根物件時保留該值。同步解析未就緒的 Provider 或尚未建構的非同步工廠會回傳型別化的非同步解析錯誤。

````
