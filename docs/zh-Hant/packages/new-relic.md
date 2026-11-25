# New Relic APM（擴充）

## 啟用遙測 {#configuration}

將 `"new-relic"` 加入應用程式 `Cargo.toml` 的 `[features].default` 陣列，保留既有功能。若尚無該陣列，請先建立，再重新建置。套件探索會自動註冊 `NewRelicServiceProvider`。獨立使用擴充套件的應用程式可以明確註冊此 Provider。

為需要回報遙測的 HTTP 和 Worker 程序設定環境變數。

```dotenv
NEW_RELIC_ENABLED=true
NEW_RELIC_APP_NAME=Orders
NEW_RELIC_REGION=us
```

透過執行階段環境提供 New Relic 擷取授權金鑰 `NEW_RELIC_LICENSE_KEY`。Provider 啟動時讀取授權金鑰，設定快取不會儲存該值。服務名稱預設採用應用程式名稱。依帳號的擷取區域選擇 `us`、`eu`、`jp` 或 `us-gov`。變更啟用狀態、服務名稱或區域後，重新產生應用程式設定快取。

使用 OpenTelemetry Collector 時，將 `NEW_RELIC_COLLECTOR_ENDPOINT` 設為其基礎 URL。接受 HTTPS 或使用回送 IP 的 HTTP 位址，位址不得包含認證資訊、路徑、查詢或片段。應用程式使用 OTLP HTTP/protobuf 傳送至 `/v1/traces` 和 `/v1/metrics`。請在 Collector 的 New Relic 匯出器中設定授權金鑰。

需要將 W3C 追蹤傳遞給可信的下游服務時，將 `NEW_RELIC_PROPAGATE_ORIGINS` 設為以逗號分隔的精確來源，例如 `https://api.example.com,https://internal.example.com:8443`。比對通訊協定、主機和有效連接埠。留空時停用自動注入。變更後重新產生設定快取。

## 收集內容 {#telemetry}

HTTP 遙測包含伺服器 Span 和 `http.server.request.duration` 直方圖。符合的請求按已註冊的路由範本彙總，未符合的請求省略路由標籤。記錄回應狀態碼，並將 5xx 標為錯誤。收到 W3C Trace Context 時可接續上游追蹤。HTTP 耗時統計到應用程式產生回應為止。

Artisan 命令（包含說明、清單及版本快速入口）與佇列執行嘗試都會產生 Span 和耗時指標。執行失敗會標為錯誤。收集範圍不包含請求或回應本文、URL 參數、Cookie、命令引數、工作酬載或例外訊息。獨立命令與 HTTP 程序正常結束時會送出緩衝的遙測資料。匯出使用有界 Span 佇列並重試暫時性故障，超載或持續故障可能造成資料遺失。

執行期間的外送 HTTP 與 SQL 操作以目前的 Span 為父節點。HTTP 用戶端 Span 涵蓋每次網路嘗試，直到回應本文傳輸完成或該次嘗試出錯。資料庫 Span 與耗時指標涵蓋預備陳述式、交易查詢與游標，屬性只採用驅動程式和固定的操作名稱，不收集 SQL 本文或繫結值。

透過佇列管理器派發的工作可以攜帶選用追蹤中繼資料。批次派發時，每個工作使用獨立的建立 Span。非同步 Worker 的 Span Link 會關聯工作的來源。既有工作缺少此欄位時仍可照常執行。佇列與命令耗時指標可分別使用 `openrust.queue.job.duration` 和 `openrust.artisan.command.duration` 查詢。

## 日誌與例外 {#logs-and-errors}

設定 `NEW_RELIC_LOGS_ENABLED=true`，並在 `logging.channels` 中明確新增通道。下例是通道的設定值。

```json
{
  "driver": "monolog",
  "handler": "new_relic",
  "formatter": "default",
  "level": "error",
  "bubble": true,
  "with": {"fields": ["context.operation"], "report_errors": true}
}
```

只有選用此通道的日誌會傳送。預設格式器使用結構化記錄及其訊息。透過 `with.fields` 明確列出允許匯出的頂層 context 或 extra 欄位。處理器保留有限長度的純量和簡單陣列，並附加日誌產生時的 Trace ID 和 Span ID。只需轉送日誌時設定 `report_errors=false`。已由應用程式例外處理器回報的錯誤，經此通道轉送時不會再建立一個錯誤 Span。使用預設格式器可避免把其他欄位寫入匯出訊息。可用 `with.transaction_name` 設定固定交易名稱，或在 `with.fields` 中允許 `context.transaction_name` 讓日誌覆寫該名稱。日誌只能重新命名自身仍在執行的交易。

例外回報使用錯誤類型和安全摘要。設定 `NEW_RELIC_EXCEPTION_STACKTRACE=true` 後，只有應用程式已透過例外上下文提供 `stacktrace` 欄位，例如由 `ExceptionBehavior` 提供在錯誤來源擷取的堆疊時才會傳送截斷後的堆疊。啟用前請確認該欄位適合匯出。

## 執行控制 {#runtime-controls}

擴充啟用後預設傳送追蹤與指標。可用 `NEW_RELIC_TRACES_ENABLED=false` 或 `NEW_RELIC_METRICS_ENABLED=false` 個別停用。將 `NEW_RELIC_TRACE_SAMPLE_RATIO` 設為 0 到 1，為根 Span 設定取樣比例，並沿用父 Span 的取樣決定。指標計數不受追蹤取樣影響。需要明確區分服務副本時，可設定 `NEW_RELIC_INSTANCE_ID`。使用設定快取的應用程式在變更這些設定後需重新產生快取。

從應用程式容器解析 `openrust_new_relic::NewRelic`。在交易作用域內，可用 `name_transaction` 和 `add_attribute` 更新 Span 名稱與有限的自訂屬性，HTTP 指標繼續依路由彙總。嵌入式使用者可在自己的工作結束後呼叫 `flush().await` 或 `shutdown().await`。`export_stats()` 統計實際 HTTP 請求嘗試與失敗，重試個別計數。`dropped_spans` 和 `dropped_logs` 統計應用程式的匯出佇列已滿時丟棄的記錄。`rejected_items` 統計接收端在成功 OTLP 回應中明確拒收的記錄。部分接收不會由此擴充重新傳送。
