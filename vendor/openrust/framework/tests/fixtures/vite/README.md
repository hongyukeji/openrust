# Vite 契约期望

- 行为源：Framework v13.40.0，提交 `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 [Vite.php](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Vite.php)。
- `manifest.json` 为本项目的受控输入，`contract.json` 为该类与原 Collection 在 PHP 8.5.12 的实际输出。辅助环境使用该版本声明的 Symfony polyfill-php86 v1.36.0 原始 SortDirection；不替换排序算法。
- 全局 public_path／asset 仅提供固定夹具路径及URL，本对照不代表完整 PHP Application／HTTP 部署验收。Rust 自身使用真实应用路径和 URL 生成器，另有真实 HTTP Kernel、Artisan、MySQL／Redis Worker 与 Vite 构建／浏览器验证。
- 验证内容包括静态导入、CSS 与预加载顺序、完整性摘要、nonce、属性、开发标签、React前置脚本，以及具有副作用的路径解析回调9次调用顺序。React应用本身的HMR不包含在原始字符串对照结论中。
- 本地脚本、固定源字节摘要、原始错误和成功输出位于 `target/tmp/facade-alignment/vite/`，按项目规则不跟踪PHP脚本；重新执行须准备同一固定源与原生配套依赖。
