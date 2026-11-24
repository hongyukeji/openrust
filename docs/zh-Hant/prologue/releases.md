# 發行說明

## 版本與上游基準

框架與同步發布的官方套件從根 `Cargo.toml` 的 `workspace.package.version` 繼承版本。目前原始碼版本直接讀取該清單：

<<< ../../../Cargo.toml#framework-version{toml}

準備發布時只修改此值，執行 `cargo metadata --format-version 1` 更新 `Cargo.lock`，檢查鎖定檔差異，再執行驗收並更新變更說明。發布建置使用已提交的鎖定檔，並記錄原始碼提交號。歷史發布說明保留當時的版本。

修改套件版本後需要重新建置；使用設定快取的應用程式還需重新產生快取，以更新預設的 `app.version`。明確設定的 `APP_VERSION` 仍由應用程式自行管理。

儲存庫預設應用程式也繼承此版本。業務應用程式可以單獨宣告自己的 `[package].version`；`app.version` 預設取該應用程式的 Cargo 套件版本，也可由 `APP_VERSION` 覆寫。`App::version()` 和 Artisan `--version` 傳回框架套件版本。第三方 vendor 套件保留上游版本。文件的 `master`／`v0.x` 選單表示原始碼分支，並非應用程式版本。

各套件目前宣告 `publish = false`，統一版本不會自動啟用註冊表發布。原始碼安裝保留 workspace 清單；Cargo 產生的 `.crate` 封存會將繼承中繼資料解析為自身清單中的值。單獨複製成員目錄不會包含其 workspace 中繼資料。容器映像標籤仍透過 `APP_IMAGE_VERSION` 設定，見[部署指南](../getting-started/deployment.md)。

## 環境相容範圍

原始碼已在 macOS ARM64、Linux ARM64／x86_64 和 Windows ARM64 上執行驗證。須針對目標作業系統與架構建置，二進位檔不能跨平台通用。Windows Docker Desktop 需要可用的 Linux 容器後端。

現有驅動程式檢查涵蓋 SQLite 3.51.3、MySQL 5.7.44／8.0.46／8.4.13、MariaDB 10.3.39／11.8.9、SQL Server 2017／2022 和 PostgreSQL 10–18。這僅描述特定版本和情境的檢查範圍，不代表所有平台、修補版本和功能組合均可用。驅動程式設定及版本限制見[資料庫](../database/getting-started.md)。Redis 佇列和 Reverb 整合已在 Redis 8.10 上執行驗證。

## 部署要求

按[部署指南](../getting-started/deployment.md)建置和執行獨立產物。MySQL 5.7 須設定[支援 ECDHE 的 TLS](../database/getting-started.md#mysql-57-tls)；Oracle Linux 7 映像需使用[衍生映像](../database/getting-started.md#mysql-57-oracle-linux-7)。移植資料庫寫入邏輯時，參閱[影響列數](../database/queries.md#mysql-affected-rows)。

正式環境 TLS 由反向代理或負載平衡器終止，須核對部署主機名稱和憑證鏈。託管 SQS 需要正確的區域、佇列 URL 和 IAM 憑證，本機協定測試不能確認 AWS 端權限。憑證按佇列授予 `sqs:SendMessage`、`sqs:ReceiveMessage`、`sqs:DeleteMessage`、`sqs:ChangeMessageVisibility` 和 `sqs:GetQueueAttributes` 權限，臨時憑證同時設定工作階段權杖。參閱[佇列](../digging-deeper/queues.md)。

## 變更

應用程式開發者可在儲存庫的[更新記錄](https://github.com/hongyukeji/openrust/blob/master/CHANGELOG.md)查看變更。遷移現有應用程式前請查閱[升級指南](upgrade.md)。

## 相依安全

鎖定建置使用 lettre 0.11.24、由 libsqlite3-sys 0.39.1 提供的 SQLite 3.54.1 與穩定版 RSA 0.10.0。SQLx SQLite 配接器保留現有 0.9.0 實作，改用修復後的 SQLite 相依；WebAuthn 密碼配接器更新 RSA feature 宣告與型別匯入。上游原始碼與授權條款保留在 `vendor/`。Passport 保留金鑰產生與 PEM 格式，包括明確產生 1024 位金鑰；線上簽章仍使用 AWS-LC。

日期輔助方法使用 Jiff 0.2.46 原生型別與內建 IANA 2025b 時區資料，SQL 驅動程式使用官方 `time` 整合。建構與日曆運算見[輔助方法](../digging-deeper/helpers.md)。本機圖片及 AWS 傳輸相依採用上游對 paste 與 pin-utils 的替代實作，並保留原始碼與授權條款。應用程式發布前須稽核實際相依並驗證目標環境。
