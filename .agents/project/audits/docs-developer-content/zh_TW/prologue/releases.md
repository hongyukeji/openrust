# 开发者文档边界整理：docs/zh_TW/prologue/releases.md

2026-11-20。历史原文对照，不作为当前验收结论。

````diff
--- docs/zh_TW/prologue/releases.md (迁移前)
+++ docs/zh_TW/prologue/releases.md (开发者正文)
@@ -4,50 +4,17 @@

 目前原始碼在 `Cargo.toml` 中宣告開發版 `0.0.0`。建置時使用已提交的 `Cargo.lock`，部署時記錄原始碼提交。OpenRust 版本號描述 OpenRust 自身的版本。Laravel v13.40.0 是相容性基準，應用程式啟動與組裝參考 Goravel v1.18.0。 [Laravel 原始碼](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc) · [Goravel 原始碼](https://github.com/goravel/goravel/tree/31651f646ccedf1701471bf01285111270a9d370)

-## 已驗收環境
+## 環境相容範圍

-| 環境 | 驗收範圍 |
-| --- | --- |
-| macOS ARM64、Rust 1.94.0 | 原始碼安裝、獨立部署、HTTP 和 Artisan |
-| Linux ARM64、Debian Bookworm 容器、Rust 1.94.0 | 以非特權使用者執行獨立產物、HTTP、SQLite 遷移及 Artisan |
-| Linux ARM64、Debian Bookworm 容器、LinuxKit 7.0.14 | 預設同程序啟動、SQLite 佇列與獨立 Artisan 對照、SIGTERM 優雅結束 |
-| SQLite 3.51.3 | 乾淨檢出、12 項應用程式遷移、Artisan 與 HTTP |
-| MySQL 5.7.44，Oracle 通用 Linux 封存檔 | TLS 1.2 憑證和伺服器名稱驗證、12 項遷移、等冪重跑、HTTP、資料庫工作階段和 Artisan 工作處理 |
-| MySQL 5.7.44，Oracle Linux 7 衍生映像 | 在 amd64 相容環境通過 TLS 1.2 和 ECDHE 驗證、12 項遷移、等冪重跑、HTTP、資料庫工作階段及 Artisan 工作處理 |
-| MySQL 8.0.46、8.4.13 | 隔離資料庫、12 項遷移、重複執行、Artisan 資料庫 Worker、HTTP 與資料庫工作階段寫入 |
-| MariaDB 10.3.39、11.8.9 | TLS 驗證、12 項遷移、等冪重跑、HTTP、資料庫工作階段、JSON 與 Schema 檢查及 Artisan 工作處理 |
-| SQL Server 2017（14.0.3570.2）、2022（16.0.4325.4） | 對 Linux amd64 相容環境中的伺服器容器實測 Rust TDS 和 TLS 驗證、12 項遷移、等冪重跑、HTTP、資料庫工作階段及 Artisan 工作處理 |
-| PostgreSQL 10–18 | 隔離資料庫、12 項遷移、重複執行、Artisan 資料庫 Worker、HTTP 與資料庫工作階段寫入。另在 10 和 18 實測工作派送與保留 |
-| Redis 8.10 | 佇列驗收及兩個程序間的 Reverb 發佈 |
-| Ubuntu 24.04.3 LTS x86_64、Docker Engine 29.9.3 | 從 `07307c6e1` 使用 Rust 1.94.0 原生建置發佈映像，實測 Compose 的 MySQL 與 Redis、12 項遷移、HTTP 和 Artisan、Redis 工作、雙應用程式執行個體共用資料庫與快取、儲存持續性及 SIGTERM 結束 |
-| Apple Silicon 上的 Docker Desktop、amd64 相容模式 | SQLite、Artisan、HTTP、持續性儲存與 SIGTERM 結束 |
-| Parallels 中的 Windows 11 Pro ARM64 Build 26200.9457 | 原生 ARM64 MSVC 發佈版通過 SQLite 遷移、Artisan、首頁與健康路由、公開檔案、目錄遷移和 Ctrl+C 優雅結束。Docker Desktop 後端尚待驗收 |
+原始碼已在 macOS ARM64、Linux ARM64／x86_64 和 Windows ARM64 上執行驗證。須針對目標作業系統與架構建置，二進位檔不能跨平台通用。Windows Docker Desktop 需要可用的 Linux 容器後端。

-各平台與驅動的驗收範圍見表，功能邊界見相應指南，例如 [Reverb 擴充套件](../packages/reverb.md)。獨立產物的建置與執行見[部署](../getting-started/deployment.md)。
+現有驅動程式檢查涵蓋 SQLite 3.51.3、MySQL 5.7.44／8.0.46／8.4.13、MariaDB 10.3.39／11.8.9、SQL Server 2017／2022 和 PostgreSQL 10–18。這僅描述特定版本和情境的檢查範圍，不代表所有平台、修補版本和功能組合均可用。驅動程式設定及版本限制見[資料庫](../database/getting-started.md)。Redis 佇列和 Reverb 整合已在 Redis 8.10 上執行驗證。

-資料庫列記錄發佈驗收結果。PostgreSQL 10–18 的結果以實測修補版本為準。Oracle Linux 7 原映像需建置[啟用 ECDHE 的衍生映像](../database/getting-started.md#mysql-57-oracle-linux-7)以接入 OpenRust。實測連線設定請參閱 [MySQL 5.7 TLS](../database/getting-started.md#mysql-57-tls)、[MariaDB](../database/getting-started.md#mariadb) 和 [SQL Server](../database/getting-started.md#sql-server)。
+## 部署要求

-## 獨立發布套件複驗 {#standalone-package-recheck}
+按[部署指南](../getting-started/deployment.md)建置和執行獨立產物。MySQL 5.7 須設定[支援 ECDHE 的 TLS](../database/getting-started.md#mysql-57-tls)；Oracle Linux 7 映像需使用[衍生映像](../database/getting-started.md#mysql-57-oracle-linux-7)。移植資料庫寫入邏輯時，參閱[影響列數](../database/queries.md#mysql-affected-rows)。

-從 `2d1f83dfd` 的乾淨原始碼封存檔在 macOS ARM64 上使用 Rust 1.94.0 與已提交的鎖定檔建置獨立發布執行檔。發布目錄從其他工作目錄啟動，連線至 MySQL 8.4.13 與 Redis 8.10.3。金鑰產生、12 項遷移與冪等重跑、首頁和健康路由、兩類公共檔案、資料庫會話、受管和獨立 Redis Worker，以及 SIGTERM 優雅結束均通過。搬移發布目錄後仍可存取公共儲存空間和原遷移帳本。
-
-## 舊版 MySQL 與 MariaDB 驅動複驗 {#older-mysql-family-recheck}
-
-Oracle Linux 7 ECDHE 衍生映像中的 MySQL 5.7.44 和 MariaDB 10.3.39 分別通過 TLS 1.2 和 TLS 1.3 驗證及影響列數迴歸。HTTP 和 Artisan 都驗證了原值更新和重複列 upsert，兩個版本均通過應用程式遷移與等冪重跑。MySQL 伺服器執行於 amd64 相容環境，MariaDB 伺服器執行於 ARM64。預設行為與連線覆寫見[影響列數](../database/queries.md#mysql-affected-rows)。
-
-## 模擬正式環境與實機驗證 {#pending-deployment-acceptance}
-
-08-04 已按模擬正式環境驗收結案。表中列有各目標環境的實機驗證項目。
-
-| 目標 | 已有本機驗收 | 實機驗證 |
-| --- | --- | --- |
-| Windows Docker Desktop | Windows PowerShell 已解析 Compose profile 和含空格的繫結路徑。Windows ARM64 獨立發佈已通過 HTTP 和 Artisan。 | 需要可登入且執行 Docker Desktop Linux 容器後端的 Windows 主機，測試帳戶可使用 Docker、Compose 和 Buildx，並讀寫隔離的 Windows 測試目錄。實測乾淨建置、部署、唯讀 public、可寫的持續性 storage、HTTP 和 Artisan。 |
-| 正式環境 TLS | 本機代理已驗證 TLS 1.2／1.3、主機名稱與 CA 驗證、缺少中繼憑證時拒絕、同一端點輪替終端憑證、真實 HTTP 與 Secure 工作階段 Cookie。 | 在真實 HTTPS 位址核對部署主機名稱、憑證鏈和協定原則，實測首頁和健康路由，並對同一部署執行唯讀 Artisan。 |
-| 受管 AWS SQS 標準／FIFO | ElasticMQ 已驗證傳遞語意和批次復原。獨立回送端點重新計算兩種佇列的真實 Artisan 傳送、指標、接收及刪除請求的 SigV4，並拒絕缺少工作階段權杖和無效簽章。 | 需要標準與 FIFO 各一條隔離佇列的 URL、對應區域、佇列範圍 IAM 憑證和隔離 MySQL 批次帳本。使用真實 Worker 驗證 AWS 端 IAM 與臨時憑證、佇列指標、重新傳遞、FIFO 去重和失敗復原。 |
-
-測試憑證須限制在測試佇列，具備 `sqs:SendMessage`（含批次傳送）、`sqs:ReceiveMessage`、`sqs:DeleteMessage`、`sqs:ChangeMessageVisibility` 和 `sqs:GetQueueAttributes` 權限。使用臨時憑證時須同時設定工作階段權杖。
-
-佇列權限請參閱 [AWS SQS API 參考](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-api-permissions-reference.html)。
+正式環境 TLS 由反向代理或負載平衡器終止，須核對部署主機名稱和憑證鏈。託管 SQS 需要正確的區域、佇列 URL 和 IAM 憑證，本機協定測試不能確認 AWS 端權限。憑證按佇列授予 `sqs:SendMessage`、`sqs:ReceiveMessage`、`sqs:DeleteMessage`、`sqs:ChangeMessageVisibility` 和 `sqs:GetQueueAttributes` 權限，臨時憑證同時設定工作階段權杖。參閱[佇列](../digging-deeper/queues.md)。

 ## 變更

````
