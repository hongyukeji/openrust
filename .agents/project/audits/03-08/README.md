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

## 组件绑定、命名空间与插槽属性

- 对照固定 [ComponentTagCompiler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/ComponentTagCompiler.php)。`:$property` 读取当前视图值，`::` 转为单冒号的客户端属性，包组件沿用既有命名空间查找和匿名索引优先级。插槽保存独立 HTML 与属性包，按上游修剪首尾空白。Rust 属性包复用统一转义，支持 `merge`、`class`、`style`、`only` 和 `except`。旧版无对应 Blade 标签编译器。
- macOS ARM64、Rust 1.94.0。8 项定向测试通过。真实 HTTP 验证包组件索引、快捷绑定、双冒号、插槽绑定、属性筛选和合并、转义、跨应用隔离及非法组件路径返回 500。隔离副本的工作区格式与严格 Clippy 通过。
- 实际 Artisan 缓存和清理三语示例。三语 VitePress 构建和 12 项浏览器组合通过。

## 自定义条件

- 对照固定 [BladeCompiler 的 if / check](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/BladeCompiler.php)。Rust 用 `r#if` 登记应用级谓词，支持原始名称、`unless`、`else` 和 `end` 变体，并通过 `check` 提供同一谓词。缓存记录条件名与指令名，求值阶段调用本应用的 Rust 回调。旧版无对应 Blade 编译接口。
- macOS ARM64、Rust 1.94.0。真实 HTTP 验证多分支、否定、嵌套、共用缓存时的谓词隔离、缺少结束指令返回 500。8 项定向测试、隔离副本全工作区格式与严格 Clippy 通过。
- 隔离应用将三语共用 Rust 示例接入 Provider，实际 Artisan 生成含注册条件指令的三个编译产物并清理。三语构建和 12 项浏览器组合通过。

## 组件数据继承

- 对照固定 [ManagesComponents](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Concerns/ManagesComponents.php) 和 `CompilesComponents::compileAware`。Rust 在组件体与插槽渲染阶段维护显式属性的作用域。模板开头的 `@aware` 保存声明顺序并读取最近一层值，父组件内部的 `@props` 默认值留在该模板。旧版无此 Blade 指令。
- macOS ARM64、Rust 1.94.0。8 项定向测试通过。真实 HTTP 验证默认和具名插槽内的继承、组件模板中的子组件、最近值、父默认值的作用域、回退和转义。隔离副本通过全工作区格式与严格 Clippy。实际 Artisan 缓存和清理三语示例，三语构建与 12 项浏览器组合通过。

## 作用域插槽

- 对照固定 [类组件编译](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/Concerns/CompilesComponents.php) 和 Blade Scoped Slots 章节。Rust 先解析一次已注册组件并收集 `Component::data`，默认与具名槽通过 `$component` 读取该数据，组件本体通过普通变量读取。运行时方法发现对应 Rust 中的显式数据计算，已有组件使用默认空数据。旧版 MiniJinja 无对应类组件插槽。
- macOS ARM64、Rust 1.94.0。8 项定向测试通过。真实 HTTP 验证两应用隔离、组件实例序号、默认与具名槽、外部变量恢复及转义。隔离副本通过工作区格式与严格 Clippy，实际 Artisan 缓存和清理三语示例，三语构建与 12 项浏览器组合通过。

## 匿名组件挂载

- 对照固定 BladeCompiler 的 `anonymousComponentPath`、`anonymousComponentNamespace` 和 ViewCacheCommand。Rust 在 Factory 显式挂载视图目录及文件系统根，复用 Finder 的根目录约束。普通命名视图优先，其次是匿名命名空间和额外路径，各位置按同名、索引、目录同名模板查找。Artisan 预编译包含已登记的 Finder 目录。
- macOS ARM64、Rust 1.94.0。8 项定向测试通过，真实 HTTP 验证带前缀和无前缀的额外路径、命名空间、动态索引、双应用数据与缺失 500。隔离副本全工作区格式及严格 Clippy 通过。实际 Artisan 用三语 Rust 示例登记位置，生成五个产物并确认其中包含挂载根模板，清理成功。三语构建和 12 项浏览器组合通过。

## 布局候选、区段与条件栈

