# 本地化与文本机器翻译验收

范围和正式协议见[拆分计划](../../plans/localization-and-text-translation.md)。使用固定 Laravel v13.40.0 的词条解析和装配行为，以及计划已核定的 Rust 命名。复核 `v0.x` 的语言目录和 FileLoader 提交 `0d34dfd2c`、`d61369b56`，沿用主线已有实现。

## L10N-01 核心迁移

| 原入口 | 新入口及核对 |
| --- | --- |
| `openrust::translation` 和 `openrust::contracts::translation` | `openrust::localization` 和 `openrust::contracts::localization`。Rust、生成模板、应用、各包、测试与三语导入均已迁移。 |
| 具体类型／trait `Translator`、`TranslationServiceProvider` | 分别迁到 `Localizer`、`LocalizationServiceProvider`。容器具名 `lang`、具体类型和 trait 解析同一实例。 |
| 具名 `translation.loader` | `lang.loader` 和 `dyn Loader` 指向同一来源。直接管理应用路径与包语言资源的消费者按具体类型解析 `FileLoader`。 |
| `Validator::get_translator`、`set_translator` | `get_localizer`、`set_localizer`。Validator Factory、异常工厂、RuleContext 和未知字段消息使用 `Localizer`。 |
| `Lang`、`__`、`trans_choice`、`lang:publish` | 行为、语言标签、配置及应用 `lang/` 契约沿用。内置的 180 份语言资源字节一致。 |

工作树基于 `1989780ba`，分支 `codex/localization-translate`。原工作树和既有暂存区由原协作者继续维护。产物与日志在主仓库 `target/tmp/localization-translate/`。仅本地提交。

| 验收 | 结果 |
| --- | --- |
| Rust 1.94.0 格式、严格 Clippy | `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 clippy --workspace --all-targets --locked -- -D warnings` 通过。首次构建发现旧模块可见性路径，改为新路径后重跑。 |
| 三语站点构建 | `npm run build` 通过，三语各 112 个导航章节、114 个 Markdown 文件通过内容检查。 |
| 三语浏览器 | 真实 Chrome 桌面／移动、`/`／`/docs/` 共 12 组，另逐个渲染两种站点路径下的三语导航。通过 `check_browser.mjs`、`check_scope.mjs`。 |
| 框架回归 | 10 个测试目标共 79 项通过，覆盖具名／类型／trait 同一实例及旧键退出、路由中间件、应用作用域、BCP 47、Blade、复数选择器、FileLoader、包资源和 Validator。另单独执行 2 项邮件／通知语言测试，含独立 Artisan Worker，均通过。 |
| 根应用回归 | `application_skeleton`、`feature`、`localization`、`localization_resources`、`welcome` 共 70 项通过。应用测试含 Artisan 本地化帮助与退出码、18 语通知和 Blade 页面。 |
| 实际根进程与资源 | `localization_bundled_http.py` 的 4 套候选配置，`localization_18_acceptance.py` 的 18 语帮助及失败提示、两种语言目录的发布／保留／覆盖、一共 36 次终端生成，以及 `localization_standard_resources.py` 的 4 项均通过。 |
| 18 语真实页面 | 独立应用与真实 Chrome 的 18 语 × 4 组设备、主题组合共 72 项通过。 |
| 生成应用 | 独立 MySQL 8.4.13 执行 `artisan init` 成功，重新构建生成应用。另从该二进制实际验证英、简中、繁中、日文 HTTP 与独立 `lang:publish`。旧完整 init 脚本的无关 `/api/user` 断言返回 404。生成应用保留原 `routes/api.rs`，该文件在基线只注册 API 根路径。认证路由仍由其专项处理。首次 init 尝试触及构建守卫停线，已用独立目录重试，原日志保留。 |

## L10N-02 默认 Loader 与数据库词条

- `lang.loader` 是应用 Provider 替换入口，`dyn Loader` 为同一绑定。应用启动在所有普通 Provider 的 boot 后读取声明，文件路径注册先完成。空列表不会解析持有数据库连接的来源。
- `lang.resources` 声明精确语言、组和可选命名空间。`Lang::refresh_default_source` 和 `Localizer::refresh_default_source` 锁外读取整批，校验后交换来源快照。竞争中的旧刷新返回 `CatalogChanged`。运行时词条层独立，刷新失败与来源删除均不覆盖它。
- 数据库包迁到 `openrust-localization-database`。既有结构、JSON、SQL 升级脚本未改动。新增的可选发布资源沿用 `create_language_lines_table` 后缀，并按现有 Artisan 发布规则保留已发布文件。原包没有自动登记迁移，已有应用继续使用自己的迁移标识。
- 定向框架 11 项通过，覆盖启动顺序、错误、并发刷新、词条删除与回到文件模式。数据库 9 项各自在 SQLite、MySQL 8.4.13、PostgreSQL 18.7 通过，含独立应用真实 TCP、现有 JSON 数据语义与升级脚本。SQLite 另编译通过 4 项包文档用例。
- `tests/localization_database_acceptance.py` 三库全部通过，实跑独立 HTTP、Artisan 和数据库 Worker。发布迁移、重复发布、`migrate`、`lang:publish`、缺表启动失败、HTTP 刷新失败保留旧值、删词回退及缓存重启均有断言。MySQL／PostgreSQL 使用项目隔离容器与 REQUIRE 门禁。
- 格式、工作区严格 Clippy、含可选 feature 的消费者严格 Clippy、三语构建、两挂载 Chrome 12 组和数据库章节额外 6 组均通过。日志见 `target/tmp/localization-translate/l10n-02-*`。
