# Blade 章节对照

按固定 Laravel 13.x [Blade 文档快照](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/blade.md)、Laravel Framework v13.40.0 的 [View 源码](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View)及 [Blade 测试](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/View/Blade)核对。实际命令和逐批证据见 [03-07](../03-07/README.md) 和 [03-08](README.md)。

| 上游章节 | OpenRust 已覆盖 | 后续差异 |
| --- | --- | --- |
| Introduction、Livewire | 应用从路由返回命名视图。编译器缓存纯 Rust 指令树。 | Livewire 属于独立生态范围。 |
| Displaying Data、HTML Entity Encoding | 默认转义、可信原样输出、标量、变量和属性读取。 | 应用级关闭双重编码，PHP 任意函数调用改由 Rust 处理器、创建器或组合器计算。 |
| Blade and JavaScript Frameworks、Comments | 字面量转义、`@verbatim` 及 Blade 注释。 | 安全嵌入 JSON 的 `Js::from` 对应接口。 |
| If Statements | `@if`、`@elseif`、`@unless`、`@isset`、`@empty`、`@hasSection`、`@sectionMissing`，逻辑、严格比较与空值回退。 | Auth、Environment、Session、Context 专用指令及 PHP 通用宽松比较。 |
| Switch Statements、Loops、The Loop Variable | Switch 的匹配、默认分支、贯穿与跳转。支持 `@foreach`、`@forelse`、键和值、循环元数据、计数 `@for`、`@while` 及带条件或层级的 `@break` / `@continue`。 | PHP 任意可变循环表达式和流式集合长度语义。 |
| Conditional Classes and Styles、Additional Attributes | 条件列表、类与样式、布尔 HTML 属性。 | 更广的 PHP 数组构造表达式。 |
| Including Subviews、Rendering Views for Collections | `@include`、`@includeIf`、`@includeWhen`、`@includeUnless` 支持继承和覆盖数据。支持 `@includeFirst`、`@includeIsolated` 与 `@each` 的独立集合渲染。 | 集合由物化的 Rust 视图数据提供。 |
| The Once Directive、Raw PHP、Fonts | `@once` 及显式渲染树标识。 | `@php` 由调用方 Rust 代码替代。Fonts 仍待明确资产接线。 |
| Components、Index Components、Registering Components | 应用注册 Rust 组件、匿名索引、动态组件及既有视图命名空间的组件查找。 | PHP 类自动发现由 Rust 工厂替代。额外匿名组件路径和包发布接线。 |
| Passing Data to Components、Component Attributes、Reserved Keywords | 静态、绑定与简写属性、双冒号客户端绑定、`@props` 默认值、属性包的 `merge`、`class`、`style`、`only`、`except`。 | 更多筛选与方法链、保留名称和任意类型对象。依赖通过 Rust 工厂显式装配。 |
| Slots、Inline Component Views | 默认和具名插槽、绑定插槽名、插槽属性包和 Rust 内联组件视图。 | 作用域插槽、生成器及组件本地回调方法。 |
| Anonymous Components、Accessing Parent Data | 匿名组件及默认数据属性。 | `@aware`、额外匿名组件路径和命名空间。 |
| Building Layouts | 组件布局、继承、区段、`@parent`、`@yield`。 | 区段追加和覆盖变体、扩展布局数据等。 |
| Forms | `@csrf`、`@error`、默认与具名错误包、`@method` 字段。 | `_method` 到请求方法的应用装配仍待路由接线。 |
| Stacks | `@push`、`@prepend`、`@stack`、`@hasStack` 与单次形式。 | 上游其他或组合用法按单元复验。 |
| Service Injection | 在 Rust 处理器、组件工厂或组合器中解析服务并传入视图。 | 模板 `@inject` 的编译时类型绑定待定。 |
| Rendering Inline Blade Templates | 使用应用 Factory 从受信任的模板文本在内存渲染。 | Laravel 临时缓存文件和删除开关对应内存生命周期。 |
| Rendering Blade Fragments | 单个、多个、全部和条件响应，保持片段顺序和请求隔离。 | 多个具名片段统一渲染一次，组合器执行次数见 [验收记录](README.md#blade-片段)。 |
| Extending Blade、Custom Echo Handlers、Custom If Statements | Rust 自定义指令回调返回待转义文本或可信 HTML。支持注册自定义条件、否定形式、多分支和 Rust `check`。 | PHP 表达式生成改用 Rust 回调。任意 PHP 对象的自动输出处理待明确 Rust 数据接口。 |

03-07 和 03-08 保持部分验收状态。

## 本阶段验收与缓存回收

各批本地提交依次为 `30fd551b`、`ae2ca44a`、`724a5e38`、`35e69a74`、`a06be16d`、`171d8490`、`da5da518`、`7c71b4b1`、`9600a50b`。

结束时使用 `cargo clean --target-dir` 回收隔离 Cargo 编译产物 3.3 GiB。文档站、浏览器及验收临时目录移入废纸篓。项目目录 `df -kP .` 复查可用 112,447,288 KiB（115.15 GB／107.24 GiB）。
