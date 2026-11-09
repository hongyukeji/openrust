# 本地化

## 简介

应用、模块与框架错误共用 `Translator` 和当前语言作用域。默认文件消息不要求数据库或 Redis；数据库翻译来源仍在开发，不能按文件消息的验收推断其已经可用。当前章节说明语言选择与统一错误显示，完整 Laravel Localization 仍有后续对应范围。

## 配置语言

在 `config/app.rs` 设置 `AppConfig` 的 `locale` 和 `fallback_locale`。两者默认均为 `en`。部署时通过同一配置仓储用环境变量覆盖。

```dotenv
APP_LOCALE=zh-CN
APP_FALLBACK_LOCALE=en
```

升级已有应用时，把语言设置移入 `config/app.rs`，并将部署环境的语言变量改为上述名称。使用配置缓存的应用更新后需重建缓存。翻译器构造时读取当前应用配置，语言标签须有效，且选定的语言须有资源。 [Laravel 翻译 Provider](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Translation/TranslationServiceProvider.php)

`TranslationServiceProvider` 将同一个应用服务接入异常 Handler。需要通过公开异常渲染入口复用策略时，可读取 `Application::exception_handler()`；该引用来自已登记的应用策略，未登记时返回 `None`，不会另建默认 Handler。HTTP 请求从 `Accept-Language` 协商已支持语言；没有语言头或请求语言不受支持时采用配置默认值。精确语言候选先于同语言候选。普通译器按 group 资源回退；全局 JSON 的跨语言回退由显式 Catalog 视图提供。

命令或后台任务可在共享服务的作用域中选择语言，不改进程全局默认：

```rust,ignore
let locale = openrust::translation::Locale::new("zh-CN")?;
i18n.with_locale(locale, async {
    // 在这里调用复用同一 Translator 的业务或框架服务。
}).await;

i18n.with_default_locale(async {
    // 没有 HTTP 请求时，显式采用本应用默认语言。
}).await;
```

## 语言文件与消息读取

唯一示例的消息位于 `modules/example/lang/en.json`、`zh-CN.json`，通过模块 Provider 登记 `example` 命名空间。处理器使用 `RequestLocale` 与共享 `Translator` 读取消息，不直接按请求头拼文件名。发布覆盖、命名空间与默认目录遵循当前模块资源机制；完整可替换 Loader 和数据库来源另行交付。

消息支持具名参数及当前已实现的有限 plural/select 模板。来源保留合法空字符串，不将其当作缺失。普通 Laravel 查找的全局 JSON 空串或字符串 `0` 返回键本身；显式 Catalog 视图保留这些值。不要把业务诊断文本直接用作未登记的翻译键，也不要另建与框架错误分离的翻译器。

## ICU 模板与显式 Catalog 查找

结构化JSON的`plural`/`select`是OpenRust已保留的ICU增强，不等于Laravel的`choice`。默认`TranslationLookup::Laravel`只在请求语言查全局JSON键；未命中后按group资源回退，不会自动补查其他语言的全局JSON。完整 `choice` 与 Loader 生命周期尚未全部对应，结构化模板不替代这些接口。

需要平面目录跨语言回退的调用方显式选择`TranslationLookup::Catalog`，复用Container里同一服务：

```rust,ignore
use openrust::translation::{Locale, MessageArgs, TranslationLookup, Translator};
let shared = container.resolve::<Translator>().await?;
let catalog_view = shared.as_ref().clone().with_lookup(TranslationLookup::Catalog);
let args = MessageArgs::from([("count".to_owned(), "1".to_owned())]);
let text = catalog_view.translate(&Locale::new("en")?, "example::reading_count", &args);
```

这个clone共享目录与Source，仅切换本次句柄的查找策略。两种策略找到相同语言的结构消息后共用ICU formatter；选择Catalog主要决定全局键/语言回退和空值等增强，不会替换为另一翻译内核。来源安装器保留普通共享译器的 Laravel 查找策略。错误渲染、验证和明确需要增强的业务调用才选择 Catalog 视图，不更换全局单例。

唯一示例的`/api/example/reading-summary`通过`RequestLocale`和共享`Translator`读取模块`reading_count`，使用模块内的中英文 plural 资源。要使用select，可在同一模块语言JSON追加例如：

```json
{"reading_role":{"select":{"argument":"role","cases":{"admin":"Administrator","other":"Member"}}}}
```

随后用同一`catalog_view.translate(&locale, "example::reading_role", &MessageArgs::from([("role".into(), "admin".into())]))`读取；未知或缺少role使用other。plural 使用消息资源语言的 ICU 规则及资源声明的 one/few/many/other 分支。阅读摘要接口展示 plural；select 可通过上述资源和共享译器调用。

## 框架错误与验证消息

默认 JSON、HTML 以及应用明确启用的 API 错误封装使用同一个语言目录。HTML 的语言、标题和用户消息保持一致。稳定 `code`、HTTP 状态、协议字段、请求标识和 `Retry-After` 不因语言变化而改变。

