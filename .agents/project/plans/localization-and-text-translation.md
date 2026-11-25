# 本地化与文本翻译拆分计划

2026-11-25，确认按模块职责彻底拆分应用词条资源和文本机器翻译。本计划对应 DRV-03 与 DRV-06，定义正式命名、迁移顺序和交付条件。当前状态为计划，代码及验收待实施。

## 模块职责与名称

| 职责 | 计划归属及公开名称 |
| --- | --- |
| 应用本地化 | 核心 `openrust::localization`。应用入口为 `Lang`、`__` 和 `trans_choice`。`Localizer` 负责按 locale 解析词条、回退、替换参数和选择复数分支。具体实现放在 `openrust::localization::Localizer`，接口放在 `openrust::contracts::localization::Localizer`。`LocalizationServiceProvider` 注册 `lang` 和 `lang.loader`。 |
| 词条来源 | `Loader` 定义在 `openrust::contracts::localization`，`FileLoader`、框架内置语言资源和消息选择器归本地化模块。原 `openrust-translation-database` 迁成可选包 `openrust-localization-database`，由 `DatabaseLoader` 读取应用已有词条。数据库表、列、JSON 和语言标识沿用当前格式。 |
| 文本机器翻译 | 独立可选包 `openrust-translate`。`Translate` 是门面，`TranslateManager` 解析默认和具名驱动，`Translator` 是外部 crate 可实现的异步驱动契约。可选 Provider 在所属应用注册 `translator`，配置使用独立的 `translate` 命名空间。 |

选择 `Localizer`，因为这个类型同时处理语言选择和消息解析。`Catalog` 适合表示词条集合。保留固定 Laravel 的 `Lang`、词条加载与回退行为，将上游 `Illuminate\Translation\Translator` 映射到 Rust 的 `Localizer`。旧版 `v0.x` 的 `vendor/openrust/framework/src/translation.rs` 提供请求作用域消息目录参考。`Localizer` 的术语也可参考 .NET 的 [`IStringLocalizer`](https://learn.microsoft.com/en-us/dotnet/api/microsoft.extensions.localization.istringlocalizer)。[固定 Laravel 源码](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Translation)

## 公开调用与配置

```rust,ignore
let title = Lang::get("welcome.heading", &Default::default(), None)?;
let result = Translate::translate("你好", "en").await?;
let result = Translate::driver("office")?.translate("你好", "en").await?;
```

- 文本翻译请求接受原文、目标 BCP 47 语言标签及可选源语言。便捷入口指定目标语言，完整请求入口允许明确指定源语言。省略源语言时按驱动声明的自动识别能力处理。返回译文及驱动实际提供的识别信息。空内容、不支持的语言、认证错误、配额、超时和限流均有可区分的结果。
- 按 `Sms::driver` 的具名选择与 `Storage::disk` 的配置复用方式组织。应用通过 `ServiceProvider` 注册驱动工厂；同一驱动可有多组配置，实例复用和刷新按应用隔离。独立 Artisan 与 Worker 各自装配工厂，队列载荷只保存必要的配置名和业务参数。凭据、原文和译文按业务边界处理，默认诊断不记录敏感内容。
- `Lang` 使用应用 locale 与回退配置。文本翻译由应用明确调用并提供目标语言，缺失词条按本地化的现有策略处理。框架内置 18 语运行时资源，应用可按键覆盖。开发者文档按现行三语范围交付。

## 实施顺序

| 批次 | 实施范围 | 退出条件 |
| --- | --- | --- |
| L10N-01 核心边界迁移 | 盘点旧模块、`Translator` 类型和 trait、`translator`／`translation.loader` 绑定及直接使用者，整体迁移到 `localization` 与 `Localizer`。迁移验证消息、Blade、HTTP、Artisan、邮件／通知及包资源发布。检查内置资源路径、安装产物和容器别名。应用的 `Lang`／`__`／`trans_choice` 调用和资源格式保持原语义。 | 旧 Rust 导入及容器键退出本地化链路，旧名称留作文本翻译；新增迁移表和可编译例子，真实应用从同一 `Localizer` 取得相同词条。 |
| L10N-02 资源来源与 DRV-06 | 迁移数据库词条包和依赖，核对 Cargo.lock、迁移发布及下游。实现应用级默认 `Loader` 可替换接线，并明确复合来源、文件回退、异步加载与原子发布。 | 独立消费 crate 实现 `Loader`，真实 HTTP、Artisan 和 Worker 的 `Lang` 入口均能读到其词条；文件、JSON、命名空间、18 语默认值、语言回退及三库加载通过。 |
| TR-01 DRV-03 驱动底座 | 确定 `Translate`、`TranslateManager`、`Translator`、配置、错误和异步请求契约；复用应用容器、配置、HTTP Client 与现有 Provider 装配，公开外部工厂注册。 | 外部测试驱动经正式入口完成默认与具名翻译，覆盖多配置、多应用、配置缓存、工厂替换、失败重试和独立进程注册。 |
| TR-02 服务商与发布验收 | 对照维护中的官方协议和成熟 SDK，确定首批服务商、维护成本、许可及资费。逐家实现和核对签名、语言代码、错误和可选能力。 | 合成文本经真实应用调用本地协议服务，验证译文响应映射、错误、限流及超时。获授权后再验证真实账号、实际译文、区域与配额；账本独立标记模拟与实机结果。 |

## 迁移与最终验收

- 本地化和文本翻译的代码、配置、包发现及容器绑定均独立。迁移说明覆盖 `openrust::translation`、`contracts::translation`、旧词条类型与容器键、数据库词条包和任何受影响的直接调用。应用 `.env` 中的 locale 配置、词条文件和现有数据库数据走原有入口。下游以一次迁移更新导入；旧的 `translator` 词条别名退出后再注册文本翻译服务，避免同一个键解析为两种职责。
- 先以同一隔离数据对比迁移前后的本地化 HTTP、Blade、验证、独立 Artisan、Worker、语言资源发布及配置缓存，覆盖 BCP 47、应用隔离、缺失／复数／回退和 SQLite、MySQL、PostgreSQL 上的数据库词条。再运行机器翻译的独立驱动与故障矩阵。检查禁用扩展时本地化可用，以及启用扩展后缺字典词条只触发已配置的本地化回退。
- 每批保留 Rust 1.94.0、相关测试、格式、定向严格 Clippy、三语文档和证据。按项目约定分别提交本地迁移和新功能，不推送。DRV-03、DRV-06 分别按实际结果关闭。
