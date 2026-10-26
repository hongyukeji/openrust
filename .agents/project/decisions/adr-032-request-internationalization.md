# ADR-032：请求级国际化

状态：Accepted
日期：2026-10-26

## 背景

v1 要求框架错误、验证信息与邮件/短信模板能够按请求语言展示；语言选择必须隔离并发请求，稳定错误码和字段路径不随语言改变。现有 `ResponseContext` 已按 Tokio task-local 隔离请求 ID 与 HTTP 状态策略，可沿用该作用域而不引入可变全局语言。

## 决策

1. 国际化能力属于根 package 的 `openrust::i18n`，由显式 `I18nPlugin` 注册；不新增空 crate，也不增加第三方运行时依赖。
2. 消息资源使用受校验的 JSON 对象：一级键为 locale tag，二级键为稳定消息 key，值为 UTF-8 模板字符串。框架内置 `en` 与 `zh-CN`；应用可从文件、嵌入资源或代码提供额外 JSON，并通过同一个 `MessageCatalog` API 合并或覆盖资源。
3. 模板只支持 `{name}` 参数替换，不执行表达式、函数或 HTML 转义。缺参保留原占位符；未知 key 原样返回 key，避免静默丢失诊断信息。
4. 选择优先级为显式 `RequestLocale` 覆盖、`Accept-Language` 中首个受支持语言、应用默认语言、配置回退语言。语言标签按 BCP 47 常见语言/地区形式规范化；不支持的标签跳过。此版本不承诺复数规则或地区数字/日期格式化。
5. `I18n` 是不可变、可并发共享的应用服务；每个请求的 `RequestLocale` 进入请求扩展与现有 response task-local context。提供 `RequestLocale` 提取器和 `t(key, args)` 入口；业务可在认证后用中间件注入用户偏好覆盖，框架不读取 Cookie、Session 或用户表。
6. 框架错误用稳定 `openrust.error.<code>` key 渲染普通展示消息；机器 `code`、HTTP 状态、字段路径保持不变。应用 `Custom` 错误已有安全展示文本时原样保留；5xx 仍统一显示通用消息。验证错误按稳定 validator rule code 尝试翻译，应用自定义错误文本在没有对应规则映射时保留原文。
7. 邮件与短信模板可显式接收 `RequestLocale` / 指定 locale，并使用同一 catalog 查消息；发送任务没有请求上下文时由调用方明确选择 locale，不能读取全局当前语言。
8. `apps/server` 安装插件并展示 `Accept-Language`、显式覆盖、回退与并发隔离。默认不改变既有 API 响应语言，除框架内置错误消息可由应用配置选择默认语言。

## 验收

- 单元测试覆盖 locale 规范化、Accept-Language 顺序/quality 与无效输入、区域到语言回退、默认/最终 fallback、缺失 key/参数、资源合并、错误码/字段路径稳定。
- 并发 HTTP 请求分别选择中英文，成功与错误消息不串语言；显式覆盖高于请求头；超时/404/validation 等可处理框架错误使用当前语言。
- `apps/server` 集成测试和独立生成项目示例展示资源扩展及请求选择。
- 该能力无数据库行为，不要求双库集成测试；模板供应商发送验收留给邮件/短信功能簇。

## 后果

- 中性 locale 与消息 key 可以由邮件、短信和错误响应共享；调用方显式传入语言，后台任务不依赖 HTTP 生命周期。
- JSON + 简单变量插值满足 v1 消息需求，不提供 Fluent 的复数、嵌套选择器等高级能力。新增格式能力须有实际需求和独立决策。
