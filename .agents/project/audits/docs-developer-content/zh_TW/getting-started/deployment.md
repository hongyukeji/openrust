# 开发者文档边界整理：docs/zh_TW/getting-started/deployment.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_TW/getting-started/deployment.md (迁移前)
+++ docs/zh_TW/getting-started/deployment.md (开发者正文)
@@ -150,7 +150,7 @@

 將應用程式綁定在私有位址，由反向代理或負載平衡器終止 TLS。將 `APP_URL` 設為公開 HTTPS 位址，並設定代理轉送公開主機名稱和通訊協定。從用戶端驗證憑證鏈和主機名稱，檢查 HTTPS 首頁及健康路由。

-隔離發佈探針已用測試 CA 驗證 TLS 1.2 和 1.3。正式端點和實際部署憑證仍待環境驗收。
+

 ## Docker 部署 {#docker-deployment}

````

````diff
--- docs/zh_TW/getting-started/deployment.md (迁移前)
+++ docs/zh_TW/getting-started/deployment.md (开发者正文)
@@ -151,7 +151,6 @@
 將應用程式綁定在私有位址，由反向代理或負載平衡器終止 TLS。將 `APP_URL` 設為公開 HTTPS 位址，並設定代理轉送公開主機名稱和通訊協定。從用戶端驗證憑證鏈和主機名稱，檢查 HTTPS 首頁及健康路由。


-
 ## Docker 部署 {#docker-deployment}

 Linux 使用具備 Compose 與 Buildx 的 Docker Engine，macOS 與 Windows 使用 Linux 容器模式的 Docker Desktop。同一份 Dockerfile 支援建置 Linux `amd64` 與 `arm64` 映像。以下命令從儲存庫根目錄執行，Windows PowerShell 第一步使用 `Copy-Item docker/.env.example docker/.env`。
````
