# 开发者文档边界整理：docs/zh_CN/getting-started/deployment.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_CN/getting-started/deployment.md (迁移前)
+++ docs/zh_CN/getting-started/deployment.md (开发者正文)
@@ -150,7 +150,7 @@

 将应用绑定在私有地址，由反向代理或负载均衡器终止 TLS。把 `APP_URL` 设为公开 HTTPS 地址，并配置代理转发公开主机名和协议。从客户端验证证书链和主机名，检查 HTTPS 首页和健康路由。

-隔离发布探针已用测试 CA 验证 TLS 1.2 和 1.3。生产端点和实际部署证书仍待环境验收。
+

 ## Docker 部署 {#docker-deployment}

````

````diff
--- docs/zh_CN/getting-started/deployment.md (迁移前)
+++ docs/zh_CN/getting-started/deployment.md (开发者正文)
@@ -151,7 +151,6 @@
 将应用绑定在私有地址，由反向代理或负载均衡器终止 TLS。把 `APP_URL` 设为公开 HTTPS 地址，并配置代理转发公开主机名和协议。从客户端验证证书链和主机名，检查 HTTPS 首页和健康路由。


-
 ## Docker 部署 {#docker-deployment}

 Linux 使用带 Compose 和 Buildx 的 Docker Engine，macOS 和 Windows 使用 Linux 容器模式的 Docker Desktop。同一份 Dockerfile 支持构建 Linux `amd64` 和 `arm64` 镜像。以下命令从仓库根目录执行，Windows PowerShell 第一步用 `Copy-Item docker/.env.example docker/.env`。
````
