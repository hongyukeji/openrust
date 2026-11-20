# Reverb

## 安裝

建置時啟用 Cargo 的 `reverb` 功能。該功能註冊 Reverb Provider、`reverb` 廣播驅動程式與 Artisan 命令。

```sh
cargo build --features reverb
```

套件本身提供預設伺服器設定，啟用 feature 不會產生 `config/reverb.rs`。需要應用覆寫時執行 `./artisan vendor:publish --tag=reverb-config`，在 `config/mod.rs` 的模組宣告與設定映射中分別加入 `mod reverb;` 與 `("reverb".to_owned(), reverb::load(env)?),`，然後重新建置。明確執行 `init --extensions=reverb` 會發布並登記此設定；重複初始化保留既有自訂，除非傳入 `--force`。

使用 `REVERB_APP_ID`、`REVERB_APP_KEY` 和 `REVERB_APP_SECRET` 設定監聽端與發佈端共用的憑證。用戶端使用公開金鑰及 WebSocket 位址，秘密金鑰保留在伺服器端。

## 執行服務

設定 `REVERB_ENABLED=true` 使用預設的內嵌模式。啟動應用程式時會繫結 HTTP 和 Reverb 兩個監聽器。`REVERB_SERVER_HOST`、`REVERB_SERVER_PORT` 與 `REVERB_SERVER_PATH` 設定 Reverb 監聽器。`REVERB_HOST`、`REVERB_PORT` 與 `REVERB_SCHEME` 設定廣播驅動程式的發佈目標。本機監聽器未設定 TLS 代理時，使用 `REVERB_SCHEME=http`。

需要獨立程序時，設定 `REVERB_MODE=standalone` 並透過 Artisan 啟動：

```sh
./target/debug/main artisan reverb:start --host=127.0.0.1 --port=8080
./target/debug/main artisan reverb:restart
```

`reverb:start --hostname=edge.example.com` 會在綁定位址旁顯示用戶端連線的主機名稱，預設讀取 `REVERB_HOST`。實際綁定由 `--host` 和 `--port` 決定。使用 `--debug` 在終端機顯示連線、協定事件和發布診斷。

重新啟動命令會將訊號寫入應用程式快取。同一部主機可使用 `CACHE_STORE=file` 共用訊號。獨立程序收到訊號後會關閉監聽器和連線，可由程序管理器再次啟動。內嵌程序隨應用程式關閉及重新啟動。

## 廣播與擴展 {#broadcasting-and-scaling}

設定 `BROADCAST_CONNECTION=reverb`，依[廣播文件](../digging-deeper/broadcasting.md)啟用私人與在線頻道授權。Pusher 通訊協定用戶端連線至設定前綴下的 `/app/{key}`。正式環境可在應用程式的 `allowed_origins` 設定受信任的主機名稱，由反向代理終止 TLS。

在各執行個體設定 `REVERB_SCALING_ENABLED=true`。同一應用程式的節點共用 `REVERB_APP_ID`、`REVERB_APP_KEY`、`REVERB_APP_SECRET`、`REVERB_SCALING_CHANNEL` 和 Redis 連線設定。可以使用 `REDIS_URL` 或分別設定 Redis 參數。不同應用程式共用擴展頻道時應使用不同的應用程式 ID。同一擴展頻道的節點應執行相同版本的 OpenRust Reverb。

訂閱在線頻道時，用戶端會收到全域成員清單和人數。同一使用者建立多個連線仍只計一人，其他訂閱者只會看到該使用者首次加入與最後一次離開。Redis 可用時，節點正常結束會移除其連線。節點異常結束後，仍有訂閱者的節點大約每 2 秒檢查一次，清理已逾 10 秒租期的連線。新訂閱傳回成員清單前也會清理過期連線。

Redis Pub/Sub 跨節點投遞已簽署的 HTTP 發佈和已授權的用戶端事件。用戶端事件只傳送給同一應用程式、同一頻道的其他訂閱連線，排除傳送者的連線。節點失去 Redis 訂閱時會關閉本節點的在線頻道連線，由用戶端在恢復後重新連線並訂閱。事件盡力投遞，斷線期間錯過的事件不會補送。

## Pusher HTTP API 與快取頻道 {#pusher-http-api}

向 `POST /apps/{id}/events` 或 `POST /apps/{id}/batch_events` 傳送簽章請求。批次請求的 `batch` 陣列依序投遞，每項包含事件名稱、頻道、JSON 編碼的 `data`，以及選用的 `socket_id` 和 `info`。伺服器先驗證整批資料，再投遞事件。兩個端點都可以回傳所要求的頻道統計。

使用簽章 GET 請求查詢 `/apps/{id}/connections`、`/apps/{id}/channels`、`/apps/{id}/channels/{channel}` 或 `/apps/{id}/channels/{channel}/users`。頻道清單可用 `filter_by_prefix` 篩選，透過 `info` 選擇 `subscription_count`、`user_count` 或 `cache` 等欄位。啟用 Redis 擴充後，監聽器合併各回應節點的統計，並移除重複的在線頻道使用者 ID。

帶有 `cache-`、`private-cache-` 或 `presence-cache-` 前綴的頻道，在訂閱成功後重播最近一次事件。快取為空時傳送 `pusher:cache_miss`。頻道授權依然適用。遵循 Reverb 的快取生命週期，各監聽器在有本機訂閱者時保留快取，最後一位本機訂閱者離開後清除。

## Laravel Echo {#laravel-echo}

安裝 `laravel-echo` 與 `pusher-js`。下列範例連線到本機未啟用 TLS 的 Reverb 監聽器。設定路徑前綴時，請將 `wsPath` 設為伺服器路徑。

```js
import Echo from "laravel-echo"
import Pusher from "pusher-js"

const echo = new Echo({
  broadcaster: "reverb",
  Pusher,
  key: "your-public-app-key",
  wsHost: window.location.hostname,
  wsPort: 8080,
  forceTLS: false,
  enabledTransports: ["ws"],
})

echo.channel("news").listen(".article.updated", event => console.log(event))
```

使用 `echo.private("orders.42")` 訂閱私人頻道，使用 `echo.join("rooms.42")` 加入在線頻道。兩者都透過應用程式的 `/broadcasting/auth` 和已註冊的頻道守衛授權。連線恢復後，Echo 會自動重新訂閱。

## Pusher Channels {#pusher-channels}

`reverb` Cargo 特性也會註冊 `pusher` 廣播驅動。設定 `BROADCAST_CONNECTION=pusher`、`PUSHER_APP_ID`、`PUSHER_APP_KEY`、`PUSHER_APP_SECRET` 和 `PUSHER_APP_CLUSTER`。預設發布端點為 `https://api-{cluster}.pusher.com:443`。連接相容 Pusher 的測試端點時，可覆寫 `PUSHER_HOST`、`PUSHER_PORT` 和 `PUSHER_SCHEME`。

使用託管 Pusher Channels 時，將 Echo 的 `broadcaster` 設為 `"pusher"`，並傳入公開 Key 和 Cluster。私人與在線頻道仍透過應用程式的 `/broadcasting/auth` 和頻道規則授權。
