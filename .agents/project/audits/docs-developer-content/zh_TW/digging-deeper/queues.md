# 开发者文档边界整理：docs/zh_TW/digging-deeper/queues.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_TW/digging-deeper/queues.md (迁移前)
+++ docs/zh_TW/digging-deeper/queues.md (开发者正文)
@@ -464,7 +464,7 @@
 kill -s TERM "$WORKER_PID"
 ```

-Unix 上的 `SIGTERM`、`SIGQUIT` 和 `SIGINT` 請求正常停止，Windows 可用 Ctrl+C。已在 Windows 11 的 SQLite Worker 上使用實際主控台 Ctrl+C 事件驗收。常駐程序派發 `WorkerInterrupted`，等待目前工作完成，再派發 `WorkerStopping`。監聽器可讀取訊號編號、連線、佇列及 Worker 選項。已開始的驅動程式阻塞預約可能在暫停生效前傳回。
+Unix 上的 `SIGTERM`、`SIGQUIT` 和 `SIGINT` 請求正常停止，Windows 可用 Ctrl+C。常駐程序派發 `WorkerInterrupted`，等待目前工作完成，再派發 `WorkerStopping`。監聽器可讀取訊號編號、連線、佇列及 Worker 選項。已開始的驅動程式阻塞預約可能在暫停生效前傳回。

 #### 通知執行中的工作 {#notifying-a-running-job}

````
