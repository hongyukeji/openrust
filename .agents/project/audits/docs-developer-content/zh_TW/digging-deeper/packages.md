# 开发者文档边界整理：docs/zh_TW/digging-deeper/packages.md

2026-11-20 历史原文对照。

````diff
--- docs/zh_TW/digging-deeper/packages.md (迁移前)
+++ docs/zh_TW/digging-deeper/packages.md (开发者正文)
@@ -39,7 +39,7 @@
 | `loadMigrationsFrom` | 靜態的編譯型 `MigrationEntry` 切片。Artisan 使用應用程式的遷移紀錄執行和回復套件遷移。 |
 | `commands` | 使用 `Artisan` 註冊命令的編譯型函式，由主控台核心啟動時呼叫。 |

-透過啟動時載入的 Provider 註冊套件 HTTP 路由，確保路由組裝時能夠讀取。部署時隨應用程式散佈檢視和翻譯檔，並將發佈後的目錄傳給 Provider。可執行的宿主與 Provider 請見[獨立 crate 驗收範例](https://github.com/hongyukeji/openrust/tree/master/vendor/openrust/framework/tests/fixtures/package)。
+透過啟動時載入的 Provider 註冊套件 HTTP 路由，確保路由組裝時能夠讀取。部署時隨應用程式散佈檢視和翻譯檔，並將發佈後的目錄傳給 Provider。可執行的宿主與 Provider 請見[獨立 crate 範例](https://github.com/hongyukeji/openrust/tree/master/vendor/openrust/framework/tests/fixtures/package)。

 ## 發佈資源

````
