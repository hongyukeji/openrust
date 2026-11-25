# 本地化与文本机器翻译拆分计划

2026-11-25 审计修订。用户确认按职责彻底拆分应用本地化、词条来源与文本机器翻译，并确认七个内置驱动的交付范围。本计划覆盖 DRV-03 与 DRV-06，当前状态为待实施。

## 调研及裁决

| 参考 | 采用范围 |
| --- | --- |
| [固定 Laravel 13 的 TranslationServiceProvider](https://raw.githubusercontent.com/laravel/framework/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Translation/TranslationServiceProvider.php) · [Manager](https://raw.githubusercontent.com/laravel/framework/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Manager.php) | 上游 `Lang` 背后由容器装配 Loader 和词条解析器。保留 `Lang`、`__`、`trans_choice`、locale、回退及发布行为。按用户决策在 Rust 将词条解析器命名为 `Localizer`，容器键迁到 `lang` 系列。文本翻译借鉴 Manager 的默认驱动、具名缓存、`extend` 和显式清除方式。 |
| [Spatie Laravel Translation Loader 2.8.3](https://github.com/spatie/laravel-translation-loader/releases/tag/2.8.3) · [加载实现](https://raw.githubusercontent.com/spatie/laravel-translation-loader/2.8.3/src/TranslationLoaderManager.php) | MIT。复用现有 `DatabaseLoader::with_fallback` 的文件基础、数据库同键覆盖和包命名空间文件规则。Spatie 在同步查找里读取数据库，OpenRust 将异步查询放在显式装配／刷新阶段，查找仍同步。 |
| [OpenRust v0.x 来源组合](https://github.com/hongyukeji/openrust/blob/6d1a81924/vendor/openrust/framework/src/translation/composite_loader.rs) | 参考来源优先级、先校验后发布的场景。当前 `load_from` 已有精确组加载、异步 I/O 和并发版本检查，优先扩展这些现有机制。完整刷新会话和轮询体系留在旧实现。 |
| [Laravel-Lang/translator 固定提交](https://github.com/Laravel-Lang/translator/tree/67fcb9386aa0a368af8718939d4a46f7c4fd282b) · [overworks/laravel-translator 固定提交](https://github.com/overworks/laravel-translator/tree/f536b5ce9007e6948a454f3966406af9034f7218) | Laravel 生态已有独立文本翻译包。借鉴门面、提供商驱动及语言参数的组织方式。项目内 `SmsManager` 和 `FilesystemManager` 已有 Rust 工厂注册、具名配置与应用隔离，优先沿用其生命周期。 |
| [DeepL 官方 PHP SDK v1.21.0](https://raw.githubusercontent.com/DeepL/deepl-php/v1.21.0/README.md) | 官方 SDK 为 MIT，作为协议及错误映射参考。OpenRust 的适配器复用现有 HTTP Client。 |
| [LibreTranslate v1.9.6](https://raw.githubusercontent.com/LibreTranslate/LibreTranslate/v1.9.6/README.md) | 服务端为 AGPL-3.0，按外部自托管服务接入，OpenRust 提供独立 HTTP 客户端。 |

## 七个内置驱动

| 驱动键 | 服务与正式入口 | 专项验收 |
| --- | --- | --- |
| `deepl` | [DeepL v2 Translate](https://developers.deepl.com/api-reference/translate/request-translation) | API Free／Pro 地址与鉴权、区域语言码、配额和源语言识别。 |
| `libretranslate` | [LibreTranslate `/translate`](https://docs.libretranslate.com/api/operations/translate/) | 自托管地址、可选 API key、显式源语言和 `auto`。 |
| `azure` | [Azure Translator v3 Translate](https://learn.microsoft.com/en-us/azure/ai-services/translator/text-translation/reference/v3/translate) | 区域和密钥装配、`from` 缺省、单目标语言、响应结构与错误码。 |
| `google` | [Google Cloud Translation Advanced v3 `translateText`](https://docs.cloud.google.com/translate/docs/reference/rest/v3/projects.locations/translateText) | 项目和地点、官方凭据接线、访问令牌刷新以及 Google API 错误。 |
| `tencent` | [腾讯云 TokenHub 混元专用翻译接口](https://cloud.tencent.com/document/product/1823/132252) | `/v1/api/translations` 的非流式单文本请求、专用翻译模型配置、Bearer 鉴权和语言码。按[腾讯云退市公告](https://cloud.tencent.com/announce/detail/2482)选用官方推荐替代接口。 |
| `aliyun` | [阿里云机器翻译 `TranslateGeneral`，2018-10-12](https://help.aliyun.com/zh/machine-translation/developer-reference/api-alimt-2018-10-12-translategeneral) | 通用纯文本场景、区域和 RAM 权限、官方 RPC 鉴权、业务状态码与 `auto` 识别结果。优先核对已有阿里云请求签名能否复用。 |
| `baidu` | [百度翻译开放平台通用文本翻译 API](https://fanyi-api.baidu.com/product/11) | APP ID 和密钥签名、`auto`、语言码、分段译文按顺序还原及业务错误。实现前核对官方接入文档的请求编码和 HTTPS POST。 |

七个驱动均为本轮计划交付项。分批实现以控制验收范围，每个驱动统一交付单条纯文本翻译并支持具名配置，继续开放外部 crate 自定义驱动。腾讯云接入混元翻译专用接口。文档、HTML、批量、术语表、自动切换供应商、译文缓存和队列编排属于独立范围。AI Agent 翻译通过同一驱动契约预留，见下文。

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

## 后续可选能力：AI Agent 翻译

2026-11-25 用户确认后续再安排，暂不实施。现阶段的预留来自已有 `Translator` 自定义驱动契约。Agent 适配、专用配置及模拟 Agent 验收均归后续批次，TR-01 使用普通外部驱动夹具验证公开扩展入口。

| 参考 | 用于本计划的部分 |
| --- | --- |
| [Laravel AI SDK 固定 Agent 契约](https://raw.githubusercontent.com/laravel/ai/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Contracts/Agent.php) · [结构化输出](https://laravel.com/framework/docs/13.x/ai-sdk#structured-output) | 借鉴一个专用 Agent 封装翻译指令与输出结构，模型和供应商由 AI 侧管理。OpenRust 开发者通过应用 Provider 装配既有 `Translator`，与普通具名驱动的用法一致。 |
| [Rig 固定 Rust 示例](https://raw.githubusercontent.com/0xPlaygrounds/rig/f4d73dacdfabb3d9daa3f1de091efa490a84c601/examples/README.md) | Rust 生态可复用的类型化输出和有界调用参考。后续针对实际模型、结构化输出能力、Rust 1.94.0 和依赖成本核对兼容性，再裁定采用应用已有 SDK 或 Rig。 |
| [Andrew Ng 的 Translation Agent 固定示例](https://raw.githubusercontent.com/andrewyng/translation-agent/e0fc605acbb5d78cb7a58a98bc8bd8f0056df49c/README.md) | 参考初译、审校、修订的有界流程，以及术语、口吻和地区差异的用例。作者将该项目定位为 Python 演示，译文效果随测试而异。只有质量对照证明有收益时，才为明确选择的场景启用额外调用。 |

- **预留现有入口。** `Translator` 持续接收 `TranslationRequest`，返回统一译文结果。应用可自行注册 `agent` 类型工厂，并用 `translate.drivers.<profile>` 择一绑定已配置的专用 Agent。`Translate::driver("editorial")` 与其他 profile 一样返回驱动句柄。模型、供应商凭据、固定指令、术语和调用预算在应用的 Agent 配置管理。翻译公共请求保持原文及语言字段的供应商无关性。
- **优先交付一轮调用。** 未来适配器把原文作为数据传给只允许翻译的 Agent，以简单结构返回译文。只有实际可用的识别来源才填源语言。校验非空结果和业务声明的受保护占位符，例如 `:name`。结果截断、模型拒绝、超时或格式不符映射到现有错误类别。原文中的命令、链接或工具调用请求均按文本处理。本轮建议使用无工具、无会话历史的翻译 Agent。
- **把增强留给具名场景。** 需要润色、专业术语或人工复核时，由业务显式选用其他 profile 或在应用工作流中编排。若后续采用初译、审校、修订，固定步骤上限、总体超时、输入上限及模型调用预算，比较固定语料的遗漏、术语一致性、占位符、延迟和费用。各 profile 的数据去向可审计，文本采用既有脱敏策略。
- **后续批次验收。** 届时用模拟 Agent 通过既有工厂走正式 `Translate` 入口，确认 Agent、七家适配器与 `Lang` 各自独立装配。实测格式错误、超时、无工具执行、占位符和费用上限，生产交付按实际采用的模型逐项复验。

## 实施顺序与验收

| 批次 | 交付和退出条件 |
| --- | --- |
| L10N-01 核心迁移 | 先完成拆分命名与所有直接消费者。比较迁移前后独立 Artisan／HTTP 的行为和生成应用。覆盖 Blade、验证、中间件、邮件／通知、BCP 47、locale 作用域、内置 18 语和 `lang:publish`。校验旧服务键退出以及包资源注册。 |
| L10N-02 词条来源 | 迁移数据库词条包，完成异步来源接线。用外部 crate 注册一次默认 Loader，验证 HTTP、独立 Artisan 和 Worker 的普通 `Lang` 调用。以 SQLite、MySQL、PostgreSQL 核对原数据、发布迁移和失败恢复。用真实分组验证并发刷新、资源删除、应用隔离、文件回退与运行时覆盖。 |
| TR-01 驱动底座 | 让外部 crate 的测试驱动经正式默认、具名、工厂注册和配置缓存入口运行。验证工厂失败、并发、重配、跨应用及 HTTP／独立 Artisan／Worker。验证扩展启停和 `Lang` 缺词条时均不会意外发出翻译请求。 |
| TR-02 七个内置适配器 | TR-02A 实施 DeepL、LibreTranslate，用两种协议验证统一契约。TR-02B 实施腾讯云、阿里云、百度。TR-02C 实施 Azure、Google。每家用合成文本驱动真实程序访问协议夹具，分别验收鉴权、语言映射、成功／错误响应、限流、超时、请求单次发送和脱敏。LibreTranslate 的真实译文可在隔离自托管服务复验。其余六家真实译文、地区、账号、权限及配额由授权发布验收逐家确认。 |

每批使用 Rust 1.94.0，提交对应回归、格式、定向严格 Clippy 和三语开发者文档证据。记录实际跑过的入口、数据及环境，区分协议模拟、实际翻译效果与发布条件。按批次本地提交，默认不推送。
