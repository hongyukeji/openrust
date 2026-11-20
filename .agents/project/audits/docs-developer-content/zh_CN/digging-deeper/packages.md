# 开发者文档边界整理：docs/zh_CN/digging-deeper/packages.md

2026-11-20 历史原文对照。

````diff
--- docs/zh_CN/digging-deeper/packages.md (迁移前)
+++ docs/zh_CN/digging-deeper/packages.md (开发者正文)
@@ -39,7 +39,7 @@
 | `loadMigrationsFrom` | 静态的编译型 `MigrationEntry` 切片。Artisan 使用应用的迁移记录执行和回滚包迁移。 |
 | `commands` | 使用 `Artisan` 注册命令的编译型函数，由控制台内核启动时调用。 |

-通过启动时加载的 Provider 注册包 HTTP 路由，确保路由装配时能够读取。部署时随应用分发视图和翻译文件，并将发布后的目录传给 Provider。可运行的宿主与 Provider 见[独立 crate 验收示例](https://github.com/hongyukeji/openrust/tree/master/vendor/openrust/framework/tests/fixtures/package)。
+通过启动时加载的 Provider 注册包 HTTP 路由，确保路由装配时能够读取。部署时随应用分发视图和翻译文件，并将发布后的目录传给 Provider。可运行的宿主与 Provider 见[独立 crate 示例](https://github.com/hongyukeji/openrust/tree/master/vendor/openrust/framework/tests/fixtures/package)。

 ## 发布资源

````
