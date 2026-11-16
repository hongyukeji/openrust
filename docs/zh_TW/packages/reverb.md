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

## 廣播與擴展 {#broadcasting-and-scaling}

設定 `BROADCAST_CONNECTION=reverb`，依[廣播文件](../digging-deeper/broadcasting.md)啟用私人與在線頻道授權。Pusher 通訊協定用戶端連線至設定前綴下的 `/app/{key}`。正式環境可在應用程式的 `allowed_origins` 設定受信任的主機名稱，由反向代理終止 TLS。

在各執行個體設定 `REVERB_SCALING_ENABLED=true`。同一應用程式的節點共用 `REVERB_APP_ID`、`REVERB_APP_KEY`、`REVERB_APP_SECRET`、`REVERB_SCALING_CHANNEL` 和 Redis 連線設定。可以使用 `REDIS_URL` 或分別設定 Redis 參數。不同應用程式共用擴展頻道時應使用不同的應用程式 ID。同一擴展頻道的節點應執行相同版本的 OpenRust Reverb。

訂閱在線頻道時，用戶端會收到全域成員清單和人數。同一使用者建立多個連線仍只計一人，其他訂閱者只會看到該使用者首次加入與最後一次離開。Redis 可用時，節點正常結束會移除其連線。節點異常結束後，仍有訂閱者的節點大約每 2 秒檢查一次，清理已逾 10 秒租期的連線。新訂閱傳回成員清單前也會清理過期連線。

Redis Pub/Sub 跨節點投遞已簽署的 HTTP 發佈和已授權的用戶端事件。用戶端事件只傳送給同一應用程式、同一頻道的其他訂閱連線，排除傳送者的連線。節點失去 Redis 訂閱時會關閉本節點的在線頻道連線，由用戶端在恢復後重新連線並訂閱。事件盡力投遞，斷線期間錯過的事件不會補送。
