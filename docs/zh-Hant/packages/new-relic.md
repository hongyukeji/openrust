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

## 收集內容 {#telemetry}

HTTP 遙測包含伺服器 Span 和 `http.server.request.duration` 直方圖。符合的請求按已註冊的路由範本彙總，未符合的請求省略路由標籤。記錄回應狀態碼，並將 5xx 標為錯誤。收到 W3C Trace Context 時可接續上游追蹤。HTTP 耗時統計到應用程式產生回應為止。

Artisan 命令完成和有計時資訊的佇列執行嘗試都會產生 Span 和耗時指標。執行失敗會標為錯誤。收集範圍不包含請求或回應本文、URL 參數、Cookie、命令引數、工作酬載或例外訊息。獨立命令與 HTTP 程序正常結束時會送出緩衝的遙測資料。匯出使用有界 Span 佇列並重試暫時性故障，超載或持續故障可能造成資料遺失。

此整合不轉送日誌，不會自動產生 SQL 或外送 HTTP 子 Span，也不收集例外堆疊。佇列與命令耗時指標可分別使用 `openrust.queue.job.duration` 和 `openrust.artisan.command.duration` 查詢。
