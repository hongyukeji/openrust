# 擴充套件開發

## 安裝與服務提供者

將擴充套件新增為 Cargo 相依套件。OpenRust 按目前目標與特性探索已宣告的套件。需要明確裝配時，將 Provider 加入 `bootstrap/providers.rs`。

在 Provider 的 `register` 中繫結服務，並呼叫 `mergeConfigFrom(app, defaults, key)` 合併套件預設值，應用程式設定覆寫第一層的同名鍵。使用 `replaceConfigRecursivelyFrom(app, defaults, key)` 保留巢狀預設值，陣列按索引由應用程式覆寫。預設值使用 JSON 物件或陣列，設定鍵使用應用程式的點號路徑。

## 套件自動探索 {#package-discovery}

擴充套件透過 Cargo 資訊清單宣告探索資訊，並匯出 `openrust_providers() -> Vec<Box<dyn openrust::support::ServiceProvider>>`。下例也匯出一個型別化的 Rust 別名。

```toml
[package.metadata.openrust.discovery]
version = 1
providers = true
aliases = { ExampleFacade = "facades::Example" }
```

OpenRust 應用程式在 `build.rs` 呼叫 `openrust_package_discovery::generate()`，透過 `withPackages` 裝配產生的清單。Provider 工廠和別名都會編譯。部署後的執行檔可直接使用已連結的清單啟動。

`bootstrap/packages.rs` 透過 `manifest()` 提供編譯後的套件清單，應用入口呼叫 `.withPackages(super::packages::manifest())`。應用程式自身的 Provider 仍在 `bootstrap/providers.rs` 登記；無需編輯產生的檔案或手動列出自動發現的套件。 編譯後的 Rust 別名透過 `crate::bootstrap::packages::package_aliases` 存取，可用項目隨啟用的 feature 與套件發現排除規則變化。

在應用程式的 `[package.metadata.openrust.discovery]` 下設定 `version = 1` 和 `dont-discover = ["example-package"]`。`["*"]` 停用所有自動探索。套件也可宣告要排除的套件名稱。排除清單中的套件仍可明確註冊 Provider。

HTTP 和 Artisan 啟動時直接使用已連結的清單。要檢查目前建置包含哪些套件，執行 `./artisan package:discover` 列出套件名稱，並將清單匯出至 `bootstrap/cache/packages.json`。用 `APP_PACKAGES_CACHE` 指定其他匯出路徑。執行 `./artisan clear-compiled` 可清除匯出檔案。相依套件、特性或排除規則變更後需重新編譯應用程式。

應用程式解析環境後，`getCachedPackagesPath()` 回傳匯出路徑。延遲服務提供者的目錄隨執行檔連結。

## 資源

在 Provider 的 `boot` 中向應用程式註冊資源。

| 方法 | 資源 |
| --- | --- |
| `loadRoutesFrom` | 使用 `Route` 宣告路由的編譯型函式。應用程式先註冊自己的路由，再註冊套件路由。 |
| `loadViewsFrom` | 檢視目錄和命名空間。使用預設檢視根目錄時，`probe::welcome` 優先讀取應用程式的 `resources/views/vendor/probe/welcome.html`，缺少時讀取套件檢視。 |
| `loadTranslationsFrom` | 翻譯目錄和命名空間。`lang/vendor/probe/en/messages.json` 可覆寫 `probe::messages.title` 的英文內容，其餘鍵繼續回退至套件翻譯。 |
| `loadJsonTranslationsFrom` | 匿名 JSON 翻譯目錄。應用程式對應語言的 JSON 可覆寫套件內同名翻譯。 |
| `loadViewComponentsAs` | 為編譯型元件登記前綴與別名。例如前綴 `probe` 和別名 `badge` 會渲染為 `<x-probe-badge />`。套件的 `views/components` 中的匿名元件可透過 `<x-probe::badge />` 使用。 |
| `loadMigrationsFrom` | 靜態的編譯型 `MigrationEntry` 切片。Artisan 使用應用程式的遷移紀錄執行和回復套件遷移。 |
| `commands` | 使用 `Artisan` 註冊命令的編譯型函式，由主控台核心啟動時呼叫。 |

透過啟動時載入的 Provider 註冊套件 HTTP 路由，確保路由組裝時能夠讀取。部署時隨應用程式散佈檢視和翻譯檔，並將發佈後的目錄傳給 Provider。可執行的宿主與 Provider 請見[獨立 crate 範例](https://github.com/hongyukeji/openrust/tree/master/vendor/openrust/framework/tests/fixtures/package)。

## 發佈資源

嵌入檔案使用 `publishes`，本機檔案或目錄使用 `publishesPaths`，目標路徑相對於應用程式根目錄。執行 `./artisan vendor:publish` 後，輸入文字即可篩選 Provider、標籤或全部資源。用 ↑ 和 ↓ 移動，Enter 確認，Esc 取消。也可用 `--provider`、`--tag` 或 `--all` 直接選取。發佈目錄時保留檔案的相對路徑。預設保留既有檔案，`--force` 覆寫既有檔案，`--existing` 只更新已存在的檔案。

可組合 `--provider` 與一個或多個 `--tag`，依來源路徑篩選資源。`--all` 跨 Provider 與標籤發佈。`--existing` 和 `--force` 同時使用時也只更新已存在的檔案。相對來源路徑從應用程式根目錄解析，安裝在其他位置的套件可明確提供絕對來源路徑。應用程式內的來源路徑和目的路徑均拒絕符號連結，遷移檔名查找也遵守此規則。
