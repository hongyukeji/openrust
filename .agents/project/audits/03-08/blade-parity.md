# Blade 章节对照

按固定 Laravel 13.x [Blade 文档快照](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/blade.md)、Laravel Framework v13.40.0 的 [View 源码](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View)及 [Blade 测试](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/View/Blade)核对。实际命令和逐批证据见 [03-07](../03-07/README.md) 和 [03-08](README.md)。

| 上游章节 | OpenRust 已覆盖 | 后续差异 |
| --- | --- | --- |
| Introduction、Livewire | 应用从路由返回命名视图。编译器缓存纯 Rust 指令树。 | Livewire 属于独立生态范围。 |
| Displaying Data、HTML Entity Encoding | 默认转义、可信原样输出、标量、变量和属性读取，应用级 HTML 实体双重编码开关。 | PHP 任意函数调用改由 Rust 处理器、创建器或组合器计算。 |
| Blade and JavaScript Frameworks、Comments | 字面量转义、`@verbatim`、Blade 注释、`Js::from` 和 `@js` 的脚本表达式输出。 | PHP JSON 标志和自定义序列化对象由 Rust 数据模型替代。 |
| If Statements | `@if`、`@elseif`、`@unless`、`@isset`、`@empty`、区段判断、环境匹配、会话与公开上下文绑定、HTTP 守卫，以及 Rust 视图数据的严格和宽松比较。 | PHP 用户定义对象与比较重载无直接对应。 |
| Switch Statements、Loops、The Loop Variable | Switch 的匹配、默认分支、贯穿与跳转。支持 `@foreach`、`@forelse`、键和值、循环元数据、计数 `@for`、`@while` 及带条件或层级的 `@break` / `@continue`。 | PHP 任意可变循环表达式和流式集合长度语义。 |
| Conditional Classes and Styles、Additional Attributes | 条件列表、类与样式、布尔 HTML 属性。 | 更广的 PHP 数组构造表达式。 |
| Including Subviews、Rendering Views for Collections | `@include`、`@includeIf`、`@includeWhen`、`@includeUnless` 支持继承和覆盖数据。支持 `@includeFirst`、`@includeIsolated` 与 `@each` 的独立集合渲染。 | 集合由物化的 Rust 视图数据提供。 |
| The Once Directive、Raw PHP、Fonts | `@once` 及显式渲染树标识。 | `@php` 由调用方 Rust 代码替代。Fonts 仍待明确资产接线。 |
| Components、Index Components、Registering Components | 应用注册 Rust 组件、匿名索引、动态组件、命名空间与额外路径。 | PHP 类自动发现由 Rust 工厂替代。包发布接线继续独立验收。 |
| Passing Data to Components、Component Attributes、Reserved Keywords | 静态、绑定与简写属性、双冒号客户端绑定、`@props` 默认值、属性包的 `merge`、`class`、`style`、`only`、`except`、前缀筛选与链式调用。 | 自定义筛选回调、保留名称和任意类型对象。依赖通过 Rust 工厂显式装配。 |
| Slots、Inline Component Views | 默认、具名与绑定名插槽、插槽属性包、Rust 内联组件视图和 `Component::data` 提供的作用域插槽。 | 组件生成器与方法发现。组件方法输出通过 Rust `data` 提供。 |
| Anonymous Components、Accessing Parent Data | 匿名组件、默认属性、`@aware`、额外路径和命名空间注册，扩展路径纳入视图预编译。 | Rust 在视图工厂上注册组件位置。 |
| Building Layouts | 组件布局、具名与候选继承、显式布局数据、区段追加与覆盖、`@parent`、`@yield`。 | Rust 使用视图数据映射。 |
| Forms | `@csrf`、`@error`、默认与具名错误包、`@method` 字段与 POST 路由方法覆盖，已用真实会话 CSRF 验证。 | Rust 请求同时暴露有效方法和原始方法。 |
| Stacks | `@push`、`@prepend`、`@stack`、`@hasStack`、单次形式及条件栈分支。 | 组合用法按整单元复验。 |
| Service Injection | `@inject` 使用 Rust 注册的类型化服务别名，将投影后的视图值绑定到变量。 | PHP 任意对象和方法调用对应 Rust 服务的显式数据投影。 |
| Rendering Inline Blade Templates | 使用应用 Factory 从受信任的模板文本在内存渲染。 | Laravel 临时缓存文件和删除开关对应内存生命周期。 |
| Rendering Blade Fragments | 单个、多个、全部和条件响应，保持片段顺序和请求隔离。 | 多个具名片段统一渲染一次，组合器执行次数见 [验收记录](README.md#blade-片段)。 |
| Extending Blade、Custom Echo Handlers、Custom If Statements | Rust 自定义指令回调返回待转义文本或可信 HTML。支持注册自定义条件、否定形式、多分支和 Rust `check`。 | PHP 表达式生成改用 Rust 回调。任意 PHP 对象的自动输出处理待明确 Rust 数据接口。 |

03-07 和 03-08 保持部分验收状态。

## 本阶段验收与缓存回收

各批本地提交依次为 `30fd551b`、`ae2ca44a`、`724a5e38`、`35e69a74`、`a06be16d`、`171d8490`、`da5da518`、`7c71b4b1`、`9600a50b`。

结束时使用 `cargo clean --target-dir` 回收隔离 Cargo 编译产物 3.3 GiB。文档站、浏览器及验收临时目录移入废纸篓。项目目录 `df -kP .` 复查可用 112,447,288 KiB（115.15 GB／107.24 GiB）。
