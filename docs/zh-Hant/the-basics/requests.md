# HTTP 請求

## 取得輸入

用 `request.query(Some("key"))` 讀取精確的頂層查詢鍵。用 `request.input(Some("team.name")).await` 讀取巢狀輸入，傳入 `None` 可讀取整組輸入。JSON 和表單的頂層欄位優先於查詢參數。只讀取 JSON 本文時使用 `request.json`。

OpenRust 成功讀取本文後會保留原始位元組，供後續讀取。預設上限為 8 MiB。輸入無效回傳 400，超量回傳 413。

## 檔案

用 `request.file("attachments.0").await` 讀取 multipart 上傳檔案，用 `request.hasFile("attachments").await` 檢查欄位是否包含檔案。上傳物件提供檔案位元組，以及用戶端提供的檔名和媒體類型。儲存前應檢查檔案內容。

輸入讀取及 JSON 回應的完整範例見[控制器](./controllers.md)。

## Cookie

用 `request.cookie(Some("theme"))?` 讀取指定 Cookie，用 `request.cookie(None)?` 讀取整個 Cookie 集合。啟用 `EncryptCookies` 後，受保護的 Cookie 會在進入處理器前解密並驗證。密文無效或名稱綁定不符時，讀取到的值為 null。
