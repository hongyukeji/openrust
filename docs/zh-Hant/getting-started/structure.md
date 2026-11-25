# 目錄結構

## 應用程式目錄

根目錄的 `main.rs` 連接 Tokio 和 `bootstrap::create_application`。`Cargo.toml` 與 `Cargo.lock` 宣告應用程式、工作區和鎖定的相依套件。應用程式程式碼位於 `app/`，編譯型設定載入器位於 `config/`，Web 和主控台路由宣告位於 `routes/`。

框架原始碼位於 `vendor/openrust/framework/`。檢視位於 `resources/views/`，網站檔案位於 `public/`，公開儲存磁碟位於 `storage/app/public/`。測試放在根應用程式或所屬 crate 的 `tests/` 目錄。

## 模型、持久化與服務

`app/models/model.rs` 提供應用程式模型使用的 Eloquent `Model` trait。參照 `User` 實作欄位映射、屬性轉換及關聯，並按需覆寫預設方法。產生的資料表和欄位名稱常數放在模型頂部附近。

`app/repositories/repository.rs` 提供 `EloquentRepository<M>` 和 `Repository`。用 `EloquentRepository::<M>::new()` 處理常見的新增、查詢、更新及刪除。客製化時，實作 `Repository` 並宣告 `type Model = M`，按需覆寫 `query` 或個別操作。`UserRepository` 示範加入具名查詢。分頁時將已篩選的 Builder 傳給 `paginate`。查詢條件不會自動寫入 `create` 的屬性。`update` 和 `delete` 處理呼叫方傳入的模型，呼叫前應在授權範圍內選定紀錄。

`app/services/service.rs` 提供 `TransactionalService`。實作 `connection` 選擇業務使用的連線，再呼叫 `transaction` 將相關寫入放進同一交易。`UserService` 示範建構函式注入，以及在模型使用的資料庫連線上執行交易更新。控制器、命令和工作可於授權與輸入驗證後呼叫同一服務。

## Bootstrap 目錄

應用程式的 `bootstrap/` 目錄存放建立應用程式所需的裝配宣告。

`bootstrap/app.rs` 宣告設定、路由、中介軟體和命令入口。`bootstrap/mod.rs` 向二進位入口匯出 `bootstrap::create_application`。`bootstrap/providers.rs` 列出應用程式的[服務提供者](../architecture-concepts/providers.md#註冊服務提供者)。

裝配入口透過 `with_routing` 宣告 Web 路由、控制台路由和健康路徑。Rust 使用 `Routing` Builder 表達這些選項，並透過 `with_config`、`with_providers` 和 `with_commands` 註冊編譯進應用程式的設定、Provider 和命令。

框架使用這些宣告的順序見[請求生命週期](../architecture-concepts/lifecycle.md)。
## 應用程式建構鉤子

`Application::configure` 支援 `with_bindings::<T, _>`、`with_singletons::<T, _>`、`with_scoped_singletons::<T, _>` 工廠。按 Rust 型別逐項鏈式註冊，工廠傳回 `Result<Arc<T>, BindingResolutionError>`。一般綁定每次解析，單例屬於應用程式，作用域單例屬於目前請求或佇列工作。

`registered`、`booting`、`booted` 圍繞 Provider 啟動依序執行，成功啟動後重複 boot 不再執行。鉤子失敗會停止後續階段並傳回啟動錯誤。`with_schedule` 延遲登記排程設定，不在應用啟動時執行工作。`prefers_json_responses(true)` 在 booted 階段啟用寬泛 Accept 標頭的 JSON 偏好，顯式例外 JSON 策略仍優先。
