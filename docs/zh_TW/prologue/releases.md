# 發行說明

## 版本與上游基準

目前原始碼在 `Cargo.toml` 中宣告開發版 `0.0.0`。建置時使用已提交的 `Cargo.lock`，部署時記錄原始碼提交。OpenRust 版本號描述 OpenRust 自身的版本。Laravel v13.40.0 是相容性基準，應用程式啟動與組裝參考 Goravel v1.18.0。 [Laravel 原始碼](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc) · [Goravel 原始碼](https://github.com/goravel/goravel/tree/31651f646ccedf1701471bf01285111270a9d370)

## 已驗收環境

| 環境 | 驗收範圍 |
| --- | --- |
| macOS ARM64、Rust 1.94.0 | 原始碼安裝、獨立部署、HTTP 和 Artisan |
| Linux ARM64、Debian Bookworm 容器、Rust 1.94.0 | 以非特權使用者執行獨立產物、HTTP、SQLite 遷移及 Artisan |
| Linux ARM64、Debian Bookworm 容器、LinuxKit 7.0.14 | 預設同程序啟動、SQLite 佇列與獨立 Artisan 對照、SIGTERM 優雅結束 |
| SQLite | 新應用程式遷移及獨立應用程式啟動 |
| MySQL 8.4 | macOS ARM64 獨立產物遷移及框架 Eloquent 整合驗收 |
| Redis 8.10 | 佇列驗收及兩個程序間的 Reverb 發佈 |
| Linux x86_64 和 Windows 二進位檔 | 目標平台實機驗收待完成 |

各平台與驅動的驗收範圍見表，功能邊界見相應指南，例如 [Reverb 擴充套件](../packages/reverb.md)。獨立產物的建置與執行見[部署](../getting-started/deployment.md)。

## 變更

應用程式開發者可在儲存庫的[更新記錄](https://github.com/hongyukeji/openrust/blob/master/CHANGELOG.md)查看變更。遷移現有應用程式前請查閱[升級指南](upgrade.md)。
