# ADR-032：请求级国际化

- 状态：Accepted（请求隔离、简单插值、ICU/CLDR 复数与选择、数字/日期格式化均已实现）
- 日期：2026-10-26

## 背景

v1 要求框架错误、验证信息与邮件/短信模板能够按请求语言展示；语言选择必须隔离并发请求，稳定错误码和字段路径不随语言改变。现有 `ResponseContext` 已按 Tokio task-local 隔离请求 ID 与 HTTP 状态策略，可沿用该作用域而不引入可变全局语言。

## 决策

1. 国际化能力属于根 package 的 `openrust::i18n`，由显式 `I18nPlugin` 注册；不新增空 crate。计数规则使用 ICU4X `icu_plurals`，数字/日期使用 `icu_decimal` 与 `icu_datetime`，locale 解析使用 `icu_locale`；依赖 2.3.x 稳定组件及编译数据，避免运行时网络或外部数据服务。只启用所需组件，数据/二进制大小作为依赖成本记录。
2. 消息资源使用 JSON 对象：一级键为 locale tag，二级键为稳定消息 key；纯字符串值维持原模板行为，对象值只能声明 `plural` 或 `select` 之一。`plural` 用 CLDR 类别键，`select` 用有限字符串键；二者必须有 `other`，无效参数、未知分支、缺回退或过多分支在资源加载时拒绝。框架内置 `en` 与 `zh-CN`；应用仍可从文件、嵌入资源或代码提供额外 JSON，并通过同一个 `MessageCatalog` 合并或覆盖。
3. 普通模板继续支持 `{name}` 参数替换，不执行表达式、函数或 HTML 转义。缺失 key 原样返回；structured message 缺少选择参数或无效 plural 数值时选 `other`，未提供的 `{name}` 占位符保留原文。plural 类别按实际命中的消息资源 locale 计算，包括 configured fallback，避免请求 locale 与 fallback 资源语言不一致。新增 typed `insert_plural` / `insert_select` 与 JSON 资源共用相同验证，不创建第二套模板语言。
4. 选择优先级为显式 `RequestLocale` 覆盖、`Accept-Language` 中首个受支持语言、应用默认语言、配置回退语言。语言标签按 BCP 47 常见语言/地区形式规范化；不支持的标签跳过。`format_number` 接收精确十进制文本，不接受浮点近似、不做舍入；`format_date` 使用调用方确定的公历日期，`format_datetime` 使用 Chrono 值的本地字段，调用方须先显式转换到业务时区。格式化不改变 API 机器字段、数据库 UTC 时间或错误码。
5. `I18n` 是不可变、可并发共享的应用服务；每个请求的 `RequestLocale` 进入请求扩展与现有 response task-local context。提供 `RequestLocale` 提取器和 `t(key, args)` 入口；业务可在认证后用中间件注入用户偏好覆盖，框架不读取 Cookie、Session 或用户表。
6. 框架错误用稳定 `openrust.error.<code>` key 渲染普通展示消息；机器 `code`、HTTP 状态、字段路径保持不变。应用 `Custom` 错误已有安全展示文本时原样保留；5xx 仍统一显示通用消息。验证错误按稳定 validator rule code 尝试翻译，应用自定义错误文本在没有对应规则映射时保留原文。
7. 邮件与短信模板可显式接收 `RequestLocale` / 指定 locale，并使用同一 catalog 查消息；发送任务没有请求上下文时由调用方明确选择 locale，不能读取全局当前语言。
8. `apps/server` 安装插件并展示 `Accept-Language`、显式覆盖、回退与并发隔离。默认不改变既有 API 响应语言，除框架内置错误消息可由应用配置选择默认语言。

## 验收

- 单元测试覆盖 locale 规范化、Accept-Language 顺序/quality 与无效输入、区域到语言回退、默认/最终 fallback、缺失 key/参数、资源合并、错误码/字段路径稳定。
- 并发 HTTP 请求分别选择中英文，成功与错误消息不串语言；显式覆盖高于请求头；超时/404/validation 等可处理框架错误使用当前语言。
- `apps/server` 集成测试和生成应用示例展示资源扩展及请求选择；应用布局按 ADR-039 的源码 workspace 方式。
- 该能力无数据库行为，不要求双库集成测试；模板供应商发送验收留给邮件/短信功能簇。
- V1 补强验收覆盖中文、英文及多复数规则语言、零/小数/区间、缺参/回退、选择分支与并发隔离；locale、时区和数值精度显式，展示格式不能修改 API 机器字段、稳定错误码或数据库 UTC 时间。HTTP、RPC、任务和邮件复用同一展示规则。

- 当前实现使用 ICU4X 编译数据；未接入 Fluent、货币/计量单位格式、精确数值分支、任意模板代码或 ICU 数据运行时更新。新增内容通过现有 `I18n` 服务在 HTTP、邮件、短信及任务调用方显式选择 locale；gRPC 在其接入簇复用并验收。

## 后果

- 中性 locale 与消息 key 可以由邮件、短信和错误响应共享；调用方显式传入语言，后台任务不依赖 HTTP 生命周期。
- JSON + 简单插值保持向后兼容；structured plural/select 与 ICU4X 地区格式化扩展同一个 catalog/service。编译数据无需云服务或运行时拉取；应用负责日期时间的业务时区选择和输出上下文转义。
