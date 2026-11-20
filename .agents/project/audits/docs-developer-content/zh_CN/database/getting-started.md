# 开发者文档边界整理：docs/zh_CN/database/getting-started.md

2026-11-20。历史原文对照，不作为当前验收结论。

````diff
--- docs/zh_CN/database/getting-started.md (迁移前)
+++ docs/zh_CN/database/getting-started.md (开发者正文)
@@ -22,7 +22,7 @@

 ### MySQL 5.7 TLS {#mysql-57-tls}

-MySQL 5.7.44 需要能通过 ECDHE 协商 TLS 1.2 的服务端构建。Oracle 通用 Linux 归档使用 RSA 服务端证书和以下 `mysqld` 配置通过验收。通过 `ssl-ca`、`ssl-cert` 和 `ssl-key` 配置 CA、服务端证书及私钥。
+MySQL 5.7.44 需要能通过 ECDHE 协商 TLS 1.2 的服务端构建。Oracle 通用 Linux 归档使用 RSA 服务端证书和以下 `mysqld` 配置。通过 `ssl-ca`、`ssl-cert` 和 `ssl-key` 配置 CA、服务端证书及私钥。

 ```ini
 [mysqld]
````
