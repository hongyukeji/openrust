# 擴充套件開發

## 安裝與服務提供者

將擴充套件加入 Cargo 依賴，再將其 Provider 加入 `bootstrap/providers.rs`。OpenRust 將 Provider 和資源註冊函式編譯進應用程式，由應用程式在組裝時明確選擇 Provider。

在 Provider 的 `register` 中繫結服務，並呼叫 `mergeConfigFrom(app, defaults, key)` 合併套件預設值，應用程式設定覆寫第一層的同名鍵。使用 `replaceConfigRecursivelyFrom(app, defaults, key)` 保留巢狀預設值，陣列按索引由應用程式覆寫。預設值使用 JSON 物件或陣列，設定鍵使用應用程式的點號路徑。

## 資源

在 Provider 的 `boot` 中向應用程式註冊資源。

| 方法 | 資源 |
| --- | --- |
| `loadRoutesFrom` | 使用 `Route` 宣告路由的編譯型函式。應用程式先註冊自己的路由，再註冊套件路由。 |
| `loadViewsFrom` | 檢視目錄和命名空間。使用預設檢視根目錄時，`probe::welcome` 優先讀取應用程式的 `resources/views/vendor/probe/welcome.html`，缺少時讀取套件檢視。 |
| `loadTranslationsFrom` | 翻譯目錄和命名空間。`lang/vendor/probe/en/messages.json` 可覆寫 `probe::messages.title` 的英文內容，其餘鍵繼續回退至套件翻譯。 |
| `loadMigrationsFrom` | 靜態的編譯型 `MigrationEntry` 切片。Artisan 使用應用程式的遷移紀錄執行和回復套件遷移。 |
| `commands` | 使用 `Artisan` 註冊命令的編譯型函式，由主控台核心啟動時呼叫。 |

透過啟動時載入的 Provider 註冊套件 HTTP 路由，確保路由組裝時能夠讀取。部署時隨應用程式散佈檢視和翻譯檔，並將發佈後的目錄傳給 Provider。可執行的宿主與 Provider 請見[獨立 crate 驗收範例](https://github.com/hongyukeji/openrust/tree/master/vendor/openrust/framework/tests/fixtures/package)。

OpenRust 使用 Cargo 明確安裝擴充套件並註冊 Provider。自動探索和 Artisan 資源發佈尚待接入。
