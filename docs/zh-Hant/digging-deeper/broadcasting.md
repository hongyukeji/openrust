# 廣播

## 廣播連線

透過 `BROADCAST_CONNECTION` 選擇 `config/broadcasting.rs` 中的連線。預設 `null` 連線捨棄投遞。`log` 驅動程式將頻道名稱、事件名稱、酬載及來源通訊端寫入應用程式記錄。

Provider 可以解析 `BroadcastManager`，用 `extend` 註冊驅動名稱及建構函式。管理程式將所選連線的設定傳給該建構函式。驅動程式實作 `Broadcaster`，負責投遞事件及產生授權回應。

## 頻道授權

用 `Broadcast::channel` 註冊規則，並透過 `Routing::channels` 或 `with_broadcasting` 載入。框架在 Web 中介層群組啟用 GET 和 POST `/broadcasting/auth`。該路由排除防止請求偽造的中介層，訂閱仍須通過驅動程式的授權檢查。

`orders.{id}` 等規則接收已驗證的使用者及命名參數。允許私人訂閱時回傳 `ChannelAuthorization::Allowed`，公開在線成員資料時回傳 `ChannelAuthorization::Presence(data)`。用 `channel_with_options` 指定守衛的查找順序。執行 `artisan channel:list` 查看已註冊的規則。

對於 `orders.{order}`，使用 `channel_with_bindings` 和 `ChannelBindings::default().model::<Order>("order")` 宣告模型參數。實作 `ChannelHandler::join_bound`，透過 `models.get::<Order>("order")` 取得模型。查詢使用模型的路由鍵，支援 slug 等自訂鍵。隱式模型缺失時回傳 HTTP 403。同名的 `Router::bind` 或 `Router::model` 顯式綁定優先，找不到時回傳 HTTP 404。各頻道模型分別解析。

重疊規則依註冊順序執行，使用首個符合規則指定的守衛。回傳 `ChannelAuthorization::Abstain` 繼續嘗試下一條規則，回傳 `Denied` 則立即拒絕。

## 定義廣播事件

實作 `ShouldBroadcast`，在 Provider 中呼叫 `Dispatcher::register_broadcast::<E>()` 註冊編譯後的型別。一般事件分派隨後將投遞送入佇列。實作 `ShouldBroadcastNow` 的事件使用 `register_broadcast_now::<E>()` 註冊，分派時同步投遞。

`broadcast_on` 選擇公開、私人或在線頻道。`broadcast_as` 和 `broadcast_with` 自訂名稱及酬載。預設使用事件名稱和 `broadcast_data` 明確回傳的欄位。`broadcast_when` 回傳 false 時跳過投遞。佇列儲存序列化後的投遞資料。

實作 `ShouldDispatchAfterCommit` 並透過 `Dispatcher::mark_after_commit::<E>()` 標記事件後，進行中交易內的事件將延後至提交後分派。佇列廣播也遵循 `broadcast_after_commit` 和佇列連線的 `after_commit`。使用 `EventContext::to_others(&request)` 將請求的 `X-Socket-ID` 傳給驅動程式，供驅動程式排除來源連線。

## 傳送匿名廣播 {#anonymous-broadcasting}

使用 `Broadcast::on("news")`、`Broadcast::private("orders.42")` 或 `Broadcast::presence("rooms.42")` 建立匿名廣播。`Broadcast::on` 也接受頻道清單。透過 `r#as` 設定傳送名稱，透過 `with` 設定 JSON 資料，透過 `via` 選擇廣播連線。預設傳送名稱為 `AnonymousEvent`。

等待 `send()` 執行一般佇列投遞，或等待 `send_now()` 同步傳送。請求包含 `X-Socket-ID` 時，`to_others(&request)` 會排除來源通訊端。預設接收對象包含來源通訊端。

## 唯一佇列廣播 {#unique-broadcasts}

實作 `contracts::broadcasting::ShouldBeUnique` 後，用 `Dispatcher::register_unique_broadcast::<E>()` 註冊。唯一性由事件的 Rust 型別和 `unique_id()` 共同決定。用 `unique_for()` 設定鎖定期限秒數，用 `unique_via()` 選擇具名快取儲存區。預設 ID 為空，鎖定無到期時間並使用預設快取儲存區。

鎖定已被持有時，佇列派送 `UniqueJobSkipped`。重試期間維持鎖定，工作成功或永久失敗後釋放。同步廣播無需唯一鎖定。顯式 Rust 交易中的唯一佇列投遞在提交時取得鎖定，交易回復會捨棄待投遞工作。交易進行期間，唯一投遞若明確指定 `broadcast_after_commit() == Some(false)`，會回傳設定錯誤。

內建連線為 `null` 和 `log`。啟用 [Reverb 擴充套件](../packages/reverb.md) 後可使用相容 Pusher 的傳輸。
## 使用者認證路由

`Broadcast::user_routes(group, connection)` 註冊 GET、POST `/broadcasting/user-auth`，使用 web 中介軟體群組並豁免 CSRF。預設路由群組或連線傳入 `None`。透過 `authenticateUserUsing` 登記可讀取目前請求的解析回呼，傳回使用者 JSON 資料，訪客傳回無使用者。使用者認證和頻道認證各自獨立。

Reverb 驅動驗證 socket ID，並按 Pusher 協定簽署使用者認證回應，具名連線使用各自憑證。其他未實作驅動傳回 `UnsupportedUserAuthentication`。該介面不會為 Reverb 伺服器增加 `pusher:signin` 處理器；託管平台和完整用戶端登入仍需另行整合。
