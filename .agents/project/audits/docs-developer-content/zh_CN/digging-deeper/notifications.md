# 开发者文档边界整理：docs/zh_CN/digging-deeper/notifications.md

2026-11-20。历史原文对照，不作为当前验收结论。

````diff
--- docs/zh_CN/digging-deeper/notifications.md (迁移前)
+++ docs/zh_CN/digging-deeper/notifications.md (开发者正文)
@@ -254,7 +254,7 @@
     .await?;
 ```

-Bot API 使用 `chat.postMessage`。本地协议验收可将回环 HTTP URL 配置为 `services.slack.notifications.endpoint`。在 Slack 内确认送达需要安装应用、授予 Bot 权限并使用获授权的工作区。
+Bot API 使用 `chat.postMessage`。应用测试时，可将本地模拟服务的 HTTP URL 配置为 `services.slack.notifications.endpoint`。在 Slack 内确认送达需要安装应用、授予 Bot 权限并使用获授权的工作区。

 ### Block Kit 选择器 {#slack-block-kit-selects}

````
