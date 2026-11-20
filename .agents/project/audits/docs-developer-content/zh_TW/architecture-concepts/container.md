# 开发者文档边界整理：docs/zh_TW/architecture-concepts/container.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_TW/architecture-concepts/container.md (迁移前)
+++ docs/zh_TW/architecture-concepts/container.md (开发者正文)
@@ -103,7 +103,7 @@

 `forgetScopedInstances()` 清除目前生命週期的作用域快取，保留其他活動生命週期及普通單例。在執行作用域之外呼叫，則清除根作用域實例。`forgetInstance(key)` 仍按原始鍵操作；`forgetInstances()` 同時清除目前作用域實例及應用的普通實例快取。作用域服務的 `instance` 覆蓋僅在目前生命週期生效，重新綁定仍是應用級註冊操作。

-容器清理不會清除門面獨立快取的根物件；需要在同一次執行中重新解析時，同時清除門面快取。應用自行建立的 Tokio 任務不繼承環境作用域，應顯式傳入服務，或為任務設定所需生命週期。串流本文可以透過持有已解析服務的 `Arc` 延續其使用。串流及終止回呼接線仍屬於 HTTP 交付單元。
+容器清理不會清除門面獨立快取的根物件；需要在同一次執行中重新解析時，同時清除門面快取。應用自行建立的 Tokio 任務不繼承環境作用域，應顯式傳入服務，或為任務設定所需生命週期。串流本文可以透過持有已解析服務的 `Arc` 延續其使用。

 ## 上下文綁定與標籤服務

@@ -148,7 +148,7 @@

 ## 容器解析事件

-`beforeResolving`、`resolving` 和 `afterResolving` 用 `None` 登記全域回呼，用 `Some(ServiceKey)` 選擇服務身分。登記時正規化別名。回呼接收 `ResolutionEvent` 與目前容器，傳回 `Result<(), BindingResolutionError>`。實作提取舊版的事件檢視與有序選擇器設計，適配目前同步工廠。
+`beforeResolving`、`resolving` 和 `afterResolving` 用 `None` 登記全域回呼，用 `Some(ServiceKey)` 選擇服務身分。登記時正規化別名。回呼接收 `ResolutionEvent` 與目前容器，傳回 `Result<(), BindingResolutionError>`。

 執行順序為全域 before → 指定 before → 建構 → 全域 resolving → 指定 resolving → 全域 after → 指定 after。before 回呼可以登記尚未綁定的服務，每次查找都執行，快取命中也執行。明確提供的實例及共享快取命中略過建構後回呼。每階段先取得全域回呼快照，全域執行完再讀取指定回呼；因此全域回呼中新登記的指定回呼可以參與目前階段，登記到後續階段的回呼也可在本次建構中執行。

@@ -468,7 +468,7 @@

 標籤或計算型集合工廠沿用既有型別化集合契約：`needs::<Vec<Arc<T>>>().giveTagged(tag)` 或 `giveTaggedAsync(tag)`，可變參數建構式可使用該集合。`T` 的直接情境綁定優先於集合轉接。集合工廠作為自己的解析執行，項目不會繼承消費者類別的情境綁定；直接 `giveServices` 陣列則保留該類別情境。集合回呼使用 `Vec<Arc<T>>` 身分，這是 Rust 清單輸出契約的轉換。

-原始值的 `$name` 情境可提供 `Vec<Arc<T>>` 或 `Vec<ParameterValue>`。例如 `needsNamed::<Vec<Arc<String>>>("$names").giveConfig("app.names", None)` 讀取目前設定陣列；框架啟用 Serde 的 `rc` 特性支援這些型別化清單。共用項目無需實作 Clone，異質載體保留 null 並逐項驗證。所有權表示有參數包裝與參考計數成本，未宣稱零成本或數值效能收益。
+原始值的 `$name` 情境可提供 `Vec<Arc<T>>` 或 `Vec<ParameterValue>`。例如 `needsNamed::<Vec<Arc<String>>>("$names").giveConfig("app.names", None)` 讀取目前設定陣列；框架啟用 Serde 的 `rc` 特性支援這些型別化清單。共用項目無需實作 Clone，異質載體保留 null 並逐項驗證。所有權表示有參數包裝與參考計數成本。

 原始值情境閉包在建構情境內直接執行，對應 Laravel 的原始值解析器；保留建構參數快照，不為 `$name` 增加服務事件、快取或解析歷史。閉包內的普通相依服務仍按容器機制解析，非同步閉包可在相同情境等待初始化。

