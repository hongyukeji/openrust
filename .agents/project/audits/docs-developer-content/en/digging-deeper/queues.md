# 开发者文档边界整理：docs/en/digging-deeper/queues.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/en/digging-deeper/queues.md (迁移前)
+++ docs/en/digging-deeper/queues.md (开发者正文)
@@ -464,7 +464,7 @@
 kill -s TERM "$WORKER_PID"
 ```

-On Unix, `SIGTERM`, `SIGQUIT`, and `SIGINT` request a graceful stop. On Windows, Ctrl+C requests the same stop. On Windows 11, the SQLite Worker was verified using a console-generated Ctrl+C event. The daemon emits `WorkerInterrupted` before `WorkerStopping` and lets its current job finish. The signal number, connection, queues, and worker options are available to listeners. A blocking driver reservation that has already begun can return before a pause takes effect.
+On Unix, `SIGTERM`, `SIGQUIT`, and `SIGINT` request a graceful stop. On Windows, Ctrl+C requests the same stop. The daemon emits `WorkerInterrupted` before `WorkerStopping` and lets its current job finish. The signal number, connection, queues, and worker options are available to listeners. A blocking driver reservation that has already begun can return before a pause takes effect.

 #### Notifying a running job {#notifying-a-running-job}

````
