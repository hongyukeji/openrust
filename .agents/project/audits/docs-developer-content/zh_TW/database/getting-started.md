# 开发者文档边界整理：docs/zh_TW/database/getting-started.md

2026-11-20 历史原文对照。

````diff
--- docs/zh_TW/database/getting-started.md (迁移前)
+++ docs/zh_TW/database/getting-started.md (开发者正文)
@@ -22,7 +22,7 @@

 ### MySQL 5.7 TLS {#mysql-57-tls}

-MySQL 5.7.44 需要能透過 ECDHE 協商 TLS 1.2 的伺服器建置。Oracle 通用 Linux 封存檔使用 RSA 伺服器憑證及下列 `mysqld` 設定通過驗收。透過 `ssl-ca`、`ssl-cert` 和 `ssl-key` 設定 CA、伺服器憑證及私密金鑰。
+MySQL 5.7.44 需要能透過 ECDHE 協商 TLS 1.2 的伺服器建置。Oracle 通用 Linux 封存檔使用 RSA 伺服器憑證及下列 `mysqld` 設定。透過 `ssl-ca`、`ssl-cert` 和 `ssl-key` 設定 CA、伺服器憑證及私密金鑰。

 ```ini
 [mysqld]
````
