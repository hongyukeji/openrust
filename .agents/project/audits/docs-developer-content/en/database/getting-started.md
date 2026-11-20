# 开发者文档边界整理：docs/en/database/getting-started.md

2026-11-20。历史原文对照，不作为当前验收结论。

````diff
--- docs/en/database/getting-started.md (迁移前)
+++ docs/en/database/getting-started.md (开发者正文)
@@ -18,11 +18,11 @@

 Set `DB_CONNECTION=sqlsrv` with `DB_HOST`, `DB_PORT`, `DB_DATABASE`, `DB_USERNAME` and `DB_PASSWORD`. A connection URL uses `sqlsrv://`. OpenRust uses a Rust TDS client with TLS and server certificate verification. Set `DB_SSL_CA` to the issuer certificate when using a private CA, and use a host name or address covered by the server certificate.

-The verified installation uses a pre-created database on SQL Server 2017 or 2022. Run `artisan migrate --force` before starting HTTP and the database queue worker. Check the [release matrix](../prologue/releases.md) for the tested SQL Server builds.
+Create the database on SQL Server 2017 or 2022 before connecting. Run `artisan migrate --force` before starting HTTP and the database queue worker. Check the [release matrix](../prologue/releases.md) for the tested SQL Server builds.

 ### MySQL 5.7 TLS {#mysql-57-tls}

-MySQL 5.7.44 needs a server build that can negotiate TLS 1.2 with ECDHE. The Oracle generic Linux archive passed with an RSA server certificate and the following `mysqld` settings. Supply the CA, server certificate and private key through `ssl-ca`, `ssl-cert` and `ssl-key`.
+MySQL 5.7.44 needs a server build that can negotiate TLS 1.2 with ECDHE. For the Oracle generic Linux archive, use an RSA server certificate and the following `mysqld` settings. Supply the CA, server certificate and private key through `ssl-ca`, `ssl-cert` and `ssl-key`.

 ```ini
 [mysqld]
````
