# 契約

契約獨立於具體實作定義服務介面。Rust 中使用 `openrust::contracts` 下的 trait，介面服務在容器中使用 `Arc<dyn Trait>` 綁定。

| 職責 | 匯入路徑 |
| --- | --- |
| 服務綁定與解析 | `openrust::contracts::container::Container` |
| 容器錯誤 | `openrust::contracts::container::BindingResolutionError` |
| 應用服務與引導狀態 | `openrust::contracts::foundation::Application` |
| 設定儲存庫 | `openrust::contracts::config::Repository` |

透過 `use …::Container as _;` 匯入契約方法，避免遮蔽 `openrust::container::Container`。在 Provider 中註冊實作，呼叫方解析 trait，而不是依賴具體類型。參閱[容器綁定](../architecture-concepts/container.md#綁定)和 [Provider](../architecture-concepts/providers.md)。

Rust trait 實作替代 PHP 介面宣告。沒有自動執行時反射，需要在工廠中宣告介面綁定及建構依賴。本頁涵蓋啟動和容器基線使用的契約，其他契約檔案可能仍是骨架，隨所屬模組驗收。