@@ -554,7 +554,7 @@

 實際可呼叫的物件或靜態方法，只在邏輯建構堆疊尚無該類別時加入類別情境。`Class@method` 按 Laravel BoundMethod.callClass 保留父情境；相依仍建立自己的正常建構情境。逐次非同步 poll 與取消時恢復情境，確保並行與多應用隔離。Application 入口同時建立該應用的 Facade 情境。進行中的呼叫保留選定綁定、原始參數與接收者，直到完成或取消。

-編譯型別 `C` 替代 PHP 類名反射；Rust trait 別名與具體類別間不自動投影方法。介接器以 `Result<R, BindingResolutionError>` 傳播注入錯誤，`R` 本身可為業務 Result。為支援 Rust 1.94 的非同步借用，儲存回呼及呼叫分派 Future 使用 Box。未做效能基準，不宣稱零配置。本節尚不包含參數屬性或 SelfBuilding。
+編譯型別 `C` 替代 PHP 類名反射；Rust trait 別名與具體類別間不自動投影方法。介接器以 `Result<R, BindingResolutionError>` 傳播注入錯誤，`R` 本身可為業務 Result。為支援 Rust 1.94 的非同步借用，儲存回呼及呼叫分派 Future 使用 Box。

 ## 延遲閉包

@@ -597,7 +597,7 @@
 }
 ```

-`Callback::reusable` 向 Rust 1.94 的閉包推斷提供可重複執行的 `Fn` 契約，一般呼叫的 `Callback::new` 繼續支援 `FnOnce`。包裝回呼需要 `Send + Sync`，可變共享狀態使用同步機制。回傳閉包借用 Container/Application，不能比它存活更久。每次回傳獨立 Send Future，可回傳外部借用值。具體閉包型別避免包裝器本身的 Box，共享回呼使用 Arc，非同步分派保留前述 Box Future 介接；不宣稱實測效能收益。
+`Callback::reusable` 向 Rust 1.94 的閉包推斷提供可重複執行的 `Fn` 契約，一般呼叫的 `Callback::new` 繼續支援 `FnOnce`。包裝回呼需要 `Send + Sync`，可變共享狀態使用同步機制。回傳閉包借用 Container/Application，不能比它存活更久。每次回傳獨立 Send Future，可回傳外部借用值。具體閉包型別避免包裝器本身的 Box，共享回呼使用 Arc，非同步分派保留前述 Box Future 介接。

 ## 綁定目錄與清理

@@ -643,7 +643,7 @@

 應用自行持有的物件及原工廠快照仍有效，已准入建構可回傳舊值，但不能回填已移除或替換的綁定及其歷史。此基礎容器操作不重新啟動 Application Provider，也不清除獨立 Facade 快取；應用終止由生命週期職責負責。

-Rust 使用型別化定義與擁有所有權的 ServiceKey 替代動態 PHP 陣列。快照查詢按定義數量線性查找，屬於明確檢查，不加入一般請求解析路徑。為支援並行請求的全域 flush，每個生命週期在登記/註銷時以短 Mutex 操作維護 Weak 參考，flush 遍歷活動記錄。讀取已預熱共享綁定僅保留配方，不保留快取物件。未做效能基準，不宣稱零成本。
+Rust 使用型別化定義與擁有所有權的 ServiceKey 替代動態 PHP 陣列。快照查詢按定義數量線性查找，屬於明確檢查，不加入一般請求解析路徑。為支援並行請求的全域 flush，每個生命週期在登記/註銷時以短 Mutex 操作維護 Weak 參考，flush 遍歷活動記錄。讀取已預熱共享綁定僅保留配方，不保留快取物件。

 ## 環境解析回呼

@@ -681,7 +681,7 @@

 回呼在登錄表鎖外執行，可以重入或替換解析器；進行中的呼叫保留原回呼直到回傳。舊捕獲在鎖外釋放，錯誤與 panic 不污染登錄表鎖。更換或清除解析器不驅逐服務快取、不觸發解析事件，`flush` 保留解析器，各容器策略獨立。

-Rust 用借用查詢列舉、結果列舉、Result 與可選 Arc 回呼對應 PHP 聯合型別、例外和可空 callable。回呼要求 Send + Sync，以支援同容器並行請求。環境檢查僅在短讀鎖內複製回呼參考，未做效能基準。
+Rust 用借用查詢列舉、結果列舉、Result 與可選 Arc 回呼對應 PHP 聯合型別、例外和可空 callable。回呼要求 Send + Sync，以支援同容器並行請求。環境檢查僅在短讀鎖內複製回呼參考。

 應用在載入設定時安裝預設比對器。參見[確定目前環境](../getting-started/configuration.md#determining-the-current-environment)。

````
