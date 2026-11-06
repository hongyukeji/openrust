# 本地化

## 简介

应用、模块与框架错误共用 `Translator` 和当前语言作用域。默认文件消息不要求数据库或 Redis；数据库翻译来源仍在开发，不能按文件消息的验收推断其已经可用。当前章节说明语言选择与统一错误显示，完整 Laravel Localization 仍有后续对应范围。

## 配置语言

编辑 `config/i18n.rs`，默认语言和回退语言分别配置。框架默认均为 `en`，不会因请求缺少语言头自动改成中文：

```rust
use openrust::config::{ConfigContext, ConfigError};
use openrust::translation::TranslationConfig;
use openrust::translation::Locale;

pub fn config(_ctx: &ConfigContext) -> Result<TranslationConfig, ConfigError> {
    Ok(TranslationConfig {
        default_locale: Locale::new("zh-CN")?,
        fallback_locale: Locale::new("en")?,
    })
}
```

`TranslationServiceProvider` 将同一个应用服务接入异常 Handler。需要通过公开异常渲染入口复用策略时，可读取 `Application::exception_handler()`；该引用来自已登记的应用策略，未登记时返回 `None`，不会另建默认 Handler。HTTP 请求从 `Accept-Language` 协商已支持语言；没有语言头或请求语言不受支持时采用配置默认值。精确语言候选先于同语言候选；所选语言缺少消息时再查配置回退语言。

命令或后台任务可在共享服务的作用域中选择语言，不改进程全局默认：

```rust,ignore
let locale = openrust::translation::Locale::new("zh-CN")?;
i18n.with_locale(locale, async {
    // 在这里调用复用同一 I18n 的业务或框架服务。
}).await;

i18n.with_default_locale(async {
    // 没有 HTTP 请求时，显式采用本应用默认语言。
}).await;
```

## 语言文件与消息读取

唯一示例的消息位于 `modules/example/lang/en.json`、`zh-CN.json`，通过模块 Provider 登记 `example` 命名空间。处理器使用 `RequestLocale` 与共享 `Translator` 读取消息，不直接按请求头拼文件名。发布覆盖、命名空间与默认目录遵循当前模块资源机制；完整可替换 Loader 和数据库来源另行交付。

消息支持具名参数及当前已实现的有限 plural/select 模板。缺失键返回键本身，合法空字符串不等于缺失。不要把业务诊断文本直接用作未登记的翻译键，也不要另建与框架错误分离的翻译器。

## 框架错误与验证消息

默认 JSON、HTML 以及应用明确启用的 API 错误封装使用同一个语言目录。HTML 的语言、标题和用户消息保持一致。稳定 `code`、HTTP 状态、协议字段、请求标识和 `Retry-After` 不因语言变化而改变。

验证的字段消息由验证或业务翻译入口产生，异常 Handler 只统一摘要，不能再次翻译已有字段文本。示例阅读摘要校验从同一命名空间生成字段消息，并保留其明确选择的 400 状态；通用表单验证默认 422 不因此改变。

生产环境保持 `APP_DEBUG=false`：5xx 只显示所选语言的安全通用消息，不泄露 SQL、配置值、凭据或 source 链。内部日志与错误 `Display` 保留诊断语义；显式调试模式的详情仅用于受控开发环境，不作为生产安全输出。

## 尚待闭合的对应范围

完整 Loader 替换、数据库来源、字符串键与短键的全部冲突规则、完整复数与区域格式、全部官方本地化工作流仍按各功能簇验收。本页不将当前 JSON 资源或有限模板实现冒称完整 Laravel 翻译引擎。

固定来源：[Laravel 13.x 本地化章节](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/localization.md)。当前可编辑 Rust 配置和异步语言作用域是项目的类型化对应方式，源码、默认行为与完整开发流程仍须分别验收。

显式 `BadRequest`、`Forbidden`、`NotFound`、`Conflict` 及非5xx带来源HTTP异常保留应用提供的安全详情；通用状态码翻译不能覆盖它们。应用主动使用稳定自定义错误码时可登记对应消息。服务尚未初始化的安全5xx使用内置英语目录；JSON、HTML与应用封装共用 `internal server error`，请求协商、HEAD和request-id仍保持各自契约。
