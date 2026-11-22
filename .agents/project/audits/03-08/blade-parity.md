# Blade 章节对照

按固定 Laravel 13.x [Blade 文档快照](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/blade.md)、Laravel Framework v13.40.0 的 [View 源码](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View)及 [Blade 测试](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/View/Blade)核对。实际命令和逐批证据见 [03-07](../03-07/README.md) 和 [03-08](README.md)。

| 上游章节 | OpenRust 已覆盖 | Rust 实现边界与关联单元 |
| --- | --- | --- |
| Introduction、Livewire | 应用从路由返回命名视图。编译器缓存纯 Rust 指令树。 | Livewire 属于独立生态范围。 |
| Displaying Data、HTML Entity Encoding | 默认转义、可信原样输出、标量、数组表达式、变量和属性读取，应用级 HTML 实体双重编码开关。 | PHP 任意函数调用改由 Rust 处理器、创建器或组合器计算。 |
| Blade and JavaScript Frameworks、Comments | 字面量转义、`@verbatim`、Blade 注释、`Js::from` 和 `@js` 的脚本表达式输出。 | PHP JSON 标志和自定义序列化对象由 Rust 数据模型替代。 |
| If Statements | `@if`、`@elseif`、`@unless`、`@isset`、`@empty`、区段判断及与 `@yield` 共用的显式默认值、环境匹配、会话与公开上下文绑定、HTTP 守卫，以及 Rust 视图数据的严格和宽松比较。 | PHP 用户定义对象与比较重载无直接对应。 |
| Switch Statements、Loops、The Loop Variable | Switch 的匹配、默认分支、贯穿与跳转。支持 `@foreach`、`@forelse`、键和值、循环元数据、计数 `@for`、`@while` 及带条件或层级的 `@break` / `@continue`。 | 流式集合按已知或未知长度生成循环元数据，提前跳出时停止迭代。PHP 任意可变循环表达式仍通过 Rust 数据准备表达。 |
| Conditional Classes and Styles、Additional Attributes | `@class`、`@style` 接受字面量和运行时集合。布尔 HTML 属性按条件输出。 | 数组表达式和 CSS 输出复用 Rust 视图数据模型，最终 HTML 属性值转义。见 [验收](README.md#运行时-css-集合与-blade-定稿)。 |
| Including Subviews、Rendering Views for Collections | `@include`、`@includeIf`、`@includeWhen`、`@includeUnless` 支持继承和覆盖数据。`@includeFirst` 按列表或带键映射的值和插入顺序查找。支持 `@includeIsolated` 与 `@each` 的独立集合渲染。 | `@each` 接受物化数据或注册的有限迭代器工厂，按精确长度或两次遍历实现 Laravel 的计数与渲染。 |
| The Once Directive、Raw PHP、Fonts | `@once` 及显式渲染树标识，`@fonts` 支持开发和生产清单、别名及预加载。 | `@php` 和 `@use` 对应调用方 Rust 代码及模块导入。完整 Vite 构建与 Node 插件接线由资产主题验收。 |
| Components、Index Components、Registering Components | 应用注册 Rust 组件、匿名索引、动态组件、命名空间、额外路径与 `shouldRender`。 | PHP 类自动发现由 Rust 工厂替代。包发布接线继续独立验收。 |
| Passing Data to Components、Component Attributes、Reserved Keywords | 静态、绑定与简写属性、双冒号客户端绑定、`@props` 默认值、属性包的 `merge`、`class`、`style`、`only`、`except`、前缀筛选与链式调用、属性值和键检查、默认值追加和 Rust 筛选回调。组件和插槽支持标签上的 CSS 指令，也可向子组件传递经过筛选、合并的属性包。 | Rust 显式暴露组件数据与装配依赖，属性包和插槽由模板保留。 |
| Slots、Inline Component Views | 默认、具名与绑定名插槽、插槽属性包、插槽内容判断、Rust 内联组件视图、`Component::data` 提供的作用域插槽，以及默认、内联和匿名组件生成。 | Rust 模块声明和工厂注册由应用完成。组件方法输出通过 Rust `data` 提供。 |
| Anonymous Components、Accessing Parent Data | 匿名组件、默认属性、`@aware`、额外路径和命名空间注册，扩展路径纳入视图预编译。 | Rust 在视图工厂上注册组件位置。 |
| Building Layouts | 组件布局、具名与候选继承、显式布局数据、区段追加与覆盖、`@parent`、`@yield`。 | Rust 使用视图数据映射。 |
| Forms | `@csrf`、`@error`、默认与具名错误包、`@method` 字段与 POST 路由方法覆盖，已用真实会话 CSRF 验证。 | Rust 请求同时暴露有效方法和原始方法。 |
| Stacks | `@push`、`@prepend`、`@stack`、`@hasStack`、单次形式及条件栈分支。 | 组合用法按整单元复验。 |
| Service Injection | `@inject` 使用 Rust 注册的类型化服务别名，将投影后的视图值绑定到变量。 | PHP 任意对象和方法调用对应 Rust 服务的显式数据投影。 |
| Rendering Inline Blade Templates | 使用应用 Factory 从受信任的模板文本在内存渲染。 | Laravel 临时缓存文件和删除开关对应内存生命周期。 |
| Rendering Blade Fragments | 单个、多个、全部和条件响应，保持片段顺序和请求隔离。 | 每个具名项独立渲染同一视图并再次运行组合器，缺失项返回对应的完整输出。全部片段共用一次渲染。见 [验收记录](README.md#具名片段的逐次渲染)。 |
| Extending Blade、Custom Echo Handlers、Custom If Statements | Rust 自定义指令回调返回待转义文本或可信 HTML。支持自定义条件、否定形式、多分支、Rust `check`、类型化 `stringable` 和集合输出处理器。 | PHP 表达式生成改用 Rust 回调，应用通过 `withStringable` 显式传递类型化值。 |

03-07 和 03-08 按交付计划完成核心闭合。纯 Rust 模板使用显式类型投影、组件注册和渲染回调。Gate 授权指令已通过 [05-09 的专项验收](../05-09/README.md#blade-授权指令)。完整 Vite 工具链及 Livewire 集成按各自主题验收。已按固定文档的能力范围复核 Blade 本体并定稿。保留 PHP 运行时语法与对象语义的 Rust 适配，维持 HTML 属性转义。 [定稿验收](README.md#运行时-css-集合与-blade-定稿)

## 本阶段验收与缓存回收

各批本地提交依次为 `30fd551b`、`ae2ca44a`、`724a5e38`、`35e69a74`、`a06be16d`、`171d8490`、`da5da518`、`7c71b4b1`、`9600a50b`。

结束时使用 `cargo clean --target-dir` 回收隔离 Cargo 编译产物 3.3 GiB。文档站、浏览器及验收临时目录移入废纸篓。项目目录 `df -kP .` 复查可用 112,447,288 KiB（115.15 GB／107.24 GiB）。


## 03-07 与 03-08 整单元复验

- 共享工作区的 `view_finder`、`csrf`、`request_input`、`routing_groups`、`route_metadata` 共 11 项通过。真实 HTTP 验证视图数据、双应用和请求状态隔离、循环、布局、组件、插槽、组合器、缺失视图的回退及 CSRF 表单覆盖。
- 应用首页入口测试在共享工作区受并行 `WelcomeController` 导入变更阻塞。使用已提交的路由和测试在隔离副本复验，命名视图优先级、源文件实时更新、响应头和缺失模板 500 均通过。隔离副本全工作区格式与严格 Clippy 通过。
- 各批实际执行 Artisan，覆盖 `view:cache`、`view:clear` 和组件生成。三语 VitePress 构建通过，根路径与 `/docs/` 的 12 项浏览器组合通过。三语构建产物包含 7 个复核的 Blade 锚点。验证环境为 macOS ARM64 和 Rust 1.94.0。
- 本轮实施提交依次为 `2e9508194`、`82c28402c`、`d0b7970a2`、`5b6bfceb5`、`e1fd0f5da`、`ad3b8af42`、`faa31259f`、`b935a011f`、`d19127b48`、`b37cc1f50`、`c7af443c0`、`94065ba23`、`11d119dcc`、`c9fc7760b`、`5ad60f073`、`c23c1d3e2`、`1d05bc688`、`c5938fc4f`、`a073612cd`、`993e407ea`。
- 隔离目录的 `cargo clean --target-dir` 回收 13.5 GiB，临时验收副本移入废纸篓。项目卷可用 77,787,892 KiB（79.65 GB／74.18 GiB）。

## V03 表达式边界复核（2026-11-20）

[专项验收](../03-07/expression-contracts.md)补齐原生数组展开及 include 数据，缓存版本 34；明确函数、引用、Traversable、对象重载和编译阶段的 Rust 接口及拒绝边界。96 组固定 PHP、27 项定向测试、根应用 HTTP／Artisan、严格 Clippy、三语构建与 12＋6 浏览器检查通过。这里的闭合不包含任意 PHP 解释器，也不替代其他发布门禁。

## 模板翻译接入（2026-11-22）

`__`／`trans`／`trans_choice` 与带参数 `@lang`／`@choice` 已接入同一 Lang 服务，模板渲染时读取请求语言，缓存版本更新为 36。语言作用域跨既有阻塞线程传递，子模板与组件共用。补齐 `@lang … @endlang` 和数组参数区块，嵌套替换行为按固定上游 Trait 对照；无参翻译器访问和任意 PHP 调用仍未支持；不改变前述历史表达式验收范围。来源、实际消费者和验收证据见 [统一请求与模板国际化](../03-07/welcome-language-2026-11-22.md#统一请求与模板国际化)。


## d4a40d7f4 翻译契约复核（2026-11-22）

缓存版本更新为 37；补齐 __()／__(null)、原生分组／列表、空语言、@lang 第四个 fallback 参数及复数 count=null。新增模板缺键回调消费者，既有转义、嵌套区块、缓存、子模板和组件回归重新运行。blade_translations 6 项及其他受影响目标通过；全批共100项，另有26项固定PHP对照及24组真实HTTP浏览器检查。完整证据、适配和未实现边界见 [d4a40d7f4 基线复核](../03-07/welcome-language-2026-11-22.md#d4a40d7f4-基线复核)。trans() 取得译器服务对象仍未实现，不归因于Rust不能表达；类型校验与直接输出数组报错属于明确适配。未暂存、提交或推送。

## BCP 47 迁移（2026-11-22）

公开语言及资源采用 en／zh-Hans／zh-Hant，缓存版本升为38，既有翻译区块、转义、子模板／组件及跨线程作用域继续复验通过；冷／热缓存和独立部署重建已实测。其余表达式及服务对象边界不变。当前源码和完整证据见[语言标识迁移](../03-07/locale-bcp47-2026-11-22.md)，前述版本号及旧语言仅代表各历史批次。
