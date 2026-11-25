# 本地化与文本机器翻译拆分计划

2026-11-25 审计修订。用户确认按职责彻底拆分应用本地化、词条来源与文本机器翻译。本计划覆盖 DRV-03 与 DRV-06，当前状态为待实施。

## 调研及裁决

| 参考 | 采用范围 |
| --- | --- |
| [固定 Laravel 13 的 TranslationServiceProvider](https://raw.githubusercontent.com/laravel/framework/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Translation/TranslationServiceProvider.php) · [Manager](https://raw.githubusercontent.com/laravel/framework/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Manager.php) | 上游 `Lang` 背后由容器装配 Loader 和词条解析器。保留 `Lang`、`__`、`trans_choice`、locale、回退及发布行为。按用户决策在 Rust 将词条解析器命名为 `Localizer`，容器键迁到 `lang` 系列。文本翻译借鉴 Manager 的默认驱动、具名缓存、`extend` 和显式清除方式。 |
| [Spatie Laravel Translation Loader 2.8.3](https://github.com/spatie/laravel-translation-loader/releases/tag/2.8.3) · [加载实现](https://raw.githubusercontent.com/spatie/laravel-translation-loader/2.8.3/src/TranslationLoaderManager.php) | MIT。复用现有 `DatabaseLoader::with_fallback` 的文件基础、数据库同键覆盖和包命名空间文件规则。Spatie 在同步查找里读取数据库，OpenRust 将异步查询放在显式装配／刷新阶段，查找仍同步。 |
| [OpenRust v0.x 来源组合](https://github.com/hongyukeji/openrust/blob/6d1a81924/vendor/openrust/framework/src/translation/composite_loader.rs) | 参考来源优先级、先校验后发布的场景。当前 `load_from` 已有精确组加载、异步 I/O 和并发版本检查，优先扩展这些现有机制。完整刷新会话和轮询体系留在旧实现。 |
| [Laravel-Lang/translator 固定提交](https://github.com/Laravel-Lang/translator/tree/67fcb9386aa0a368af8718939d4a46f7c4fd282b) · [overworks/laravel-translator 固定提交](https://github.com/overworks/laravel-translator/tree/f536b5ce9007e6948a454f3966406af9034f7218) | Laravel 生态已有独立文本翻译包。借鉴门面、提供商驱动及语言参数的组织方式。项目内 `SmsManager` 和 `FilesystemManager` 已有 Rust 工厂注册、具名配置与应用隔离，优先沿用其生命周期。 |
| [DeepL 官方 PHP SDK v1.21.0](https://raw.githubusercontent.com/DeepL/deepl-php/v1.21.0/README.md) · [DeepL v2 协议](https://developers.deepl.com/api-reference/translate/request-translation) | 官方 SDK 为 MIT，作为错误及协议参考。首批托管驱动建议用 DeepL v2 纯文本 REST，通过现有 HTTP Client 发送请求。API 认证、服务区与配额按官方契约验收。 |
| [LibreTranslate v1.9.6](https://raw.githubusercontent.com/LibreTranslate/LibreTranslate/v1.9.6/README.md) · [翻译协议](https://docs.libretranslate.com/api/operations/translate/) | 首批自托管驱动建议实现独立服务的 `/translate` HTTP 客户端，验收显式源语言、`auto`、可选 API key 和响应结构。服务端为 AGPL-3.0，按外部服务接入，框架发行组合采用自有 HTTP 适配。 |
| [Azure Translator v3](https://learn.microsoft.com/en-us/azure/ai-services/translator/text-translation/reference/v3/translate) · [Google Cloud Translation v3](https://docs.cloud.google.com/translate/docs/reference/rest/v3/projects.locations/translateText) | 作为后续驱动对照。现有接口需要分别处理 Azure 的区域／密钥及 Google 的项目、地点和授权。等实际集成需求明确后，再加入对应接线。 |

建议用 **DeepL + LibreTranslate** 验证同一公开契约在两种部署方式下都成立。首批交付单条纯文本。文档、HTML、批量、术语表、自动切换供应商、译文缓存、LLM 和队列编排属于独立范围。两家首批选型是本次审计建议，真实账号与对外请求单独安排。

## 模块与公开命名

| 职责 | 归属 |
| --- | --- |
| 应用本地化 | `openrust::localization`。应用入口为 `Lang`、`__`、`trans_choice`。`Localizer` 按 locale 解析消息、执行回退、替换参数和选择复数。具体类型与 trait 分别归 `openrust::localization::Localizer` 和 `openrust::contracts::localization::Localizer`。由 `LocalizationServiceProvider` 装配。 |
| 词条来源 | `Loader` 归 `openrust::contracts::localization`。核心提供 `FileLoader`、内置语言和消息选择器。数据库加载器由 `openrust-translation-database` 一次迁移到 `openrust-localization-database`，继续读取既有 `language_lines`。 |
| 文本机器翻译 | 可选包 `openrust-translate`。`Translate` 是门面，`TranslateManager` 管理驱动，`Translator` 是外部 crate 可实现的异步契约。由包 Provider 按当前应用装配，`translator` 绑定指向同一个 `TranslateManager` 实例，配置归 `translate` 命名空间。 |

`Localizer` 表示读取并解析与 locale 有关的词条。内部词条集合可以继续叫 Catalog。命名参考 [Microsoft IStringLocalizer](https://learn.microsoft.com/en-us/dotnet/api/microsoft.extensions.localization.istringlocalizer)，容器键的改名属于已确认的 Rust 适配。语言代码规范化和共用 BCP 47 工具沿用核心逻辑，文本翻译依赖它们的语言标签校验，词条加载链路与文本翻译配置各自装配。

### 迁移对应

| 旧本地化入口 | 目标入口 |
| --- | --- |
| `openrust::translation`、`openrust::contracts::translation` | 各自迁到 `localization` |
| 本地化具体类型／trait `Translator`，`TranslationServiceProvider` | `Localizer`、`LocalizationServiceProvider` |
| 容器 `translator`、`translation.loader` | `lang`、`lang.loader`。词条迁移提交中撤销旧键，随后才给机器翻译注册 `translator`。 |
| 直接依赖文件路径管理的消费者 | 使用同一应用的 `FileLoader`，独立于具名默认异步来源。明确迁移 Validator 的译文访问入口及类型签名。 |
| `openrust-translation-database` crate | 更新包名、目录、Cargo.lock、导入和发布路径。保持表名、列、JSON 数据、既有迁移标识和 `lang:publish` 行为。 |

锁定涉及的 app/config、框架、独立包、安装生成模板、三语示例、测试与许可引用逐项迁移。应用目录 `lang/`、环境变量、locale 与数据格式沿用。直接 Rust 导入及容器键属于公开破坏性改名，随迁移提交提供对照表。回退时配对恢复旧二进制与源码依赖组合。

## DRV-06 默认 Loader 接线

现有 `Translator` 持有 `Arc<FileLoader>`，默认查找同步调用 `load_lines`。应用即使重绑 `dyn Loader`，默认 `Lang` 也不会读取新来源。`Lang::load_from` 能用异步 Loader 预加载已知分组，现有行为会把整个分组发布到 runtime 层，不能直接作为独立的来源刷新层。

1. `lang.loader` 是正式的可替换来源入口，解析为 `Arc<dyn Loader>`。命名键和 trait 解析使用同一个来源。独立保留应用自有 `FileLoader`，供路径、包命名空间、候选语言发现和未装配分组的文件读取。
2. 外部来源通过 Provider 的 `register` 注册，显式声明其负责的精确 locale、group、namespace。在现有异步 `ServiceProvider::boot` 生命周期内，按已核对的顺序在语言目录装配后读取声明分组，再把结果发布到同一个 `Localizer`。重用 `load_from` 的分组校验、锁外 I/O、原子发布与并发冲突检测思路。HTTP、独立 Artisan、Worker 在同一配置下消费同一契约，常驻进程通过显式刷新或滚动重启接收更新。
3. 将配置来源结果与应用运行时 `add_lines`／显式 `load_from` 分层。运行时词条优先。接管的分组读取已发布的默认来源快照，未接管分组按原文件规则读取，locale 回退仍由 `Localizer` 处理。数据库来源复用现有文件合并，数据库同键覆盖，命名空间按包文件规则。空字符串是有效值，缺少值交由已有回退规则。刷新应移除上一份来源里已删除的词条。
4. 初次加载所需来源失败时明确报告启动失败。显式刷新失败时保留上次成功版本并返回错误。核对部分加载失败、配置变更、并发 `add_lines`、刷新竞争、失效旧快照及逐键优先级。同步查找按内存快照或既有文件路径执行，避免请求内数据库 I/O、运行时阻塞或后台轮询。
5. 默认声明列表为空，文件模式无需数据库。数据库来源先安装并执行迁移，再启用需要预加载的分组。验收干净安装、`migrate`、`lang:publish`、配置缓存及启用后的启动。配置缓存保存来源声明，进程启动时读取词条。数据库不可用的恢复流程通过停用该来源回到文件模式，恢复后重新启用并重启。

## DRV-03 驱动契约

```rust,ignore
let title = Lang::get("welcome.heading", &Default::default(), None)?;
let translation = Translate::translate("你好", "en").await?;
let translation = Translate::driver("office")?
    .translate(TranslationRequest::new("你好", "en")?)
    .await?;
```

- `Translate::translate(text, target)` 走默认具名配置。需要显式源语言时通过 `TranslationRequest` 设置后调用 `translate_request` 或直接使用具名驱动。驱动提供一个异步 `translate(request)`，具名解析返回驱动句柄。结果包含译文及驱动实际返回的可选源语言信息。
- 配置沿用 `translate.default` 与 `translate.drivers.<profile>.driver`。profile 是业务选择名称，driver 是工厂注册名。未装扩展时核心本地化继续独立工作。未配置默认、具名配置不存在或驱动未注册时分别报错。凭据通过既有环境读取机制装配，教程、错误与日志示例使用占位值，验收检查配置缓存与日志里的敏感字段。
- 外部 crate 经 Provider 调用 Manager 的 `extend`。同应用、同 profile 的并发首次解析只建一个实例，工厂失败可重试，调用工厂时释放管理器全局锁。更换工厂或配置后显式 `forget_drivers`，已取得的句柄按原配置工作。各应用和独立进程分别注册。采用现有具名驱动机制的经验，公共抽象待有其他真实调用方再提取。
- 校验 BCP 47 和空白输入，保留非空原文的原始空白。各驱动负责供应商语言码转换，缺少映射时明确报错，避免把简体／繁体或地域变体悄悄合并。源语言缺省表示请求自动识别，驱动不支持时返回明确错误。
- 区分请求校验、认证、语言不支持、配额、限流、超时、服务端错误和无效响应。复用 HTTP Client 的超时和 `redact_payload`。翻译 POST 默认发送一次，超时或响应丢失保留结果未知的语义。目标地址来自受控配置，处理认证时拒绝跨目标转发。错误上下文只记录安全的驱动名、状态码和请求标识，实测 HTTP 事件和默认观测不含凭据、原文、译文或响应体。

## 实施顺序与验收

| 批次 | 交付和退出条件 |
| --- | --- |
| L10N-01 核心迁移 | 先完成拆分命名与所有直接消费者。比较迁移前后独立 Artisan／HTTP 的行为和生成应用。覆盖 Blade、验证、中间件、邮件／通知、BCP 47、locale 作用域、内置 18 语和 `lang:publish`。校验旧服务键退出以及包资源注册。 |
| L10N-02 词条来源 | 迁移数据库词条包，完成异步来源接线。用外部 crate 注册一次默认 Loader，验证 HTTP、独立 Artisan 和 Worker 的普通 `Lang` 调用。以 SQLite、MySQL、PostgreSQL 核对原数据、发布迁移和失败恢复。用真实分组验证并发刷新、资源删除、应用隔离、文件回退与运行时覆盖。 |
| TR-01 驱动底座 | 让外部 crate 的测试驱动经正式默认、具名、工厂注册和配置缓存入口运行。验证工厂失败、并发、重配、跨应用及 HTTP／独立 Artisan／Worker。验证扩展启停和 `Lang` 缺词条时均不会意外发出翻译请求。 |
| TR-02 首批适配器 | 用合成文本驱动真实程序访问本地 HTTP 夹具，分别验收两种协议的请求、成功／失败响应、鉴权、语言映射、限流、超时，以及 POST 单次发送。LibreTranslate 的真实译文可在隔离自托管服务复验。DeepL 真实服务的译文、地区、账号及配额由授权发布验收确认。 |

每批使用 Rust 1.94.0，提交对应回归、格式、定向严格 Clippy 和三语开发者文档证据。记录实际跑过的入口、数据及环境，区分协议模拟、实际翻译效果与发布条件。按批次本地提交，默认不推送。
