# 03-07 视图发现、变量与模板输出

## 契约与实现

- 对照 Laravel v13.40.0 的 FileViewFinder、Factory、ViewServiceProvider、CompilerEngine、Blade 测试、ViewCacheCommand 和 ViewClearCommand。按顺序查找路径、处理点号名称和命名空间，并通过应用容器按需解析视图工厂。参考 Goravel 的服务提供者和编译型应用装配。 [Laravel Views](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/views.md) · [Laravel Blade](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/blade.md) · [Goravel View ServiceProvider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/view/service_provider.go)
- 旧版 `6d1a8192` 的 `view.rs` 及 `tests/http/view.rs` 提供根目录限制、越界符号链接、HTML 转义和开发时更新的参考实现与回归。当前保持原有 `view("welcome")` 路由入口，运行时通过文件引擎读取 HTML，通过 Blade 引擎处理 `.blade.html`。
- Blade 当前支持默认转义、显式可信原始输出、注释、客户端字面量和 verbatim、条件及空判断、foreach 和 forelse。Rust 编译器将模板转换成指令树。缓存用模板路径定位、源文件摘要校验并通过同目录重命名发布，可发现同一时间戳内的内容更新。Artisan 预编译及清理与 HTTP 复用同一编译器。
- HTML 异常响应使用应用视图工厂，提供 status、message 变量，保留原状态码、协商规则和内置回退。工厂延迟读取应用配置，使业务 Provider 可在启动时替换配置。实际回归验证两应用隔离。

## 验收

隔离基线为 `21a65e98` 加已验收的依赖与本批改动，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline` 和 `cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。391 项测试报告通过，其中 3 项视图测试核对查找、转义、条件、循环、缓存过期、损坏恢复及链接边界。此批未启用真实 MySQL。
- 实际应用 `GET /` 验证静态 HTML、实时更新、文件缺失 500 和越界符号链接 500。三语文档的 Rust 和 Blade 片段逐一编译并接入回环 HTTP，验证数据渲染、原始输出与默认转义、定制 404 及缺失模板 500。
- 实际运行 `./artisan view:cache`、`./artisan view:clear`、`./artisan inspire`，均成功。通过配置切换编译目录，验证两份编译产物生成、清理及保留无关文件。
- 三语文档站构建成功。浏览器在根路径和 `/docs/` 子路径通过 12 项通用组合和 6 项视图章节专项检查，验证章节链接及 Blade 字面量显示。

## 后续边界

当前表达式使用 Rust 视图数据，支持字面量、变量及对象属性。PHP 函数、语句和其他 Blade 指令仍未覆盖。布局、组件、子视图的组合与视图组合器按 03-08 继续。Windows 实机未验收。

## 键值循环与循环变量

- 对照固定 [Blade 循环章节](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/blade.md#the-loop-variable)、[CompilesLoops](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/Concerns/CompilesLoops.php)、[ManagesLoops](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Concerns/ManagesLoops.php) 及固定 foreach、forelse 编译测试。以 PHP 8.5.12 独立容器运行固定 ManagesLoops，验证迭代序号、剩余数、首尾、奇偶、深度、父循环及退出恢复。
- 旧版 `6d1a8192` 的 `src/view.rs` 和 `tests/http/view.rs` 使用 MiniJinja。当前沿用已提取的查找、转义及请求隔离契约，在纯 Rust 指令树中增加键和值绑定。视图数组提供整数索引，JSON 对象保留视图数据映射顺序并提供字符串键。循环限于物化视图集合，PHP 可迭代对象及 LazyCollection 的未知长度语义留待相应接线。
- macOS ARM64、Rust 1.94.0。锁定离线 `cargo test -p openrust-framework --test view_finder` 五项通过。真实回环 HTTP 覆盖两应用并发、嵌套视图父循环、空分支复用外层循环、退出清理、转义及非法键绑定 500。
- 隔离副本通过 Rust 1.94.0 工作区格式检查和 `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`。隔离副本格式化了已有 Sanctum 测试差异。共享工作区的格式检查另外报告在途首页测试的格式差异。
- 实际 `./artisan view:cache` 预编译三个语言版本的示例，`./artisan view:clear` 清除三份产物并保留无关文件，`./artisan inspire` 成功。三语 VitePress 构建、根路径和 `/docs/` 的 12 项常规浏览器检查及 6 项循环章节检查通过。