- 对照固定 `CompilesLayouts`、`ManagesLayouts` 和 `CompilesConditionals`。布局候选共用现有 Finder 和显式视图数据，区段可追加、覆盖或结束。条件栈支持 `@pushIf` 和 `@elsePushIf`。Rust 视图值的 `==` / `!=` 复用 Switch 的宽松比较，保留 `===` / `!==` 的严格行为。
- macOS ARM64、Rust 1.94.0。共享工作区的并行 Mail 改动触发 `CompositeTransport` 与 `SendmailTransport` 私有重导出错误。隔离基线的 8 项定向测试通过，真实 HTTP 覆盖布局回退、优先数据、区段追加和覆盖、条件栈、比较运算优先级与缺失结束标签 500。隔离副本全工作区格式和严格 Clippy、实际 Artisan 缓存与清理、三语构建与 12 项浏览器组合通过。

## 视图服务注入

- 对照固定 Blade Service Injection 章节。Rust Factory 在装配时注册服务类型、模板别名与数据投影，`@inject` 声明在模板开头从当前应用容器解析服务。Rust 适配保留模板变量查找，服务方法在 Rust 侧计算。未注册服务返回渲染错误。
- macOS ARM64、Rust 1.94.0。隔离基线 8 项定向测试通过，真实 HTTP 验证双应用各自容器、对象属性、转义和缺失服务 500。隔离副本全工作区格式与严格 Clippy 通过。用三语共用 Rust 示例装配 Provider 后运行实际 Artisan，三个示例编译和清理成功。三语构建和 12 项浏览器组合通过。

## 属性包链式调用与前缀筛选

- 对照固定 [ComponentAttributeBag](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/ComponentAttributeBag.php)。组件和插槽的属性包按书写顺序求值，支持 `whereStartsWith`、`thatStartWith` 和 `whereDoesntStartWith`。筛选接受单个字符串或字符串列表。链内筛选只影响当前输出，最终属性由统一例程转义。旧版 MiniJinja 无对应的 Blade 属性包。
- macOS ARM64、Rust 1.94.0。隔离副本 8 项定向测试通过，真实 HTTP 验证组件及具名插槽链式筛选、调用顺序、嵌套字符串里的箭头、属性转义及未知方法 500。工作区格式与严格 Clippy 通过，实际 Artisan 预编译并清理三个三语示例产物，三语构建与 12 项浏览器组合通过。

## Artisan 组件生成

- 对照固定 [ComponentMakeCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ComponentMakeCommand.php)。`make:component` 默认生成 Rust 类型和 Blade 视图，支持嵌套名称、`--inline`、`--view`、`--path`、`--force` 以及可发布的 Rust 源码 stub。沿用源码生成器的应用锁和文件写入，第二个文件创建失败时恢复第一个文件。Rust 类型按应用模块树和 Factory 显式注册。旧版无同名生成器。
- macOS ARM64、Rust 1.94.0。实际 Artisan 从三语正文运行生成命令，并验证自定义目录、路径检查、重复、回滚、强制重建，以及每个应用的 `view:cache` 和 `view:clear`。隔离 HTTP 探针编译实际生成的两种 Rust 类型，三次真实回环请求均渲染具名、内联与匿名组件。8 项共享工作区定向测试、隔离格式和严格 Clippy、三语构建和 12 项浏览器组合通过。

## 属性读取、插槽内容与组件可见性

- 对照固定 [ComponentAttributeBag](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/ComponentAttributeBag.php)、[ComponentSlot](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/ComponentSlot.php) 和 Blade 组件章节。属性包支持 `prepends`、`first`、`get`、`has`、`hasAny` 和 Rust 注册的 `filter`。插槽支持空值、非空值和忽略注释的内容判断。`Component::shouldRender` 在渲染组件或插槽之前判断可见性。旧版无相应 Blade 机制。
- macOS ARM64、Rust 1.94.0。共享工作区 11 项定向测试通过，真实 HTTP 覆盖筛选回调、前缀默认值、属性读取和转义、插槽注释与标记、隐藏组件跳过子视图，以及未注册过滤器 500。隔离全工作区格式和严格 Clippy、实际 Artisan 缓存与清理三个三语示例、三语 VitePress 构建与 12 项浏览器组合通过。

### 具名片段的逐次渲染

- PHP 8.5 容器加载固定 Laravel Framework v13.40.0 的真实 BladeCompiler、View 和 Factory。以有状态组合器和 `@once` 复现实测行为。具名列表依序渲染同一个 View，缺失项执行该轮渲染并返回完整输出，重复名称重新执行组合器，空列表跳过渲染。全部片段共用一次渲染。 [固定 View 实现](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/View.php)
- Rust 每轮保留同一实例的组合器数据和请求上下文，以独立渲染树清理片段、栈和 `@once` 状态。实际 HTTP 覆盖顺序、重复、缺失、全部、条件回退、子视图和并发应用隔离。含 5 项 `view_finder` 定向测试及严格 Clippy 均通过。旧版视图为静态 HTML 和内嵌模板，未找到可复用的片段组合器实现。
- 隔离应用实际执行 `./artisan view:cache` 和 `./artisan view:clear`，编译并清理三语片段示例。三语 VitePress 构建通过。

