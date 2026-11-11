# 03-10 国际化、回退和复数

## 契约与实现

- 对照固定 Laravel Translator、FileLoader、MessageSelector、TranslationServiceProvider 与翻译测试。Goravel 的应用容器保存 loader 与 translator，作为 OpenRust 装配参考。 [Laravel Localization](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/localization.md) · [Goravel TranslationServiceProvider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/translation/service_provider.go)
- 应用级翻译服务按需读取并缓存资源，分组和列表可用点号读取。分组键查当前语言及回退语言，普通 JSON 字面键只查当前语言。缺失键返回键本身。包路径覆盖递归合并，拒绝路径穿越，解析失败留待重试。
- 从 `openrust-v0.x` 的 `6d1a8192` 提取复数规则、范围、替换大小写与 Unicode 首字母映射，附 Zend 和 Unicode 许可。计数区分整数和双精度，精确整数避免预先转换成浮点。复数选择使用命中资源的语言规则，支持调用方提供 `:count`。
- Rust 分组资源使用 JSON。应用和任务通过 `Lang::withLocale` 切换作用域语言，任务结束或取消时 Tokio 恢复外层作用域。翻译 API 以 `Result` 公开资源错误。ICU 扩展、动态翻译源、Artisan 输出本地化和 Windows 实机仍待覆盖。Artisan 本地化归入 06-08。

## 验收

隔离基线 `21a65e98` 加已验收的依赖与本批改动，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，测试报告 397 项通过。本批未启用真实数据库和外部存储。
- 两个实际 HTTP 应用并发处理英语、简体中文和繁體中文，验证替换大小写、分组回退、JSON 字面键、复数、嵌套列表、缺键以及恢复两个不同的默认语言。专项测试覆盖命名空间覆盖、路径穿越拒绝、修复后重试、显式范围、地区表和大整数。
- 从三语文档逐一提取并编译 Rust 和 JSON 示例，每个示例实际发出三种语言的请求。执行 `./artisan inspire` 成功。
- 文档站构建成功，根路径与子路径通过 12 项通用浏览器检查与 6 项国际化正文专项检查。

下一批为 04-01 路由参数、分组及诊断，依赖 00-02 和 01-01。
