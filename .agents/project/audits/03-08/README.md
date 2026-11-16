# 03-08 布局、组件、视图组合器

[固定 Laravel Blade 章节对照](blade-parity.md)记录逐节已验收行为和剩余差异。

## 契约与实现

- 对照固定 Laravel 的 ManagesLayouts、ManagesComponents、ManagesEvents、Factory 和 Blade 组件编译入口。Goravel 的应用级视图服务保存共享数据。复用旧版请求级视图数据与显式覆盖共享默认值的合并规则。 [Laravel Views](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/views.md#view-composers) · [Laravel Blade](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/blade.md#building-layouts) · [Goravel View](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/view/view.go)
- 支持布局继承、区段、默认内容、`@parent` 和继承局部数据的子视图。继承链共享子区段，渲染树退出时随状态一并释放，失败后不会留在应用容器中。嵌套渲染限制深度。
- 支持匿名组件、显式注册的 Rust 类组件、静态与绑定属性、默认及命名插槽。已渲染插槽以 HTML 片段传递，其中的变量照常转义。应用容器保存组合器、创建器和共享值，回调对每个视图实例操作。渲染保留当前应用的门面作用域。
- Rust 使用闭包和 Component trait 注册类组件。剩余 Blade 属性包、`@props`、动态组件、PHP 表达式及其他指令仍待覆盖。Windows 实机未验收。

## 验收

隔离副本以 `21a65e98` 与此前已验收的依赖为基线，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，测试报告 392 项通过。未为本批启用真实数据库及外部存储。
- 真正的回环 TCP 发送 5 次 HTTP 请求，覆盖两个独立应用并发渲染、两级继承、默认与命名插槽、类组件、组合器与创建器时机、缺失父布局 500，以及失败后普通布局的区段回退。
- 三语文档的 Rust 注册代码与 Blade 模板编译并各通过一次真实 HTTP。实际运行 `./artisan view:cache`、`./artisan view:clear` 和 `./artisan inspire`，退出码均为 0。
- 文档构建通过，根路径与 `/docs/` 子路径通过 12 项通用浏览器检查和 6 项组合器、布局、组件专项检查。

下一批为 03-09 Redis，配置和缓存前置分别为 01-01 与 03-06。

## 动态组件与匿名索引视图

- 对照固定 [DynamicComponent](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/DynamicComponent.php)、[ComponentTagCompiler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/ComponentTagCompiler.php) 及 [BladeComponentTagCompilerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/View/Blade/BladeComponentTagCompilerTest.php) 的无类组件索引查找。旧版 `6d1a8192` 使用 MiniJinja，查不到可直接移植的 Blade 标签。动态名称按现有组件名称规则验证并通过当前 Factory 分发，Rust 组件沿用显式注册，匿名组件的查找顺序为同名视图、`index`、目录同名文件。目标组件收到其余属性和默认及具名插槽。
- macOS ARM64、Rust 1.94.0。锁定离线 `cargo test -p openrust-framework --test view_finder` 五项通过。真实回环 HTTP 覆盖两个应用并发、动态嵌套、Rust 注册组件、转义、插槽、匿名索引优先顺序与回退、非法及缺失组件 500。隔离副本全工作区格式和严格 Clippy 通过，已有 Sanctum 测试仅在副本中格式化。
- 实际 `./artisan view:cache` 编译三语动态组件示例，`./artisan view:clear` 清除三个产物并保留无关文件。三语 VitePress 构建、根路径和 `/docs/` 的 12 项通用浏览器检查及 6 项动态组件章节检查通过。
- PHP 类与组件发现留给上游运行时。OpenRust 使用应用级 Rust 工厂登记和现有视图查找。Blade 组件命名空间、扩展匿名组件路径、属性包和 `@props` 仍待验收。

## 属性声明与属性包

- 对照固定 [CompilesComponents](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/Concerns/CompilesComponents.php)、[ComponentAttributeBag](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/ComponentAttributeBag.php) 及 Blade 组件属性和匿名组件测试。模板开头可声明 `@props`，支持具名默认值和 camelCase 对应的 kebab-case 调用。剩余参数作为独立属性包输出，可用 `merge` 提供默认值，合并 class / style，普通属性采用调用方传值。
- Rust 使用 `ViewData` 的具名值和渲染上下文保存属性包。HTML 属性由统一转义例程输出，支持标量、布尔和空值。PHP 任意对象及可追加属性对象、属性过滤和类组件构造器自动提取留待各自契约。旧版 MiniJinja 没有可移植的 Blade 属性包。
- macOS ARM64、Rust 1.94.0。锁定离线 `cargo test -p openrust-framework --test view_finder` 五项通过。真实回环 HTTP 覆盖并发应用、两级属性包隔离、默认与覆盖、普通属性和 class / style 合并、引号转义、布尔省略及非法声明和非标量值 500。隔离副本通过工作区格式和严格 Clippy，已有 Sanctum 测试只在副本中格式化。
- 实际 `./artisan view:cache` 编译三语属性示例，`./artisan view:clear` 清除三个产物并保留无关文件。三语 VitePress 构建、根路径和 `/docs/` 的 12 项通用浏览器检查及 6 项属性章节检查通过。

## 布局栈和单次渲染

- 对照固定 [CompilesStacks](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/Concerns/CompilesStacks.php) 与 [ManagesStacks](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Concerns/ManagesStacks.php)。固定 ManagesStacks 在 PHP 8.5.12 中复验跨深度 push、prepend 排序、空栈和清理。旧版 MiniJinja 无 Blade 栈管理器可供提取。
- Rust 在顶层渲染内共享栈与单次渲染标识，覆盖布局、子视图和组件。`@once`、`@pushOnce` 和 `@prependOnce` 的无参标识由编译模板及指令位置确定，显式标识使用非空字符串，跨模板共用。此批覆盖 `@push`、`@prepend`、`@stack`、`@hasStack` 以及上述单次形式。
- macOS ARM64、Rust 1.94.0。锁定离线 `cargo test -p openrust-framework --test view_finder` 五项通过。真实 HTTP 覆盖跨层排序、重复子视图去重、两应用并发、指令不匹配 500、重复请求和空栈回退。隔离副本通过工作区格式和严格 Clippy，已有 Sanctum 测试只在副本中格式化。
- 实际 `./artisan view:cache` 编译三语示例，`./artisan view:clear` 清除三个产物并保留无关文件。三语 VitePress 构建、根路径和 `/docs/` 的 12 项通用浏览器检查及 6 项栈章节检查通过。

## Blade 片段

- 对照固定 [ManagesFragments](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Concerns/ManagesFragments.php) 和 [View](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/View.php)。`@fragment` 捕获已渲染标签，同一视图树内的子视图可贡献片段。Rust 用链式 View 方法选择响应，多个具名片段按参数顺序拼接，整个视图每响应求值一次。Laravel 的具名列表可多次调用 render，Rust 组合器只执行一次。旧版 MiniJinja 无对应的 Blade 片段捕获器。
- macOS ARM64、Rust 1.94.0。锁定离线 `cargo test -p openrust-framework --test view_finder --test csrf` 八项通过。真实 HTTP 覆盖单个、多个、全部、缺失、条件回退、子视图、并发应用隔离及未闭合片段 500。隔离副本通过全工作区格式与严格 Clippy，已有 Sanctum 测试只在副本中格式化。
- 实际 `./artisan view:cache` 编译三语片段示例，`./artisan view:clear` 清除三个产物并保留无关文件。三语 VitePress 构建、根路径和 `/docs/` 的 12 项通用浏览器检查及 6 项片段章节检查通过。

## 内联模板与 Rust 自定义指令

- 对照固定 [内联模板](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/blade.md#rendering-inline-blade-templates) 和 [Blade 扩展](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/blade.md#extending-blade)。Rust 的 `Factory::renderInline` 在内存中渲染模板，复用应用解析器。自定义指令以已求值的视图值调用应用注册的 Rust 闭包，显式返回待转义文本或可信 HTML。编译产物校验应用的注册名称集合，回调只在运行时解析。旧版 MiniJinja 的自定义注册 ABI 与 Blade 不同。
- macOS ARM64、Rust 1.94.0，锁定离线 `cargo test -p openrust-framework --test view_finder --test csrf` 八项通过。覆盖直接内联渲染、默认转义、可信标记、已登记指令的非法表达式、注册导致缓存失效以及共用缓存目录的两应用实际 HTTP 隔离。隔离副本通过全工作区格式和严格 Clippy，已有 Sanctum 测试只在副本中格式化。
- 隔离应用用三语共用的 Rust 文档代码在 Provider 注册示例指令。实际 `./artisan view:cache` 产出三份包含已注册指令的编译产物，`./artisan view:clear` 清除产物并保留无关文件。三语站点构建、根路径与 `/docs/` 的 12 项通用浏览器检查和 6 项扩展章节检查通过。

## 子视图数据、优先顺序和局部隔离

- 对照固定 [CompilesIncludes](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/Concerns/CompilesIncludes.php) 和 [Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Factory.php)。子视图参数可以用具名列表或映射对象给出，显式数据覆盖继承值。`@includeFirst` 通过既有 Factory 查找顺序选出第一个存在的视图。`@includeIsolated` 继承应用共享值和渲染树状态，调用方的普通数据、插槽和组件属性各自隔离。旧版 `6d1a8192` 的查找及视图数据优先级已在已有 Factory 中复用。
- macOS ARM64、Rust 1.94.0。锁定离线 `cargo test -p openrust-framework --test view_finder --test csrf` 八项通过。真实 HTTP 验证显式数据覆盖、条件选择、缺页回退、共享数据与调用者变量隔离、无候选 500 及默认转义。按当时 HEAD 重建隔离副本后通过全工作区格式和严格 Clippy，已有 Sanctum 测试只在副本中格式化。
- 实际 `./artisan view:cache` 编译三语示例，`./artisan view:clear` 清除三个产物并保留无关文件。三语 VitePress 构建、根路径和 `/docs/` 的 12 项通用浏览器检查及 6 项子视图检查通过。

## 集合子视图

- 对照固定 Laravel Factory 的 `renderEach`。使用现有命名视图渲染与局部数据隔离，逐条传入原始键、指定变量和应用共享值。空集合支持缺省空串、命名模板和 `raw|` 字面量。旧版 MiniJinja 可参考数据边界，沿用已有视图查找和请求隔离实现。
- macOS ARM64、Rust 1.94.0。真实 HTTP 验证数组索引、对象键、应用数据隔离、默认转义及非法集合返回 500，8 项定向测试通过。隔离副本的工作区格式和严格 Clippy 通过，既有 Sanctum 格式差异仅在副本整理。
- 实际 Artisan `view:cache` 预编译三语示例，`view:clear` 清理三个产物。三语 VitePress 构建通过。