### 流式集合的循环元数据

- 在 PHP 8.5 容器中使用固定 Blade 编译器实测 Laravel `LazyCollection` 和实现 `Countable` 的迭代器。未知长度时三个依赖长度的字段为 null，父循环保留元数据。嵌套 `@break(2)` 仅消费首项。已知长度时以 `3,2,1,0` 验证总数和剩余量，并核对首项和末项。 [固定 ManagesLoops](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Concerns/ManagesLoops.php)
- Rust 通过 `withIterator` 或 `withExactSizeIterator` 显式传递可重开迭代器工厂。实际 HTTP 覆盖键值、已知长度、未知长度、嵌套父循环和空流分支，惰性探针在多读时直接失败。旧版模板没有可复用的 Blade 循环实现。
- 实际 Artisan 缓存及清理三语示例，示例 Rust 入口接入回环 HTTP。5 项 `view_finder` 定向测试、严格 Clippy 和三语 VitePress 构建通过。

### 数组表达式与下标

- PHP 8.5 容器以固定 Laravel BladeCompiler 渲染与 Rust HTTP 相同的模板。覆盖短数组、`array(...)`、变量键、嵌套取值、重复键保持插入位置、规范整数键和保留前导零的键、布尔与空值转键、浮点截断、负键后自动编号、合并运算和 HTML 转义。旧版静态视图未提供数组表达式实现。 [固定 BladeCompiler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/BladeCompiler.php)
- 纯 Rust 求值沿用 JSON 视图数据。连续整数键为列表，混合键为有序对象，取不到下标返回 null。无法转换的键和隐式整数溢出返回 null，已通过实际 HTTP 验证。PHP 的引用、数组展开与对象重载由 Rust 调用方准备数据。缓存版本随语法树更新。
- 11 项 `view_finder`、`csrf` 和 `authorization` 定向回归通过，改动文件格式及严格 Clippy 通过。隔离应用用实际 Artisan 缓存及清理三语模板示例，缓存生成后通过 HTTP 验证转义结果。三语 VitePress 构建、章节输出和 12 项浏览器检查通过。

### 三项边界与定稿复核

- 在 PHP 8.5 容器内用固定源码的 BladeCompiler、Factory 和 View 实测。Countable 的计数、打开迭代器和取项次数为 1、1、2。两项非空 LazyCollection 打开两次并读取四项，空 LazyCollection 只打开一次。Rust 用 `withExactSizeIterator` 提供不消费条目的精确长度，用 `withIterator` 提供可重开的有限迭代器。真实 HTTP 检查已知长度、未知长度的逐项读取次数、第二轮数据、键值转义、回退和双应用隔离。 [固定 Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Factory.php) · [固定 LazyCollection](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php)
- 真实 HTTP 和固定源码均确认 `@includeFirst` 读取带键数组的值，按插入顺序选择首个存在的视图。验证模板字面量、运行时映射、回退、继承数据及全部缺失的错误。复用纯 Rust 通用数组表达式。
- 真实 HTTP 和固定源码均确认 `@hasSection`、`@sectionMissing` 的第二参数进入 `yieldContent` 语义。已定义区段优先，覆盖空白、字符串 `0`、布尔默认值、HTML 转义和 PHP 默认 trim 字符范围。旧版 MiniJinja 视图源码中未发现这三项的等价实现。 [固定 CompilesConditionals](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/Concerns/CompilesConditionals.php) · [固定 ManagesLayouts](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Concerns/ManagesLayouts.php)
- Rust 1.94.0 的 `view_finder` 与 `csrf` 共 8 项通过。改动文件格式和隔离工作区定向严格 Clippy 通过。实际 Artisan 缓存并清理九份三语片段，缓存生成后在隔离应用用四条回环 HTTP 请求复验。三语 VitePress 构建与 12 项浏览器检查通过。
- 定稿复核发现固定 Blade 可用 `@class($classes)` 和 `@style($styles)` 处理运行时集合。PHP 固定基线实际生成 `class="base emphasis" style="display: block;"`，Rust 解析要求字面量，加入相同模板后实际 `artisan view:cache` 返回编译错误，移除探针后缓存及清理再次通过。这项可由 Rust 实现，Blade 本体暂缓记录最终定稿。Vite 和 Livewire 仍归各自单元。 [固定 CompilesClasses](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/Concerns/CompilesClasses.php) · [固定 CompilesStyles](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/Concerns/CompilesStyles.php)
