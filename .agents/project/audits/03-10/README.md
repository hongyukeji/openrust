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

## 发布默认语言目录

对照固定 Laravel `LangPublishCommand` 及四份英文语言资源。将 PHP 数组转为运行时可读取的 JSON，保留上游 MIT 许可证，命令运行无需 PHP。发布与加载共享路径解析，已有 `resources/lang` 时写入该目录，其他应用写入根 `lang/en`。旧版 `v0.x` 有语言文件及加载器，未找到发布命令。 [固定 Laravel 命令](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/LangPublishCommand.php)

macOS ARM64 使用 Rust 1.94.0 构建真实二进制，在临时应用运行默认发布、重复发布、`--existing`、`--force` 及两个选项同时使用。核对四个 JSON 文件、既有自定义值和缺失文件的处理，验证旧目录可发布。三语文档构建及 12 项浏览器检查通过。

## 语言标识归一

应用配置、翻译资源和文档路径统一使用 `en`、`en_GB`、`zh_CN`、`zh_TW` 等标识。ICU4X 的语言标识解析统一大小写、分隔符、文字和地区，保留明确指定的地区目录。简繁中文文字别名归到默认目录。HTML 使用 BCP 47 语言标签。既有资源需采用归一后的文件名。 [固定 Laravel 本地化文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/localization.md#defining-translation-strings) · [Unicode UTS #35](https://www.unicode.org/reports/tr35/)

隔离运行 `cargo +1.94.0 test --locked --offline --test localization --test feature` 及框架 `translation_choice`、`validation_http`，共 16 项通过。实际 Artisan 验证中英文变体和非法环境配置拒绝。格式和全目标严格 Clippy 通过。文档构建覆盖根路径和子路径，核对语言切换、搜索、404 和站点正文。

## 翻译存在性、回退顺序与缺键回调

- 对照固定 Laravel Translator 的 `has`、`hasForLocale`、`handleMissingKeysUsing`、`determineLocalesUsing`。复用旧版 `v0.x` 的应用级回调和同步重入保护，接入现有 JSON 字面键优先与分组回退。存在性检查跳过缺键回调，区分原样的分组文本与实际登记的 JSON 字面键。 [固定 Laravel Translator](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Translation/Translator.php)
- 回退回调可重排分组语言，返回语言先规范化再去重。缺键回调在正常替换前运行，允许安全查询同一译器。回调运行期间释放内部锁，解除回调时恢复默认机制。Rust 用类型化上下文提供键、替换参数与语言，错误由 `Result` 返回。
- 隔离基线 `fb2a0bf39`，Rust 1.94.0。定向测试 8 项通过，包括两套实际 HTTP 应用、中文别名、缺键处理、精确存在性、回退顺序、复数、非法语言拒绝、回调 panic 后恢复、Validator 翻译。三语文档共用示例编译后，实际 Artisan 以 `fr-CA`、`zh-Hant` 和 `en` 验证回退及替换，另实跑 `lang:publish` 与 `inspire`。
- 改动文件格式、工作区严格 Clippy、三语构建、12 项通用浏览器和 6 项正文锚点／示例检查通过。全仓格式仍报告既有 `sanctum_authentication.rs:1412`。本批隔离构建缓存已回收。磁盘可用 115.21 GB（107.29 GiB），周额度剩余 66%，第二窗口无读数。证据目录 `/private/tmp/openrust-03-10-hooks.KqyTG6Ej`。
- 03-10 接续运行期翻译行、动态加载源及替换扩展。核心账本保持 54/66。
