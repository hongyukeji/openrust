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

## TR-01 文本翻译底座

- 新增独立可选 `openrust-translate`。已启用的应用由包发现注册同一个具名／类型化 `TranslateManager`，`translator` 绑定和 `lang` 各自装配。默认应用保持按需启用。
- `TranslationRequest` 规范化 BCP 47，拒绝空白原文，保留非空原文的首尾空白。外部 `Translator` 从文本请求返回译文及服务商识别出的源语言。请求和结果的 Debug 输出省略文本。
- 复用 SmsManager／FilesystemManager 的具名槽位方案和 Laravel Manager 的工厂注册与清除约定。并发首次解析对同名配置只构造一次，工厂执行不持有管理器级锁。失败保留重试能力。替换工厂与配置后显式清除，旧句柄保持原配置。
- 包的 2 项公开契约回归（含 12 线程竞争）与 1 项 Rustdoc 示例通过。根应用作为该包的独立消费者注册外部驱动，实跑跨应用 HTTP、独立 Artisan 与数据库 Worker。验收默认与具名选择、配置缓存冻结、清除后重配、无默认驱动错误，以及 Lang 缺词条的零翻译调用。默认骨架测试 2 项通过。
- Rust 1.94 格式、默认工作区与启用扩展的严格 Clippy、三语文档构建、两挂载 Chrome 12 组和导航均通过。验收日志见 `target/tmp/localization-translate/tr-01-*`。

## TR-02A DeepL 与 LibreTranslate