验证的字段消息由验证或业务翻译入口产生，异常 Handler 只统一摘要，不能再次翻译已有字段文本。示例阅读摘要校验从同一命名空间生成字段消息，并保留其明确选择的 400 状态；通用表单验证默认 422 不因此改变。

生产环境保持 `APP_DEBUG=false`：5xx 只显示所选语言的安全通用消息，不泄露 SQL、配置值、凭据或 source 链。内部日志与错误 `Display` 保留诊断语义；显式调试模式的详情仅用于受控开发环境，不作为生产安全输出。

## 尚待闭合的对应范围

完整 Loader 替换、数据库来源、字符串键与短键的全部冲突规则、完整复数与区域格式、全部官方本地化工作流仍按各功能簇验收。本页不将当前 JSON 资源或有限模板实现冒称完整 Laravel 翻译引擎。

固定来源：[Laravel 13.x 本地化章节](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/localization.md)。当前可编辑 Rust 配置和异步语言作用域是项目的类型化对应方式，源码、默认行为与完整开发流程仍须分别验收。

显式 `BadRequest`、`Forbidden`、`NotFound`、`Conflict` 及非5xx带来源HTTP异常保留应用提供的安全详情；通用状态码翻译不能覆盖它们。应用主动使用稳定自定义错误码时可登记对应消息。服务尚未初始化的安全5xx使用内置英语目录；JSON、HTML与应用封装共用 `internal server error`，请求协商、HEAD和request-id仍保持各自契约。


## 显式翻译来源（Source）

框架默认 `TranslationServiceProvider` 仍不代表 Laravel 默认 FileLoader、locale/deferred 生命周期已经等价；官方 `get`/`choice`/Loader 完整接口继续保留对应缺口。应用安装器只选择来源，复用唯一 Container `Translator` 并保留普通 Laravel 查找默认；Files、Database 或 Composite 都不会隐式切换查找策略。明确业务需要跨语言全局 JSON 回退或原样空值时，使用该译器的 `clone().with_lookup(TranslationLookup::Catalog)` 视图；错误渲染与验证也复用同一目录。

根应用和新生成应用在 `config/translation_sources.rs` 编辑 `SourceSettings`。默认 `files` 读取 `lang`，无需数据库扩展、Redis 或后台任务。文件和显式数据库来源共用 `app.locale` 与 `app.fallback_locale`。显式数据库初始捕获失败会阻止启动，不静默当成文件来源成功。

启用 `translation-database` 和相应 `db-mysql` 后，首次安装先保持 `mode: Files` 并设 `install_database: true`，运行标准 `migrate`；成功后改 `Database` 或 `Composite`。命名连接使用 `translation_database.connection` 与同一 `Databases`，迁移显式传 `migrate --database <连接名>`，Loader 连接不会改写命令目标。不要在新库尚未建表时直接选择数据库来源。

组合顺序固定为 `[files, database]`：同一 locale/resource/key 后者覆盖前者；空串是有效资源值，禁用/删除表示该来源缺失。以下平面 JSON 返回示例使用显式 Catalog 业务视图；普通 Laravel 查找仍遵循其 JSON falsey 和 group 回退规则。先在请求的精确语言查全部来源，再按既有语言回退顺序查询。例如 `fr` 文件为 `Bonjour`、`en` 数据库为 `Hello` 时，显式 `fr` 返回 `Bonjour`。`en` 文件 `label=file`、数据库 `label=db` 返回 `db`；数据库值改空串返回空串；禁用/删除后返回 `file`。模块文件别名仅补缺，不能覆盖已捕获 Source 的精确全局键或 `namespace::key`；数据库删除并发布下一快照后恢复模块文件。JSON 字面键优先规则保持原有公开行为。

唯一模块示例使用命名空间 `example-source`，配置 `namespaces` 映射到 `modules/example/lang/source-overrides`；编译数据库扩展并登记来源后可运行 `example:translation show [locale]`、`set locale text`、`disable locale`、`delete locale`、`refresh`。该命令是运维入口，没有匿名 HTTP 写入口。写操作回执区分已提交修订、通知交付和提交后刷新错误，刷新失败不伪装数据库已回滚。

同步翻译仅访问已发布快照；HTTP `response_context` 在取得请求槽后、共享绝对期限及取消信号内核对刷新，再按原始请求头协商一次，后台任务显式刷新。批量捕获、校验和原子发布失败保留上次有效快照并记录脱敏固定错误码；通知是提示，有界修订核对负责最终收敛，不承诺全节点即时一致。外部 SQL 必须按契约推进修订或显式刷新。Windows 原子文件打开边界与完整 Laravel Loader 对应仍是公开缺口。

应用安装器在唯一 `default_providers()` 清单移出原默认 Translation 后追加到默认末尾，在 package register 前首次登记来源工厂；不是 application Provider 列表中晚 force 替换。合法 package register 提前解析与 boot 后解析复用同一个 Arc/Source 目录。JWT 生成继续仅有一个 default_providers 函数，保留显式 Fortify 替换与 Cookie/Session 排除。
