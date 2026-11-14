# 升級指南

## 從 v0.x 原始碼遷移

準備新的檢出目錄，對照目前的 `.env.example` 逐項遷移應用程式設定。依現有啟動結構接入應用程式路由、Provider 和設定。將 `APP_SERVER_HOST` 遷移為 `APP_HOST`，將 `SERVER_PORT` 遷移為 `APP_PORT`，並核對公開的 `APP_URL`。重新建置並部署 `main`。原始碼命令使用 `./artisan`，部署後使用 `./main artisan`。 [應用程式生命週期](../architecture-concepts/lifecycle.md)

## 保留應用程式資料

備份資料庫與持久化儲存。現有資料依賴正式環境金鑰時保留該金鑰。用舊環境的副本驗證加密資料、Cookie 及工作階段。對照目前遷移核對資料庫結構和遷移紀錄，在副本先執行遷移及應用程式自己的資料轉換。 [遷移](../database/migrations.md) · [加密](../security/encryption.md)

## 驗證部署

使用鎖定檔建置新產物，在預備環境驗證 `/up`、應用程式路由、Artisan 與啟用的佇列 worker。核對部署後的檢視、翻譯及公開檔案路徑。保留上一版二進位檔和部署設定。舊二進位檔無法讀取新資料庫結構時，復原應用程式須配合資料庫復原或已驗證的反向遷移。 [部署](../getting-started/deployment.md)