| 驱动 | 协议核对与验收 |
| --- | --- |
| DeepL v2 | Free／Pro 地址可选，使用 `DeepL-Auth-Key`，单条原文放在数组中。显式源语言和缺省检测、地区与文字体系目标、403、456、429、500、结构错误、重定向拒绝、超时均通过。见 [Translate text](https://developers.deepl.com/api-reference/translate/request-translation) 和 [错误处理](https://developers.deepl.com/docs/best-practices/error-handling)。 |
| LibreTranslate | 自托管 `/translate` 单条文本，带钥和匿名、`auto` 和显式源语言、繁体 `zt`、巴葡 `pb`、检测语言反向映射、403、429、500、结构错误、重定向拒绝、超时及缓存进程注入通过。见 [Translate Text](https://docs.libretranslate.com/api/operations/translate/) 和 [语言模型](https://docs.libretranslate.com/guides/supported_languages/)。 |

使用独立 Artisan 通过一次性回环 HTTP 服务按网络请求计数，分别核对一次 POST 与零重定向。HTTP 请求、响应和连接失败事件验证原文、译文、鉴权材料和响应载荷不可见，日志错误仅包含安全类别，缓存只保存环境变量名。超时返回结果未知；重试由应用决策。Rust 1.94 格式、工作区及启用功能后的严格 Clippy、2 项包回归和 1 项 Rustdoc、三语构建与 12 组 Chrome 通过。日志见 `target/tmp/localization-translate/tr-02a-*`。付费 DeepL 实账号与自托管 LibreTranslate 真实译文独立记录。

## TR-02B 腾讯云、阿里云与百度

| 驱动 | 协议核对与验收 |
| --- | --- |
| 腾讯云 | [TokenHub 混元专用翻译](https://cloud.tencent.com/document/product/1823/132252) 的 `/v1/api/translations`，默认 `hy-mt3-plus`，Bearer 和非流式单文本。通过源语言缺省／显式、`zh-TR`、响应终止状态、参数／鉴权／额度／限流／服务错误及超时。 |
| 阿里云 | [TranslateGeneral](https://help.aliyun.com/zh/machine-translation/developer-reference/api-alimt-2018-10-12-translategeneral) 的 2018-10-12 纯文本 general 表单。按[官方 SDK](https://github.com/aliyun/alibabacloud-python-sdk/blob/master/alimt-20181012/alibabacloud_alimt20181012/client.py) 使用表单正文，复用阿里云短信的 ACS3 签名算法，按实际字节计算摘要。普通密钥和 STS、`auto`、`zh-tw`、语言／额度／超时业务码及 HTTP 错误均通过。夹具独立用 Python HMAC 重算签名。 |
| 百度 | [通用文本翻译](https://fanyi-api.baidu.com/product/11) 和[签名说明](https://baidufanyi.apifox.cn/doc-1006705)的 HTTPS POST 表单。使用原始 UTF-8 字节生成 MD5，之后编码表单。通过合成 APP ID 签名校验、自动检测／显式语种、简繁及日韩映射、分段顺序、业务错误与超时。 |

三个驱动各自通过隔离服务真实 HTTP、单发、禁止重定向和事件／错误脱敏。阿里云临时安全令牌在出站观测中随同其他标记敏感的头移除。DeepL／LibreTranslate 协议整批重跑通过。既有 HTTP Client 3 项与 SMS 真实协议 1 项回归通过。日志见 `target/tmp/localization-translate/tr-02b-*`。

Rust 1.94 格式、工作区及启用功能后的严格 Clippy、2 项包回归、1 项 Rustdoc、三语站点与 12 组 Chrome 通过。

## TR-02C Azure 与 Google Cloud

| 驱动 | 协议核对与验收 |
| --- | --- |
| Azure | [Translator v3](https://learn.microsoft.com/en-us/azure/ai-services/translator/text-translation/reference/v3/translate) 的区域／全局密钥头、`api-version=3.0`、单条纯文本和单一目标。验证 `from` 缺省／显式、简繁映射、成功结构与[供应商错误码](https://learn.microsoft.com/en-us/azure/ai-services/translator/text-translation/reference/status-response-codes)、限流、超时、单发和脱敏。 |
| Google Cloud | [Advanced v3](https://docs.cloud.google.com/translate/docs/reference/rest/v3/projects.locations/translateText) 项目／地点路径、`text/plain`、单条内容、显式／自动识别和简繁映射。采用 [google-cloud-auth 1.19.0](https://docs.rs/google-cloud-auth/1.19.0/google_cloud_auth/) 的 ADC。以受控授权用户凭据文件和本地 OAuth 端点验证 scope、配额项目、令牌复用与提前刷新。同一个 Artisan 进程在刷新前后分别发送一次翻译请求，使用两代不同令牌。验证鉴权失败、额度、限流、超时、结构错误、禁止重定向和脱敏。 |

在最终源码上重跑七家协议和外部驱动 HTTP／Artisan／Worker、配置缓存及应用隔离回归。Google 首次用例的 1 秒寿命落入 SDK 10 秒轮询分支，改用符合 SDK 提前 4 分钟刷新策略的 242 秒寿命后，通过实际 OAuth 请求验证刷新。Google 官方凭据延迟到第一次异步翻译初始化，具名解析本身不触发鉴权。

Rust 1.94 的格式、工作区与启用功能后的严格 Clippy、包公开契约 2 项及 Rustdoc 1 项均通过。三语站点构建、两挂载 12 组 Chrome 与导航通过。同期重跑全部七个驱动，结果见 `target/tmp/localization-translate/tr-02c-verified.log`、`tr-02c-docs.log`。

## 真实环境验收

- LibreTranslate 1.9.6 已使用官方 Linux ARM64 镜像、Argos en→es 1.0 模型和隔离 HTTP。真实独立 Artisan 将 `Hello world.` 翻译为 `Hola mundo.`，显式语言和自动识别都通过，后者返回 `en`。初次容器启动未在时限内装好模型，改用预下载模型后通过。两次验收容器均已回收。镜像多架构摘要 `sha256:1de2d7056bb8ad607a412f4563d9abe324ff632b43b5be9428bcc8e213aebb32`。日志见 `target/tmp/localization-translate/libretranslate-real-retry.log`。
- DeepL 的 Free／Pro 权限与实际额度、腾讯云的站点及模型权限、阿里云的区域与 RAM／STS、百度的产品开通与 APP ID、Azure 的区域资源与订阅权限、Google Cloud 的项目／地点、生产 ADC 身份与 IAM，均留待分别使用授权凭据和端点验收实际译文、配额、计费错误与失败恢复。
- LibreTranslate 的其他部署、模型组合及远端 TLS／API key 由对应发布环境验收。AI Agent 翻译仍属于后续计划。
