# Reverb

## 安裝

建置時啟用 Cargo 的 `reverb` 功能。該功能註冊 Reverb Provider、`reverb` 廣播驅動程式與 Artisan 命令。

```sh
cargo build --features reverb
```

使用 `REVERB_APP_ID`、`REVERB_APP_KEY` 和 `REVERB_APP_SECRET` 設定監聽端與發佈端共用的憑證。用戶端使用公開金鑰及 WebSocket 位址，秘密金鑰保留在伺服器端。

## 執行服務

設定 `REVERB_ENABLED=true` 使用預設的內嵌模式。啟動應用程式時會繫結 HTTP 和 Reverb 兩個監聽器。`REVERB_SERVER_HOST`、`REVERB_SERVER_PORT` 與 `REVERB_SERVER_PATH` 設定 Reverb 監聽器。`REVERB_HOST`、`REVERB_PORT` 與 `REVERB_SCHEME` 設定廣播驅動程式的發佈目標。本機監聽器未設定 TLS 代理時，使用 `REVERB_SCHEME=http`。

需要獨立程序時，設定 `REVERB_MODE=standalone` 並透過 Artisan 啟動：

```sh
./target/debug/main artisan reverb:start --host=127.0.0.1 --port=8080
./target/debug/main artisan reverb:restart
```

重新啟動命令會將訊號寫入應用程式快取。同一部主機可使用 `CACHE_STORE=file` 共用訊號。獨立程序收到訊號後會關閉監聽器和連線，可由程序管理器再次啟動。內嵌程序隨應用程式關閉及重新啟動。

## 廣播與擴展

設定 `BROADCAST_CONNECTION=reverb`，依[廣播文件](../digging-deeper/broadcasting.md)啟用私人與在線頻道授權。Pusher 通訊協定用戶端連線至設定前綴下的 `/app/{key}`。正式環境可在應用程式的 `allowed_origins` 設定受信任的主機名稱，由反向代理終止 TLS。

在各執行個體設定 `REVERB_SCALING_ENABLED=true`，使用 Redis 跨程序投遞經簽署的 HTTP 發佈事件。各執行個體共用 `REVERB_SCALING_CHANNEL` 和 Redis 設定。可以使用 `REDIS_URL`，或分別設定 Redis 主機、憑證、連接埠和資料庫。頻道成員狀態和用戶端發起的事件由各自的監聽器處理。
