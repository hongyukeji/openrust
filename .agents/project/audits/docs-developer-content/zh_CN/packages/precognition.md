# 开发者文档边界整理：docs/zh_CN/packages/precognition.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_CN/packages/precognition.md (迁移前)
+++ docs/zh_CN/packages/precognition.md (开发者正文)
@@ -48,7 +48,7 @@

 ## JavaScript helper

-官方 `laravel-precognition` 核心 helper `2.0.0` 已通过 Chromium 与真实 OpenRust HTTP 路由验收：
+使用官方 `laravel-precognition` 核心 helper `2.0.0` 调用配置好的 OpenRust 路由：

 ```js
 import { client } from 'laravel-precognition';
````
