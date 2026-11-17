# 發行說明

## 版本與上游基準

目前原始碼在 `Cargo.toml` 中宣告開發版 `0.0.0`。建置時使用已提交的 `Cargo.lock`，部署時記錄原始碼提交。OpenRust 版本號描述 OpenRust 自身的版本。Laravel v13.40.0 是相容性基準，應用程式啟動與組裝參考 Goravel v1.18.0。 [Laravel 原始碼](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc) · [Goravel 原始碼](https://github.com/goravel/goravel/tree/31651f646ccedf1701471bf01285111270a9d370)

## 已驗收環境

| 環境 | 驗收範圍 |
| --- | --- |
| macOS ARM64、Rust 1.94.0 | 原始碼安裝、獨立部署、HTTP 和 Artisan |
| Linux ARM64、Debian Bookworm 容器、Rust 1.94.0 | 以非特權使用者執行獨立產物、HTTP、SQLite 遷移及 Artisan |
| Linux ARM64、Debian Bookworm 容器、LinuxKit 7.0.14 | 預設同程序啟動、SQLite 佇列與獨立 Artisan 對照、SIGTERM 優雅結束 |
| SQLite 3.51.3 | 乾淨檢出、12 項應用程式遷移、Artisan 與 HTTP |
| MySQL 5.7.44，Oracle 通用 Linux 封存檔 | TLS 1.2 憑證和伺服器名稱驗證、12 項遷移、等冪重跑、HTTP、資料庫工作階段和 Artisan 工作處理 |
| MySQL 8.0.46、8.4.13 | 隔離資料庫、12 項遷移、重複執行、Artisan 資料庫 Worker、HTTP 與資料庫工作階段寫入 |
| PostgreSQL 10–18 | 隔離資料庫、12 項遷移、重複執行、Artisan 資料庫 Worker、HTTP 與資料庫工作階段寫入。另在 10 和 18 實測工作派送與保留 |
| Redis 8.10 | 佇列驗收及兩個程序間的 Reverb 發佈 |
| Ubuntu 24.04 x86_64 和 Apple Silicon 上的 Docker Desktop | Ubuntu 已有原生 amd64 容器驗收。當前發佈映像在 Docker Desktop 的 amd64 相容模式複驗 SQLite、Artisan、HTTP、持久儲存與 SIGTERM 結束 |
| Parallels 中的 Windows 11 Pro ARM64 Build 26200.9457 | 原生 ARM64 MSVC 發佈版通過 SQLite 遷移、Artisan、首頁與健康路由、公開檔案、目錄遷移和 Ctrl+C 優雅結束。Docker Desktop 後端尚待驗收 |

各平台與驅動的驗收範圍見表，功能邊界見相應指南，例如 [Reverb 擴充套件](../packages/reverb.md)。獨立產物的建置與執行見[部署](../getting-started/deployment.md)。

資料庫列記錄發佈驗收結果。PostgreSQL 10–18 的結果以實測修補版本為準。以 Oracle Linux 7 為基底的 MySQL 5.7.44 容器建置未能與 OpenRust 的 TLS 用戶端協商 ECDHE。實測伺服器設定請參閱 [MySQL 5.7 TLS](../database/getting-started.md#mysql-57-tls)。MariaDB 和 SQL Server 已預留連線名稱，目前均回傳驅動程式無法使用。

目前 Windows ARM64 客體缺少巢狀虛擬化和可運作的 WSL 2 後端，獨立部署已通過驗收。Windows Docker Desktop 和目前版本的原生 Ubuntu 複驗仍需目標環境。

本機反向代理透過測試 CA 驗證了 TLS 1.2 與 1.3、憑證鏈和主機名稱。正式端點仍待實機驗收。

託管 AWS SQS 標準／FIFO 仍待實機驗收。ElasticMQ 的證據見佇列實作紀錄。

## 變更

應用程式開發者可在儲存庫的[更新記錄](https://github.com/hongyukeji/openrust/blob/master/CHANGELOG.md)查看變更。遷移現有應用程式前請查閱[升級指南](upgrade.md)。
