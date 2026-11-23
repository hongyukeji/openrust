# API 參考

Rust 型別和 trait 使用 `UpperCamelCase`，方法與欄位使用 `snake_case`，常數使用 `SCREAMING_SNAKE_CASE`。workspace 在正常建置時拒絕命名違規。執行 `python3 tests/rust_naming.py` 可核對策略繼承和實際編譯器拒絕案例；序列化鍵、事件、CLI 名稱與範本運算式保留既有契約。

使用指南介紹應用程式行為。Rustdoc 列出建置快照中公開的 Rust 模組、型別、方法、feature 條件及原始碼連結。

`openrust-translation-database` 提供 `DatabaseLoader`，可明確將資料庫翻譯載入既有翻譯器。裝配、檔案回退及資料庫支援範圍見[資料庫翻譯](../digging-deeper/localization.md#database-translation-loader)。

`openrust-settings` 提供 `Settings<T>`、`SettingsGroup` 與 `SettingsServiceProvider`，用於型別化資料庫設定。註冊、遷移、加密及交易與作用域行為見[資料庫設定](../packages/settings.md)。

## 建置原始碼快照

使用 Python 3.11 或更新版本、Rust 1.94.0 與已提交的乾淨原始碼快照。在儲存庫根目錄選擇新的輸出目錄與專用 Cargo 產物目錄：

```sh
python3 -B docs/scripts/build_api.py --output target/tmp/api/export --target-dir target/tmp/api/cargo-target
```

沒有 Git 中繼資料的原始碼封存須加上 `--revision`，傳入該封存完整的 40 位提交 SHA。命令使用鎖定檔，以全部 feature 建置每個 workspace 程式庫的文件，拒絕損壞及指向私有項目的文件連結。每次重建前清理專用 target 的 Rustdoc 頁面，防止已刪除 API 殘留；Cargo 相依編譯快取保留。既有匯出目錄不會被覆寫。根應用程式執行檔不屬於程式庫 API。

匯出面向建置主機，`build-info.json` 記錄目標三元組；平台專屬 API 按該目標的條件編譯顯示。腳本使用主機目標，不採用 `CARGO_BUILD_TARGET`。全部 feature 用於展示選用 API；應用程式仍須在 Cargo.toml 按需啟用功能。外部相依 API 連結至對應 Rustdoc，不隨產物打包。匯出包含本機搜尋、原始碼頁與 `build-info.json`，記錄原始碼提交、Rust 版本、套件清單、鎖定檔雜湊及原始碼指紋。

## 將 API 加入文件站

```sh
cd docs
DOCS_API_DIR=../target/tmp/api/export npm run build
```

站點先核對目前鎖定檔及原始碼指紋，再複製至 `api/`。提供匯出目錄時，側欄增加 Rustdoc 入口；支援站點根路徑及 `/docs/` 等部署子路徑。一般建置不需 API 產物，仍可閱讀本指南。單獨預覽匯出時，以本機 HTTP 服務託管其目錄並開啟 `index.html`。

產生 API 與站點不會發布版本。程式庫原始碼、workspace 清單或鎖定檔變更後須重新產生。發布產物應對應記錄的原始碼提交，參見[版本說明](../prologue/releases.md)與[部署](../getting-started/deployment.md)。
