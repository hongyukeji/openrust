# 开发者文档边界整理：docs/en/getting-started/deployment.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/en/getting-started/deployment.md (迁移前)
+++ docs/en/getting-started/deployment.md (开发者正文)
@@ -150,7 +150,7 @@

 Bind the application to a private address and terminate TLS at your reverse proxy or load balancer. Set `APP_URL` to the public HTTPS origin and configure the proxy to pass the public host and scheme. Verify the certificate chain and hostname from a client and check both the welcome page and health route over HTTPS.

-The isolated release probe exercised TLS 1.2 and 1.3 with a test CA. The production endpoint and its deployed certificate remain deployment checks.
+

 ## Docker deployment {#docker-deployment}

````

````diff
--- docs/en/getting-started/deployment.md (迁移前)
+++ docs/en/getting-started/deployment.md (开发者正文)
@@ -151,7 +151,6 @@
 Bind the application to a private address and terminate TLS at your reverse proxy or load balancer. Set `APP_URL` to the public HTTPS origin and configure the proxy to pass the public host and scheme. Verify the certificate chain and hostname from a client and check both the welcome page and health route over HTTPS.


-
 ## Docker deployment {#docker-deployment}

 Use Docker Engine with Compose and Buildx on Linux, or Docker Desktop in Linux-container mode on macOS or Windows. The same Dockerfile builds Linux `amd64` and `arm64` images. Run the commands below from the repository root. On Windows PowerShell use `Copy-Item docker/.env.example docker/.env` for the first step.
````
