# 开发者文档边界整理：docs/zh_CN/digging-deeper/queues.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_CN/digging-deeper/queues.md (迁移前)
+++ docs/zh_CN/digging-deeper/queues.md (开发者正文)
@@ -464,7 +464,7 @@
 kill -s TERM "$WORKER_PID"
 ```

-Unix 上的 `SIGTERM`、`SIGQUIT` 和 `SIGINT` 请求优雅退出，Windows 可用 Ctrl+C。已在 Windows 11 的 SQLite Worker 上使用实际控制台 Ctrl+C 事件验收。守护进程派发 `WorkerInterrupted`，等待当前任务完成，再派发 `WorkerStopping`。监听器可读取信号编号、连接、队列及 Worker 选项。已开始的驱动阻塞预约可能在暂停生效前返回。
+Unix 上的 `SIGTERM`、`SIGQUIT` 和 `SIGINT` 请求优雅退出，Windows 可用 Ctrl+C。守护进程派发 `WorkerInterrupted`，等待当前任务完成，再派发 `WorkerStopping`。监听器可读取信号编号、连接、队列及 Worker 选项。已开始的驱动阻塞预约可能在暂停生效前返回。

 #### 通知运行中的任务 {#notifying-a-running-job}

````
