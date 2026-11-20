# 發行說明

## 版本與上游基準

目前原始碼在 `Cargo.toml` 中宣告開發版 `0.0.0`。建置時使用已提交的 `Cargo.lock`，部署時記錄原始碼提交。OpenRust 版本號描述 OpenRust 自身的版本。Laravel v13.40.0 是相容性基準，應用程式啟動與組裝參考 Goravel v1.18.0。 [Laravel 原始碼](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc) · [Goravel 原始碼](https://github.com/goravel/goravel/tree/31651f646ccedf1701471bf01285111270a9d370)

## 環境相容範圍

原始碼已在 macOS ARM64、Linux ARM64／x86_64 和 Windows ARM64 上執行驗證。須針對目標作業系統與架構建置，二進位檔不能跨平台通用。Windows Docker Desktop 需要可用的 Linux 容器後端。

現有驅動程式檢查涵蓋 SQLite 3.51.3、MySQL 5.7.44／8.0.46／8.4.13、MariaDB 10.3.39／11.8.9、SQL Server 2017／2022 和 PostgreSQL 10–18。這僅描述特定版本和情境的檢查範圍，不代表所有平台、修補版本和功能組合均可用。驅動程式設定及版本限制見[資料庫](../database/getting-started.md)。Redis 佇列和 Reverb 整合已在 Redis 8.10 上執行驗證。

## 部署要求

按[部署指南](../getting-started/deployment.md)建置和執行獨立產物。MySQL 5.7 須設定[支援 ECDHE 的 TLS](../database/getting-started.md#mysql-57-tls)；Oracle Linux 7 映像需使用[衍生映像](../database/getting-started.md#mysql-57-oracle-linux-7)。移植資料庫寫入邏輯時，參閱[影響列數](../database/queries.md#mysql-affected-rows)。

正式環境 TLS 由反向代理或負載平衡器終止，須核對部署主機名稱和憑證鏈。託管 SQS 需要正確的區域、佇列 URL 和 IAM 憑證，本機協定測試不能確認 AWS 端權限。憑證按佇列授予 `sqs:SendMessage`、`sqs:ReceiveMessage`、`sqs:DeleteMessage`、`sqs:ChangeMessageVisibility` 和 `sqs:GetQueueAttributes` 權限，臨時憑證同時設定工作階段權杖。參閱[佇列](../digging-deeper/queues.md)。

## 變更

應用程式開發者可在儲存庫的[更新記錄](https://github.com/hongyukeji/openrust/blob/master/CHANGELOG.md)查看變更。遷移現有應用程式前請查閱[升級指南](upgrade.md)。
