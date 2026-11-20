# 开发者文档边界整理：docs/en/packages/precognition.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/en/packages/precognition.md (迁移前)
+++ docs/en/packages/precognition.md (开发者正文)
@@ -48,7 +48,7 @@

 ## JavaScript helper

-The official `laravel-precognition` core helper `2.0.0` has been tested in Chromium against actual OpenRust HTTP routes:
+Use the official `laravel-precognition` core helper `2.0.0` with the configured OpenRust routes:

 ```js
 import { client } from 'laravel-precognition';
````
