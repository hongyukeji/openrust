# 錯誤處理

## 報告及轉譯例外

在 `bootstrap/app.rs` 的 `withExceptions` 閉包中註冊回呼。用 `report::<E>` 報告型別為 `E` 的錯誤，比對範圍包括錯誤的來源鏈。報告回呼依註冊順序執行。傳回 `ReportDecision::Continue` 繼續報告，傳回 `ReportDecision::Stop` 停止後續回呼和預設報告。一般 HTTP 400、403、404 和 405 不會報告。未被回呼截停的 HTTP 500 會將診斷寫入 stderr。Artisan 報告命令執行和引導錯誤，並保留預設的 stderr 輸出和結束狀態。

用 `render::<E>` 根據錯誤和請求傳回 HTTP 回應。傳回 `None` 時繼續嘗試後續回呼，最後回退至預設 JSON 或 HTML 轉譯。用 `respond` 調整最終例外回應，自訂和預設回應都會經過它。這兩個回呼僅用於 HTTP。

## 將例外轉譯為 JSON

應用程式在 `bootstrap/app.rs` 中透過 `withExceptions` 宣告例外策略。路徑符合 `api/*` 或請求預期 JSON 時，框架傳回 JSON 例外回應。其他請求收到 HTML 錯誤頁。

例如，存取不存在的 API 路由會傳回 `404` 和包含 `message` 欄位的 JSON。存取不存在的 Web 路由會傳回 HTML `404`，如果請求的 `Accept` 優先選擇 JSON 媒體類型，則傳回 JSON。

## 路由不存在

應用的 `withExceptions` 策略對符合 `api/*` 的路徑或期望 JSON 的請求回傳 JSON，其他缺失路由回傳 HTML。回應格式改變，404 狀態保持。缺失靜態檔案也使用此策略；HEAD 保留狀態和內容類型，本文為空。

```sh
curl -i http://localhost:8000/missing
curl -i http://localhost:8000/api/missing
curl -i -H 'Accept: application/json' http://localhost:8000/missing
```
