# Vite 契约期望

- 行为源：Framework v13.40.0，提交 `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 [Vite.php](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Vite.php)。
- `manifest.json` 为本项目的受控输入，`contract.json` 为该类与原 Collection 在 PHP 8.5.12 的实际输出。辅助环境使用该版本声明的 Symfony polyfill-php86 v1.36.0 原始 SortDirection；不替换排序算法。
- 全局 public_path／asset 仅提供固定夹具路径及URL，本对照不代表完整 PHP Application／HTTP 部署验收。Rust 自身使用真实应用路径和 URL 生成器，另有真实 HTTP Kernel、Artisan、MySQL／Redis Worker 与 Vite 构建／浏览器验证。
- 验证内容包括静态导入、CSS 与预加载顺序、完整性摘要、nonce、属性、开发标签、React前置脚本，以及具有副作用的路径解析回调9次调用顺序。React应用本身的HMR不包含在原始字符串对照结论中。
- 本地脚本、固定源字节摘要、原始错误和成功输出位于 `target/tmp/facade-alignment/vite/`，按项目规则不跟踪PHP脚本；重新执行须准备同一固定源与原生配套依赖。


`fonts.json` 和 `fonts-contract.json` 对照同一固定提交的 Foundation/ViteFonts.php 与 Vite::fonts 原实现，涵盖别名顺序、重复预加载、nonce、清理缓存、自定义属性、字体清单名称、自定义 hot 目录和回调中修改配置。来源记录及PHP实际执行输出保存在 `target/tmp/facade-alignment/vite-fonts/`；不将本地对照脚本作为运行依赖。OpenRust沿用既有字体CSS公开目录能力边界及style结束标签检查，其余受信任标签属性遵循固定资源输出契约。


`prefetch-manifest.json` 与 `prefetch-contract.json` 对照固定Vite.php的动态导入遍历及两种脚本输出，包含循环、共享导入、CSS、特殊字符、事件切换和失败分支。`foundation/vite/stubs/`中的脚本保留同一固定源码的可观察调度行为；本地对照、真实Worker及Chrome网络并发证据保存在 `target/tmp/facade-alignment/vite-prefetch/`。序列化复用框架已有Blade JavaScript编码。
