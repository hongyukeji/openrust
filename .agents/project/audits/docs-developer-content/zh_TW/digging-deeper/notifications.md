# 开发者文档边界整理：docs/zh_TW/digging-deeper/notifications.md

2026-11-20 历史原文对照。

````diff
--- docs/zh_TW/digging-deeper/notifications.md (迁移前)
+++ docs/zh_TW/digging-deeper/notifications.md (开发者正文)
@@ -254,7 +254,7 @@
     .await?;
 ```

-Bot API 使用 `chat.postMessage`。本機協定驗收可將回送 HTTP URL 設為 `services.slack.notifications.endpoint`。在 Slack 內確認送達需要安裝應用程式、授予 Bot 權限並使用已授權的工作區。
+Bot API 使用 `chat.postMessage`。應用程式測試時，可將本機模擬服務的 HTTP URL 設為 `services.slack.notifications.endpoint`。在 Slack 內確認送達需要安裝應用程式、授予 Bot 權限並使用已授權的工作區。

 ### Block Kit 選擇器 {#slack-block-kit-selects}

````
