# 廣播

## 廣播連線

透過 `BROADCAST_CONNECTION` 選擇 `config/broadcasting.rs` 中的連線。預設 `null` 連線捨棄投遞。`log` 驅動程式將頻道名稱、事件名稱、酬載及來源通訊端寫入應用程式記錄。

Provider 可以解析 `BroadcastManager`，用 `extend` 註冊驅動名稱及建構函式。管理程式將所選連線的設定傳給該建構函式。驅動程式實作 `Broadcaster`，負責投遞事件及產生授權回應。

## 頻道授權

用 `Broadcast::channel` 註冊規則，並透過 `Routing::channels` 或 `withBroadcasting` 載入。框架在 Web 中介層群組啟用 GET 和 POST `/broadcasting/auth`。該路由排除防止請求偽造的中介層，訂閱仍須通過驅動程式的授權檢查。

`orders.{id}` 等規則接收已驗證的使用者及命名參數。允許私人訂閱時回傳 `ChannelAuthorization::Allowed`，公開在線成員資料時回傳 `ChannelAuthorization::Presence(data)`。用 `channelWithOptions` 指定守衛的查找順序。執行 `artisan channel:list` 查看已註冊的規則。

## 定義廣播事件

實作 `ShouldBroadcast`，在 Provider 中呼叫 `Dispatcher::registerBroadcast::<E>()` 註冊編譯後的型別。一般事件分派隨後將投遞送入佇列。實作 `ShouldBroadcastNow` 的事件使用 `registerBroadcastNow::<E>()` 註冊，分派時同步投遞。

`broadcastOn` 選擇公開、私人或在線頻道。`broadcastAs` 和 `broadcastWith` 自訂名稱及酬載。預設使用事件名稱和 `broadcastData` 明確回傳的欄位。`broadcastWhen` 回傳 false 時跳過投遞。佇列儲存序列化後的投遞資料。

實作 `ShouldDispatchAfterCommit` 並透過 `Dispatcher::markAfterCommit::<E>()` 標記事件後，進行中交易內的事件將延後至提交後分派。佇列廣播也遵循 `broadcastAfterCommit` 和佇列連線的 `after_commit`。使用 `EventContext::toOthers(&request)` 將請求的 `X-Socket-ID` 傳給驅動程式，供驅動程式排除來源連線。

內建連線為 `null` 和 `log`。啟用 [Reverb 擴充套件](../packages/reverb.md) 後可使用相容 Pusher 的傳輸。
